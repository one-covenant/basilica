#!/usr/bin/env python3
"""GRPO for a large MoE model (Qwen3-30B-A3B) through a Basilica BYOT session,
with the trainer sharded across the GPUs of one box by FSDP2.

The loop is the same as byot_grpo_minimal.py: sample through the session ->
score -> PPO-clip update against the sampler's own logprobs -> publish the new
weights as a sparse patch. What changes at 30B:

- Memory. fp32 master weights + AdamW state + fp32 grads are ~16 bytes per
  parameter, ~490 GB for 30.5B parameters. FSDP2 shards all of it across the
  GPUs (8x H100 80GB: ~61 GB each), and computes in bf16.
- Loading. The model is built on the meta device (no memory) and every rank
  copies ONLY its own shard of each tensor out of the safetensors files.
- One driver. Rank 0 talks to Basilica (sampling, publishing); the sampled
  batch is broadcast and every rank trains on its slice of the sequences.
- Publishing. Weights are gathered to rank 0 one tensor at a time and cast to
  bf16 under the checkpoint's own names (~61 GB of host RAM on rank 0, plus
  what the publisher keeps to compute the next sparse patch).
- MoE routing. Different ranks route tokens to different experts. FSDP2
  reduce-scatters the gradients of a layer's parameters that got one, so an
  expert that saw tokens on one rank but not on another would desynchronise
  that collective. patch_moe_every_expert() makes every expert run on every
  forward (on an empty batch when no token picked it), so every expert
  parameter always has a gradient, zero when unused.

    pip install "basilica-sdk[publisher]>=0.36.1" "transformers>=4.51,<5" datasets
    export BASILICA_STAGING_API_KEY=... BYOT_BUCKET=... BYOT_ENDPOINT=... \
           BYOT_ACCESS_KEY_ID=... BYOT_SECRET_ACCESS_KEY=...
    torchrun --nproc-per-node 8 byot_grpo_fsdp.py

transformers is pinned below 5 because v5 fuses expert weights into tensors
whose names no longer match the checkpoint; the publisher must emit the
checkpoint's names, which is what the fleet's vLLM loads.
"""
import datetime, hashlib, json, math, os, re, secrets, statistics, time

os.environ.setdefault("HF_HUB_DISABLE_XET", "1")  # plain HTTPS downloads: Xet can stall

import torch
import torch.distributed as dist
from torch.distributed.device_mesh import init_device_mesh
from torch.distributed.fsdp import CPUOffloadPolicy, MixedPrecisionPolicy, OffloadPolicy, fully_shard
from torch.distributed.tensor import DTensor, distribute_tensor

MODEL = os.environ.get("BYOT_MODEL", "Qwen/Qwen3-30B-A3B")
STEPS = int(os.environ.get("STEPS", os.environ.get("BYOT_TRAIN_STEPS", "20")))
PROMPTS = int(os.environ.get("BYOT_PROMPTS", "8"))  # prompts per step
N, CLIP = 8, 0.2  # 8 samples per prompt (the GRPO group), PPO clip range
LR = float(os.environ.get("BYOT_LR", "1e-6"))
TASK_NAME = os.environ.get("BYOT_TASK", "gsm8k")  # gsm8k | countdown (see TASKS)
MICRO = int(os.environ.get("BYOT_MICRO_BATCH", "2"))  # sequences per forward, per rank
MAX_TOKENS = int(os.environ.get("BYOT_MAX_TOKENS", "384"))
# Fixed evaluation: the same held-out problems, greedy, on the anchor, every
# EVAL_EVERY steps and on the last step. Per-step training reward scores a
# different batch each step, so only this shows whether the model learns.
EVAL_PROMPTS = int(os.environ.get("BYOT_EVAL_PROMPTS", "128"))
EVAL_EVERY = int(os.environ.get("BYOT_EVAL_EVERY", "5"))  # 0: no evaluation
# Train/inference mismatch: every K3_SYNC_EVERY steps the batch is sampled at
# the trainer's CURRENT weights (wait for the latest revision, assert it), so
# its k3 measures numerics alone, with no staleness mixed in. 0: never.
K3_SYNC_EVERY = int(os.environ.get("BYOT_K3_SYNC_EVERY", "5"))
# Record every batch (token ids, sampler logprobs, rewards, advantages, served
# revision) to s3://<bucket>/byot-recordings/<policy>/ for offline replay.
RECORD = os.environ.get("BYOT_RECORD") == "1"
SESSION_GPU = os.environ.get("BYOT_SESSION_GPU", "H200")
SESSION_GPUS = int(os.environ.get("BYOT_SESSION_GPUS", "1"))  # tensor parallel per replica
SESSION_MEMORY_GIB = int(os.environ.get("BYOT_SESSION_MEMORY_GIB", "128"))  # per replica pod
SESSION_CPU_CORES = int(os.environ.get("BYOT_SESSION_CPU_CORES", "16"))
# A 61 GB model needs an 80 GB-class GPU (A100 also comes in 40 GB).
SESSION_MIN_GPU_GB = int(os.environ.get("BYOT_SESSION_MIN_GPU_GB", "80"))
REPLICAS = int(os.environ.get("BYOT_REPLICAS", "1"))
CPU_OFFLOAD = os.environ.get("BYOT_CPU_OFFLOAD") == "1"  # fewer/smaller GPUs: optimizer on host
ASK = "\nSolve step by step. Put the final numeric answer on the last line as '#### <number>'."


# ----------------------------------------------------------------- the model

def patch_moe_every_expert():
    """Run every expert on every forward (see the module docstring)."""
    import torch.nn.functional as F
    from transformers.models.qwen3_moe import modeling_qwen3_moe as qm

    def forward(self, hidden_states):
        b, s, h = hidden_states.shape
        x = hidden_states.view(-1, h)
        router_logits = self.gate(x)
        weights = F.softmax(router_logits, dim=1, dtype=torch.float)
        weights, chosen = torch.topk(weights, self.top_k, dim=-1)
        if self.norm_topk_prob:
            weights /= weights.sum(dim=-1, keepdim=True)
        weights = weights.to(x.dtype)
        out = torch.zeros_like(x)
        mask = F.one_hot(chosen, num_classes=self.num_experts).permute(2, 1, 0)
        for e, expert in enumerate(self.experts):
            slot, token = torch.where(mask[e])  # empty when no token chose expert e
            y = expert(x[token]) * weights[token, slot, None]
            out.index_add_(0, token, y.to(x.dtype))
        return out.view(b, s, h), router_logits

    qm.Qwen3MoeSparseMoeBlock.forward = forward


def patch_old_sdk_upload():
    """SDKs up to 0.36.3 upload with boto3's 8 MiB parts (~7,300 for a 61 GB
    anchor) and a 60 s read timeout: R2 takes over a minute to complete that
    many parts, botocore retries the completion that already landed, and the
    publish fails with NoSuchUpload. Newer SDKs fix this (UPLOAD_PART_SIZE);
    for older ones, upload with 64 MiB parts and a 900 s read timeout."""
    import basilica.publisher as pub
    if hasattr(pub, "UPLOAD_PART_SIZE"):
        return
    from boto3.s3.transfer import TransferConfig
    from botocore.config import Config
    part = 64 * 2**20
    orig_client = pub.RlPolicyHandle._s3_client

    def _s3_client(self):
        if self._s3 is None:
            c = orig_client(self)
            self._s3 = pub._boto3().client(
                "s3", endpoint_url=self._storage.endpoint, region_name=self._storage.region,
                aws_access_key_id=self._storage.access_key_id,
                aws_secret_access_key=self._storage.secret_access_key,
                config=c.meta.config.merge(Config(read_timeout=900)))
        return self._s3

    def _upload(self, key, path):
        self._s3_client().upload_file(str(path), self._storage.bucket, key,
                                      Config=TransferConfig(multipart_threshold=part, multipart_chunksize=part))
        return f"s3://{self._storage.bucket}/{key}"

    pub.RlPolicyHandle._s3_client = _s3_client
    pub.RlPolicyHandle._upload = _upload
    print("SDK upload patched: 64 MiB parts, 900 s read timeout", flush=True)


def checkpoint_names(snap):
    """Tensor name -> safetensors file, straight from the checkpoint index."""
    index = os.path.join(snap, "model.safetensors.index.json")
    if os.path.exists(index):
        return json.load(open(index))["weight_map"]
    from safetensors import safe_open
    with safe_open(os.path.join(snap, "model.safetensors"), "pt") as f:
        return {n: "model.safetensors" for n in f.keys()}


def build_model(snap, device, param_dtype=torch.bfloat16, cpu_offload=False):
    """The model, FSDP2-sharded over all ranks, fp32 master weights loaded from
    the checkpoint shard by shard (no rank ever holds the full fp32 model)."""
    from safetensors import safe_open
    from transformers import AutoConfig, AutoModelForCausalLM

    config = AutoConfig.from_pretrained(snap)
    with torch.device("meta"):
        model = AutoModelForCausalLM.from_config(config, torch_dtype=torch.float32)
    model.config.use_cache = False
    model.gradient_checkpointing_enable(gradient_checkpointing_kwargs={"use_reentrant": False})

    mesh = init_device_mesh(device.type, (dist.get_world_size(),))
    mp = MixedPrecisionPolicy(param_dtype=param_dtype, reduce_dtype=torch.float32)
    offload = CPUOffloadPolicy() if cpu_offload else OffloadPolicy()
    for layer in model.model.layers:  # one all-gather / reduce-scatter unit per decoder layer
        fully_shard(layer, mesh=mesh, mp_policy=mp, offload_policy=offload)
    fully_shard(model, mesh=mesh, mp_policy=mp, offload_policy=offload)
    model.to_empty(device="cpu" if cpu_offload else device)

    # Rotary tables are non-persistent buffers: not in the checkpoint, so
    # recompute what to_empty left uninitialised.
    for m in model.modules():
        if hasattr(m, "rope_init_fn"):
            inv_freq, m.attention_scaling = m.rope_init_fn(m.config, device)
            m.inv_freq = inv_freq
            m.original_inv_freq = inv_freq

    files = checkpoint_names(snap)
    params = dict(model.named_parameters())
    missing = sorted(set(params) - set(files))
    if missing:
        raise SystemExit(f"model parameters not in the checkpoint: {missing[:5]}")
    handles = {}
    with torch.no_grad():
        for name, p in params.items():
            f = handles.get(files[name]) or handles.setdefault(
                files[name], safe_open(os.path.join(snap, files[name]), "pt"))
            full = f.get_tensor(name).to(device, torch.float32)
            # src_data_rank=None: every rank slices its own shard locally.
            shard = distribute_tensor(full, p.device_mesh, p.placements, src_data_rank=None)
            p.to_local().copy_(shard.to_local())
            del full, shard
    return model


def gather_bf16(model, names):
    """The checkpoint's tensors as the fleet serves them (bf16), on rank 0.
    Every rank must call this: each tensor is one all-gather."""
    params = dict(model.named_parameters())
    rank0 = dist.get_rank() == 0
    # The device the mesh shards across (the GPU in training, CPU in the self-test).
    device = torch.device("cuda", torch.cuda.current_device()) if torch.cuda.is_available() \
        else torch.device("cpu")
    out = []
    with torch.no_grad():
        for n in names:
            p = params[n]
            if isinstance(p, DTensor):
                # Cast the local shard to bf16 on this rank's GPU, then gather
                # over NCCL: half the bytes of an fp32 gather, and with CPU
                # offload it avoids a gloo all-gather over host memory (638 s
                # for Qwen3-30B-A3B). Casting before the gather gives the same
                # bf16 values as casting after.
                local = p.to_local().to(device, torch.bfloat16)
                full = DTensor.from_local(local, p.device_mesh, p.placements, run_check=False,
                                          shape=p.size(), stride=p.stride()).full_tensor()
            else:
                full = p
            if rank0:
                out.append((n, full.to(torch.bfloat16).cpu()))
            del full
    return out


# ----------------------------------------------------------------- one update

def logprobs(model, seqs, plens, device):
    """The trainer's logprob of each completion token. Right-padded with NO
    attention mask: under causal attention real tokens never see the padding
    after them, and a padding mask can make bf16 attention gradients NaN."""
    width = max(len(s) for s in seqs)
    ids = torch.tensor([s + [0] * (width - len(s)) for s in seqs], device=device)
    logits = model(input_ids=ids).logits
    return [torch.log_softmax(logits[i, p - 1:len(s) - 1].float(), -1)
            .gather(-1, ids[i, p:len(s), None]).squeeze(-1) for i, (s, p) in enumerate(zip(seqs, plens))]


def grpo_backward(model, batch, device):
    """PPO-clip loss over this rank's slice of the batch, backpropagated.

    Every rank must run the SAME number of forward/backward passes (each one
    all-gathers and reduce-scatters every layer), so a rank that runs out of
    sequences does a zero-weighted pass on a dummy sequence. The loss is
    scaled by world size because FSDP2 averages gradients across ranks while
    the GRPO objective is a sum over all sequences / all completion tokens.
    """
    rank, world = dist.get_rank(), dist.get_world_size()
    seqs, plens = batch["seqs"], batch["plens"]
    # Train/inference mismatch over valid completion tokens, with
    # d = trainer logprob - sampler logprob (Fireworks' convention):
    # sums of d (k1), of e^d - d - 1 (k3, estimates KL(sampler || trainer)),
    # the token count and tokens outside the clip band; plus max |d|.
    stats = torch.zeros(4, device=device, dtype=torch.float64)
    dmax = torch.zeros((), device=device, dtype=torch.float64)
    mine = list(range(rank, len(seqs), world))
    passes = math.ceil(math.ceil(len(seqs) / world) / MICRO)
    scale = world / batch["ntok"]
    for m in range(passes):
        idx = mine[m * MICRO:(m + 1) * MICRO]
        if not idx:  # keep the collectives in step
            lp = logprobs(model, [[0, 0]], [1], device)[0]
            (lp.sum() * 0.0).backward()
            continue
        loss = torch.zeros((), device=device)
        for j, lp in zip(idx, logprobs(model, [seqs[i] for i in idx], [plens[i] for i in idx], device)):
            old = torch.tensor(batch["sampler_lp"][j], device=device)
            # No gradient where the sampler's logprob is non-finite or at
            # vLLM's -9999 floor (exp would overflow; inf * 0 is NaN), and a
            # clamped log-ratio, as PPO trainers do.
            bad = ~torch.isfinite(old) | (old < -1e3)
            ratio = torch.exp((lp - old).masked_fill(bad, 0.0).clamp(-20.0, 20.0))
            with torch.no_grad():
                d = (lp.detach() - old)[~bad].double()
                if d.numel():
                    stats += torch.stack([d.sum(), (torch.exp(d) - d - 1).sum(),
                                          torch.tensor(float(d.numel()), device=device, dtype=torch.float64),
                                          ((d.exp() < 1 - CLIP) | (d.exp() > 1 + CLIP)).sum().double()])
                    dmax = torch.maximum(dmax, d.abs().max())
            a = batch["adv"][j]
            loss = loss - torch.minimum(ratio * a, ratio.clamp(1 - CLIP, 1 + CLIP) * a).sum()
        (loss * scale).backward()
    dist.all_reduce(stats, op=dist.ReduceOp.SUM)
    dist.all_reduce(dmax, op=dist.ReduceOp.MAX)
    n = max(stats[2].item(), 1.0)
    return {"k1": stats[0].item() / n, "k3": stats[1].item() / n,
            "clip": stats[3].item() / n, "dmax": dmax.item(), "tokens": int(stats[2].item())}


def grad_norm(model):
    norm = torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
    return norm.full_tensor() if isinstance(norm, DTensor) else norm


def trainer_only(model, opt, names, device):
    """BYOT_TRAINER_ONLY=1: no Basilica at all. Full-length synthetic batches
    (random tokens, the real batch shape) measure step time and peak GPU
    memory, then one bf16 gather measures what a publish would cost."""
    import resource
    rank = dist.get_rank()
    g = torch.Generator().manual_seed(0)
    vocab = model.config.vocab_size
    for step in range(1, STEPS + 1):
        box = [None]
        if rank == 0:
            seqs, plens, lps, adv = [], [], [], []
            for _ in range(PROMPTS * N):
                plen, comp = 120, int(torch.randint(MAX_TOKENS // 2, MAX_TOKENS + 1, (1,), generator=g))
                seqs.append(torch.randint(0, vocab, (plen + comp,), generator=g).tolist())
                plens.append(plen)
                lps.append((-3 * torch.rand(comp, generator=g)).tolist())
                adv.append(float(torch.randn(1, generator=g)))
            box[0] = dict(seqs=seqs, plens=plens, sampler_lp=lps, adv=adv,
                          ntok=sum(len(s) - p for s, p in zip(seqs, plens)))
        dist.broadcast_object_list(box, src=0)
        batch = box[0]
        torch.cuda.reset_peak_memory_stats(device)
        torch.cuda.synchronize()
        t = time.time()
        opt.zero_grad(set_to_none=True)
        grpo_backward(model, batch, device)
        gnorm = grad_norm(model)
        if torch.isfinite(gnorm):
            opt.step()
        torch.cuda.synchronize()
        peak = torch.tensor([torch.cuda.max_memory_allocated(device) / 2**30], device=device)
        dist.all_reduce(peak, op=dist.ReduceOp.MAX)
        say(f"step {step:3d} trainer-only: fwd+bwd+optimizer {time.time() - t:.0f}s, "
            f"grad-norm {gnorm.item():.3f}, peak GPU memory {peak.item():.1f} GiB (max over ranks), "
            f"{batch['ntok']} completion tokens")
    t = time.time()
    weights = gather_bf16(model, names)
    if rank == 0:
        rss = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 2**20  # KiB on Linux
        say(f"step gather: {len(weights)} tensors, {sum(w.numel() for _, w in weights) * 2 / 1e9:.1f} GB bf16 "
            f"in {time.time() - t:.0f}s; rank 0 peak host RSS {rss:.0f} GiB")
    del weights


# ----------------------------------------------------------------- the loop

# ----------------------------------------------------------------- the tasks
# Each task: load() -> (train rows, eval rows), prompt(row) -> the user message,
# gold(row) -> the answer, reward(text, row) -> 0.0 / 1.0.

def gsm8k_reward(text, gold):
    """1 if the last '#### <number>' in the completion equals the answer."""
    found = re.findall(r"####\s*\$?(-?[\d,]*\.?\d+)", text)
    try:
        return float(bool(found) and math.isclose(float(found[-1].replace(",", "")), float(gold)))
    except ValueError:
        return 0.0


def gsm8k_load():
    from datasets import load_dataset
    train = load_dataset("openai/gsm8k", "main", split="train").shuffle(seed=0)
    test = load_dataset("openai/gsm8k", "main", split="test")
    return train, test.select(range(min(EVAL_PROMPTS, len(test))))


def gsm8k_gold(row):
    return row["answer"].split("####")[-1].strip().replace(",", "")


def length_stats(out, texts=None):
    """Completion length, the share cut off at MAX_TOKENS (finish_reason
    "length"), and with `texts` the share with no <answer> tag: a policy
    that grows more verbose loses reward to truncation, not to wrong maths."""
    lens = [len(ids) for ids in out.token_ids] or [0]
    cut = sum(1 for r in out.finish_reasons if r == "length") / max(1, len(out.finish_reasons))
    msg = f"mean {statistics.fmean(lens):.0f} tokens, max {max(lens)}, {cut:.0%} cut at {MAX_TOKENS}"
    if texts is not None and TASK_NAME == "countdown":
        no_tag = sum(1 for t in texts if "<answer>" not in t) / max(1, len(texts))
        msg += f", {no_tag:.0%} without <answer>"
    return msg


def countdown_reward(text, nums, target):
    """1 if the last <answer>...</answer> is an arithmetic expression that uses
    each of `nums` exactly once and evaluates to `target`. The expression may
    only contain digits, + - * / ( ) . and whitespace (no names, no '**'), so
    eval() cannot reach anything but integer arithmetic on the given numbers."""
    m = re.findall(r"<answer>(.*?)</answer>", text, re.S)
    if not m:
        return 0.0
    eq = " ".join(m[-1].split("=")[0].split())  # one line: eval() rejects bare newlines
    if not eq or len(eq) > 100 or "**" in eq or not re.fullmatch(r"[\d+\-*/().\s]+", eq):
        return 0.0
    if sorted(int(x) for x in re.findall(r"\d+", eq)) != sorted(nums):
        return 0.0
    try:
        v = eval(eq, {"__builtins__": {}}, {})  # noqa: S307 (input restricted above)
        return float(abs(v - target) < 1e-5)
    except Exception:  # noqa: BLE001 (ZeroDivisionError, OverflowError, SyntaxError, ...)
        return 0.0


def countdown_load():
    """Single train split: the LAST EVAL_PROMPTS rows are the fixed eval set,
    the rest (shuffled, seed 0) is the training stream."""
    from datasets import load_dataset
    ds = load_dataset("Jiayi-Pan/Countdown-Tasks-3to4", split="train")
    cut = len(ds) - EVAL_PROMPTS
    return ds.select(range(cut)).shuffle(seed=0), ds.select(range(cut, len(ds)))


TASKS = {
    "gsm8k": dict(
        load=gsm8k_load,
        prompt=lambda row: row["question"] + ASK,
        gold=gsm8k_gold,
        reward=lambda text, row: gsm8k_reward(text, gsm8k_gold(row))),
    "countdown": dict(
        load=countdown_load,
        prompt=lambda row: (f"Using the numbers {list(row['nums'])}, create an equation that equals "
                            f"{row['target']}. You can use + - * / and each number exactly once. "
                            "Show your work, then give only the equation inside <answer></answer>."),
        gold=lambda row: row["target"],
        reward=lambda text, row: countdown_reward(text, list(row["nums"]), row["target"])),
}
if TASK_NAME not in TASKS:
    raise SystemExit(f"BYOT_TASK={TASK_NAME!r}: expected one of {', '.join(TASKS)}")
TASK = TASKS[TASK_NAME]


def say(*a):
    if dist.get_rank() == 0:
        print(*a, flush=True)


def main():
    from huggingface_hub import snapshot_download

    # The checkpoint (~61 GB) plus the publisher's work dir (an anchor is the
    # full bf16 state again) must fit; fail now, not an hour in. Every rank
    # checks before the process group exists, so they all stop together.
    import shutil
    # The publisher's work dir holds a full bf16 anchor (~61 GB) before it is
    # uploaded. Put it in /dev/shm when the box has the RAM for it, so the
    # disk only has to hold the checkpoint download.
    publish_dir = os.environ.get("BYOT_PUBLISH_DIR", "")
    if not publish_dir:
        shm_free = shutil.disk_usage("/dev/shm").free / 1e9 if os.path.isdir("/dev/shm") else 0
        publish_dir = "/dev/shm/byot-publish" if shm_free > 150 else "./publish"
    on_disk = not publish_dir.startswith("/dev/shm")
    need = float(os.environ.get("BYOT_MIN_FREE_GB", "150" if on_disk else "70"))
    free = shutil.disk_usage(os.path.expanduser("~")).free / 1e9
    if free < need:
        raise SystemExit(f"only {free:.0f} GB free on the trainer, need ~{need:.0f} GB "
                         "(set BYOT_MIN_FREE_GB to override)")

    # Rank 0 blocks the others during sampling and publishing (a 61 GB anchor
    # upload takes a while), so the collective timeout must cover it.
    # With CPU offload the sharded grads and master weights are CPU DTensors,
    # so clip_grad_norm_ and the publish gather run collectives on CPU
    # tensors, which NCCL cannot do: route those through gloo.
    backend = "cpu:gloo,cuda:nccl" if CPU_OFFLOAD else "nccl"
    dist.init_process_group(backend, timeout=datetime.timedelta(hours=3))
    rank, world = dist.get_rank(), dist.get_world_size()
    local = int(os.environ.get("LOCAL_RANK", "0"))
    torch.cuda.set_device(local)
    if CPU_OFFLOAD:
        # torchrun pins every rank to OMP_NUM_THREADS=1, but with offload the
        # grad accumulation and AdamW run on the host: give each rank its
        # share of the cores (BYOT_OFFLOAD_THREADS overrides).
        per_rank = max(1, (os.cpu_count() or 8) // int(os.environ.get("LOCAL_WORLD_SIZE", "1")) - 1)
        torch.set_num_threads(int(os.environ.get("BYOT_OFFLOAD_THREADS", per_rank)))
    device = torch.device("cuda", local)
    patch_moe_every_expert()

    patterns = ["*.safetensors", "*.json", "tokenizer*", "*.txt"]
    status = [None]  # rank 0 downloads; a failure must stop every rank, not hang them
    if rank == 0:
        try:
            snapshot_download(MODEL, allow_patterns=patterns)  # ~61 GB the first time
            status[0] = "ok"
        except Exception as e:  # noqa: BLE001 (reported on every rank below)
            status[0] = f"{type(e).__name__}: {e}"
    dist.broadcast_object_list(status, src=0)
    if status[0] != "ok":
        raise SystemExit(f"rank {rank}: downloading {MODEL} failed: {status[0]}")
    snap = snapshot_download(MODEL, allow_patterns=patterns)
    names = list(checkpoint_names(snap))

    t0 = time.time()
    model = build_model(snap, device, cpu_offload=CPU_OFFLOAD)
    opt = torch.optim.AdamW(model.parameters(), lr=LR, weight_decay=0.0)
    say(f"loaded {MODEL} over {world} GPUs in {time.time() - t0:.0f}s; "
        f"{torch.cuda.memory_allocated(device) / 2**30:.1f} GiB on GPU 0; "
        f"task {TASK_NAME}, lr {LR:g}, {PROMPTS} prompts x {N} samples per step")

    if os.environ.get("BYOT_TRAINER_ONLY") == "1":
        trainer_only(model, opt, names, device)
        dist.destroy_process_group()
        return

    # Everything that talks to Basilica lives on rank 0.
    if rank == 0:
        from transformers import AutoTokenizer
        from basilica import BasilicaClient
        from basilica.publisher import NonFiniteWeights, PolicyStorage
        patch_old_sdk_upload()

        tok = AutoTokenizer.from_pretrained(snap)
        data, evalset = TASK["load"]()
        if not EVAL_EVERY:
            evalset = None

        def encode(rows):
            """The chat-formatted prompt token ids of `rows`, and the rows as dicts."""
            rows = [dict(r) for r in rows]
            prompts = [tok(tok.apply_chat_template([{"role": "user", "content": TASK["prompt"](r)}],
                                                   add_generation_prompt=True, tokenize=False,
                                                   enable_thinking=False),
                           add_special_tokens=False)["input_ids"] for r in rows]
            return prompts, rows
        bucket = dict(bucket=os.environ["BYOT_BUCKET"], endpoint=os.environ["BYOT_ENDPOINT"],
                      access_key_id=os.environ["BYOT_ACCESS_KEY_ID"],
                      secret_access_key=os.environ["BYOT_SECRET_ACCESS_KEY"])
        rl = BasilicaClient(base_url=os.environ.get("BASILICA_API_URL", "https://api-staging.basilica.ai"),
                            api_key=os.environ["BASILICA_STAGING_API_KEY"]).rl
        def record_batch(step, lag, mm, gnorm, batch):
            """One gzipped JSON per step under byot-recordings/<policy>/: enough to
            replay the run offline against the policy's anchor + patch chain."""
            import gzip
            import boto3
            from botocore.config import Config
            s3 = ctx.get("rec_s3") or ctx.setdefault("rec_s3", boto3.client(
                "s3", endpoint_url=bucket["endpoint"], region_name="auto",
                aws_access_key_id=bucket["access_key_id"], aws_secret_access_key=bucket["secret_access_key"],
                config=Config(request_checksum_calculation="when_required",
                              response_checksum_validation="when_required")))
            base = f"byot-recordings/{policy}"
            if "rec_meta" not in ctx:
                pol = rl.get_policy(policy)
                ctx["rec_meta"] = meta = dict(
                    policy=policy, policy_uid=pol.get("policyUid"), prefix=pol.get("effectivePrefix"),
                    model=MODEL, task=TASK_NAME, lr=LR, prompts=PROMPTS, n=N, clip=CLIP,
                    max_tokens=MAX_TOKENS, k3_sync_every=K3_SYNC_EVERY, started=time.time())
                s3.put_object(Bucket=bucket["bucket"], Key=f"{base}/meta.json",
                              Body=json.dumps(meta).encode())
            rec = dict(step=step, lag=lag, served=batch["served"], synced=batch["synced"],
                       mismatch=mm, grad_norm=gnorm, reward=batch["reward"], rewards=batch["rewards"],
                       adv=batch["adv"], plens=batch["plens"], seqs=batch["seqs"],
                       sampler_lp=batch["sampler_lp"])
            s3.put_object(Bucket=bucket["bucket"], Key=f"{base}/batches/step-{step:04d}.json.gz",
                          Body=gzip.compress(json.dumps(rec).encode(), 3))

        policy = os.environ.get("BYOT_POLICY") or "byot-grpo-" + secrets.token_hex(3)
    ctx = {}  # rank 0's Basilica state: session info, publisher handle, open session

    def rank0(fn):
        """Run fn on rank 0 and share its result (or its failure) with every
        rank, so a Basilica error stops all ranks instead of hanging them."""
        box = [None]
        if rank == 0:
            try:
                box[0] = ("ok", fn())
            except BaseException as e:  # noqa: BLE001 (re-raised below on every rank)
                box[0] = ("err", f"{type(e).__name__}: {e}")
        dist.broadcast_object_list(box, src=0)
        if box[0][0] == "err":
            raise SystemExit(f"rank {rank}: stopping, rank 0 failed: {box[0][1]}")
        return box[0][1]

    try:
        def start():
            # A policy is the model lineage, registered against your bucket.
            rl.create_policy(policy, repo=MODEL, commit=os.path.basename(snap.rstrip("/")), **bucket,
                             tokenizer_digest="sha256:" + hashlib.sha256(
                                 open(f"{snap}/tokenizer.json", "rb").read()).hexdigest())
            # The rollout fleet: each replica serves the model tensor-parallel over SESSION_GPUS.
            s = ctx["session_info"] = rl.create_session(
                policy, gpu_model=SESSION_GPU, gpu_count=SESSION_GPUS, replicas=REPLICAS,
                memory_gib=SESSION_MEMORY_GIB, cpu_cores=SESSION_CPU_CORES,
                min_gpu_memory_gb=SESSION_MIN_GPU_GB)
            print(f"session {s['sessionUid']} starting: {REPLICAS} replica(s) x {SESSION_GPUS} {SESSION_GPU}, "
                  f"{SESSION_MEMORY_GIB} GiB, {SESSION_CPU_CORES} cores", flush=True)
            while rl.get_session(s["sessionUid"])["state"] not in ("ready", "active"):
                time.sleep(20)

        rank0(start)

        # The starting weights go out once in full (the anchor); every step after is a patch.
        weights = gather_bf16(model, names)

        def anchor():
            s = ctx["session_info"]
            handle = rl.policy(policy, storage=PolicyStorage(**bucket), work_dir=publish_dir)
            t = time.time()
            handle.publish_anchor(weights, revision="step-0000")
            print(f"anchor uploaded in {time.time() - t:.0f}s; waiting for the fleet to load it", flush=True)
            handle.wait_until_active("step-0000", timeout=4 * 3600)
            print(f"anchor Active {time.time() - t:.0f}s after publish", flush=True)
            ctx["handle"] = handle
            ctx["session"] = rl.open_session(s["url"], s["token"], publisher=handle,
                                             session_uid=s["sessionUid"])

        rank0(anchor)
        del weights

        def evaluate(rev):
            """Greedy accuracy of revision `rev` on the fixed test problems.
            Waits for `rev` to be Active, then asserts the session serves it."""
            if not EVAL_EVERY:
                return
            t = time.time()
            ctx["handle"].wait_until_active(rev, timeout=3600)
            prompts, rows = encode(evalset)
            out = ctx["session"].generate(token_ids=prompts, n=1, max_tokens=MAX_TOKENS,
                                          temperature=0.0, seed=0, revision=rev)
            texts = [tok.decode(ids, skip_special_tokens=True) for ids in out.token_ids]
            acc = statistics.fmean(TASK["reward"](txt, rows[i]) for i, txt in enumerate(texts))
            print(f"eval {rev}: accuracy {acc:.3f} on {len(rows)} fixed {TASK_NAME} problems "
                  f"(greedy, served {out.served_revision}, {time.time() - t:.0f}s); "
                  f"{length_stats(out, texts)}", flush=True)
            for txt in texts[:2]:  # two fixed problems, to see how answers change over training
                print(f"   eval {rev} sample: {txt[-300:]!r}", flush=True)

        rank0(lambda: evaluate("step-0000"))

        for step in range(1, STEPS + 1):
            def sample():
                prompts, rows = encode(data.select(range((step - 1) * PROMPTS, step * PROMPTS)))
                # Async: whatever revision the fleet serves right now answers, and
                # says which. Every K3_SYNC_EVERY steps, sample at the trainer's
                # current weights instead, so that batch's k3 is pure mismatch.
                sync = K3_SYNC_EVERY and step > 1 and step % K3_SYNC_EVERY == 0
                rev = f"step-{step - 1:04d}" if sync else None
                if sync:
                    ctx["handle"].wait_until_active(rev, timeout=3600)
                out = ctx["session"].generate(token_ids=prompts, n=N, max_tokens=MAX_TOKENS,
                                              temperature=1.0, seed=step, revision=rev)
                rewards = [TASK["reward"](tok.decode(ids, skip_special_tokens=True), rows[i // N])
                           for i, ids in enumerate(out.token_ids)]
                adv = []
                for g in range(PROMPTS):
                    group = rewards[g * N:(g + 1) * N]
                    mean, std = statistics.fmean(group), statistics.pstdev(group)
                    adv += [(r - mean) / (std + 1e-4) if std else 0.0 for r in group]
                seqs = [prompts[i // N] + list(ids) for i, ids in enumerate(out.token_ids)]
                return dict(seqs=seqs, plens=[len(prompts[i // N]) for i in range(len(seqs))],
                            sampler_lp=[list(lp) for lp in out.logprobs], adv=adv,
                            ntok=max(1, sum(len(ids) for ids in out.token_ids)),
                            reward=statistics.fmean(rewards), served=out.served_revision,
                            lengths=length_stats(out), rewards=rewards, synced=bool(sync))

            batch = rank0(sample)  # every rank gets the same batch and trains on its slice
            opt.zero_grad(set_to_none=True)
            mm = grpo_backward(model, batch, device)
            gnorm = grad_norm(model)  # global norm: every rank takes the same branch
            if not torch.isfinite(gnorm):  # never take (or publish) a non-finite step
                say(f"step {step:3d}  skipped: non-finite gradient")
                continue
            opt.step()

            weights = gather_bf16(model, names)

            def publish():
                t = time.time()
                try:
                    ctx["session"].publish(weights, revision=f"step-{step:04d}")
                except NonFiniteWeights as e:
                    raise SystemExit(f"step {step}: {e}")
                served = batch["served"] or ""
                lag = (step - 1) - int(served[5:]) if served.startswith("step-") else -1
                print(f"step {step:3d}  reward {batch['reward']:.3f}  grad-norm {gnorm.item():.3f}  "
                      f"publish {time.time() - t:.0f}s  (samples from {batch['served']}; "
                      f"{batch['lengths']})", flush=True)
                print(f"mismatch step {step:3d}  lag {lag}{' (synced)' if batch['synced'] else ''}  "
                      f"k3 {mm['k3']:.2e}  k1 {mm['k1']:+.2e}  clip {mm['clip']:.2%}  "
                      f"max|d| {mm['dmax']:.3f}  tokens {mm['tokens']}", flush=True)
                if RECORD:
                    record_batch(step, lag, mm, gnorm.item(), batch)

            rank0(publish)
            del weights
            if EVAL_EVERY and (step % EVAL_EVERY == 0 or step == STEPS):
                rank0(lambda: evaluate(f"step-{step:04d}"))

        rank0(lambda: print("usage:", ctx["session"].usage(), flush=True))
    finally:
        if rank == 0:
            uid = ctx.get("session_info", {}).get("sessionUid")
            if uid:
                rl.delete_session(uid)
            try:
                rl.delete_policy(policy)
            except Exception as e:  # noqa: BLE001 (never created, or already gone)
                print(f"delete_policy: {e}", flush=True)
            print(f"cleaned up: session {uid} and policy {policy} deleted", flush=True)
        dist.destroy_process_group()


if __name__ == "__main__":
    main()
