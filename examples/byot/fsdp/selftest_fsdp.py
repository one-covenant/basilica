"""CPU self-test for byot_grpo_fsdp.py: the sharding, loading, gradient and
gather logic on a tiny random Qwen3-MoE, no GPU and no Basilica needed.

    torchrun --nproc-per-node 2 selftest_fsdp.py      # gloo on CPU

Checks, per rank count and micro-batch size:
1. sharded load: gathering the loaded model gives the checkpoint back bit for bit
2. gradients equal an unsharded single-process reference of the same GRPO loss
   (loss scaling by world size, uneven slices, zero-weighted dummy passes)
3. every expert parameter has a gradient on every rank (the MoE collective rule)
4. after an optimizer step, the gather returns the checkpoint's names and shapes

SELFTEST_NO_PATCH=1 skips patch_moe_every_expert(): rank 1 then aborts in its
first backward (the ranks' expert gradients no longer line up in FSDP2's
reduce-scatter), which is why the trainer needs the patch.
"""
import os, sys, tempfile

import torch
import torch.distributed as dist
from torch.distributed.tensor import DTensor

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import byot_grpo_fsdp as T  # noqa: E402


def make_checkpoint(path):
    from transformers import Qwen3MoeConfig, Qwen3MoeForCausalLM
    torch.manual_seed(0)
    cfg = Qwen3MoeConfig(vocab_size=256, hidden_size=64, intermediate_size=128, moe_intermediate_size=32,
                         num_hidden_layers=2, num_attention_heads=4, num_key_value_heads=2, head_dim=16,
                         num_experts=8, num_experts_per_tok=2, tie_word_embeddings=False,
                         max_position_embeddings=128)
    model = Qwen3MoeForCausalLM(cfg).to(torch.bfloat16)  # the real checkpoint is bf16
    model.save_pretrained(path, max_shard_size="200KB")  # several files + an index, like the real one


def batch_for(ref, n_seqs, device):
    g = torch.Generator().manual_seed(1)
    seqs, plens = [], []
    for i in range(n_seqs):
        plen = 3 + i % 3
        seqs.append(torch.randint(1, 256, (plen + 4 + i % 5,), generator=g).tolist())
        plens.append(plen)
    with torch.no_grad():
        lps = T.logprobs(ref, seqs, plens, device)
    # The sampler's logprobs: close to the trainer's, not equal (ratio != 1).
    sampler = [(lp + 0.05 * torch.randn(lp.shape, generator=g)).tolist() for lp in lps]
    adv = torch.randn(n_seqs, generator=g).tolist()
    ntok = sum(len(s) - p for s, p in zip(seqs, plens))
    return dict(seqs=seqs, plens=plens, sampler_lp=sampler, adv=adv, ntok=ntok)


def reference_grads(snap, batch, device):
    """The same objective, unsharded, one process: sum over ALL sequences / ntok."""
    from transformers import AutoModelForCausalLM
    ref = AutoModelForCausalLM.from_pretrained(snap, torch_dtype=torch.float32)
    ref.config.use_cache = False
    loss = torch.zeros(())
    for j, lp in enumerate(T.logprobs(ref, batch["seqs"], batch["plens"], device)):
        ratio = torch.exp(lp - torch.tensor(batch["sampler_lp"][j]))
        a = batch["adv"][j]
        loss = loss - torch.minimum(ratio * a, ratio.clamp(1 - T.CLIP, 1 + T.CLIP) * a).sum()
    (loss / batch["ntok"]).backward()
    return ref, {n: p.grad.clone() for n, p in ref.named_parameters() if p.grad is not None}


def main():
    dist.init_process_group("gloo")
    rank, world = dist.get_rank(), dist.get_world_size()
    device = torch.device("cpu")
    if not os.environ.get("SELFTEST_NO_PATCH"):
        T.patch_moe_every_expert()

    tmp = [tempfile.mkdtemp(prefix="fsdp-selftest-") if rank == 0 else None]
    dist.broadcast_object_list(tmp, src=0)
    snap = tmp[0]
    if rank == 0:
        make_checkpoint(snap)
    dist.barrier()
    names = list(T.checkpoint_names(snap))
    ok = True

    def check(cond, msg):
        nonlocal ok
        if rank == 0:
            print(("PASS " if cond else "FAIL ") + msg, flush=True)
        ok = ok and cond

    # 1. sharded load round-trips the checkpoint exactly
    model = T.build_model(snap, device, param_dtype=torch.float32)
    loaded = dict(T.gather_bf16(model, names))
    if rank == 0:
        from safetensors import safe_open
        files = T.checkpoint_names(snap)
        same = all(torch.equal(loaded[n], safe_open(os.path.join(snap, files[n]), "pt").get_tensor(n))
                   for n in names)
    check(rank != 0 or same, f"[world {world}] sharded load gathers back the checkpoint bit for bit")

    # 2 + 3. gradients vs the unsharded reference, for uneven slices and dummy passes
    for n_seqs, micro in ((7, 2), (3, 1), (1, 1)):
        T.MICRO = micro
        plain = _plain(snap)
        batch = batch_for(plain, n_seqs, device)
        _, want = reference_grads(snap, batch, device)
        model.zero_grad(set_to_none=True)
        mm = T.grpo_backward(model, batch, device)
        # The fake sampler is the reference logprob plus N(0, 0.05^2) noise, so
        # the token-mean k3 = E[e^d - d - 1] should be near sigma^2/2 = 1.25e-3.
        check(mm["tokens"] == batch["ntok"] and 3e-4 < mm["k3"] < 4e-3 and 0 < mm["dmax"] < 1,
              f"[world {world}, {n_seqs} seqs, micro {micro}] mismatch stats: k3 {mm['k3']:.2e}, "
              f"k1 {mm['k1']:+.2e}, clip {mm['clip']:.2%}, max|d| {mm['dmax']:.3f}, tokens {mm['tokens']}")
        worst, missing = 0.0, []
        for n, p in model.named_parameters():
            if p.grad is None:
                missing.append(n)
                continue
            g = p.grad.full_tensor() if isinstance(p.grad, DTensor) else p.grad
            w = want.get(n, torch.zeros_like(g))
            worst = max(worst, (g - w).abs().max().item() / (w.abs().max().item() + 1e-12))
        check(not missing, f"[world {world}, {n_seqs} seqs, micro {micro}] every parameter has a gradient "
                           f"(missing: {missing[:3]})")
        check(worst < 1e-4, f"[world {world}, {n_seqs} seqs, micro {micro}] gradients match the unsharded "
                            f"reference (worst relative error {worst:.2e})")

    # 4. a step, then the gather: checkpoint names and shapes, values moved
    opt = torch.optim.AdamW(model.parameters(), lr=1e-3, weight_decay=0.0)
    norm = T.grad_norm(model)
    opt.step()
    after = T.gather_bf16(model, names)
    if rank == 0:
        shapes_ok = [n for n, _ in after] == names and all(t.shape == loaded[n].shape for n, t in after)
        moved = sum(not torch.equal(t, loaded[n]) for n, t in after)
    check(bool(torch.isfinite(norm)), f"[world {world}] global grad norm is finite ({norm.item():.4f})")
    check(rank != 0 or shapes_ok, f"[world {world}] gathered weights keep the checkpoint's names and shapes")
    check(rank != 0 or moved > len(names) // 2,
          f"[world {world}] the step changed the weights ({moved if rank == 0 else '?'} of {len(names)} tensors)")

    flag = torch.tensor([0 if ok else 1])
    dist.all_reduce(flag)
    if rank == 0:
        print("SELFTEST", "OK" if flag.item() == 0 else "FAILED", flush=True)
    dist.destroy_process_group()
    sys.exit(int(flag.item() != 0))


def _plain(snap):
    from transformers import AutoModelForCausalLM
    m = AutoModelForCausalLM.from_pretrained(snap, torch_dtype=torch.float32)
    m.config.use_cache = False
    return m


if __name__ == "__main__":
    main()
