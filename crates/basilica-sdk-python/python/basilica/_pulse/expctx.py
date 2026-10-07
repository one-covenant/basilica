# Vendored from pulse-verl (basilica-backend python/pulse-verl/src/pulse_verl/expctx.py)
# for the publisher surface. The NORMATIVE spec is
# docs/architecture/RL-TRAINING-API-CONTRACTS.md C2.4.2 in basilica-backend;
# this copy must track it byte-for-byte (parity pinned by tests/test_pulse_codec.py).
# Heavy deps (torch/numpy/xxhash/zstandard/safetensors) are publisher-only:
# this package is imported LAZILY - never at basilica import time.
"""Patch index encoding ``exp-class-rice`` (format v3): changed cells coded
per class of their OLD cell's BF16 exponent.

At a low learning rate a BF16 weight changes only when its update crosses
half an ULP, and the ULP is set by the exponent, so the old exponent predicts
which cells change. Both ends hold the old cells (the producer diffs against
them; the consumer's snapshot is the digest-verified previous state, which v2
values already read), so the context costs nothing on the wire. On recorded
Qwen3-8B patches it halves the index stream again (about 4.3 bits per changed
cell against 9.3 for ``rice-gap``).

Cells are partitioned by the exponent byte ``(old >> 7) & 0xFF`` of their old
value. Each class with changes codes the ranks of its changed cells inside
the class (the number of earlier cells of the same class), or, when more than
half the class changed, the ranks of its unchanged cells. A rank list is one
v2 ``rice-gap`` block (``indices.encode_indices_rice`` over the ranks, whose
"numel" is the class size). Layout of one tensor's stream (empty when
nothing changed):

  u16 LE  L                  class records, 1..256
  L x     u8     exp         strictly ascending
          u8     flags       bit 0: complement; other bits zero
          u32 LE m           ranks coded in the block
          u32 LE B           block byte length
  L x     B bytes            rice-gap block of the m ranks

A class with c changed cells out of n_k is a complement class iff 2c > n_k;
then m = n_k - c (m = 0 and B = 0 when the whole class changed), otherwise
m = c >= 1. The decoder rejects every other combination, the class sizes it
computes itself, a total other than ``changed``, and trailing bytes, so a
corrupted stream raises ``IndexFormatError`` instead of yielding indices.

Two implementations produce identical bytes: the native path in the SDK's
extension module (``basilica._basilica``, Rust), about 1 ns per cell, and
the numpy reference below, roughly ten times slower. The native path is used
unless ``BASILICA_PULSE_EXPCTX_NATIVE=0``.
"""

from __future__ import annotations

import os

import numpy as np

from .indices import IndexFormatError, decode_indices_rice, encode_indices_rice

IDX_ENC_EXPCTX = "exp-class-rice"
_RECORD = 10
_MAX_NUMEL = 2**32

try:  # pragma: no cover - depends on the build
    from basilica._basilica import _pulse_expctx_decode, _pulse_expctx_encode
except ImportError:  # pragma: no cover
    _pulse_expctx_encode = _pulse_expctx_decode = None


def native_available() -> bool:
    return _pulse_expctx_encode is not None and os.environ.get("BASILICA_PULSE_EXPCTX_NATIVE", "1") != "0"


def _decode_threads() -> int:
    raw = os.environ.get("BASILICA_PULSE_EXPCTX_THREADS", "").strip()
    if raw:
        try:
            return max(1, int(raw))
        except ValueError:
            pass
    return max(1, min(8, os.cpu_count() or 1))


def _cells(old: np.ndarray) -> np.ndarray:
    """The old tensor's cells as a flat, C-contiguous uint16 view (no copy when possible)."""
    a = np.ascontiguousarray(old).reshape(-1)
    if a.dtype.itemsize != 2:
        raise IndexFormatError(f"old cells must be 16-bit, got {a.dtype}")
    return a.view(np.uint16)


def _classes(cells: np.ndarray) -> np.ndarray:
    return ((cells >> 7) & 0xFF).astype(np.uint8)


def encode_indices_expctx(idx: np.ndarray, old: np.ndarray) -> bytes:
    """Encode sorted, strictly ascending changed indices against the old cells."""
    cells = _cells(old)
    idx = np.asarray(idx)
    if idx.size == 0:
        return b""
    if cells.size >= _MAX_NUMEL:
        raise IndexFormatError("numel exceeds 2^32")
    idx = idx.astype(np.int64, copy=False)
    if int(idx[0]) < 0 or int(idx[-1]) >= cells.size or (idx.size > 1 and int(np.diff(idx).min()) <= 0):
        raise IndexFormatError("indices must be strictly ascending and in range")
    if native_available():
        try:
            return _pulse_expctx_encode(cells.astype("<u2", copy=False).tobytes(), idx.astype("<u8").tobytes())
        except ValueError as e:
            raise IndexFormatError(str(e)) from e
    lab = _classes(cells)
    sizes = np.bincount(lab, minlength=256)
    cls = lab[idx]
    records, blocks = [], []
    for e in np.unique(cls):
        members = np.flatnonzero(lab == e)
        ranks = np.searchsorted(members, idx[cls == e])
        c, nk = ranks.size, int(sizes[e])
        comp = 2 * c > nk
        if comp:
            keep = np.ones(nk, dtype=bool)
            keep[ranks] = False
            ranks = np.flatnonzero(keep)
        block = encode_indices_rice(ranks)
        records.append(
            bytes([int(e), int(comp)])
            + np.array([ranks.size, len(block)], dtype="<u4").tobytes()
        )
        blocks.append(block)
    return np.array([len(records)], dtype="<u2").tobytes() + b"".join(records) + b"".join(blocks)


def decode_indices_expctx(buf: bytes, changed: int, old: np.ndarray, threads: int | None = None) -> np.ndarray:
    """Decode to int64 changed indices, strictly ascending, all < old.size.

    ``threads`` splits one tensor's scan (native only); default
    ``BASILICA_PULSE_EXPCTX_THREADS``, else min(8, CPUs).
    """
    cells = _cells(old)
    n = cells.size
    if changed == 0:
        if buf:
            raise IndexFormatError("nonempty index bytes with changed=0")
        return np.empty(0, dtype=np.int64)
    if changed > n:
        raise IndexFormatError(f"changed={changed} exceeds numel={n}")
    if native_available():
        try:
            out = _pulse_expctx_decode(cells.astype("<u2", copy=False).tobytes(), int(changed), bytes(buf), threads or _decode_threads())
        except ValueError as e:
            raise IndexFormatError(str(e)) from e
        return np.frombuffer(out, dtype="<u8").astype(np.int64)
    if len(buf) < 2:
        raise IndexFormatError("truncated index stream")
    count = int(np.frombuffer(buf, "<u2", count=1)[0])
    if not 1 <= count <= 256:
        raise IndexFormatError(f"class record count {count} out of range")
    pos = 2 + count * _RECORD
    if len(buf) < pos:
        raise IndexFormatError("truncated class records")
    lab = _classes(cells)
    sizes = np.bincount(lab, minlength=256)
    parts, total, prev = [], 0, -1
    for r in range(count):
        rec = buf[2 + r * _RECORD : 2 + (r + 1) * _RECORD]
        e, flags = rec[0], rec[1]
        m, b = (int(x) for x in np.frombuffer(rec, "<u4", offset=2, count=2))
        if e <= prev:
            raise IndexFormatError("class records must be strictly ascending by exponent")
        prev = e
        if flags & ~1:
            raise IndexFormatError("unknown class flags")
        comp = bool(flags & 1)
        if not comp and m == 0:
            raise IndexFormatError("empty non-complement class record")
        if pos + b > len(buf):
            raise IndexFormatError("truncated class block")
        nk = int(sizes[e])
        if m > nk:
            raise IndexFormatError(f"class {e}: rank beyond the class size")
        ranks = decode_indices_rice(bytes(buf[pos : pos + b]), m, nk) if m else _empty_block(buf[pos : pos + b])
        pos += b
        c = nk - m if comp else m
        if (2 * c > nk) != comp:
            raise IndexFormatError(f"class {e}: non-canonical complement flag")
        members = np.flatnonzero(lab == e)
        if comp:
            keep = np.ones(nk, dtype=bool)
            keep[ranks] = False
            parts.append(members[keep])
        else:
            parts.append(members[ranks])
        total += c
    if pos != len(buf):
        raise IndexFormatError("trailing bytes after the class blocks")
    if total != changed:
        raise IndexFormatError(f"stream decodes to {total} indices, header says changed={changed}")
    return np.sort(np.concatenate(parts)).astype(np.int64, copy=False)


def _empty_block(b: bytes) -> np.ndarray:
    if b:
        raise IndexFormatError("nonempty block with zero codes")
    return np.empty(0, dtype=np.int64)
