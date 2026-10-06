# Vendored from pulse-verl (basilica-backend python/pulse-verl/src/pulse_verl/indices.py)
# for the publisher surface (#1666 / SDK 0.36). The NORMATIVE spec is
# docs/architecture/RL-TRAINING-API-CONTRACTS.md C2.4-C2.5 in basilica-backend;
# this copy must track it byte-for-byte (parity pinned by tests/test_pulse_codec.py).
# Heavy deps (torch/numpy/xxhash/zstandard/safetensors) are publisher-only:
# this package is imported LAZILY - never at basilica import time.
"""Patch index encodings: C2.4 ``delta-u16-esc`` (v1) and ``rice-gap`` (v2, #2156).

Indices are element offsets into the C-contiguous flattened tensor, sorted
strictly ascending. Encoding: first index as u32 LE; each subsequent gap as
u16 LE, except gaps >= 0xFFFF which are the u16 marker 0xFFFF followed by the
u32 LE gap. Both forms are even-length, so the stream after the first u32
stays on a 2-byte grid — but payload cells of an escape may themselves equal
0xFFFF, so markers are only identifiable by a sequential walk (cheap: escapes
are rare at PULSE sparsity).
"""

from __future__ import annotations

import math

import numpy as np

GAP_ESCAPE = 0xFFFF
MAX_FIRST_INDEX = 2**32 - 1

IDX_ENC_RICE = "rice-gap"
RICE_Q_ESCAPE = 32
RICE_MAX_K = 31


class IndexFormatError(ValueError):
    pass


def encode_indices(idx: np.ndarray) -> bytes:
    """Encode sorted, strictly-ascending indices; empty input encodes to ``b""``."""
    if idx.size == 0:
        return b""
    idx = idx.astype(np.uint64, copy=False)
    if int(idx[0]) > MAX_FIRST_INDEX:
        raise IndexFormatError("first index exceeds u32 (v1 numel limit, C2.4)")
    first = np.array([idx[0]], dtype="<u4").tobytes()
    if idx.size == 1:
        return first
    gaps = np.diff(idx.astype(np.int64))
    if int(gaps.min()) < 1:
        raise IndexFormatError("indices must be strictly ascending")
    return first + _encode_gaps(gaps)


def _encode_gaps(gaps: np.ndarray) -> bytes:
    if int(gaps.max()) >= 2**32:
        raise IndexFormatError("gap exceeds u32 (v1 numel limit)")
    small = gaps < GAP_ESCAPE
    sizes = np.where(small, 2, 6).astype(np.int64)
    offs = np.zeros(gaps.size, dtype=np.int64)
    np.cumsum(sizes[:-1], out=offs[1:])
    buf = np.zeros(int(sizes.sum()), dtype=np.uint8)
    u16 = np.where(small, gaps, GAP_ESCAPE).astype("<u2").view(np.uint8).reshape(-1, 2)
    buf[offs] = u16[:, 0]
    buf[offs + 1] = u16[:, 1]
    if not small.all():
        big = gaps[~small].astype("<u4").view(np.uint8).reshape(-1, 4)
        boffs = offs[~small]
        for k in range(4):
            buf[boffs + 2 + k] = big[:, k]
    return buf.tobytes()


def decode_indices(buf: bytes, changed: int) -> np.ndarray:
    """Decode to int64 absolute indices; validates the count against ``changed``."""
    if changed == 0:
        if buf:
            raise IndexFormatError("nonempty index bytes with changed=0")
        return np.empty(0, dtype=np.int64)
    if len(buf) < 4 or (len(buf) - 4) % 2 != 0:
        raise IndexFormatError("truncated index stream")
    first = int(np.frombuffer(buf, "<u4", count=1)[0])
    gaps = _decode_gaps(np.frombuffer(buf, "<u2", offset=4))
    idx = np.empty(gaps.size + 1, dtype=np.int64)
    idx[0] = first
    if gaps.size:
        np.cumsum(gaps, out=idx[1:])
        idx[1:] += first
    if idx.size != changed:
        raise IndexFormatError(f"decoded {idx.size} indices, header says changed={changed}")
    return idx


def _decode_gaps(cells: np.ndarray) -> np.ndarray:
    parts: list[np.ndarray] = []
    pos = 0
    for m in np.flatnonzero(cells == GAP_ESCAPE):
        if m < pos:  # payload cell of a prior escape, not a marker
            continue
        if m + 3 > cells.size:
            raise IndexFormatError("truncated escape sequence")
        parts.append(cells[pos:m].astype(np.int64))
        parts.append(np.array([int(cells[m + 1]) | (int(cells[m + 2]) << 16)], dtype=np.int64))
        pos = m + 3
    parts.append(cells[pos:].astype(np.int64))
    gaps = np.concatenate(parts)
    if gaps.size and int(gaps.min()) < 1:
        raise IndexFormatError("non-positive gap")
    return gaps


# --- rice-gap (format v2) -----------------------------------------------------
#
# Same index set as delta-u16-esc, Golomb-Rice coded. For n = changed sorted
# indices i_0 < ... < i_{n-1}, the coded values are v_0 = i_0 and
# v_j = i_j - i_{j-1} - 1 (j >= 1), so every v >= 0 and a duplicate or
# descending index cannot be expressed. Each v is split with the per-tensor
# parameter k into a quotient q = v >> k and a k-bit remainder. Layout:
#
#   u8      k                 0..31, chosen by the encoder (``rice_parameter``)
#   u32 LE  U                 byte length of the unary stream
#   U bytes unary stream      per v, in order: min(q, 32) one-bits then a zero
#                             bit; bits are packed MSB-first and the last byte
#                             is padded with ONE bits (fewer than 8), so the
#                             stream holds exactly n zero bits
#   R bytes remainder stream  per v, in order: the low k bits of v, MSB-first,
#                             packed back to back; R = ceil(n * k / 8), the
#                             last byte zero-padded
#   E x u32 LE escapes        the full quotient q of every v whose unary code
#                             was the escape (32 one-bits), in order; q >= 32
#
# The escape bounds a huge gap (up to 2^32 - 1) at 65 + k bits instead of a
# multi-megabyte unary run. An empty tensor (n = 0) encodes to b"". The
# encoder picks k by exact bit cost among the neighbours of two estimates:
# floor(log2(mean(v) * ln 2)), the optimum for geometric gaps, and
# floor(log2(median(v))) over a fixed subsample, which a few huge gaps cannot
# drag up; the decoder never needs to know how k was chosen. Decoding is
# vectorized numpy: one unpackbits + flatnonzero for the unary stream and a
# 4-byte (8-byte for k > 25) big-endian gather per remainder.


def _rice_cost_bits(v: np.ndarray, k: int) -> int:
    q = v >> k
    n_esc = int(np.count_nonzero(q >= RICE_Q_ESCAPE))
    return int(np.minimum(q, RICE_Q_ESCAPE).sum()) + v.size * (1 + k) + 32 * n_esc


def rice_parameter(v: np.ndarray) -> int:
    """Rice parameter for the coded values ``v`` (non-empty, >= 0)."""

    def estimate(x: float) -> int:
        return math.floor(math.log2(x)) if x >= 1 else 0

    by_mean = estimate(float(v.mean()) * math.log(2))
    by_median = estimate(float(np.median(v[:: max(1, v.size // 4096)])))
    candidates = {k for e in (by_mean, by_median) for k in (e - 1, e, e + 1) if 0 <= k <= RICE_MAX_K}
    return min(sorted(candidates), key=lambda k: _rice_cost_bits(v, k))


def _pack_fixed(r: np.ndarray, k: int) -> bytes:
    """Pack uint32 values, k (1..31) low bits each, MSB-first back to back."""
    w = (k + 7) // 8
    be = r.astype(">u4").view(np.uint8).reshape(-1, 4)[:, 4 - w :]
    bits = np.unpackbits(be, axis=1)[:, 8 * w - k :]
    return np.packbits(np.ascontiguousarray(bits).reshape(-1)).tobytes()


def _unpack_fixed(buf: np.ndarray, n: int, k: int) -> np.ndarray:
    """Inverse of ``_pack_fixed``: n k-bit fields from ``buf`` as int64."""
    pos = np.arange(n, dtype=np.int64) * k
    byte, shift = pos >> 3, pos & 7
    p = np.concatenate([buf, np.zeros(8, dtype=np.uint8)])
    if k <= 25:  # k + 7 bits fit one 32-bit window
        w = p[byte].astype(np.uint32) << 24
        w |= p[byte + 1].astype(np.uint32) << 16
        w |= p[byte + 2].astype(np.uint32) << 8
        w |= p[byte + 3].astype(np.uint32)
        return ((w >> (32 - k - shift).astype(np.uint32)) & np.uint32((1 << k) - 1)).astype(np.int64)
    w64 = np.zeros(n, dtype=np.uint64)
    for j in range(8):
        w64 |= p[byte + j].astype(np.uint64) << np.uint64(56 - 8 * j)
    return ((w64 >> (64 - k - shift).astype(np.uint64)) & np.uint64((1 << k) - 1)).astype(np.int64)


def encode_indices_rice(idx: np.ndarray) -> bytes:
    """Encode sorted, strictly-ascending indices as ``rice-gap``; empty input encodes to ``b""``."""
    if idx.size == 0:
        return b""
    idx = idx.astype(np.int64, copy=False)
    if int(idx[0]) < 0 or int(idx[0]) > MAX_FIRST_INDEX:
        raise IndexFormatError("first index exceeds u32 (numel limit, C2.4)")
    v = np.empty(idx.size, dtype=np.int64)
    v[0] = idx[0]
    np.subtract(idx[1:], idx[:-1], out=v[1:])
    v[1:] -= 1
    if int(v.min()) < 0:
        raise IndexFormatError("indices must be strictly ascending")
    if int(v.max()) > MAX_FIRST_INDEX:
        raise IndexFormatError("gap exceeds u32 (numel limit)")
    k = rice_parameter(v)
    q = v >> k
    ends = np.cumsum(np.minimum(q, RICE_Q_ESCAPE) + 1) - 1
    bits = np.ones(-(-(int(ends[-1]) + 1) // 8) * 8, dtype=np.uint8)  # one-bit padding
    bits[ends] = 0
    unary = np.packbits(bits).tobytes()
    rem = _pack_fixed((v & ((1 << k) - 1)).astype(np.uint32), k) if k else b""
    escapes = q[q >= RICE_Q_ESCAPE].astype("<u4").tobytes()
    return bytes([k]) + np.array([len(unary)], dtype="<u4").tobytes() + unary + rem + escapes


def decode_indices_rice(buf: bytes, changed: int, numel: int) -> np.ndarray:
    """Decode ``rice-gap`` to int64 absolute indices, all < ``numel``.

    Validates the stream exactly: the count against ``changed``, the padding,
    the escapes, the total length, and the range, so a truncated or corrupted
    stream raises ``IndexFormatError`` instead of yielding indices.
    """
    if changed == 0:
        if buf:
            raise IndexFormatError("nonempty index bytes with changed=0")
        return np.empty(0, dtype=np.int64)
    if changed > numel:
        raise IndexFormatError(f"changed={changed} exceeds numel={numel}")
    if len(buf) < 5:
        raise IndexFormatError("truncated index stream")
    k = buf[0]
    if k > RICE_MAX_K:
        raise IndexFormatError(f"rice parameter {k} exceeds {RICE_MAX_K}")
    ulen = int(np.frombuffer(buf, "<u4", offset=1, count=1)[0])
    rlen = (changed * k + 7) // 8
    if 5 + ulen + rlen > len(buf):
        raise IndexFormatError("truncated index stream")
    zeros = np.flatnonzero(np.unpackbits(np.frombuffer(buf, np.uint8, offset=5, count=ulen)) == 0)
    if zeros.size != changed:
        raise IndexFormatError(f"unary stream holds {zeros.size} codes, header says changed={changed}")
    # Only one-bit padding may follow the last terminator, less than a byte of it.
    if ulen != int(zeros[-1]) // 8 + 1:
        raise IndexFormatError("malformed unary stream padding")
    q = np.diff(zeros[:changed], prepend=-1) - 1
    if int(q.max()) > RICE_Q_ESCAPE:
        raise IndexFormatError("unary quotient exceeds the escape code")
    esc = q == RICE_Q_ESCAPE
    n_esc = int(np.count_nonzero(esc))
    if len(buf) != 5 + ulen + rlen + 4 * n_esc:
        raise IndexFormatError("index stream length does not match its escapes")
    if n_esc:
        big = np.frombuffer(buf, "<u4", offset=5 + ulen + rlen, count=n_esc).astype(np.int64)
        if int(big.min()) < RICE_Q_ESCAPE:
            raise IndexFormatError("non-canonical escaped quotient")
        q[esc] = big
    v = q << k
    if k:
        v |= _unpack_fixed(np.frombuffer(buf, np.uint8, offset=5 + ulen, count=rlen), changed, k)
    if int(v.max()) >= numel:
        raise IndexFormatError("index gap out of range")
    v += 1  # gaps, all >= 1: strictly ascending by construction
    if int(v.sum(dtype=np.uint64)) > numel:
        raise IndexFormatError("index out of range")
    idx = np.cumsum(v)
    idx -= 1
    return idx
