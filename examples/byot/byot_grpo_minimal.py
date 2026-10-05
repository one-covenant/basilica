#!/usr/bin/env python3
"""Minimal GRPO through a Basilica BYOT session: the training loop, nothing else.

Your trainer (this script, on a CUDA GPU) keeps fp32 master weights. Basilica
serves your rollouts from a private vLLM fleet that runs the weights you just
published. Each step: sample -> score -> PPO-clip update against the sampler's
own logprobs -> publish the new weights as a sparse patch.

    pip install "basilica-sdk[publisher]>=0.36.4" transformers datasets
    export BASILICA_STAGING_API_KEY=... BYOT_BUCKET=... BYOT_ENDPOINT=... \
           BYOT_ACCESS_KEY_ID=... BYOT_SECRET_ACCESS_KEY=...
    python byot_grpo_minimal.py            # STEPS=50 python ... for a longer run
"""
import glob, hashlib, math, os, re, secrets, statistics, time

os.environ.setdefault("HF_HUB_DISABLE_XET", "1")  # plain HTTPS downloads: Xet can stall

import torch
from datasets import load_dataset
from huggingface_hub import snapshot_download
from safetensors import safe_open
from transformers import AutoModelForCausalLM, AutoTokenizer
from basilica import BasilicaClient
from basilica.publisher import NonFiniteWeights, PolicyStorage

MODEL = "Qwen/Qwen2.5-1.5B-Instruct"
STEPS = int(os.environ.get("STEPS", os.environ.get("BYOT_TRAIN_STEPS", "20")))
PROMPTS, N, LR, CLIP = 8, 8, 1e-6, 0.2  # 8 prompts x 8 samples (the GRPO group) per step
ASK = "\nSolve step by step. Put the final numeric answer on the last line as '#### <number>'."

bucket = dict(bucket=os.environ["BYOT_BUCKET"], endpoint=os.environ["BYOT_ENDPOINT"],
              access_key_id=os.environ["BYOT_ACCESS_KEY_ID"],
              secret_access_key=os.environ["BYOT_SECRET_ACCESS_KEY"])
rl = BasilicaClient(base_url=os.environ.get("BASILICA_API_URL", "https://api-staging.basilica.ai"),
                    api_key=os.environ["BASILICA_STAGING_API_KEY"]).rl


def reward(text, gold):
    """1 if the last '#### <number>' in the completion equals the answer."""
    found = re.findall(r"####\s*\$?(-?[\d,]*\.?\d+)", text)
    try:
        return float(bool(found) and math.isclose(float(found[-1].replace(",", "")), float(gold)))
    except ValueError:
        return 0.0


def logprobs(model, seqs, plens):
    """The trainer's logprob of each completion token. Right-padded with NO
    attention mask: under causal attention real tokens never see the padding
    after them, and a padding mask can make bf16 attention gradients NaN."""
    width = max(len(s) for s in seqs)
    ids = torch.tensor([s + [0] * (width - len(s)) for s in seqs], device="cuda")
    with torch.autocast("cuda", dtype=torch.bfloat16):
        logits = model(input_ids=ids).logits
    return [torch.log_softmax(logits[i, p - 1:len(s) - 1].float(), -1)
            .gather(-1, ids[i, p:len(s), None]).squeeze(-1) for i, (s, p) in enumerate(zip(seqs, plens))]


snap = snapshot_download(MODEL, allow_patterns=["*.safetensors", "*.json", "tokenizer*", "*.txt"])
tok = AutoTokenizer.from_pretrained(snap)
names = [n for f in sorted(glob.glob(f"{snap}/*.safetensors")) for n in safe_open(f, "pt").keys()]
model = AutoModelForCausalLM.from_pretrained(snap, dtype=torch.float32).cuda()  # fp32 master weights
model.gradient_checkpointing_enable()
model.config.use_cache = False
opt = torch.optim.AdamW(model.parameters(), lr=LR, weight_decay=0.0)
data = load_dataset("openai/gsm8k", "main", split="train").shuffle(seed=0)


def weights():
    """What the fleet serves: the checkpoint's tensors, cast to bf16."""
    state = model.state_dict()
    return [(n, state[n].detach().to(torch.bfloat16).cpu()) for n in names]


# A policy is the model lineage, registered against your bucket.
policy = os.environ.get("BYOT_POLICY") or "grpo-" + secrets.token_hex(3)
rl.create_policy(policy, repo=MODEL, commit=os.path.basename(snap.rstrip("/")), **bucket,
                 tokenizer_digest="sha256:" + hashlib.sha256(open(f"{snap}/tokenizer.json", "rb").read()).hexdigest())
uid = None
try:
    # A private rollout fleet serving that policy: 2 replicas, one H100 each.
    s = rl.create_session(policy, gpu_model="H100", gpu_count=1, replicas=2)
    uid = s["sessionUid"]
    print(f"session {uid} starting (renting GPUs takes a few minutes)", flush=True)
    while rl.get_session(uid)["state"] not in ("ready", "active"):
        time.sleep(20)
    # The starting weights go out once in full (the anchor); every step after is a patch.
    handle = rl.policy(policy, storage=PolicyStorage(**bucket), work_dir="./publish")
    handle.publish_anchor(weights(), revision="step-0000")
    handle.wait_until_active("step-0000", timeout=3600)
    session = rl.open_session(s["url"], s["token"], publisher=handle, session_uid=uid)

    for step in range(1, STEPS + 1):
        rows = data.select(range((step - 1) * PROMPTS, step * PROMPTS))
        prompts = [tok(tok.apply_chat_template([{"role": "user", "content": q + ASK}],
                                               add_generation_prompt=True, tokenize=False),
                       add_special_tokens=False)["input_ids"] for q in rows["question"]]
        golds = [a.split("####")[-1].strip().replace(",", "") for a in rows["answer"]]

        # 1. Sample: N completions per prompt, token ids in and out. Async: whatever
        #    revision the fleet serves right now answers, and says which one it was.
        out = session.generate(token_ids=prompts, n=N, max_tokens=320, temperature=1.0, seed=step)

        # 2. Score, and normalise advantages within each prompt's group of N.
        rewards = [reward(tok.decode(ids, skip_special_tokens=True), golds[i // N])
                   for i, ids in enumerate(out.token_ids)]
        adv = []
        for g in range(PROMPTS):
            group = rewards[g * N:(g + 1) * N]
            mean, std = statistics.fmean(group), statistics.pstdev(group)
            adv += [(r - mean) / (std + 1e-4) if std else 0.0 for r in group]

        # 3. PPO-clip update against the SAMPLER's own logprobs: the importance
        #    ratio corrects for the samples coming from a slightly older revision.
        seqs = [prompts[i // N] + list(ids) for i, ids in enumerate(out.token_ids)]
        plens = [len(prompts[i // N]) for i in range(len(seqs))]
        ntok = sum(len(ids) for ids in out.token_ids)
        opt.zero_grad(set_to_none=True)
        for m in range(0, len(seqs), 8):
            loss = torch.zeros((), device="cuda")
            for j, lp in enumerate(logprobs(model, seqs[m:m + 8], plens[m:m + 8])):
                old = torch.tensor(out.logprobs[m + j], device="cuda")
                # No gradient where the sampler's logprob is non-finite or at
                # vLLM's -9999 floor (exp would overflow; inf * 0 is NaN), and
                # a clamped log-ratio, as PPO trainers do.
                bad = ~torch.isfinite(old) | (old < -1e3)
                ratio = torch.exp((lp - old).masked_fill(bad, 0.0).clamp(-20.0, 20.0))
                a = adv[m + j]
                loss = loss - torch.minimum(ratio * a, ratio.clamp(1 - CLIP, 1 + CLIP) * a).sum() / ntok
            loss.backward()
        gnorm = torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
        if not torch.isfinite(gnorm):  # never take (or publish) a non-finite step
            print(f"step {step:3d}  skipped: non-finite gradient", flush=True)
            continue
        opt.step()

        # 4. Publish: a sparse patch over the last published revision. The SDK
        #    refuses NaN/Inf weights, so bad weights never reach the fleet.
        try:
            session.publish(weights(), revision=f"step-{step:04d}")
        except NonFiniteWeights as e:
            raise SystemExit(f"step {step}: {e}")
        print(f"step {step:3d}  reward {statistics.fmean(rewards):.3f}  "
              f"(samples from {out.served_revision})", flush=True)

    # The usage ledger refreshes about once a minute, so the last step or two
    # may not be counted yet (its observedAt says how fresh the numbers are).
    print("usage:", session.usage(), flush=True)
finally:
    if uid:
        rl.delete_session(uid)
    rl.delete_policy(policy)
    print(f"cleaned up: session {uid} and policy {policy} deleted", flush=True)
