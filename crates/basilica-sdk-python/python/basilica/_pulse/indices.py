# Vendored from pulse-verl (basilica-backend python/pulse-verl/src/pulse_verl/indices.py)
# for the publisher surface (#1666 / SDK 0.36). The NORMATIVE spec is
# docs/architecture/RL-TRAINING-API-CONTRACTS.md C2.4-C2.5 in basilica-backend;
# this copy must track it byte-for-byte (parity pinned by tests/test_pulse_codec.py).
# Heavy deps (torch/numpy/xxhash/zstandard/safetensors) are publisher-only:
# this package is imported LAZILY - never at basilica import time.
"""C2.4 ``delta-u16-esc`` index encoding.

Indices are element offsets into the C-contiguous flattened tensor, sorted
strictly ascending. Encoding: first index as u32 LE; each subsequent gap as
u16 LE, except gaps >= 0xFFFF which are the u16 marker 0xFFFF followed by the
u32 LE gap. Both forms are even-length, so the stream after the first u32
stays on a 2-byte grid — but payload cells of an escape may themselves equal
0xFFFF, so markers are only identifiable by a sequential walk (cheap: escapes
are rare at PULSE sparsity).
"""

from __future__ import annotations

import numpy as np

GAP_ESCAPE = 0xFFFF
MAX_FIRST_INDEX = 2**32 - 1


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
