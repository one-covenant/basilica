# byot: Bring Your Own Trainer

Runnable examples for Basilica's RL surface in bring-your-own-trainer mode:
you keep the trainer (your code, your GPUs, your optimizer), Basilica runs a
private vLLM rollout fleet that serves the weights you publish to your own
bucket. Start with [`BYOT-GUIDE.md`](BYOT-GUIDE.md): it explains the contract,
the credentials you need and the verified runs behind each script.

The RL surface lives on staging during the beta. Every script defaults to
`https://api-staging.basilica.ai` (override with `BASILICA_API_URL`).

## Prerequisites

- `python3` (and `ssh` for `byot-train.sh`)
- a Basilica staging API key (`BASILICA_STAGING_API_KEY`)
- a Cloudflare R2 bucket and an R2 API token that can write to it
  (`BYOT_BUCKET`, `BYOT_ENDPOINT`, `BYOT_ACCESS_KEY_ID`, `BYOT_SECRET_ACCESS_KEY`)

The two shell scripts ask for these once and save them in
`$BYOT_DEMO_DIR/env` (default `~/.basilica-byot-demo/env`, readable only by
you). Both clean up their session, policy and rented GPU on exit, also on
Ctrl-C.

## Files

| File | What it does | Run |
|------|--------------|-----|
| `BYOT-GUIDE.md` | Hands-on guide: the contract, credentials, verified runs, tips for your own trainer | read |
| `byot-demo.sh` | One-command demo of the contract from your laptop: policy, session, anchor, patch, stale-revision refusal, usage, park and resume | `bash byot-demo.sh` |
| `byot-train.sh` | Launcher for real training: rents a trainer GPU (or an 8-GPU box), uploads a trainer file, runs it detached on the box and follows its log. The job stops its own rental when it ends and copies `run.log` to `s3://$BYOT_BUCKET/byot-runs/<rental>/run.log` | `bash byot-train.sh` |
| `byot_grpo.py` | The default trainer `byot-train.sh` runs: single-GPU GRPO on GSM8K (Qwen2.5-1.5B-Instruct) with per-step staleness, logprob-gap and usage checks | via `byot-train.sh` |
| `byot_grpo_minimal.py` | A complete GRPO trainer in about 140 lines, to copy as a starting point | `python3 byot_grpo_minimal.py` on any CUDA machine |
| `fsdp/byot_grpo_fsdp.py` | Multi-GPU FSDP2 GRPO trainer: MoE or dense models, optional CPU offload, `countdown` or `gsm8k` tasks, fixed eval set, k3 trainer-vs-sampler mismatch logging, optional batch recording | via `fsdp/run-qwen3-30b.sh` or `BYOT_TRAINER_PY` |
| `fsdp/run-qwen3-30b.sh` | Qwen3-30B-A3B preset for `byot-train.sh`: 8-GPU trainer box with torchrun | `bash fsdp/run-qwen3-30b.sh` |
| `fsdp/selftest_fsdp.py` | CPU self-test of the FSDP trainer's sharded load, gradients and weight gather on a tiny random Qwen3-MoE (no GPU, no Basilica account) | see below |

## Choosing a trainer

`byot-train.sh` runs `byot_grpo.py` by default. Point `BYOT_TRAINER_PY` at
another file to run it instead, and set `BYOT_TRAINER_GPUS` above 1 to rent a
multi-GPU box and launch it with `torchrun`:

```bash
BYOT_TRAINER_PY=$PWD/fsdp/byot_grpo_fsdp.py BYOT_TRAINER_GPUS=8 \
  BYOT_TRAIN_GPU=A100 BYOT_TRAIN_MIN_GPU_GB=80 BYOT_MODEL=Qwen/Qwen3-8B \
  BYOT_TASK=countdown BYOT_TRAIN_STEPS=30 BYOT_MAX_HOURLY=18 \
  bash byot-train.sh
```

The header of `byot-train.sh` lists every setting it reads and passes
through to the trainer. The trainer box is never rented above
`BYOT_MAX_HOURLY` dollars per hour (default 10).

## CPU self-test for the FSDP trainer

```bash
cd fsdp
GLOO_SOCKET_IFNAME=lo0 \
uv run --no-project -q --with torch --with "transformers>=4.51,<5" --with safetensors \
  python -m torch.distributed.run --nnodes 1 --nproc-per-node 2 \
  --master-addr 127.0.0.1 --master-port 29551 selftest_fsdp.py
```

Use `GLOO_SOCKET_IFNAME=lo` on Linux (`lo0` is the macOS loopback).
