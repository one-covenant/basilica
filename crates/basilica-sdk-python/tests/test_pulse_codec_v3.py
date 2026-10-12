"""PULSE patch format v3 in the vendored codec (basilica._pulse).

Mirrors basilica-backend python/pulse-verl/tests/test_codec_v3.py: a v3
patch applies to the SAME bytes and digests as the v1 and v2 patches of the
same update; the numpy reference and the native path produce identical
bytes; and a damaged stream raises instead of yielding indices. Byte parity
with the normative v3 encoder is pinned by the golden fixture in
test_pulse_codec.py."""

import pytest

torch = pytest.importorskip("torch")
np = pytest.importorskip("numpy")
pytest.importorskip("xxhash")
pytest.importorskip("zstandard")

from basilica._pulse import expctx  # noqa: E402
from basilica._pulse.codec import (  # noqa: E402
    FORMAT_VERSION,
    FORMAT_VERSION_V2,
    FORMAT_VERSION_V3,
    ModelMeta,
    PatchApplyError,
    PatchFormatError,
    Snapshot,
    TensorEntry,
    build_patch,
    parse_patch,
)
from basilica._pulse.codec import _encode_entry as encode_entry  # noqa: E402
from basilica._pulse.expctx import decode_indices_expctx, encode_indices_expctx  # noqa: E402
from basilica._pulse.indices import IndexFormatError, encode_indices_rice  # noqa: E402
from test_pulse_codec_v2 import bits, encode, header_of, rand_state, retag, training_like  # noqa: E402

HAVE_NATIVE = expctx._pulse_expctx_encode is not None


@pytest.fixture(params=["numpy"] + (["native"] if HAVE_NATIVE else []))
def impl(request, monkeypatch):
    monkeypatch.setenv("BASILICA_PULSE_EXPCTX_NATIVE", "1" if request.param == "native" else "0")
    return request.param


def lowlr_tensor(n: int, seed: int, scale=0.02, lr=2e-4):
    """Old cells of a Gaussian weight and the cells an update of size ~lr changes:
    small-exponent cells change far more often, as in real training."""
    rng = np.random.default_rng(seed)
    w = torch.from_numpy(rng.standard_normal(n).astype(np.float32) * scale)
    old = w.to(torch.bfloat16)
    new = (w + torch.from_numpy(rng.standard_normal(n).astype(np.float32) * lr)).to(torch.bfloat16)
    cells = old.view(torch.int16).numpy().view(np.uint16)
    idx = np.flatnonzero(old.view(torch.int16).numpy() != new.view(torch.int16).numpy())
    return cells, idx


def classes_with_complement(cells, idx):
    lab = (cells >> 7) & 0xFF
    sizes = np.bincount(lab, minlength=256)
    chg = np.bincount(lab[idx], minlength=256)
    return int(np.count_nonzero(2 * chg > sizes))


# --- the index stream ------------------------------------------------------------


@pytest.mark.parametrize("seed", range(4))
def test_roundtrip_and_size_on_low_lr_updates(impl, seed):
    cells, idx = lowlr_tensor(200_000, seed)
    buf = encode_indices_expctx(idx, cells)
    assert np.array_equal(decode_indices_expctx(buf, idx.size, cells), idx)
    # the exponent context beats position-only coding by a wide margin here
    assert len(buf) < 0.7 * len(encode_indices_rice(idx))


def test_complement_and_whole_class_changes(impl):
    # three classes: one untouched, one fully changed, one 2/3 changed
    cells = np.array([((120 + (i % 3)) << 7) | (i % 97) for i in range(3000)], dtype=np.uint16)
    idx = np.array([i for i in range(3000) if i % 3 == 1 or (i % 3 == 2 and i % 9 != 2)], dtype=np.int64)
    assert classes_with_complement(cells, idx) == 2
    buf = encode_indices_expctx(idx, cells)
    assert np.array_equal(decode_indices_expctx(buf, idx.size, cells), idx)


@pytest.mark.parametrize("idx", [[0], [2999], [0, 2999], list(range(3000))])
def test_edges(impl, idx):
    cells = np.array([(118 + (i % 5)) << 7 for i in range(3000)], dtype=np.uint16)
    idx = np.array(idx, dtype=np.int64)
    buf = encode_indices_expctx(idx, cells)
    assert np.array_equal(decode_indices_expctx(buf, idx.size, cells), idx)


def test_empty_tensor_and_no_changes(impl):
    assert encode_indices_expctx(np.empty(0, np.int64), np.empty(0, np.uint16)) == b""
    assert decode_indices_expctx(b"", 0, np.zeros(10, np.uint16)).size == 0
    with pytest.raises(IndexFormatError):
        decode_indices_expctx(b"\x00", 0, np.zeros(10, np.uint16))


@pytest.mark.parametrize("bad", [[5, 5], [5, 4], [-1, 2], [3, 10]])
def test_encoder_rejects_bad_index_sets(impl, bad):
    with pytest.raises(IndexFormatError):
        encode_indices_expctx(np.array(bad), np.zeros(10, np.uint16))


def test_every_truncation_and_extension_raises(impl):
    cells, idx = lowlr_tensor(20_000, 7)
    buf = encode_indices_expctx(idx, cells)
    for cut in range(len(buf)):
        with pytest.raises(IndexFormatError):
            decode_indices_expctx(buf[:cut], idx.size, cells)
    with pytest.raises(IndexFormatError):
        decode_indices_expctx(buf + b"\x00", idx.size, cells)
    for c in (idx.size - 1, idx.size + 1):
        with pytest.raises(IndexFormatError):
            decode_indices_expctx(buf, c, cells)


def test_noncanonical_records_raise(impl):
    cells = np.array([(120 + (i % 2)) << 7 for i in range(1000)], dtype=np.uint16)
    idx = np.arange(0, 1000, 4, dtype=np.int64)  # class 120 only, 250 of 500: plain
    buf = bytearray(encode_indices_expctx(idx, cells))
    assert int.from_bytes(buf[:2], "little") == 1 and buf[2] == 120 and buf[3] == 0
    flip = bytearray(buf)
    flip[3] = 1  # complement flag on a class that is not more than half changed
    with pytest.raises(IndexFormatError):
        decode_indices_expctx(bytes(flip), idx.size, cells)
    flags = bytearray(buf)
    flags[3] = 2  # unknown flag bit
    with pytest.raises(IndexFormatError):
        decode_indices_expctx(bytes(flags), idx.size, cells)
    zero = bytearray(buf[:2])  # zero records
    zero[0] = 0
    with pytest.raises(IndexFormatError):
        decode_indices_expctx(bytes(zero), idx.size, cells)
    # two records for the same exponent
    rec = bytes(buf[2:12])
    dup = (2).to_bytes(2, "little") + rec + rec + bytes(buf[12:]) * 2
    with pytest.raises(IndexFormatError):
        decode_indices_expctx(dup, 2 * idx.size, cells)


def test_fuzz_never_yields_bad_indices(impl):
    cells, idx = lowlr_tensor(30_000, 11)
    buf = encode_indices_expctx(idx, cells)
    rng = np.random.default_rng(0)
    for _ in range(300):
        b = bytearray(buf)
        for p in rng.integers(0, len(b), size=rng.integers(1, 4)):
            b[p] ^= 1 << int(rng.integers(0, 8))
        try:
            out = decode_indices_expctx(bytes(b), idx.size, cells)
        except IndexFormatError:
            continue
        assert out.size == idx.size
        assert out.size == 0 or (int(out.min()) >= 0 and int(out.max()) < cells.size)
        assert np.all(np.diff(out) > 0)


@pytest.mark.skipif(not HAVE_NATIVE, reason="native module not built")
@pytest.mark.parametrize("seed", range(3))
def test_native_and_numpy_are_byte_identical(monkeypatch, seed):
    cells, idx = lowlr_tensor(300_000, 40 + seed)
    monkeypatch.setenv("BASILICA_PULSE_EXPCTX_NATIVE", "0")
    ref = encode_indices_expctx(idx, cells)
    monkeypatch.setenv("BASILICA_PULSE_EXPCTX_NATIVE", "1")
    nat = encode_indices_expctx(idx, cells)
    assert nat == ref
    for threads in ("1", "3"):
        monkeypatch.setenv("BASILICA_PULSE_EXPCTX_THREADS", threads)
        assert np.array_equal(decode_indices_expctx(ref, idx.size, cells), idx)


# --- patch envelope and apply ------------------------------------------------------


@pytest.mark.parametrize("seed", range(4))
def test_v3_applies_to_the_same_bytes_and_digests_as_v1_and_v2(impl, seed):
    s0 = rand_state(seed)
    s1 = training_like(s0, 0.05, seed + 100)
    p1, d1 = encode(s0, s1, FORMAT_VERSION)
    p2, d2 = encode(s0, s1, FORMAT_VERSION_V2)
    p3, d3 = encode(s0, s1, FORMAT_VERSION_V3)
    assert d1 == d2 == d3
    h3 = header_of(p3)
    assert h3["formatVersion"] == 3
    assert all(t["idxEnc"] == "exp-class-rice" and t["valEnc"] == "zz-delta-u8esc" for t in h3["tensors"])
    assert [t["tensorDigest"] for t in header_of(p1)["tensors"]] == [t["tensorDigest"] for t in h3["tensors"]]
    a = Snapshot(s0)
    assert a.apply_patch(parse_patch(p3)) == d1
    assert bits(a) == bits(Snapshot(s1))


def test_v3_chain(impl):
    s0 = rand_state(20)
    snap = Snapshot(s0)
    state = s0
    for step in range(1, 5):
        new = training_like(state, 0.02, step)
        patch, want = encode(state, new, FORMAT_VERSION_V3, step)
        assert snap.apply_patch(parse_patch(patch)) == want == snap.digest()
        state = new
    assert bits(snap) == bits(Snapshot(state))


def test_header_mismatches_are_format_errors():
    s0 = rand_state(40)
    s1 = training_like(s0, 0.03, 41)
    p2, _ = encode(s0, s1, FORMAT_VERSION_V2)
    p3, _ = encode(s0, s1, FORMAT_VERSION_V3)
    with pytest.raises(PatchFormatError, match="idxEnc"):
        parse_patch(retag(p2, lambda h: h.__setitem__("formatVersion", 3)))
    with pytest.raises(PatchFormatError, match="idxEnc"):
        parse_patch(retag(p3, lambda h: h.__setitem__("formatVersion", 2)))
    with pytest.raises(PatchFormatError, match="unsupported formatVersion"):
        parse_patch(retag(p3, lambda h: h.__setitem__("formatVersion", 4)))


def test_v3_indices_need_the_old_tensor():
    s0 = rand_state(45)
    s1 = training_like(s0, 0.03, 46)
    parsed = parse_patch(encode(s0, s1, FORMAT_VERSION_V3)[0])
    i = next(i for i, t in enumerate(parsed.tensors) if t["changed"])
    with pytest.raises(PatchFormatError, match="old tensor"):
        parsed.tensor_indices(i)
    with pytest.raises(PatchApplyError, match="cells"):
        parsed.tensor_indices(i, torch.zeros(3, dtype=torch.bfloat16))


def damaged_v3_patch(s0, s1, target, idx_mut):
    entries = []
    for n in sorted(s0):
        e = encode_entry(n, s1[n], s0[n], FORMAT_VERSION_V3)
        if n == target:
            e = TensorEntry(e.name, e.shape, e.numel, e.changed, idx_mut(e.idx_bytes), e.val_bytes, e.digest,
                            FORMAT_VERSION_V3)
        entries.append(e)
    meta = ModelMeta(id="t/m", digest=Snapshot(s0).digest(), param_count=1)
    return build_patch(step=1, base_step=0, model=meta, entries=entries, format_version=FORMAT_VERSION_V3)[0]


@pytest.mark.parametrize(
    "idx_mut",
    [
        lambda b: b[:-1],
        lambda b: b + b"\x00",
        lambda b: b[:3] + bytes([b[3] ^ 0x01]) + b[4:],  # complement flag
        lambda b: b[:2] + bytes([b[2] ^ 0x01]) + b[3:],  # class exponent
    ],
)
def test_damaged_v3_streams_raise(impl, idx_mut):
    s0 = rand_state(60)
    s1 = training_like(s0, 0.05, 61)
    patch = damaged_v3_patch(s0, s1, "model.z_last.weight", idx_mut)
    try:
        parsed = parse_patch(patch)
    except PatchFormatError:
        return
    with pytest.raises((IndexFormatError, PatchApplyError)):
        Snapshot(s0).apply_patch(parsed)


def test_v3_patch_on_the_wrong_base_never_applies(impl):
    # v3 indices decode against the old tensor, so a wrong base either fails
    # to decode (IndexFormatError) or decodes to cells the digest gate catches.
    s0, other = rand_state(70), rand_state(71)
    s1 = training_like(s0, 0.05, 72)
    patch, _ = encode(s0, s1, FORMAT_VERSION_V3)
    with pytest.raises((IndexFormatError, PatchApplyError)):
        Snapshot(other).apply_patch(parse_patch(patch))
