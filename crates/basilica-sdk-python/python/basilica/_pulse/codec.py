# Vendored from pulse-verl (basilica-backend python/pulse-verl/src/pulse_verl/codec.py)
# for the publisher surface (#1666 / SDK 0.36). The NORMATIVE spec is
# docs/architecture/RL-TRAINING-API-CONTRACTS.md C2.4-C2.5 in basilica-backend;
# this copy must track it byte-for-byte (parity pinned by tests/test_pulse_codec.py).
# Heavy deps (torch/numpy/xxhash/zstandard/safetensors) are publisher-only:
# this package is imported LAZILY - never at basilica import time.
"""PULSEPT1 patch codec: PULSE Algorithm 1 (arXiv:2602.03839) over the C2.4 format.

Normative format: docs/architecture/RL-TRAINING-API-CONTRACTS.md C2.4-C2.5.
Envelope: magic "PULSEPT1" | u32 headerLen | header JSON | one zstd-1 frame.
Payload: per tensor, in name-ascending header order, [indices][values].
Values are absolute BF16 cells — apply is a memory copy, never FP arithmetic (I3).
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from typing import Iterable, Iterator

import numpy as np
import torch
import zstandard

from .digest import format_digest, parse_digest, state_digest, tensor_digest
from .indices import decode_indices, encode_indices

MAGIC = b"PULSEPT1"
FORMAT_VERSION = 1
INDEX_ENCODING = "delta-u16-esc"
MAX_NUMEL = 2**32  # v1 first-index is u32 (C2.4)
ZSTD_LEVEL = 1  # paper: zstd-1


class PatchFormatError(ValueError):
    pass


class PatchApplyError(RuntimeError):
    """Integrity gate (I3): a digest mismatch AFTER a correctly-based apply, or a
    structural defect (shape/index/tensor-set). This is the hard fail — a true
    byte-corruption — and under ruling A a HALT/NO-GO at any scale."""


class StalePatchError(RuntimeError):
    """Sequencing gate: a patch whose ``base_step`` does not match the consumer's
    current version (stale, skipped, or out-of-order). A recoverable relay/
    sequencing fault, NOT an I3 integrity violation -- the catch-up layer raises
    it BEFORE the digest gate so the two are never conflated (mirrors
    autoresearch_rl.sync.codec.DeltaReplay.ingest's base_version assert)."""


@dataclass(frozen=True)
class ModelMeta:
    """C2.4 header `model` block; `digest` is the stateDigest of the step-0 base state."""

    id: str
    digest: str
    param_count: int

    def to_header(self) -> dict:
        return {"id": self.id, "digest": self.digest, "paramCount": self.param_count}

    @classmethod
    def from_header(cls, d: dict) -> "ModelMeta":
        return cls(id=d["id"], digest=d["digest"], param_count=d["paramCount"])


@dataclass(frozen=True)
class TensorEntry:
    name: str
    shape: tuple[int, ...]
    numel: int
    changed: int
    idx_bytes: bytes
    val_bytes: bytes
    digest: bytes  # full-tensor digest AFTER this patch applies

    def to_header(self) -> dict:
        return {
            "name": self.name,
            "shape": list(self.shape),
            "numel": self.numel,
            "dtype": "bf16",
            "changed": self.changed,
            "idxEnc": INDEX_ENCODING,
            "idxBytes": len(self.idx_bytes),
            "valBytes": len(self.val_bytes),
            "tensorDigest": format_digest(self.digest),
        }


def diff_tensor(cur: torch.Tensor, prev: torch.Tensor) -> tuple[np.ndarray, bytes]:
    """Bitwise BF16 diff: (changed flat indices, absolute BF16 values as raw LE bytes).

    Bitwise (int16 view) rather than value compare: -0.0/+0.0 and NaN-payload
    transitions must be transmitted for bit-identical reconstruction (I3).
    """
    if cur.shape != prev.shape:
        raise PatchFormatError(f"shape mismatch: {tuple(cur.shape)} vs {tuple(prev.shape)}")
    c = cur.detach().contiguous().reshape(-1).view(torch.int16)
    p = prev.detach().contiguous().reshape(-1).view(torch.int16)
    if c.device != p.device:
        raise PatchFormatError("diff requires tensors on the same device")
    idx = (c != p).nonzero(as_tuple=True)[0]
    vals = c[idx].cpu().numpy().tobytes()
    return idx.cpu().numpy().astype(np.uint64, copy=False), vals


def apply_tensor_patch(t: torch.Tensor, idx: np.ndarray, values: bytes) -> None:
    """In-place scatter of absolute BF16 cells: flat[idx] = values."""
    if idx.size == 0:
        return
    if len(values) != 2 * idx.size:
        raise PatchApplyError(f"value bytes {len(values)} != 2 x changed {idx.size}")
    flat = t.reshape(-1).view(torch.int16)
    vals = torch.from_numpy(np.frombuffer(values, dtype="<i2").copy())
    flat[torch.from_numpy(idx.astype(np.int64, copy=False)).to(t.device)] = vals.to(t.device)


def build_patch(
    *,
    step: int,
    base_step: int,
    model: ModelMeta,
    entries: list[TensorEntry],
    base_state_digest: str | None = None,
) -> tuple[bytes, str]:
    """Assemble a PULSEPT1 patch; returns (patch bytes, stateDigest string)."""
    entries = sorted(entries, key=lambda e: e.name)
    sdig = state_digest([e.digest for e in entries])
    header = {
        "formatVersion": FORMAT_VERSION,
        "step": step,
        "baseStep": base_step,
        "model": model.to_header(),
        "tensors": [e.to_header() for e in entries],
        "stateDigest": format_digest(sdig),
    }
    if base_state_digest is not None:  # optional, omitted keeps legacy patches byte-identical
        header["baseStateDigest"] = base_state_digest
    hjson = json.dumps(header, sort_keys=True, separators=(",", ":")).encode()
    cobj = zstandard.ZstdCompressor(level=ZSTD_LEVEL).compressobj()
    chunks = [MAGIC, np.array([len(hjson)], dtype="<u4").tobytes(), hjson]
    for e in entries:
        if e.idx_bytes:
            chunks.append(cobj.compress(e.idx_bytes))
        if e.val_bytes:
            chunks.append(cobj.compress(e.val_bytes))
    chunks.append(cobj.flush())
    return b"".join(chunks), format_digest(sdig)


@dataclass(frozen=True)
class ParsedPatch:
    step: int
    base_step: int
    model: ModelMeta
    state_digest: str
    tensors: list[dict]
    _payload: bytes
    _offsets: list[tuple[int, int, int]]  # (idx_start, val_start, end) per tensor
    base_state_digest: str | None = None  # C2.4 optional; None for legacy patches

    def tensor_payload(self, i: int) -> tuple[bytes, bytes]:
        idx_start, val_start, end = self._offsets[i]
        return self._payload[idx_start:val_start], self._payload[val_start:end]


def parse_patch(data: bytes) -> ParsedPatch:
    """Parse + validate a PULSEPT1 patch (structure only; digests verify on apply)."""
    if len(data) < len(MAGIC) + 4:
        raise PatchFormatError("truncated envelope")
    if data[: len(MAGIC)] != MAGIC:
        raise PatchFormatError("bad magic")
    hlen = int(np.frombuffer(data, "<u4", offset=len(MAGIC), count=1)[0])
    body = len(MAGIC) + 4
    if body + hlen > len(data):
        raise PatchFormatError("truncated header")
    try:
        header = json.loads(data[body : body + hlen])
    except ValueError as e:
        raise PatchFormatError(f"header is not valid JSON: {e}") from e
    if header.get("formatVersion") != FORMAT_VERSION:
        raise PatchFormatError(f"unsupported formatVersion {header.get('formatVersion')}")
    try:
        tensors = _validate_tensor_headers(header["tensors"])
        expected = sum(t["idxBytes"] + t["valBytes"] for t in tensors)
    except (KeyError, TypeError) as e:
        raise PatchFormatError(f"malformed header: {e!r}") from e
    raw = data[body + hlen :]
    try:
        payload = zstandard.ZstdDecompressor().decompress(raw, max_output_size=expected) if expected else b""
    except zstandard.ZstdError as e:
        raise PatchFormatError(f"payload decompression failed: {e}") from e
    if len(payload) != expected:
        raise PatchFormatError(f"payload is {len(payload)} bytes, header implies {expected}")
    offsets, pos = [], 0
    for t in tensors:
        offsets.append((pos, pos + t["idxBytes"], pos + t["idxBytes"] + t["valBytes"]))
        pos = offsets[-1][2]
    try:
        return ParsedPatch(
            step=header["step"],
            base_step=header["baseStep"],
            model=ModelMeta.from_header(header["model"]),
            state_digest=header["stateDigest"],
            tensors=tensors,
            _payload=payload,
            _offsets=offsets,
            base_state_digest=header.get("baseStateDigest"),  # None when absent (legacy)
        )
    except (KeyError, TypeError) as e:
        raise PatchFormatError(f"malformed header: {e!r}") from e


def _validate_tensor_headers(tensors: list[dict]) -> list[dict]:
    names = [t["name"] for t in tensors]
    if names != sorted(names) or len(set(names)) != len(names):
        raise PatchFormatError("tensors[] must be unique and name-ascending")
    for t in tensors:
        if t["dtype"] != "bf16":
            raise PatchFormatError(f"{t['name']}: v1 admits dtype bf16 only")
        if t["idxEnc"] != INDEX_ENCODING:
            raise PatchFormatError(f"{t['name']}: unknown idxEnc {t['idxEnc']!r}")
        if t["numel"] != int(np.prod(t["shape"], dtype=np.int64)) or t["numel"] >= MAX_NUMEL:
            raise PatchFormatError(f"{t['name']}: inconsistent or oversized numel")
        if t["valBytes"] != 2 * t["changed"]:
            raise PatchFormatError(f"{t['name']}: valBytes != 2 x changed")
    return tensors


class Snapshot:
    """Mutable BF16 state: the producer's diff base and the consumer's reconstruction target.

    Tensors are CPU, C-contiguous, keyed by the HF parameter names verl yields.
    The snapshot owns its storage (inputs are copied on ingest).
    """

    def __init__(self, tensors: dict[str, torch.Tensor]):
        offenders = [f"{n}: {t.dtype}" for n, t in tensors.items() if t.dtype is not torch.bfloat16]
        if offenders:
            raise PatchFormatError(
                f"snapshot tensors must be bf16 (v1, C2.4); non-bf16 entries: {sorted(offenders)}"
            )
        self._tensors: dict[str, torch.Tensor] = {}
        # Pre-encode state retained until commit_step()/rollback_last_encode():
        # a failed publish must be able to restore the last PUBLISHED state (H1).
        self._prev_tensors: dict[str, torch.Tensor] | None = None
        for name in sorted(tensors):
            t = tensors[name]
            if t.numel() >= MAX_NUMEL:
                raise PatchFormatError(f"{name}: numel {t.numel()} exceeds v1 limit 2^32")
            self._tensors[name] = t.detach().to(device="cpu", copy=True).contiguous()

    @classmethod
    def from_named_tensors(cls, named: Iterable[tuple[str, torch.Tensor]]) -> "Snapshot":
        return cls(dict(named))

    def names(self) -> list[str]:
        return list(self._tensors)

    def tensor(self, name: str) -> torch.Tensor:
        return self._tensors[name]

    def named_tensors(self) -> Iterator[tuple[str, torch.Tensor]]:
        yield from self._tensors.items()

    def digest(self) -> str:
        return format_digest(state_digest([tensor_digest(t) for t in self._tensors.values()]))

    def param_count(self) -> int:
        return sum(t.numel() for t in self._tensors.values())

    def encode_step(
        self,
        new_state: Iterable[tuple[str, torch.Tensor]],
        *,
        step: int,
        base_step: int,
        model: ModelMeta,
        base_state_digest: str | None = None,
        stats_out: list | None = None,
    ) -> tuple[bytes, str]:
        """Diff `new_state` against this snapshot, advance the snapshot, return (patch, stateDigest).

        The advance is ATOMIC: updates are staged and swapped in only after every
        tensor validated and the completeness check passed, so an exception midway
        (a raising weights generator, a bad dtype) leaves the snapshot untouched.
        The pre-encode state is retained until `commit_step()` (publish succeeded)
        or `rollback_last_encode()` (publish failed) — a lost upload must not
        poison the next patch's diff base (H1).

        ``stats_out`` (issue #964 causal-chain instrumentation): when a list is
        passed, one dict per tensor is appended — name, numel, nnz, nnz_frac,
        idx_bytes (encoded), val_bytes (raw bf16) — from the entries already
        built for the patch; measurement adds no extra tensor work. None (the
        default) is byte-and-behavior identical to before.
        """
        entries, seen = [], set()
        staged: dict[str, torch.Tensor] = {}
        for name, t in new_state:
            prev = self._tensors.get(name)
            if prev is None:
                raise PatchFormatError(f"unknown tensor in update: {name}")
            if name in seen:
                raise PatchFormatError(f"duplicate tensor in update: {name}")
            seen.add(name)
            cur = t.detach().to(device="cpu", copy=True).contiguous()
            if cur.dtype is not torch.bfloat16:
                raise PatchFormatError(f"{name}: update tensors must be bf16, got {cur.dtype}")
            idx, vals = diff_tensor(cur, prev)
            entries.append(
                TensorEntry(name, tuple(cur.shape), cur.numel(), int(idx.size), encode_indices(idx), vals, tensor_digest(cur))
            )
            staged[name] = cur
        if seen != set(self._tensors):
            raise PatchFormatError(f"update missing tensors: {sorted(set(self._tensors) - seen)}")
        if stats_out is not None:
            for e in entries:
                stats_out.append({
                    "name": e.name, "numel": e.numel, "nnz": e.changed,
                    "nnz_frac": (e.changed / e.numel) if e.numel else 0.0,
                    "idx_bytes": len(e.idx_bytes), "val_bytes": len(e.val_bytes),
                })
        patch = build_patch(
            step=step, base_step=base_step, model=model, entries=entries, base_state_digest=base_state_digest
        )
        self._prev_tensors = self._tensors
        # Reindex to the snapshot's canonical (sorted) key order: dict order is
        # the digest/names domain, and the update iterable's order is arbitrary.
        self._tensors = {name: staged[name] for name in self._prev_tensors}
        return patch

    def commit_step(self) -> None:
        """Publish succeeded: drop the pre-encode state retained for rollback."""
        self._prev_tensors = None

    def rollback_last_encode(self) -> None:
        """Publish failed: restore the snapshot to its pre-encode_step state (H1).

        Without this, a producer surviving a failed upload diffs its next patch
        against the UNPUBLISHED state — the consumer's sequencing gate passes,
        the digest gate fires, and a relay hiccup masquerades as an I3 HALT.
        """
        if self._prev_tensors is None:
            raise PatchFormatError("no un-committed encode_step to roll back")
        self._tensors = self._prev_tensors
        self._prev_tensors = None

    def apply_patch(self, patch: ParsedPatch) -> str:
        """Apply + verify per-tensor and state digests (C2.4 MUST); returns the stateDigest."""
        names = [t["name"] for t in patch.tensors]
        if names != self.names():
            raise PatchApplyError("patch tensor set does not match snapshot")
        digests = []
        for i, th in enumerate(patch.tensors):
            t = self._tensors[th["name"]]
            if list(t.shape) != th["shape"]:
                raise PatchApplyError(f"{th['name']}: shape mismatch")
            idx_b, val_b = patch.tensor_payload(i)
            idx = decode_indices(idx_b, th["changed"])
            if idx.size and int(idx[-1]) >= t.numel():
                raise PatchApplyError(f"{th['name']}: index out of range")
            apply_tensor_patch(t, idx, val_b)
            d = tensor_digest(t)
            if d != parse_digest(th["tensorDigest"]):
                raise PatchApplyError(f"{th['name']}: tensor digest mismatch after apply (I3 violation)")
            digests.append(d)
        sdig = format_digest(state_digest(digests))
        if sdig != patch.state_digest:
            raise PatchApplyError("state digest mismatch after apply (I3 violation)")
        return sdig
