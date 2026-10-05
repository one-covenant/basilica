# Bring Your Own Trainer on Basilica: hands-on guide (staging)

## Fastest path: the one-command demo

```
curl -fsSL https://raw.githubusercontent.com/one-covenant/basilica/main/examples/byot/byot-demo.sh | bash
```

That's the whole thing (`byot-demo.sh` in this directory). It needs:

- `python3`
- a Basilica staging API key: during the beta, ask the Basilica team for one (if you already have staging access in the Basilica CLI, `basilica tokens create` prints one)
- a Cloudflare R2 bucket and an R2 API token with write access to it (R2 is the storage backend in this version)

It asks for these once and stores them locally, readable only by you. It cleans up after itself, including on Ctrl-C.

**Which R2 value goes where.**

| The demo asks for | Use |
|---|---|
| R2 bucket name | the bucket's name |
| R2 endpoint URL | `https://<account-id>.r2.cloudflarestorage.com` |
| R2 access key id | the R2 API token's access key id |
| R2 secret access key | the R2 API token's secret access key |

Don't use the Cloudflare account API token: it looks similar but is a different credential, and the upload would fail with it.

The key goes into the demo's first prompt.

Plan on about an hour the first time, and about 25 minutes after that. The first run downloads the model (about 3 GB) unless it is already in your Hugging Face cache; after that, most of the time is renting the GPU. Set `BYOT_DEMO_SKIP_PARK=1` to skip park/resume.

**Latest verified run (2026-09-24): a brand-new user on a fresh machine** (empty home directory, SDK 0.36.0 from PyPI at the time; the demo now installs 0.36.4 or newer, Qwen2.5-1.5B-Instruct), every step through the public API:

| Step | Result | Clock |
|---|---|---|
| Model download | ~3 GB over a ~1.7 MB/s connection | 26 min |
| Session up | Serving after GPU rental | +9 min |
| Anchor | Uploaded, loaded and digest-confirmed on the fleet | +7 min |
| Generation | "The capital of France is" gives " Paris."; a group of 4 samples in the training dialect | seconds |
| Patch | Active and serving about a minute after publish; the stale anchor is refused, nothing sampled | +1 min |
| Usage | 3 requests, 14 prompt tokens, 52 completion tokens: exact, no gaps | +1 min |
| Park / resume | Final counts handed over (`unaccountedReplicaSeconds = 0`); the resumed replica serves the patch | +3 min |
| Total | 48 minutes, of which 22 were the demo itself | |

---

## Real training, one command

```
curl -fsSL https://raw.githubusercontent.com/one-covenant/basilica/main/examples/byot/byot-train.sh | bash
```

`byot-demo.sh` shows the contract with a synthetic weight change. `byot-train.sh` trains for real: it rents a GPU for *your* trainer on Basilica staging, starts a 2-replica rollout session, and runs real GRPO (Qwen2.5-1.5B-Instruct on GSM8K, `byot_grpo.py` in this directory). Each step samples 8 prompts x 8 through the session, takes an optimizer step against the sampler's own logprobs, and publishes the new weights as a sparse patch to your bucket. You watch the reward climb, see which revision served each batch, and get a verdict at the end.

It needs `python3` and `ssh`, and the same API key and R2 bucket as the demo (answers are shared). It creates its own SSH key for the rented machine and registers it with your account. Basilica allows one key per account: if yours already has a different key, the script stops and says how to proceed. It cleans up after itself, also on Ctrl-C: the session and policy are deleted and the rented GPU is stopped, with its cost printed.

**Verified run (2026-09-25), 20 steps:** 34 minutes end to end. The rented trainer GPU cost $1.50; the two rollout replicas are billed as the session's usage.

| | Result |
|---|---|
| Reward | 0.145 (first 10 steps) to 0.316 (last 10) |
| Trainer vs sampler, same revision | mean logprob gap 0.0087, ratio 0.9998 |
| Rollout staleness | median 3.5 steps behind the trainer, max 7 |
| Usage | 20 requests, 18,061 prompt and 320,989 completion tokens: exact, no gaps |

A 50-step run (`BYOT_TRAIN_STEPS=50`) took reward from 0.15 to 0.76 and includes the automatic full re-anchor at step 31.

### Large models

The multi-GPU trainer is `fsdp/byot_grpo_fsdp.py` (FSDP2, MoE or dense, optional CPU offload); `fsdp/run-qwen3-30b.sh` runs it through `byot-train.sh` with the settings below, and `fsdp/selftest_fsdp.py` checks its sharding and gradient logic on CPU.

**Verified run (2026-10-01): Qwen3-30B-A3B, 10 GRPO steps on GSM8K.** The trainer ran FSDP2 on 8x A100 80GB with CPU offload. The rollout session served from 1x A100 80GB. Every step's patch (about 0.8 GB) went Active about 40 seconds after it was published: 20 s to fetch, 19 s to apply and reload. The first 61 GB anchor took 36 minutes from publish to Active, most of it the fetch. The trainer GPU cost $29.64 for 2.4 hours.

Size the session for the model; the per-GPU defaults are made for small models:

```python
session = client.rl.create_session(
    "my-policy",
    gpu_model="A100", gpu_count=1, replicas=1,
    min_gpu_memory_gb=80,   # a 61 GB model needs an 80 GB GPU; A100 also comes in 40 GB
    memory_gib=96,          # pod memory: the session keeps the weights in memory, once
    cpu_cores=12,           # pod CPU: anchor verify and patch apply run on CPU
)
```

Use SDK 0.36.4 or newer to publish large models: earlier versions can report a 61 GB anchor upload as failed after it has landed.

**Results from the 10 steps:**

| | Result |
|---|---|
| Training reward per step | 0.72 to 0.98, mean 0.92 (different GSM8K problems each step, 64 samples) |
| Patch, publish to Active | about 40 s: fetch 0.8 GB in 20 s, apply in place 6 s, vLLM reload 11 s |
| Anchor, publish to Active | 36 min: upload 11 min, fetch 23 min, load 63 s |
| Trainer step | about 12 min, including a 3 min publish |

The base model already solves over 90% of GSM8K, so 10 small steps don't move the reward beyond batch-to-batch noise. The run proves the loop at this size, not learning.

**Notes for your own trainer at this size:**

- **Memory.** Full fine-tuning keeps fp32 weights, gradients and AdamW state: about 16 bytes per parameter, so 71 GiB per GPU on 8 GPUs for 30B. That fits a 141 GB H200 but not an 80 GB A100. With FSDP2 CPU offload (`CPUOffloadPolicy`) the peak drops to about 8 GiB per GPU, and the host needs a few hundred GB of RAM.
- **CPU offload settings.** Use the `cpu:gloo,cuda:nccl` process-group backend: gradient clipping then runs collectives on CPU tensors, which NCCL alone cannot do. Raise each rank's torch threads (`torchrun` sets 1): at 1 thread a step took 948 s, at about 24 threads 143 s.
- **Gathering weights to publish.** Cast each shard to bf16 on its GPU before gathering. That halves the bytes and keeps the gather on NCCL. Gathering fp32 shards from host memory took 10 minutes per step.
- **Trainer wall time.** The trainer's step and its publish dominate. The rollout side is not the bottleneck once the anchor has loaded.

---

## What BYOT is

You keep your trainer: your code, your GPUs, your optimizer, your data. Basilica runs the part that is expensive to operate: a private fleet of vLLM rollout replicas that always serves the weights you just trained.

The contract between the two sides is small:

1. **Your bucket holds your weights.** You register a *policy* (a model lineage: base model, commit, tokenizer) against your own Cloudflare R2 bucket (the only storage backend in this version). Your trainer uploads straight to your bucket. The fleet reads from it through a relay scoped to that one policy's prefix, and the platform keeps your storage keys write-only (they are never echoed back).
2. **Anchors and patches.** The first publish is an *anchor*: the full bf16 state. After that, each training step publishes a *patch*: only the bf16 weights that changed, in the PULSE sparse-patch format. RL updates are sparse (typically a few percent of weights change per step), so a patch is a small fraction of a checkpoint.
3. **The fleet verifies what it loads.** Every replica fetches the revision, checks its content digest, applies it, and confirms. A revision turns **Active** only when every replica has confirmed it. If a digest doesn't match, the revision is rejected and the previous one keeps serving.
4. **Every response is stamped.** Generation responses carry `servedRevision`. You can also *assert* a revision: if the fleet serves something else, the request is refused with `StaleRevisionError` before anything is sampled, so off-policy samples never slip into a batch unnoticed.
5. **The training dialect.** Token IDs in, token IDs out, the sampler's own logprobs per token, groups of `n` samples per prompt, seeds for replay. There's no retokenization drift between the rollout side and your trainer.
6. **Metered, parkable.** Usage (requests, prompt and completion tokens, GPU hours) is measured by the replicas themselves and made durable, so counts survive restarts. Parking releases the GPUs; resuming brings up fresh replicas that replay the lineage (anchor + patches) and continue where they left off.

## The same loop in your own code

`byot_grpo_minimal.py` in this directory is a complete, runnable GRPO trainer (about 140 lines) to copy as a starting point. Run it on any machine with an NVIDIA GPU, with the same settings as the demo:

```bash
pip install "basilica-sdk[publisher]>=0.36.4" transformers datasets
export BASILICA_STAGING_API_KEY=... BYOT_BUCKET=... BYOT_ENDPOINT=... \
       BYOT_ACCESS_KEY_ID=... BYOT_SECRET_ACCESS_KEY=...
python byot_grpo_minimal.py        # STEPS=50 for a longer run
```

The `publisher` extra pulls in torch, numpy, safetensors and boto3 for the trainer side; the base SDK has no dependencies. The setup, from the file:

```python
rl = BasilicaClient(base_url="https://api-staging.basilica.ai", api_key=...).rl
rl.create_policy(policy, repo=MODEL, commit=..., tokenizer_digest=..., **bucket)   # the lineage
s = rl.create_session(policy, gpu_model="H100", gpu_count=1, replicas=2)          # the rollout fleet
handle = rl.policy(policy, storage=PolicyStorage(**bucket), work_dir="./publish")
handle.publish_anchor(weights(), revision="step-0000")                            # full weights, once
handle.wait_until_active("step-0000")
session = rl.open_session(s["url"], s["token"], publisher=handle, session_uid=s["sessionUid"])
```

And one training step, exactly as it runs:

```python
# 1. Sample: N completions per prompt, token ids in and out.
out = session.generate(token_ids=prompts, n=N, max_tokens=320, temperature=1.0, seed=step)

# 2. Score, and normalise advantages within each prompt's group of N.
rewards = [reward(tok.decode(ids, skip_special_tokens=True), golds[i // N])
           for i, ids in enumerate(out.token_ids)]

# 3. PPO-clip update against the SAMPLER's own logprobs (out.logprobs).
ratio = torch.exp(trainer_logprobs - torch.tensor(out.logprobs[i], device="cuda"))
loss = -torch.minimum(ratio * a, ratio.clamp(1 - CLIP, 1 + CLIP) * a).sum() / ntok
gnorm = torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
if torch.isfinite(gnorm):
    opt.step()

# 4. Publish a sparse patch over the last published revision.
session.publish(weights(), revision=f"step-{step:04d}")
```

`weights()` casts the fp32 master weights to bf16 under the checkpoint's own tensor names. `out.served_revision` says which revision produced the samples; `session.usage()` reports requests, tokens and GPU hours at any point, and `session.park()` / `session.resume()` release and bring back the fleet.

**Tips for your own trainer:**

- **Never step on a non-finite gradient.** Check the gradient norm before `opt.step()` and skip the step if it isn't finite; that keeps NaN out of your weights in the first place. From SDK 0.36.1, `publish()` is the backstop: it raises `NonFiniteWeights` (naming the tensor) and uploads nothing. If it fires, your trainer's weights are already bad, so stop or roll back to your last good state; don't keep training from them.
- **Right-pad and drop the attention mask** when recomputing logprobs in bf16. With causal attention, real tokens never attend to trailing padding, so the mask changes nothing, and PyTorch's fused attention can return NaN gradients from a padding mask in bf16.
- **In async RL, samples run several revisions behind.** The fleet finishes loading one revision before starting the next, so with a publish every ~15 s our runs measured a median of about 4 steps and at most 8. Every sample carries `servedRevision`, so you can weight or filter by staleness. For strictly on-policy steps, `wait_until_active(rev)` first, then assert `revision=rev` when you generate.

## Notes

- **Staging, beta.** The RL surface lives on staging during the beta, and the client defaults to production, so pass `base_url` as above (or set `BASILICA_API_URL`).
- **Timing honesty.** Renting a GPU depends on marketplace stock at that moment: minutes on a good day, longer on a bad one. The demo waits it out and cleans up either way; it never leaves a GPU running.
- **Upload speed.** The anchor is the full model (about 3 GB for the 1.5B demo), uploaded from wherever the script runs; it took about 4 minutes in the verified run. Patches are a small fraction of that.
- **Settings.** `byot-demo.sh` reads `BYOT_DEMO_MODEL` (default `Qwen/Qwen2.5-1.5B-Instruct`), `BYOT_DEMO_GPU` (default `H100`) and `BYOT_DEMO_SKIP_PARK=1`. `byot-train.sh` reads `BYOT_TRAIN_STEPS` (default 20) and `BYOT_TRAIN_GPU` (default `H100`). Both keep their files in `~/.basilica-byot-demo/` (set `BYOT_DEMO_DIR` to use another directory): saved answers in `env` (delete it to be asked again) and the train script's SSH key in `ssh/`.
- **If the key is rejected**, remove its line from `~/.basilica-byot-demo/env` and re-run.
