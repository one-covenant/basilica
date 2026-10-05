#!/usr/bin/env python3
"""Basilica BYOT demo trainer: a small but REAL GRPO loop through a BYOT session.

Run by byot-train.sh on a rented GPU; also runnable by hand on any CUDA box.

The trainer (this process, on its own GPU) keeps fp32 master weights and an
AdamW optimizer; the rollout fleet is a 2-replica BYOT session on staging.
Every step: sample GSM8K prompts x N through the session in the training
dialect, score them, take a PPO-clip step against the SAMPLER's logprobs,
and publish the new bf16 weights as a patch. Nothing is faked.

Measured per step: reward, which revision served the rollouts and how far
behind the trainer it was, publish time, the revision's size in R2, and the
trainer-vs-sampler logprob gap. Every SYNC_EVERY steps: wait for the latest
revision to go Active (timed), generate with it ASSERTED, and check the gap
is at the numerical floor. At the end: usage must match what the loop
generated, no revision may be Rejected, and the session is cleaned up.

Env: BASILICA_STAGING_API_KEY, BYOT_{BUCKET,ENDPOINT,ACCESS_KEY_ID,SECRET_ACCESS_KEY}.
"""
from __future__ import annotations

import json, math, os, random, re, secrets, statistics, time
from typing import NoReturn

os.environ.setdefault("HF_HUB_DISABLE_XET", "1")  # the Xet path stalls on some networks

import boto3
import torch
from datasets import load_dataset
from huggingface_hub import snapshot_download
from safetensors import safe_open
from transformers import AutoModelForCausalLM, AutoTokenizer

import basilica
from basilica.publisher import PolicyStorage
from basilica.session import SessionServingError

API = os.environ.get("BASILICA_API_URL", "https://api-staging.basilica.ai")
MODEL = os.environ.get("BYOT_TRAIN_MODEL", "Qwen/Qwen2.5-1.5B-Instruct")
STEPS = int(os.environ.get("BYOT_TRAIN_STEPS", "20"))
PROMPTS = int(os.environ.get("BYOT_TRAIN_PROMPTS", "8"))      # prompts per step
N = int(os.environ.get("BYOT_TRAIN_N", "8"))                   # GRPO group size
MAX_TOK = int(os.environ.get("BYOT_TRAIN_MAX_TOKENS", "320"))
LR = float(os.environ.get("BYOT_TRAIN_LR", "1e-6"))
CLIP = 0.2
SYNC_EVERY = int(os.environ.get("BYOT_TRAIN_SYNC_EVERY", "10"))
REPLICAS = int(os.environ.get("BYOT_TRAIN_REPLICAS", "2"))
MICRO = 8                                                 # sequences per fwd/bwd
OUT = os.path.expanduser("~/byot/metrics.jsonl")

R2 = dict(bucket=os.environ.get("BYOT_BUCKET", ""), endpoint=os.environ.get("BYOT_ENDPOINT", ""),
          access_key_id=os.environ.get("BYOT_ACCESS_KEY_ID", ""),
          secret_access_key=os.environ.get("BYOT_SECRET_ACCESS_KEY", ""))
T0 = time.time()


def log(msg: str) -> None:
    print(f"[{(time.time() - T0) / 60:6.1f}m] {msg}", flush=True)


def fail(msg: str) -> NoReturn:
    raise SystemExit(f"\nFAILED: {msg}")


def record(**kw) -> None:
    with open(OUT, "a") as f:
        f.write(json.dumps({"t": round(time.time() - T0, 1), **kw}) + "\n")


# -- data + reward --------------------------------------------------------------

INSTRUCTION = "\nSolve the problem step by step. Put the final numeric answer on the last line as '#### <number>'."


def gold_answer(ans: str) -> str:
    return ans.split("####")[-1].strip().replace(",", "")


def extract(text: str):
    m = re.findall(r"####\s*\$?(-?[\d,]*\.?\d+)", text)
    return m[-1].replace(",", "").rstrip(".") if m else None


def reward(text: str, gold: str) -> float:
    got = extract(text)
    if got is None:
        return 0.0
    try:
        return 1.0 if math.isclose(float(got), float(gold), rel_tol=0, abs_tol=1e-6) else 0.0
    except ValueError:
        return 0.0


def prompt_ids(tok, question: str) -> list:
    text = tok.apply_chat_template([{"role": "user", "content": question + INSTRUCTION}],
                                   add_generation_prompt=True, tokenize=False)
    return tok(text, add_special_tokens=False)["input_ids"]


# -- trainer-side logprobs --------------------------------------------------------

def token_logprobs(model, seqs, prompt_lens, device):
    """Per-token logprobs of each sequence's completion under `model`
    (bf16 autocast over fp32 master weights), as a list of 1-D tensors."""
    maxlen = max(len(s) for s in seqs)
    pad = 0
    ids = torch.full((len(seqs), maxlen), pad, dtype=torch.long, device=device)
    att = torch.zeros((len(seqs), maxlen), dtype=torch.long, device=device)
    for i, s in enumerate(seqs):
        ids[i, : len(s)] = torch.tensor(s, device=device)
        att[i, : len(s)] = 1
    # No attention_mask on purpose. Sequences are RIGHT-padded and attention
    # is causal, so no real token can attend to padding after it: the mask
    # is redundant for every position the loss reads (fp32 check: identical
    # to 2e-4). Passing it made PyTorch's fused SDPA attention produce NaN
    # gradients in bf16 on some padded batches (reproduced at step 22 of
    # a GRPO run; forward finite, backward NaN).
    with torch.autocast("cuda", dtype=torch.bfloat16):
        logits = model(input_ids=ids).logits
    out = []
    for i, s in enumerate(seqs):
        p = prompt_lens[i]
        lg = logits[i, p - 1 : len(s) - 1].float()
        tgt = ids[i, p : len(s)]
        out.append(torch.log_softmax(lg, dim=-1).gather(-1, tgt[:, None]).squeeze(-1))
    return out


# -- storage introspection --------------------------------------------------------

def revision_objects(s3, prefix: str, revision: str):
    resp = s3.list_objects_v2(Bucket=R2["bucket"], Prefix=f"{prefix}{revision}/")
    return [(o["Key"].rsplit("/", 1)[-1], o["Size"]) for o in resp.get("Contents", [])]


def main() -> None:
    missing = [k for k, v in R2.items() if not v] + (
        [] if os.environ.get("BASILICA_STAGING_API_KEY") else ["BASILICA_STAGING_API_KEY"])
    if missing:
        fail(f"missing settings: {missing}")
    random.seed(0)
    torch.manual_seed(0)
    client = basilica.BasilicaClient(base_url=API, api_key=os.environ["BASILICA_STAGING_API_KEY"])
    rl = client.rl
    s3 = boto3.client("s3", endpoint_url=R2["endpoint"], region_name="auto",
                      aws_access_key_id=R2["access_key_id"],
                      aws_secret_access_key=R2["secret_access_key"])
    open(OUT, "w").close()

    log(f"downloading {MODEL} + GSM8K")
    snap = snapshot_download(MODEL, allow_patterns=["*.safetensors", "*.json", "tokenizer*", "*.txt"])
    commit = os.path.basename(snap.rstrip("/"))
    import hashlib, glob
    tok_digest = "sha256:" + hashlib.sha256(open(f"{snap}/tokenizer.json", "rb").read()).hexdigest()
    ckpt_names = []
    for f in sorted(glob.glob(f"{snap}/*.safetensors")):
        with safe_open(f, "pt") as sf:
            ckpt_names += list(sf.keys())
    data = load_dataset("openai/gsm8k", "main", split="train")
    tok = AutoTokenizer.from_pretrained(snap)

    device = "cuda"
    model = AutoModelForCausalLM.from_pretrained(snap, dtype=torch.float32).to(device)
    model.gradient_checkpointing_enable()
    model.config.use_cache = False
    opt = torch.optim.AdamW(model.parameters(), lr=LR, betas=(0.9, 0.999), weight_decay=0.0)

    def bf16_state():
        sd = model.state_dict()
        return [(n, sd[n].detach().to(torch.bfloat16).cpu()) for n in ckpt_names]

    # The orchestrator names the policy, so it can clean up even if this
    # process dies without running its own cleanup.
    policy = os.environ.get("BYOT_POLICY") or "byot-train-" + secrets.token_hex(3)
    uid = None
    usage_expected = {"requests": 0, "promptTokens": 0, "completionTokens": 0}
    published = {}  # revision -> step
    skipped = []    # steps refused for a non-finite gradient
    try:
        log(f"policy {policy} + {REPLICAS}-replica session")
        rl.create_policy(policy, repo=MODEL, commit=commit, tokenizer_digest=tok_digest, **R2)
        sess = rl.create_session(policy, gpu_model="H100", gpu_count=1, replicas=REPLICAS)
        uid, url, token = sess["sessionUid"], sess["url"], sess["token"]
        log(f"session {uid}")
        t = time.time()
        while rl.get_session(uid).get("state") not in ("ready", "active"):
            if rl.get_session(uid).get("state") == "failed":
                fail("session failed to start")
            if time.time() - t > 3600:
                fail("session not serving after 60 min")
            time.sleep(20)
        log(f"session serving after {(time.time() - t) / 60:.1f} min")

        prefix = rl.get_policy(policy)["effectivePrefix"]
        handle = rl.policy(policy, storage=PolicyStorage(**R2), work_dir=os.path.expanduser("~/byot/publish"))
        t = time.time()
        handle.publish_anchor(bf16_state(), revision="step-0000")
        pub_s = time.time() - t
        published["step-0000"] = 0
        handle.wait_until_active("step-0000", timeout=3600)
        anchor_mb = sum(sz for _, sz in revision_objects(s3, prefix, "step-0000")) / 1e6
        log(f"anchor step-0000 ({anchor_mb:.0f} MB) published in {pub_s:.0f}s, "
            f"Active after {time.time() - t:.0f}s")
        session = rl.open_session(url, token, publisher=handle, session_uid=uid)

        def generate(batch, step, revision=None):
            for attempt in range(1, 6):
                try:
                    out = session.generate(token_ids=batch, n=N, max_tokens=MAX_TOK,
                                           temperature=1.0, seed=step, revision=revision)
                    break
                except SessionServingError as e:
                    log(f"   generate retry {attempt}: {e}")
                    time.sleep(10)
            else:
                fail("generation kept failing")
            usage_expected["requests"] += 1
            usage_expected["promptTokens"] += int(out.usage.get("prompt_tokens", 0))
            usage_expected["completionTokens"] += int(out.usage.get("completion_tokens", 0))
            return out

        order = list(range(len(data)))
        random.shuffle(order)
        cursor = 0
        for step in range(1, STEPS + 1):
            rows = [data[order[(cursor + i) % len(order)]] for i in range(PROMPTS)]
            cursor += PROMPTS
            batch = [prompt_ids(tok, r["question"]) for r in rows]
            golds = [gold_answer(r["answer"]) for r in rows]
            sync = step % SYNC_EVERY == 0
            t_gen = time.time()
            if sync:
                latest = max(published, key=published.get)
                t_act = time.time()
                handle.wait_until_active(latest, timeout=3600)
                active_wait = time.time() - t_act
                out = generate(batch, step, revision=latest)   # ASSERTED: trainer == sampler
            else:
                out = generate(batch, step)
            gen_s = time.time() - t_gen
            served = out.served_revision
            lag = (step - 1) - published.get(served, -999)

            # One sample per choice, prompt-major (len == PROMPTS * N).
            if len(out.token_ids) != PROMPTS * N:
                fail(f"expected {PROMPTS * N} choices, got {len(out.token_ids)}")
            seqs, plens, old_lp, rewards = [], [], [], []
            for i, (ids, lps) in enumerate(zip(out.token_ids, out.logprobs)):
                p = batch[i // N]
                if out.prompt_token_ids[i] is not None and list(out.prompt_token_ids[i]) != list(p):
                    fail(f"choice {i} is not for prompt {i // N} (order assumption broken)")
                if len(ids) != len(lps):
                    fail(f"choice {i}: {len(ids)} tokens but {len(lps)} logprobs")
                seqs.append(list(p) + list(ids))
                plens.append(len(p))
                old_lp.append(torch.tensor(lps, dtype=torch.float32, device=device))
                rewards.append(reward(tok.decode(ids, skip_special_tokens=True), golds[i // N]))
            adv = []
            for g in range(PROMPTS):
                grp = rewards[g * N : (g + 1) * N]
                mu, sd = statistics.fmean(grp), statistics.pstdev(grp)
                adv += [(r - mu) / (sd + 1e-4) if sd > 0 else 0.0 for r in grp]

            # PPO-clip against the sampler's logprobs, token-level mean.
            t_train = time.time()
            ntok = sum(len(s) - p for s, p in zip(seqs, plens))
            gaps, ratios = [], []
            # Forensics for a non-finite step: what the inputs looked like.
            bad_tok, trainer_nf, max_d, loss_nf = 0, 0, 0.0, []
            opt.zero_grad(set_to_none=True)
            for m in range(0, len(seqs), MICRO):
                sl = slice(m, m + MICRO)
                new_lp = token_logprobs(model, seqs[sl], plens[sl], device)
                loss = torch.zeros((), device=device)
                for j, lp in enumerate(new_lp):
                    i = m + j
                    # A sampler logprob that is non-finite, or vLLM's -9999
                    # floor, would make exp(lp - old) overflow to inf, and
                    # inf * 0 (a flat group's advantage) is NaN. Such tokens
                    # get no gradient; the log-ratio is also clamped, as
                    # PPO trainers such as verl do.
                    bad = ~torch.isfinite(old_lp[i]) | (old_lp[i] < -1e3)
                    raw = lp - old_lp[i]
                    bad_tok += int(bad.sum())
                    trainer_nf += int((~torch.isfinite(lp)).sum())
                    good = raw.detach()[~bad]
                    if good.numel():
                        max_d = max(max_d, float(good.abs().max()))
                        gaps.append(float(good.abs().mean()))
                    d = raw.masked_fill(bad, 0.0).clamp(-20.0, 20.0)
                    ratio = torch.exp(d)
                    ratios.append(ratio.detach().mean().item())
                    a = adv[i]
                    obj = torch.minimum(ratio * a, torch.clamp(ratio, 1 - CLIP, 1 + CLIP) * a)
                    loss = loss - obj.sum() / ntok
                if not torch.isfinite(loss):
                    loss_nf.append(m // MICRO)
                loss.backward()
            gnorm = torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0).item()
            if not math.isfinite(gnorm):
                # Defence in depth: never take (or publish) a non-finite step.
                opt.zero_grad(set_to_none=True)
                skipped.append(step)
                log(f"step {step:3d} SKIPPED: non-finite gradient norm ({gnorm}); weights unchanged, nothing published")
                log(f"   forensics: sampler tokens non-finite or floored {bad_tok}, "
                    f"trainer logprobs non-finite {trainer_nf}, max |log-ratio| {max_d:.2f}, "
                    f"micro-batches with a non-finite loss {loss_nf or 'none'}, "
                    f"flat groups {sum(1 for g in range(PROMPTS) if statistics.pstdev(rewards[g * N:(g + 1) * N]) == 0)}/{PROMPTS}")
                record(step=step, skipped=True, reward=statistics.fmean(rewards), served=served, lag=lag,
                       sync=sync, active_wait=None, gen_s=round(gen_s, 1), train_s=0, pub_s=0,
                       rev_bytes=0, kind="skipped", gap=float("nan"), ratio=float("nan"), gnorm=gnorm,
                       comp_tokens=ntok)
                continue
            opt.step()
            train_s = time.time() - t_train

            rev = f"step-{step:04d}"
            t_pub = time.time()
            session.publish(bf16_state(), revision=rev)
            pub_s = time.time() - t_pub
            published[rev] = step
            objs = revision_objects(s3, prefix, rev)
            size = sum(s for _, s in objs)
            kind = "anchor" if any("anchor" in n for n, _ in objs) else "patch"

            mean_r = statistics.fmean(rewards)
            gap = statistics.fmean(gaps)
            rat = statistics.fmean(ratios)
            record(step=step, reward=mean_r, served=served, lag=lag, sync=sync,
                   active_wait=round(active_wait, 1) if sync else None, gen_s=round(gen_s, 1),
                   train_s=round(train_s, 1), pub_s=round(pub_s, 1), rev_bytes=size, kind=kind,
                   gap=round(gap, 4), ratio=round(rat, 4), gnorm=round(gnorm, 3),
                   comp_tokens=ntok)
            log(f"step {step:3d} reward {mean_r:.3f} | served {served} (lag {lag}) | "
                f"gap {gap:.4f} ratio {rat:.3f} | {kind} {size / 1e6:.1f} MB pub {pub_s:.0f}s | "
                f"gen {gen_s:.0f}s train {train_s:.0f}s"
                + (f" | SYNC: Active after {active_wait:.0f}s" if sync else "")
                + (f" | {bad_tok} sampler tokens floored (no gradient)" if bad_tok else ""))
            if sync:
                conds = rl.get_session(uid).get("conditions") or []
                bad = [c for c in conds if c.get("reason") in ("Rejected", "Stalled")]
                if bad:
                    fail(f"session condition: {bad}")

        # -- verdicts -----------------------------------------------------------
        rows = [r for r in (json.loads(l) for l in open(OUT)) if not r.get("skipped")]
        first = statistics.fmean(r["reward"] for r in rows[:10])
        last = statistics.fmean(r["reward"] for r in rows[-10:])
        sync_rows = [r for r in rows if r["sync"]]
        sync_gap = statistics.fmean(r["gap"] for r in sync_rows) if sync_rows else float("nan")
        sync_ratio = statistics.fmean(r["ratio"] for r in sync_rows) if sync_rows else float("nan")
        lags = [r["lag"] for r in rows if not r["sync"]]
        anchors = [r["step"] for r in rows if r["kind"] == "anchor"]
        patch_mb = [r["rev_bytes"] / 1e6 for r in rows if r["kind"] == "patch"]
        waits = [r["active_wait"] for r in sync_rows]

        log("waiting for the usage ledger to cover the last generation")
        t_last = time.time()
        u = {}
        while time.time() - t_last < 300:
            u = rl.session_usage(uid)
            if (u.get("observedAt") or 0) >= t_last and u.get("requests", 0) >= usage_expected["requests"]:
                break
            time.sleep(15)
        states = {}
        for rev in published:
            try:
                st = rl.get_revision(policy, rev).get("state")
            except KeyError:
                # A newer anchor starts a new active window; the registry
                # drops the revisions before it (their artifacts stay in the
                # bucket).
                st = "pruned (pre-anchor)"
            states[st] = states.get(st, 0) + 1

        print("\n================ BYOT TRAINING: RESULT ================", flush=True)
        print(f"reward: first 10 steps {first:.3f} -> last 10 steps {last:.3f} ({'UP' if last > first else 'NOT UP'})")
        print(f"numerics (asserted, trainer == sampler revision): mean |logp gap| {sync_gap:.4f}, mean ratio {sync_ratio:.4f}")
        print(f"async lag (trainer step - served step): min {min(lags)} / median {statistics.median(lags)} / max {max(lags)}")
        print(f"time to Active at sync points: {waits} s")
        print(f"anchors published at steps: {anchors or 'none'} (plus step-0000"
              + ("" if STEPS > 30 else "; the SDK re-anchors after 30 patches") + ")")
        print(f"patch size MB: min {min(patch_mb):.1f} / median {statistics.median(patch_mb):.1f} / max {max(patch_mb):.1f} "
              f"(full anchor {anchor_mb:.0f} MB)")
        print(f"revision states: {states}")
        print(f"usage expected {usage_expected} vs ledger requests={u.get('requests')} "
              f"promptTokens={u.get('promptTokens')} completionTokens={u.get('completionTokens')} "
              f"unaccounted={u.get('unaccountedReplicaSeconds')} replicas={u.get('replicasReporting')}")
        checks = {
            "reward trends up": last > first,
            "numerics at the floor (gap < 0.05, ratio within 5%)": sync_gap < 0.05 and abs(sync_ratio - 1) < 0.05,
            # The SDK re-anchors after 30 patches; shorter runs never reach one.
            **({"auto-anchor happened": bool(anchors)} if STEPS > 30 else {}),
            "no revision Rejected": "Rejected" not in states,
            "no non-finite steps": not skipped,
            "usage matches exactly": (u.get("requests") == usage_expected["requests"]
                                      and u.get("promptTokens") == usage_expected["promptTokens"]
                                      and u.get("completionTokens") == usage_expected["completionTokens"]),
            "usage has no gap": u.get("unaccountedReplicaSeconds") == 0,
        }
        for k, v in checks.items():
            print(f"  [{'PASS' if v else 'FAIL'}] {k}")
        print("ALL PASS" if all(checks.values()) else "SOME CHECKS FAILED", flush=True)
    finally:
        log("cleanup")
        try:
            if uid:
                rl.delete_session(uid)
                log(f"session {uid} deleted")
            rl.delete_policy(policy)
            log(f"policy {policy} deleted")
        except Exception as e:
            log(f"cleanup problem: {e}")


def preflight() -> None:
    """Exercise the trainer end to end WITHOUT staging: model load, prompt
    tokenization, trainer logprobs, one optimizer step, the published tensor
    names and dtypes, and the reward parser."""
    import glob
    snap = snapshot_download(MODEL, allow_patterns=["*.safetensors", "*.json", "tokenizer*", "*.txt"])
    names = []
    for f in sorted(glob.glob(f"{snap}/*.safetensors")):
        with safe_open(f, "pt") as sf:
            names += list(sf.keys())
    tok = AutoTokenizer.from_pretrained(snap)
    data = load_dataset("openai/gsm8k", "main", split="train")
    model = AutoModelForCausalLM.from_pretrained(snap, dtype=torch.float32).to("cuda")
    model.gradient_checkpointing_enable(); model.config.use_cache = False
    sd = model.state_dict()
    missing = [n for n in names if n not in sd]
    assert not missing, f"checkpoint names missing from the model: {missing[:5]}"
    base = {}
    for f in sorted(glob.glob(f"{snap}/*.safetensors")):
        with safe_open(f, "pt") as sf:
            for n in sf.keys():
                base[n] = sf.get_tensor(n)
    same = all(torch.equal(sd[n].detach().to(torch.bfloat16).cpu(), base[n]) for n in names)
    print(f"published tensors: {len(names)}; fp32 master cast back to bf16 == checkpoint: {same}")
    q = data[0]
    p = prompt_ids(tok, q["question"])
    print(f"prompt ids: {type(p).__name__} of {len(p)} ints, starts {p[:4]}")
    gold = gold_answer(q["answer"])
    print(f"reward('#### {gold}') = {reward('work... #### ' + gold, gold)}, reward('#### 999999') = {reward('#### 999999', gold)}, no answer = {reward('I think 18', gold)}")
    comp = tok("Let me think. 16 - 3 - 4 = 9, 9 * 2 = 18.\n#### 18", add_special_tokens=False)["input_ids"]
    opt = torch.optim.AdamW(model.parameters(), lr=LR)
    lp = token_logprobs(model, [p + comp], [len(p)], "cuda")[0]
    loss = -(lp.sum() / len(comp)); loss.backward()
    before = sd[names[5]].detach().clone()
    opt.step()
    changed = (model.state_dict()[names[5]].detach().to(torch.bfloat16) != before.to(torch.bfloat16)).float().mean().item()
    print(f"trainer logprobs: {len(lp)} tokens, mean {lp.mean().item():.3f}; one AdamW step changed "
          f"{changed * 100:.2f}% of {names[5]} in bf16")
    print(f"GPU memory peak {torch.cuda.max_memory_allocated() / 1e9:.1f} GB")
    print("PREFLIGHT OK")


if __name__ == "__main__":
    preflight() if os.environ.get("BYOT_TRAIN_PREFLIGHT") == "1" else main()
