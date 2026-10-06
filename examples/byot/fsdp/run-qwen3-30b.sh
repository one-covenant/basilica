#!/usr/bin/env bash
# Real BYOT GRPO on Qwen3-30B-A3B: an 8-GPU trainer box (FSDP2, torchrun)
# against a rollout session of BYOT_REPLICAS replicas x BYOT_SESSION_GPUS H100
# (tensor parallel). Uses the same saved answers as byot-train.sh.
#
# Needs on staging: multi-GPU sessions, and session pods with ~90 GiB of
# memory (a 61 GB model; see BYOT_SESSION_MEMORY_GIB).
#
# The trainer needs ~490 GB of GPU memory in total for fp32 weights, grads and
# AdamW state, so 8 GPUs of at least 80 GB. Default: 8x A100 80GB (staging had
# no 8x H100 on 2026-09-25; about $13.50/hr). BYOT_TRAIN_GPU=H100 when offered.
#
#   BYOT_MAX_HOURLY=20 bash run-qwen3-30b.sh          # 5 steps by default here
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
export BYOT_TRAINER_PY="$HERE/byot_grpo_fsdp.py"
export BYOT_TRAINER_GPUS="${BYOT_TRAINER_GPUS:-8}"
export BYOT_TRAIN_GPU="${BYOT_TRAIN_GPU:-A100}"
export BYOT_TRAIN_MIN_GPU_GB="${BYOT_TRAIN_MIN_GPU_GB:-80}"
export BYOT_MODEL="${BYOT_MODEL:-Qwen/Qwen3-30B-A3B}"
export BYOT_SESSION_GPUS="${BYOT_SESSION_GPUS:-1}"
export BYOT_SESSION_GPU="${BYOT_SESSION_GPU:-H200}"
export BYOT_SESSION_MEMORY_GIB="${BYOT_SESSION_MEMORY_GIB:-128}"
export BYOT_SESSION_CPU_CORES="${BYOT_SESSION_CPU_CORES:-16}"
export BYOT_MICRO_BATCH="${BYOT_MICRO_BATCH:-1}"
export BYOT_REPLICAS="${BYOT_REPLICAS:-1}"
export BYOT_TRAIN_STEPS="${BYOT_TRAIN_STEPS:-15}"
exec bash "$HERE/../byot-train.sh"
