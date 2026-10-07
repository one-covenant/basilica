"""PULSE patch format v2 (#2156) in the vendored codec (basilica._pulse).

Mirrors basilica-backend python/pulse-verl/tests/test_codec_v2.py: a v2
patch applies to the SAME bytes and digests as the v1 patch of the same
update, and a damaged v2 stream raises the module's error instead of
writing garbage. Byte parity with pulse_verl's v2 encoder is pinned by
the golden fixture in test_pulse_codec.py."""

import json

import pytest

torch = pytest.importorskip("torch")
np = pytest.importorskip("numpy")
pytest.importorskip("xxhash")
pytest.importorskip("zstandard")

from basilica._pulse.codec import (  # noqa: E402
    FORMAT_VERSION,
    FORMAT_VERSION_V2,
    MAGIC,
    ModelMeta,
    PatchApplyError,
    PatchFormatError,
    Snapshot,
    TensorEntry,
    build_patch,
    parse_patch,
)
from basilica._pulse.codec import _encode_entry as encode_entry  # noqa: E402
from basilica._pulse.digest import tensor_digest  # noqa: E402
from basilica._pulse.indices import (  # noqa: E402
    RICE_Q_ESCAPE,
    IndexFormatError,
    decode_indices,
    decode_indices_rice,
    encode_indices,
    encode_indices_rice,
)
from basilica._pulse.values import ZZ_ESCAPE, ValueFormatError, decode_values_zz, encode_values_zz

SHAPES = {
    "model.embed.weight": (128, 257),
    "model.empty": (0,),
    "model.layers.0.mlp.weight": (3, 5, 7),
    "model.layers.0.norm": (64,),
    "model.one": (1,),
    "model.scalar": (),
    "model.z_last.weight": (40, 40),
}

# bf16 bit patterns worth crossing: zeros, extremes, specials, subnormals.
SPECIAL = np.array(
    [0x0000, 0x8000, 0x3F80, 0xBF80, 0x7F7F, 0xFF7F, 0x7F80, 0xFF80, 0x7FC0, 0xFFC0, 0x7F81, 0xFFFF,
     0x0001, 0x8001, 0x007F, 0x0080],
    dtype=np.uint16,
)


def rand_state(seed: int) -> dict[str, torch.Tensor]:
    g = torch.Generator().manual_seed(seed)
    return {n: torch.randn(s, generator=g).to(torch.bfloat16) for n, s in SHAPES.items()}


def training_like(state, frac, seed):
    """Mostly 1-2 ULP moves plus sign flips, exponent changes, specials and big jumps."""
    rng = np.random.default_rng(seed)
    out = {}
    for n, t in state.items():
        c = t.clone()
        flat = c.reshape(-1).view(torch.int16).numpy().view(np.uint16)
        if flat.size:
            k = min(flat.size, max(1, int(flat.size * frac)))
            pos = rng.choice(flat.size, size=k, replace=False)
            kind = rng.integers(0, 6, size=k)
            v = flat[pos].copy()
            v = np.where(kind == 0, v + 1, v)
            v = np.where(kind == 1, v - 2, v)
            v = np.where(kind == 2, v ^ 0x8000, v)  # sign flip
            v = np.where(kind == 3, v + 0x0080, v)  # exponent step
            v = np.where(kind == 4, SPECIAL[rng.integers(0, SPECIAL.size, size=k)], v)
            v = np.where(kind == 5, rng.integers(0, 2**16, size=k), v)
            flat[pos] = v.astype(np.uint16)
        out[n] = c
    return out


def encode(base, new, version, step=1):
    snap = Snapshot(base)
    meta = ModelMeta(id="t/m", digest=snap.digest(), param_count=snap.param_count())
    return snap.encode_step(new.items(), step=step, base_step=step - 1, model=meta, format_version=version)


def bits(snap: Snapshot) -> dict[str, bytes]:
    return {n: t.reshape(-1).view(torch.int16).numpy().tobytes() for n, t in snap.named_tensors()}


def header_of(patch: bytes) -> dict:
    hlen = int(np.frombuffer(patch, "<u4", offset=len(MAGIC), count=1)[0])
    return json.loads(patch[len(MAGIC) + 4 : len(MAGIC) + 4 + hlen])


def rice_rt(idx, numel):
    idx = np.asarray(idx, dtype=np.int64)
    return decode_indices_rice(encode_indices_rice(idx), int(idx.size), numel)


# --- rice-gap indices ----------------------------------------------------------


def test_rice_empty():
    assert encode_indices_rice(np.empty(0, dtype=np.int64)) == b""
    out = decode_indices_rice(b"", 0, 10)
    assert out.size == 0 and out.dtype == np.int64


@pytest.mark.parametrize("i", [0, 1, 2**16, 2**31, 2**32 - 2])
def test_rice_single_index(i):
    assert rice_rt([i], 2**32 - 1).tolist() == [i]


def test_rice_first_and_last_index():
    numel = 1000
    idx = [0, 1, 500, numel - 2, numel - 1]
    assert rice_rt(idx, numel).tolist() == idx
    assert rice_rt(np.arange(numel), numel).tolist() == list(range(numel))


@pytest.mark.parametrize("gap", [2, 33, 2**16, 2**20, 2**31, 2**32 - 2])
def test_rice_huge_gaps_take_the_escape(gap):
    # Many small gaps pin k low, so the big gap's quotient must escape.
    idx = np.concatenate([np.arange(200), [199 + gap], 199 + gap + np.arange(1, 50)])
    numel = int(idx[-1]) + 1
    enc = encode_indices_rice(idx)
    assert len(enc) < 200  # never a quotient-length unary run
    assert rice_rt(idx, numel).tolist() == idx.tolist()


@pytest.mark.parametrize("seed", range(6))
def test_rice_random_roundtrip_across_densities(seed):
    rng = np.random.default_rng(seed)
    numel = int(rng.integers(1, 2**24))
    n = int(rng.integers(1, min(numel, 50_000) + 1))
    idx = np.sort(rng.choice(numel, size=n, replace=False))
    assert np.array_equal(rice_rt(idx, numel), idx)
    assert encode_indices_rice(idx) == encode_indices_rice(idx.copy())  # deterministic


def test_rice_wide_remainders():
    # Very sparse: k climbs past the 32-bit window (k > 25 takes the 64-bit gather).
    idx = np.array([3, 2**30, 2**31 + 7, 2**32 - 2], dtype=np.int64)
    enc = encode_indices_rice(idx)
    assert enc[0] > 25
    assert rice_rt(idx, 2**32 - 1).tolist() == idx.tolist()


@pytest.mark.parametrize("bad", [[5, 5], [5, 4], [0, 3, 3, 9], [-1, 2]])
def test_rice_encoder_rejects_duplicate_and_descending(bad):
    with pytest.raises(IndexFormatError):
        encode_indices_rice(np.array(bad, dtype=np.int64))


def test_rice_rejects_range_and_count():
    enc = encode_indices_rice(np.array([2, 9]))
    with pytest.raises(IndexFormatError, match="out of range"):
        decode_indices_rice(enc, 2, 9)  # index 9 needs numel >= 10
    with pytest.raises(IndexFormatError):
        decode_indices_rice(enc, 3, 100)
    with pytest.raises(IndexFormatError):
        decode_indices_rice(enc, 1, 100)
    with pytest.raises(IndexFormatError):
        decode_indices_rice(b"\x00", 0, 100)


def test_rice_every_truncation_and_extension_raises():
    rng = np.random.default_rng(3)
    idx = np.sort(rng.choice(10_000, size=300, replace=False))
    idx = np.concatenate([idx, [10_000 + 2**20]])  # one escape at the end
    enc = encode_indices_rice(idx)
    numel = int(idx[-1]) + 1
    for cut in range(len(enc)):
        with pytest.raises(IndexFormatError):
            decode_indices_rice(enc[:cut], idx.size, numel)
    with pytest.raises(IndexFormatError):
        decode_indices_rice(enc + b"\x00", idx.size, numel)


def unary_bytes(bits: list[int]) -> bytes:
    return np.packbits(np.array(bits + [1] * (-len(bits) % 8), dtype=np.uint8)).tobytes()


def test_rice_noncanonical_and_malformed_streams_raise():
    with pytest.raises(IndexFormatError, match="rice parameter"):
        decode_indices_rice(bytes([32]) + b"\x01\x00\x00\x00\x00", 1, 10)
    # One value, k = 0: an escape whose stored quotient would have fit the unary code.
    unary = unary_bytes([1] * RICE_Q_ESCAPE + [0])
    esc = bytes([0]) + np.array([len(unary)], "<u4").tobytes() + unary
    with pytest.raises(IndexFormatError, match="non-canonical"):
        decode_indices_rice(esc + np.array([3], "<u4").tobytes(), 1, 1000)
    assert decode_indices_rice(esc + np.array([40], "<u4").tobytes(), 1, 1000).tolist() == [40]
    # A one-bit run longer than the escape code.
    unary = unary_bytes([1] * (RICE_Q_ESCAPE + 1) + [0])
    with pytest.raises(IndexFormatError, match="exceeds the escape"):
        decode_indices_rice(bytes([0]) + np.array([len(unary)], "<u4").tobytes() + unary, 1, 1000)
    # k = 0, unary 000 + 11111 padding: a zero in the padding reads as a fourth code,
    good = encode_indices_rice(np.array([0, 1, 2]))
    bad = bytearray(good)
    bad[5] &= 0xFE
    with pytest.raises(IndexFormatError, match="header says changed=3"):
        decode_indices_rice(bytes(bad), 3, 10)
    # and a whole extra byte of one-bit padding is refused too.
    padded = bytes([0]) + np.array([2], "<u4").tobytes() + good[5:6] + b"\xff"
    with pytest.raises(IndexFormatError, match="padding"):
        decode_indices_rice(padded, 3, 10)


def test_rice_fuzz_never_yields_bad_indices():
    # Any mutation either raises or decodes to strictly ascending in-range indices.
    rng = np.random.default_rng(11)
    numel = 50_000
    idx = np.sort(rng.choice(numel, size=500, replace=False))
    enc = bytearray(encode_indices_rice(idx))
    for _ in range(2000):
        m = bytearray(enc)
        for p in rng.integers(0, len(m), size=int(rng.integers(1, 4))):
            m[p] ^= int(rng.integers(1, 256))
        try:
            out = decode_indices_rice(bytes(m), idx.size, numel)
        except IndexFormatError:
            continue
        assert out.size == idx.size and out[0] >= 0 and out[-1] < numel
        assert np.all(np.diff(out) >= 1)


def test_rice_matches_v1_index_set():
    rng = np.random.default_rng(5)
    idx = np.sort(rng.choice(2**22, size=20_000, replace=False))
    v1 = decode_indices(encode_indices(idx.astype(np.uint64)), idx.size)
    assert np.array_equal(rice_rt(idx, 2**22), v1)


def test_v1_decoder_rejects_duplicate_index():
    dup = np.array([7], "<u4").tobytes() + np.array([0], "<u2").tobytes()
    with pytest.raises(IndexFormatError):
        decode_indices(dup, 2)


# --- zz-delta-u8esc values -------------------------------------------------------


def test_values_exhaustive_specials_roundtrip():
    old = np.repeat(SPECIAL, SPECIAL.size)
    new = np.tile(SPECIAL, SPECIAL.size)
    keep = old != new
    old, new = old[keep], new[keep]
    enc = encode_values_zz(old, new)
    out = decode_values_zz(enc, old.view(np.int16), old.size)
    assert np.array_equal(out.view(np.uint16), new)


@pytest.mark.parametrize("delta", [1, -1, 2, -2, 127, -127, -128, 128, 32767, -32768])
def test_values_delta_boundaries(delta):
    rng = np.random.default_rng(abs(delta))
    old = rng.integers(0, 2**16, size=1000).astype(np.uint16)
    new = (old + np.uint16(delta & 0xFFFF)).astype(np.uint16)
    enc = encode_values_zz(old, new)
    assert np.array_equal(decode_values_zz(enc, old, old.size).view(np.uint16), new)
    if abs(delta) == 1:
        assert len(enc) == old.size  # 1 ULP is one byte, no escapes


def test_values_random_roundtrip_and_size():
    rng = np.random.default_rng(1)
    old = rng.integers(0, 2**16, size=100_000).astype(np.uint16)
    new = rng.integers(0, 2**16, size=100_000).astype(np.uint16)
    new = np.where(new == old, new + 1, new).astype(np.uint16)
    enc = encode_values_zz(old, new)
    n_esc = int(np.count_nonzero(np.frombuffer(enc, np.uint8, count=old.size) == ZZ_ESCAPE))
    assert len(enc) == old.size + 2 * n_esc
    assert np.array_equal(decode_values_zz(enc, old, old.size).view(np.uint16), new)


def test_values_damaged_streams_raise():
    old = np.array([10, 20, 30], dtype=np.uint16)
    new = np.array([11, 20 + 400, 29], dtype=np.uint16)
    enc = encode_values_zz(old, new)
    assert len(enc) == 5
    for cut in range(len(enc)):
        with pytest.raises(ValueFormatError):
            decode_values_zz(enc[:cut], old, 3)
    with pytest.raises(ValueFormatError):
        decode_values_zz(enc + b"\x00", old, 3)
    with pytest.raises(ValueFormatError, match="zero delta"):
        decode_values_zz(b"\x00" + enc[1:], old, 3)
    with pytest.raises(ValueFormatError, match="non-canonical"):
        decode_values_zz(enc[:3] + np.array([7], "<u2").tobytes(), old, 3)
    with pytest.raises(ValueFormatError):
        decode_values_zz(b"\x01", np.empty(0, np.uint16), 0)


# --- patch envelope and apply ------------------------------------------------------


@pytest.mark.parametrize("seed", range(4))
def test_v2_applies_to_the_same_bytes_and_digests_as_v1(seed):
    s0 = rand_state(seed)
    s1 = training_like(s0, 0.05, seed + 100)
    p1, d1 = encode(s0, s1, FORMAT_VERSION)
    p2, d2 = encode(s0, s1, FORMAT_VERSION_V2)
    assert d1 == d2
    h1, h2 = header_of(p1), header_of(p2)
    assert [t["tensorDigest"] for t in h1["tensors"]] == [t["tensorDigest"] for t in h2["tensors"]]
    a, b = Snapshot(s0), Snapshot(s0)
    assert a.apply_patch(parse_patch(p1)) == d1
    assert b.apply_patch(parse_patch(p2)) == d1
    assert bits(a) == bits(b) == bits(Snapshot(s1))


def test_v2_parallel_encode_is_byte_identical():
    s0 = rand_state(9)
    s1 = training_like(s0, 0.05, 10)
    serial, _ = encode(s0, s1, FORMAT_VERSION_V2)
    snap = Snapshot(s0)
    meta = ModelMeta(id="t/m", digest=snap.digest(), param_count=snap.param_count())
    pooled, _ = snap.encode_step(
        s1.items(), step=1, base_step=0, model=meta, format_version=FORMAT_VERSION_V2, workers=4
    )
    assert pooled == serial


def test_v2_chain():
    s0 = rand_state(20)
    snap = Snapshot(s0)
    state = s0
    for step in range(1, 5):
        new = training_like(state, 0.02, step)
        patch, want = encode(state, new, FORMAT_VERSION_V2, step)
        assert snap.apply_patch(parse_patch(patch)) == want == snap.digest()
        state = new
    assert bits(snap) == bits(Snapshot(state))


def test_header_dispatch_and_v1_bytes_unchanged():
    s0 = rand_state(30)
    s1 = training_like(s0, 0.03, 31)
    p_default, _ = encode(s0, s1, FORMAT_VERSION)
    snap = Snapshot(s0)
    meta = ModelMeta(id="t/m", digest=snap.digest(), param_count=snap.param_count())
    p_implicit, _ = snap.encode_step(s1.items(), step=1, base_step=0, model=meta)
    assert p_implicit == p_default  # default stays v1, byte-identical
    h1 = header_of(p_default)
    assert h1["formatVersion"] == 1
    assert all(t["idxEnc"] == "delta-u16-esc" and "valEnc" not in t for t in h1["tensors"])
    p2, _ = encode(s0, s1, FORMAT_VERSION_V2)
    h2 = header_of(p2)
    assert h2["formatVersion"] == 2
    assert all(t["idxEnc"] == "rice-gap" and t["valEnc"] == "zz-delta-u8esc" for t in h2["tensors"])
    assert parse_patch(p_default).format_version == 1
    assert parse_patch(p2).format_version == 2
    assert len(p2) < len(p_default)


def retag(patch: bytes, mutate) -> bytes:
    hlen = int(np.frombuffer(patch, "<u4", offset=len(MAGIC), count=1)[0])
    body = len(MAGIC) + 4
    header = json.loads(patch[body : body + hlen])
    mutate(header)
    hjson = json.dumps(header, sort_keys=True, separators=(",", ":")).encode()
    return MAGIC + np.array([len(hjson)], dtype="<u4").tobytes() + hjson + patch[body + hlen :]


def test_header_mismatches_are_format_errors():
    s0 = rand_state(40)
    s1 = training_like(s0, 0.03, 41)
    p1, _ = encode(s0, s1, FORMAT_VERSION)
    p2, _ = encode(s0, s1, FORMAT_VERSION_V2)

    def set_version(v):
        return lambda h: h.__setitem__("formatVersion", v)

    with pytest.raises(PatchFormatError, match="unsupported formatVersion"):
        parse_patch(retag(p2, set_version(4)))
    with pytest.raises(PatchFormatError, match="idxEnc"):
        parse_patch(retag(p1, set_version(2)))  # v1 streams under a v2 header
    with pytest.raises(PatchFormatError, match="idxEnc"):
        parse_patch(retag(p2, set_version(1)))  # v2 streams under a v1 header

    def bad_val_enc(h):
        h["tensors"][0]["valEnc"] = "abs"

    with pytest.raises(PatchFormatError, match="valEnc"):
        parse_patch(retag(p2, bad_val_enc))

    def drop_val_enc(h):
        del h["tensors"][0]["valEnc"]

    with pytest.raises(PatchFormatError, match="malformed header"):
        parse_patch(retag(p2, drop_val_enc))
    with pytest.raises(PatchFormatError):
        parse_patch(p2[: len(p2) - 7])
    with pytest.raises(PatchFormatError):
        encode(s0, s1, 4)


def test_build_patch_refuses_mixed_entries():
    s0 = rand_state(50)
    s1 = training_like(s0, 0.03, 51)
    names = sorted(s0)
    entries = [encode_entry(n, s1[n], s0[n], FORMAT_VERSION_V2 if i % 2 else FORMAT_VERSION) for i, n in enumerate(names)]
    meta = ModelMeta(id="t/m", digest=Snapshot(s0).digest(), param_count=1)
    with pytest.raises(PatchFormatError, match="not all encoded"):
        build_patch(step=1, base_step=0, model=meta, entries=entries, format_version=FORMAT_VERSION_V2)


def damaged_v2_patch(s0, s1, target, idx_mut=None, val_mut=None):
    entries = []
    for n in sorted(s0):
        e = encode_entry(n, s1[n], s0[n], FORMAT_VERSION_V2)
        if n == target:
            e = TensorEntry(e.name, e.shape, e.numel, e.changed,
                            idx_mut(e.idx_bytes) if idx_mut else e.idx_bytes,
                            val_mut(e.val_bytes) if val_mut else e.val_bytes,
                            e.digest, FORMAT_VERSION_V2)
        entries.append(e)
    meta = ModelMeta(id="t/m", digest=Snapshot(s0).digest(), param_count=1)
    return build_patch(step=1, base_step=0, model=meta, entries=entries, format_version=FORMAT_VERSION_V2)[0]


@pytest.mark.parametrize(
    "idx_mut,val_mut,err",
    [
        (lambda b: b[:-1], None, IndexFormatError),
        (lambda b: b + b"\x00", None, IndexFormatError),
        (lambda b: bytes([b[0] ^ 0x40]) + b[1:], None, IndexFormatError),
        (None, lambda b: b[:-1], PatchApplyError),
        (None, lambda b: b"\x00" + b[1:], PatchApplyError),
        (None, lambda b: b + b"\x01", PatchApplyError),
    ],
)
def test_damaged_v2_streams_raise(idx_mut, val_mut, err):
    s0 = rand_state(60)
    s1 = training_like(s0, 0.05, 61)
    patch = damaged_v2_patch(s0, s1, "model.z_last.weight", idx_mut, val_mut)
    try:
        parsed = parse_patch(patch)
    except PatchFormatError:
        return  # caught by the header's byte-count checks: also a clean refusal
    with pytest.raises(err):
        Snapshot(s0).apply_patch(parsed)


def test_v2_patch_on_the_wrong_base_fails_the_digest_gate():
    # v2 values depend on the old cells, so a wrong base decodes to wrong cells:
    # the per-tensor digest gate still catches it, as for v1.
    s0, other = rand_state(70), rand_state(71)
    s1 = training_like(s0, 0.05, 72)
    patch, _ = encode(s0, s1, FORMAT_VERSION_V2)
    with pytest.raises(PatchApplyError, match="digest mismatch"):
        Snapshot(other).apply_patch(parse_patch(patch))


def test_unchanged_and_empty_tensors_encode_to_nothing():
    s0 = rand_state(80)
    s1 = {n: t.clone() for n, t in s0.items()}
    s1["model.one"].reshape(-1).view(torch.int16)[0] += 1
    patch, want = encode(s0, s1, FORMAT_VERSION_V2)
    h = header_of(patch)
    for t in h["tensors"]:
        if t["name"] != "model.one":
            assert t["changed"] == t["idxBytes"] == t["valBytes"] == 0
    snap = Snapshot(s0)
    assert snap.apply_patch(parse_patch(patch)) == want
    assert tensor_digest(snap.tensor("model.one")) == tensor_digest(s1["model.one"])
