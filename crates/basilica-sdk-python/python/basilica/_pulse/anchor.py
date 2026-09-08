# Vendored from pulse-verl (basilica-backend python/pulse-verl/src/pulse_verl/anchor.py)
# for the publisher surface (#1666 / SDK 0.36). The NORMATIVE spec is
# docs/architecture/RL-TRAINING-API-CONTRACTS.md C2.4-C2.5 in basilica-backend;
# this copy must track it byte-for-byte (parity pinned by tests/test_pulse_codec.py).
# Heavy deps (torch/numpy/xxhash/zstandard/safetensors) are publisher-only:
# this package is imported LAZILY - never at basilica import time.
"""C2.5 anchors: full BF16 state as safetensors, the late-join/restart chain root.

Anchor files carry name-ascending keys and metadata
{"pulse.step": "<n>", "pulse.stateDigest": "xxh3_128:<hex>"}; loading verifies
the digest before the state is trusted.
"""

from __future__ import annotations

from pathlib import Path

from safetensors import safe_open
from safetensors.torch import save_file

from .codec import PatchApplyError, Snapshot

_META_STEP = "pulse.step"
_META_DIGEST = "pulse.stateDigest"


def save_anchor(snapshot: Snapshot, path: Path | str, *, step: int) -> str:
    """Write the snapshot as an anchor; returns its stateDigest."""
    digest = snapshot.digest()
    tensors = {name: t for name, t in snapshot.named_tensors()}
    save_file(tensors, str(path), metadata={_META_STEP: str(step), _META_DIGEST: digest})
    return digest


def load_anchor(path: Path | str) -> tuple[Snapshot, int, str]:
    """Load + digest-verify an anchor; returns (snapshot, step, stateDigest)."""
    with safe_open(str(path), framework="pt", device="cpu") as f:
        meta = f.metadata() or {}
        tensors = {name: f.get_tensor(name) for name in f.keys()}
    if _META_STEP not in meta or _META_DIGEST not in meta:
        raise PatchApplyError(f"anchor {path} missing pulse metadata")
    snapshot = Snapshot(tensors)
    digest = snapshot.digest()
    if digest != meta[_META_DIGEST]:
        raise PatchApplyError(f"anchor {path} digest mismatch (I3 violation)")
    return snapshot, int(meta[_META_STEP]), digest
