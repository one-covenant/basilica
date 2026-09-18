# Vendored from pulse-verl (basilica-backend python/pulse-verl/src/pulse_verl/digest.py)
# for the publisher surface (#1666 / SDK 0.36). The NORMATIVE spec is
# docs/architecture/RL-TRAINING-API-CONTRACTS.md C2.4-C2.5 in basilica-backend;
# this copy must track it byte-for-byte (parity pinned by tests/test_pulse_codec.py).
# Heavy deps (torch/numpy/xxhash/zstandard/safetensors) are publisher-only:
# this package is imported LAZILY - never at basilica import time.
"""C2.4 digest domain: xxh3_128 over raw little-endian BF16 bytes.

tensorDigest = xxh3_128 of the full C-contiguous tensor's BF16 bytes;
stateDigest  = xxh3_128 over the concatenated 16-byte tensorDigest values in
tensors[] (name-ascending) order. xxh3_128 digest bytes are the canonical
(big-endian) representation from the XXH3 spec; the hex string is what crosses
the wire.
"""

from __future__ import annotations

import sys
from typing import Iterable

import torch
import xxhash

DIGEST_PREFIX = "xxh3_128:"
DIGEST_SIZE = 16

# The wire format is little-endian (C2 conventions); torch/numpy buffers are
# host-endian, so a BE host would silently produce foreign bytes.
if sys.byteorder != "little":
    raise RuntimeError("the basilica publisher requires a little-endian host")


def _cpu_int16_flat(t: torch.Tensor) -> torch.Tensor:
    if t.dtype is not torch.bfloat16:
        raise TypeError(f"expected a bf16 tensor, got {t.dtype}")
    return t.detach().contiguous().reshape(-1).view(torch.int16).cpu()


def tensor_digest(t: torch.Tensor) -> bytes:
    """16-byte xxh3_128 of the full tensor's raw BF16 bytes (zero-copy on CPU)."""
    h = xxhash.xxh3_128()
    h.update(_cpu_int16_flat(t).numpy().data)
    return h.digest()


def state_digest(tensor_digests: Iterable[bytes]) -> bytes:
    """Digest-of-digests, in tensors[] (name-ascending) order."""
    h = xxhash.xxh3_128()
    for d in tensor_digests:
        if len(d) != DIGEST_SIZE:
            raise ValueError(f"tensor digest must be {DIGEST_SIZE} bytes, got {len(d)}")
        h.update(d)
    return h.digest()


def format_digest(raw: bytes) -> str:
    if len(raw) != DIGEST_SIZE:
        raise ValueError(f"digest must be {DIGEST_SIZE} bytes, got {len(raw)}")
    return DIGEST_PREFIX + raw.hex()


def parse_digest(formatted: str) -> bytes:
    if not formatted.startswith(DIGEST_PREFIX):
        raise ValueError(f"digest must start with {DIGEST_PREFIX!r}: {formatted!r}")
    raw = bytes.fromhex(formatted[len(DIGEST_PREFIX) :])
    if len(raw) != DIGEST_SIZE:
        raise ValueError(f"digest must be {DIGEST_SIZE} bytes, got {len(raw)}")
    return raw
