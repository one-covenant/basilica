# Vendored from pulse-verl (basilica-backend python/pulse-verl/src/pulse_verl/values.py)
# for the publisher surface (format v2, #2156). The NORMATIVE spec is
# docs/architecture/RL-TRAINING-API-CONTRACTS.md C2.4-C2.5 in basilica-backend;
# this copy must track it byte-for-byte (parity pinned by tests/test_pulse_codec.py).
# Heavy deps (torch/numpy/xxhash/zstandard/safetensors) are publisher-only:
# this package is imported LAZILY - never at basilica import time.
"""Value encodings for the changed BF16 cells of a patch tensor.

v1 (C2.4) carries the new cells as absolute BF16 (``valBytes = 2 x changed``)
and apply is a plain copy. Format v2 (#2156) carries ``zz-delta-u8esc``: per
changed cell, the difference of the BF16 bit patterns against the OLD cell,
so the decoder needs the cells it is about to overwrite.

``zz-delta-u8esc`` layout, for ``changed`` = n cells in index order:

  d_i = (new_i - old_i) mod 2^16       (uint16 views of the bf16 cells)
  z_i = zigzag(int16(d_i))             = (d << 1) ^ (d >> 15), as uint16
  [n bytes: u8 symbol s_i = z_i if z_i < 0xFF else 0xFF]
  [E x u16 LE: z_i of every escaped cell (s_i == 0xFF), in index order]

so ``valBytes = n + 2 E``. The arithmetic is modular integer arithmetic on
the bit patterns, never floating point, so the reconstruction is bit-exact
for every pattern (signed zeros, NaN payloads, infinities, exponent and sign
changes). Most training updates move a cell by one or two BF16 ULPs, i.e.
z in {1..4}, which zstd then squeezes to about 3 bits per cell.

A changed cell has d != 0, so z == 0 never occurs; the decoder rejects it,
and rejects escapes that would have fit in a byte (one canonical encoding
per update).
"""

from __future__ import annotations

import numpy as np

VAL_ENC_ZZ = "zz-delta-u8esc"
ZZ_ESCAPE = 0xFF


class ValueFormatError(ValueError):
    pass


def encode_values_zz(old: np.ndarray, new: np.ndarray) -> bytes:
    """Encode new cells against old ones (both bf16 bit patterns, int16 or uint16)."""
    if old.shape != new.shape:
        raise ValueFormatError(f"old/new cell counts differ: {old.size} vs {new.size}")
    if new.size == 0:
        return b""
    d = (new.view(np.uint16) - old.view(np.uint16)).view(np.int16).astype(np.int32)
    z = ((d << 1) ^ (d >> 15)).astype(np.uint16)
    small = z < ZZ_ESCAPE
    sym = np.where(small, z, ZZ_ESCAPE).astype(np.uint8)
    if small.all():
        return sym.tobytes()
    return sym.tobytes() + z[~small].astype("<u2").tobytes()


def decode_values_zz(buf: bytes, old: np.ndarray, changed: int) -> np.ndarray:
    """Decode to the new cells' bf16 bit patterns (int16), given the old ones."""
    if old.size != changed:
        raise ValueFormatError(f"{old.size} old cells for changed={changed}")
    if changed == 0:
        if buf:
            raise ValueFormatError("nonempty value bytes with changed=0")
        return np.empty(0, dtype=np.int16)
    if len(buf) < changed:
        raise ValueFormatError("truncated value stream")
    sym = np.frombuffer(buf, np.uint8, count=changed)
    esc = sym == ZZ_ESCAPE
    n_esc = int(np.count_nonzero(esc))
    if len(buf) != changed + 2 * n_esc:
        raise ValueFormatError(f"value stream is {len(buf)} bytes, symbols imply {changed + 2 * n_esc}")
    z = sym.astype(np.uint16)
    if n_esc:
        big = np.frombuffer(buf, "<u2", offset=changed, count=n_esc)
        if int(big.min()) < ZZ_ESCAPE:
            raise ValueFormatError("non-canonical escaped value")
        z[esc] = big
    if not z.all():
        raise ValueFormatError("zero delta for a changed cell")
    zi = z.astype(np.int32)
    d = (((zi >> 1) ^ -(zi & 1)) & 0xFFFF).astype(np.uint16)
    return (old.view(np.uint16) + d).view(np.int16)
