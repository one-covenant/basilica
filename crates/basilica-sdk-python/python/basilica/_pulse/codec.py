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

Format v2 (#2156, opt-in at encode time via ``format_version=2``) keeps the
envelope, header and digests and changes only the two per-tensor streams:
indices are ``rice-gap`` (indices.py) and values are ``zz-delta-u8esc``
(values.py), the zigzag difference of the BF16 bit patterns against the old
cell. Apply stays integer-only on bit patterns, so it is bit-exact (I3), but
it reads the cells it overwrites. The header's ``formatVersion`` selects the
decoder; v2 tensor headers also name ``valEnc``. v1 patches are unchanged.

Format v3 keeps v2's values and codes the indices as ``exp-class-rice``
(expctx.py): per class of the OLD cell's exponent byte, so the decoder also
needs the whole old tensor, not only the cells it overwrites. Both ends hold
it: the producer's diff base and the consumer's snapshot.
"""

from __future__ import annotations

import json
from concurrent.futures import Future, ThreadPoolExecutor
from dataclasses import dataclass
from typing import Iterable, Iterator

import numpy as np
import torch
import zstandard

from .digest import format_digest, parse_digest, state_digest, tensor_digest
from .expctx import IDX_ENC_EXPCTX, decode_indices_expctx, encode_indices_expctx
from .indices import IDX_ENC_RICE, decode_indices, decode_indices_rice, encode_indices, encode_indices_rice
from .values import VAL_ENC_ZZ, ValueFormatError, decode_values_zz, encode_values_zz

MAGIC = b"PULSEPT1"
FORMAT_VERSION = 1  # what encoders emit unless asked for another version
FORMAT_VERSION_V2 = 2
FORMAT_VERSION_V3 = 3
SUPPORTED_FORMAT_VERSIONS = (FORMAT_VERSION, FORMAT_VERSION_V2, FORMAT_VERSION_V3)
# Versions whose values are zz-delta against the old cell (decoded with the old cells).
DELTA_VALUE_VERSIONS = (FORMAT_VERSION_V2, FORMAT_VERSION_V3)
INDEX_ENCODING = "delta-u16-esc"
MAX_NUMEL = 2**32  # v1 first-index is u32 (C2.4)
ZSTD_LEVEL = 1  # paper: zstd-1
# encode_step(workers>1) encodes smaller tensors inline: below this size the
# pool's hand-off and GIL contention cost more than the parallel work saves.
POOL_MIN_NUMEL = 2**18


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
    format_version: int = FORMAT_VERSION  # which encodings idx_bytes/val_bytes use

    def to_header(self) -> dict:
        h = {
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
        if self.format_version in DELTA_VALUE_VERSIONS:
            h["idxEnc"] = IDX_ENC_EXPCTX if self.format_version == FORMAT_VERSION_V3 else IDX_ENC_RICE
            h["valEnc"] = VAL_ENC_ZZ
        return h


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
    if c.device.type == "cpu":
        # CPU path in numpy: single-threaded, releases the GIL, and never
        # touches torch's intra-op pool, whose per-op fan-out dominated the
        # cost on many-core hosts. Same indices (ascending) and value bytes.
        cn, pn = c.numpy(), p.numpy()
        nidx = np.flatnonzero(cn != pn)
        return nidx.astype(np.uint64, copy=False), cn[nidx].tobytes()
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


def _check_format_version(format_version: int) -> None:
    if format_version not in SUPPORTED_FORMAT_VERSIONS:
        raise PatchFormatError(f"unsupported formatVersion {format_version}")


def build_patch(
    *,
    step: int,
    base_step: int,
    model: ModelMeta,
    entries: list[TensorEntry],
    base_state_digest: str | None = None,
    format_version: int = FORMAT_VERSION,
) -> tuple[bytes, str]:
    """Assemble a PULSEPT1 patch; returns (patch bytes, stateDigest string).

    Every entry must be encoded for ``format_version`` (``TensorEntry.format_version``).
    """
    _check_format_version(format_version)
    if any(e.format_version != format_version for e in entries):
        raise PatchFormatError(f"entries are not all encoded for formatVersion {format_version}")
    entries = sorted(entries, key=lambda e: e.name)
    sdig = state_digest([e.digest for e in entries])
    header = {
        "formatVersion": format_version,
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
    format_version: int = FORMAT_VERSION

    def tensor_payload(self, i: int) -> tuple[bytes, bytes]:
        idx_start, val_start, end = self._offsets[i]
        return self._payload[idx_start:val_start], self._payload[val_start:end]

    def tensor_indices(self, i: int, old: torch.Tensor | None = None, threads: int | None = None) -> np.ndarray:
        """Decode tensor ``i``'s changed flat indices (int64), per the patch's format.

        v3 decodes against the whole tensor before the apply (``old``, any
        shape, bf16 or its int16 view); v1 and v2 ignore it. ``threads``
        splits a v3 decode within the tensor (see ``expctx``).
        """
        th = self.tensors[i]
        idx_b, _ = self.tensor_payload(i)
        if self.format_version == FORMAT_VERSION_V3:
            if old is None:
                raise PatchFormatError("format v3 indices decode against the old tensor")
            cells = old.reshape(-1).view(torch.int16).numpy()
            if cells.size != th["numel"]:
                raise PatchApplyError(f"{th['name']}: old tensor has {cells.size} cells, header says {th['numel']}")
            return decode_indices_expctx(idx_b, th["changed"], cells, threads)
        if self.format_version == FORMAT_VERSION_V2:
            return decode_indices_rice(idx_b, th["changed"], th["numel"])
        return decode_indices(idx_b, th["changed"])

    def tensor_values(self, i: int, old: torch.Tensor) -> torch.Tensor:
        """Tensor ``i``'s new cells as int16 bf16 bit patterns, in index order.

        ``old`` holds the cells at the decoded indices before the apply
        (int16 view); v1 ignores it (absolute values), v2 decodes against it.
        """
        th = self.tensors[i]
        _, val_b = self.tensor_payload(i)
        if self.format_version in DELTA_VALUE_VERSIONS:
            try:
                new = decode_values_zz(val_b, old.numpy(), th["changed"])
            except ValueFormatError as e:
                raise PatchApplyError(f"{th['name']}: {e}") from e
            return torch.from_numpy(new)
        if len(val_b) != 2 * th["changed"]:
            raise PatchApplyError(f"value bytes {len(val_b)} != 2 x changed {th['changed']}")
        return torch.from_numpy(np.frombuffer(val_b, dtype="<i2").copy())


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
    version = header.get("formatVersion")
    if version not in SUPPORTED_FORMAT_VERSIONS:
        raise PatchFormatError(f"unsupported formatVersion {version}")
    try:
        tensors = _validate_tensor_headers(header["tensors"], version)
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
            format_version=version,
        )
    except (KeyError, TypeError) as e:
        raise PatchFormatError(f"malformed header: {e!r}") from e


def _validate_tensor_headers(tensors: list[dict], version: int = FORMAT_VERSION) -> list[dict]:
    names = [t["name"] for t in tensors]
    if names != sorted(names) or len(set(names)) != len(names):
        raise PatchFormatError("tensors[] must be unique and name-ascending")
    v2 = version in DELTA_VALUE_VERSIONS
    idx_enc = {FORMAT_VERSION_V2: IDX_ENC_RICE, FORMAT_VERSION_V3: IDX_ENC_EXPCTX}.get(version, INDEX_ENCODING)
    for t in tensors:
        if t["dtype"] != "bf16":
            raise PatchFormatError(f"{t['name']}: v1 admits dtype bf16 only")
        if t["idxEnc"] != idx_enc:
            raise PatchFormatError(f"{t['name']}: unknown idxEnc {t['idxEnc']!r}")
        if t["numel"] != int(np.prod(t["shape"], dtype=np.int64)) or t["numel"] >= MAX_NUMEL:
            raise PatchFormatError(f"{t['name']}: inconsistent or oversized numel")
        if v2:
            if t["valEnc"] != VAL_ENC_ZZ:
                raise PatchFormatError(f"{t['name']}: unknown valEnc {t['valEnc']!r}")
            # zz-delta-u8esc: one byte per cell plus two per escape, so
            # changed <= valBytes <= 3 x changed.
            if not t["changed"] <= t["valBytes"] <= 3 * t["changed"]:
                raise PatchFormatError(f"{t['name']}: valBytes inconsistent with changed")
        elif t["valBytes"] != 2 * t["changed"]:
            raise PatchFormatError(f"{t['name']}: valBytes != 2 x changed")
    return tensors


def _encode_entry(
    name: str, cur: torch.Tensor, prev: torch.Tensor, format_version: int = FORMAT_VERSION
) -> TensorEntry:
    """One tensor's patch entry: diff ``cur`` against ``prev``, encode, digest ``cur``.

    v2 encodes the new cells against the ``prev`` cells they replace, which
    the producer holds anyway: it is the diff base. v3 also codes the indices
    against the whole ``prev`` tensor.
    """
    _check_format_version(format_version)
    idx, vals = diff_tensor(cur, prev)
    if format_version in DELTA_VALUE_VERSIONS:
        prev_cells = prev.reshape(-1).view(torch.int16).numpy()
        old = prev_cells[idx.astype(np.int64, copy=False)]
        if format_version == FORMAT_VERSION_V3:
            idx_bytes = encode_indices_expctx(idx, prev_cells)
        else:
            idx_bytes = encode_indices_rice(idx)
        vals = encode_values_zz(old, np.frombuffer(vals, dtype="<i2"))
    else:
        idx_bytes = encode_indices(idx)
    return TensorEntry(
        name, tuple(cur.shape), cur.numel(), int(idx.size), idx_bytes, vals, tensor_digest(cur), format_version
    )


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
        workers: int = 1,
        format_version: int = FORMAT_VERSION,
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
        idx_bytes, val_bytes (both as encoded; v1 values are raw bf16), from
        the entries already built for the patch; measurement adds no extra
        tensor work. None (the default) is byte-and-behavior identical to before.

        ``workers`` > 1 diffs, index-encodes and digests tensors of at least
        ``POOL_MIN_NUMEL`` elements on a thread pool of that size; the update
        iterable is still consumed, validated and copied on the calling
        thread, in order. Patch bytes and digests are identical for every
        value; 1 (the default) runs serially.

        ``format_version`` selects the patch encoding: 1 (the default, C2.4)
        or 2 (#2156: rice-gap indices, zigzag-delta values). Both apply to
        the same bytes and digests.
        """
        if workers < 1:
            raise ValueError(f"workers must be >= 1, got {workers}")
        _check_format_version(format_version)
        pending: list[Future | TensorEntry] = []
        seen: set[str] = set()
        staged: dict[str, torch.Tensor] = {}
        pool = ThreadPoolExecutor(max_workers=workers, thread_name_prefix="pulse-encode") if workers > 1 else None
        try:
            # Validation and the copy stay on the calling thread, in update
            # order: the iterable may be a lazy generator that reuses buffers,
            # and errors must surface for the first offending tensor.
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
                if cur.shape != prev.shape:
                    raise PatchFormatError(f"shape mismatch: {tuple(cur.shape)} vs {tuple(prev.shape)}")
                staged[name] = cur
                if pool is None or cur.numel() < POOL_MIN_NUMEL:
                    pending.append(_encode_entry(name, cur, prev, format_version))
                else:
                    # Diff, index encoding and digest are numpy/xxhash work
                    # that releases the GIL, so tensors encode in parallel.
                    pending.append(pool.submit(_encode_entry, name, cur, prev, format_version))
            entries = [p.result() if isinstance(p, Future) else p for p in pending]
        finally:
            if pool is not None:
                pool.shutdown(wait=True, cancel_futures=True)
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
            step=step, base_step=base_step, model=model, entries=entries, base_state_digest=base_state_digest,
            format_version=format_version,
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
            idx = patch.tensor_indices(i, t)
            if idx.size and int(idx[-1]) >= t.numel():
                raise PatchApplyError(f"{th['name']}: index out of range")
            if patch.format_version == FORMAT_VERSION:
                apply_tensor_patch(t, idx, patch.tensor_payload(i)[1])
            else:
                # v2 values decode against the cells they replace.
                flat = t.reshape(-1).view(torch.int16)
                tidx = torch.from_numpy(idx)
                flat[tidx] = patch.tensor_values(i, flat[tidx])
            d = tensor_digest(t)
            if d != parse_digest(th["tensorDigest"]):
                raise PatchApplyError(f"{th['name']}: tensor digest mismatch after apply (I3 violation)")
            digests.append(d)
        sdig = format_digest(state_digest(digests))
        if sdig != patch.state_digest:
            raise PatchApplyError("state digest mismatch after apply (I3 violation)")
        return sdig
