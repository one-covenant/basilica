"""Publisher-flow tests (basilica.publisher, #1666): the trainer-side
publish loop against a FAKE core and a stubbed uploader — no network, no
compiled extension. What's under test is the handle's own contract:
manifest composition, anchor cadence, and the H1 rollback invariant (a
failed publish must not poison the next patch's diff base).
"""

import hashlib
import json

import pytest

torch = pytest.importorskip("torch")
pytest.importorskip("xxhash")
pytest.importorskip("zstandard")
pytest.importorskip("safetensors")

from basilica.publisher import (  # noqa: E402
    PolicyStorage,
    PublishError,
    RevisionRejected,
    RevisionSuperseded,
    RlPolicyHandle,
)


def _t(vals):
    return torch.tensor(vals, dtype=torch.bfloat16)


def _state(x=1.0):
    return {"w.a": _t([[0.0, x], [2.0, 3.0]]), "w.b": _t([1.5, -2.5, 0.0])}


class FakeCore:
    """The `_core` surface the handle touches, recording every call."""

    def __init__(self):
        self.revisions = []  # bodies POSTed
        self.revision_states = {}  # name -> list of state dicts to serve

    def rl_get_policy(self, name):
        return json.dumps(
            {
                "name": name,
                "policyUid": "uid-1",
                "effectivePrefix": "policies/uid-1/",
                "repo": "Qwen/Qwen2.5-7B-Instruct",
                "commit": "deadbeef",
                "updateFormat": "pulse-bf16-v1",
                "totalRevisions": 0,
            }
        )

    def rl_create_revision(self, policy, body_json):
        body = json.loads(body_json)
        self.revisions.append(body)
        return json.dumps(
            {
                "revision": body["revision"],
                "parentRevision": body.get("parentRevision"),
                "state": "Validated",
                "submittedAt": "2026-09-07T16:00:00Z",
            }
        )

    def rl_get_revision(self, policy, revision):
        states = self.revision_states.get(revision, [{"state": "Active"}])
        rec = states.pop(0) if len(states) > 1 else states[0]
        return json.dumps({"revision": revision, "submittedAt": "t", **rec})


@pytest.fixture()
def handle(monkeypatch):
    core = FakeCore()
    h = RlPolicyHandle(
        core,
        "math-policy",
        storage=PolicyStorage(bucket="my-weights", endpoint="https://acc.example"),
        anchor_every=3,
    )
    uploads = {}

    def fake_upload(key, path):
        uploads[key] = hashlib.sha256(path.read_bytes()).hexdigest()
        return f"s3://my-weights/{key}"

    monkeypatch.setattr(h, "_upload", fake_upload)
    h._test_uploads = uploads
    h._test_core = core
    return h


def test_anchor_manifest_is_composed_correctly(handle):
    handle.publish_anchor(_state().items(), revision="step-0000")
    (body,) = handle._test_core.revisions
    assert body["revision"] == "step-0000"
    assert "parentRevision" not in body, "an anchor must OMIT the parent key"
    uri = body["artifact"]["uri"]
    assert uri.startswith("s3://my-weights/policies/uid-1/step-0000/")
    # The registered sha256 is the sha of the exact uploaded bytes.
    key = uri.removeprefix("s3://my-weights/")
    assert body["artifact"]["sha256"] == handle._test_uploads[key]
    assert body["expectedStateDigest"].startswith("xxh3_128:")
    assert len(body["expectedStateDigest"]) == len("xxh3_128:") + 32


def test_patch_chains_to_the_last_published_revision(handle):
    handle.publish_anchor(_state(1.0).items(), revision="step-0000")
    handle.publish(_state(1.25).items(), revision="step-0001")
    body = handle._test_core.revisions[-1]
    assert body["parentRevision"] == "step-0000"
    assert body["artifact"]["uri"].endswith("/step-0001/patch.pulsept")


def test_auto_anchor_on_fresh_handle_and_on_cadence(handle):
    # Fresh handle: publish() promotes to an anchor (no diff base yet).
    handle.publish(_state(1.0).items(), revision="r0")
    assert "parentRevision" not in handle._test_core.revisions[0]
    # anchor_every=3: three patches ride, the fourth publish re-anchors.
    for i, x in enumerate((1.1, 1.2, 1.3), start=1):
        handle.publish(_state(x).items(), revision=f"r{i}")
        assert handle._test_core.revisions[-1]["parentRevision"] == f"r{i - 1}"
    handle.publish(_state(1.4).items(), revision="r4")
    assert "parentRevision" not in handle._test_core.revisions[-1], (
        "the cadence anchor must be a chain root"
    )


def test_failed_upload_rolls_back_the_diff_base(handle, monkeypatch):
    # H1: after a failed patch publish, retrying with the SAME tensors must
    # produce a valid patch over the LAST PUBLISHED state — not over the
    # state the failed attempt staged.
    handle.publish_anchor(_state(1.0).items(), revision="step-0000")
    base_digest = handle._snapshot.digest()

    def broken_upload(key, path):
        raise ConnectionError("relay hiccup")

    monkeypatch.setattr(handle, "_upload", broken_upload)
    with pytest.raises(PublishError):
        handle.publish(_state(1.25).items(), revision="step-0001")
    assert handle._snapshot.digest() == base_digest, "diff base must roll back"
    assert handle._parent == "step-0000"

    def working_upload(key, path):
        return f"s3://my-weights/{key}"

    monkeypatch.setattr(handle, "_upload", working_upload)
    handle.publish(_state(1.25).items(), revision="step-0001")
    body = handle._test_core.revisions[-1]
    assert body["parentRevision"] == "step-0000"


def test_wait_until_active_maps_terminal_states(handle):
    core = handle._test_core
    core.revision_states["ok"] = [{"state": "Loading"}, {"state": "Active"}]
    rec = handle.wait_until_active("ok", poll_interval=0.01)
    assert rec["state"] == "Active"

    core.revision_states["bad"] = [
        {"state": "Rejected", "rejectedReason": "RevisionStateMismatch"}
    ]
    with pytest.raises(RevisionRejected, match="RevisionStateMismatch"):
        handle.wait_until_active("bad", poll_interval=0.01)

    core.revision_states["old"] = [{"state": "Superseded"}]
    with pytest.raises(RevisionSuperseded):
        handle.wait_until_active("old", poll_interval=0.01)


def test_revision_grammar_is_validated_before_any_work(handle):
    with pytest.raises(ValueError, match="letter-or-digit edges"):
        handle.publish_anchor(_state().items(), revision="-bad-")
    with pytest.raises(ValueError):
        handle.wait_until_active("Bad.Name")
    assert handle._test_core.revisions == [], "nothing must reach the registry"


def test_work_dir_threads_through_to_artifact_staging(handle, monkeypatch):
    # HIGH (#1666 review): anchors are ~15 GB — they must stage where the
    # caller says, not in the platform default tmpdir (often tmpfs).
    import tempfile

    seen_dirs = []
    real_tmpdir = tempfile.TemporaryDirectory

    def recording_tmpdir(*args, **kwargs):
        seen_dirs.append(kwargs.get("dir"))
        kwargs.pop("dir", None)  # stage in the default for the test itself
        return real_tmpdir(*args, **kwargs)

    monkeypatch.setattr(tempfile, "TemporaryDirectory", recording_tmpdir)
    handle._work_dir = "/data/scratch"
    handle.publish_anchor(_state(1.0).items(), revision="r0")
    handle.publish(_state(1.1).items(), revision="r1")
    assert seen_dirs == ["/data/scratch", "/data/scratch"], (
        "both publish paths must stage under work_dir"
    )


def test_register_failure_deletes_the_orphaned_artifact(handle, monkeypatch):
    # A manifest POST failure after a successful upload leaves an
    # unreferenced multi-GB object in the customer's bucket — best-effort
    # cleanup must fire, and the surfaced error must be the REGISTER one.
    handle.publish_anchor(_state(1.0).items(), revision="step-0000")
    deleted = []
    monkeypatch.setattr(handle, "_delete_orphan", deleted.append)

    def broken_register(*a, **k):
        raise ConnectionError("registry 503")

    monkeypatch.setattr(handle, "_register", broken_register)
    with pytest.raises(PublishError, match="registry 503"):
        handle.publish(_state(1.25).items(), revision="step-0001")
    assert deleted == ["policies/uid-1/step-0001/patch.pulsept"]
    # And H1 still holds: the diff base rolled back.
    assert handle._parent == "step-0000"


def test_upload_failure_does_not_attempt_orphan_delete(handle, monkeypatch):
    # Nothing was uploaded — a delete would be pure noise (and could mask
    # the real error if the bucket also refuses deletes).
    handle.publish_anchor(_state(1.0).items(), revision="step-0000")
    deleted = []
    monkeypatch.setattr(handle, "_delete_orphan", deleted.append)
    monkeypatch.setattr(
        handle, "_upload", lambda *a: (_ for _ in ()).throw(ConnectionError("net down"))
    )
    with pytest.raises(PublishError):
        handle.publish(_state(1.25).items(), revision="step-0001")
    assert deleted == []


def test_step_numbers_do_not_drift_on_failed_publishes(handle, monkeypatch):
    # LOW (#1666 review): a failed encode/save/upload must not consume a
    # step number — both paths advance the counter only on success.
    handle.publish_anchor(_state(1.0).items(), revision="r0")
    step_after_anchor = handle._step
    monkeypatch.setattr(
        handle, "_upload", lambda *a: (_ for _ in ()).throw(ConnectionError("down"))
    )
    with pytest.raises(PublishError):
        handle.publish(_state(1.1).items(), revision="r1")
    assert handle._step == step_after_anchor


def test_wait_until_active_fails_fast_on_unknown_state(handle):
    from basilica.exceptions import BasilicaError

    handle._test_core.revision_states["weird"] = [{"state": "Quarantined"}]
    with pytest.raises(BasilicaError, match="unrecognized state 'Quarantined'"):
        handle.wait_until_active("weird", poll_interval=0.01)


def test_create_policy_validates_credential_exclusivity_client_side():
    # LOW (#1666 review): the exactly-one credential contract fails fast,
    # before any server round-trip — the core is never touched.
    from basilica.rl import RlNamespace

    ns = RlNamespace(object())  # any core attribute access would explode
    common = dict(
        repo="r",
        commit="a" * 40,
        tokenizer_digest="sha256:" + "b" * 64,
        bucket="b",
        endpoint="https://e.example",
    )
    with pytest.raises(ValueError, match="not both"):
        ns.create_policy(
            "p", access_key_id="AK", secret_access_key="SK",
            credentials_secret="s", **common,
        )
    with pytest.raises(ValueError, match="BOTH"):
        ns.create_policy("p", access_key_id="AK", **common)
    with pytest.raises(ValueError, match="credentials are required"):
        ns.create_policy("p", **common)
