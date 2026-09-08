"""BYOT publisher (#1666, SDK 0.36; interface doc steps 3+6).

Trainer-side revision publishing for a BYOT policy lineage: BF16 state ->
PULSE sparse patch (or safetensors anchor) -> upload to the CUSTOMER's own
bucket under the policy's ``effectivePrefix`` -> manifest POST. The platform
never sees weight bytes and never receives storage credentials on this path:
the upload talks straight to the customer's S3-compatible endpoint with the
customer's own credential chain.

Everything here is reproducible without the SDK (no lock-in, by design):
an anchor is a safetensors file with ``pulse.step``/``pulse.stateDigest``
metadata, a patch is a PULSEPT1 envelope (contracts doc C2.4-C2.5), and a
publish is one ``PUT`` + one ``POST /rl/policies/{name}/revisions`` with the
artifact's sha256 and the whole-state xxh3 digest.

Dependency contract: importing this module is cheap; the heavy publisher
dependencies (torch, numpy, xxhash, zstandard, safetensors, boto3) load
lazily inside the publish paths and fail with an actionable message naming
the extra to install.
"""

from __future__ import annotations

import hashlib
import json
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable, Optional, Tuple

from basilica.exceptions import BasilicaError

#: Server grammar for revision names (rl.rs RevisionNameInvalid) — validated
#: client-side so a typo fails before tensors are diffed and bytes uploaded.
_REV_CHARS = set("abcdefghijklmnopqrstuvwxyz0123456789._-")

#: T5 anchor-cadence guidance: a fresh consumer replays the whole window
#: parent-first, so unbounded patch chains make late-join cost unbounded.
DEFAULT_ANCHOR_EVERY = 30


class RevisionRejected(BasilicaError):
    """The fleet refused this revision (digest mismatch on some replica).

    The previous revision keeps serving everywhere; the artifact and manifest
    remain for post-mortem. Publishing a corrected revision is the way
    forward — a Rejected record is terminal.
    """


class RevisionSuperseded(BasilicaError):
    """A newer revision went Active while this one was loading.

    Newest-wins is the platform contract; treat this as success-of-the-lineage
    (your newer publish is what serves) rather than a failure of this one.
    """


class PublishError(BasilicaError):
    """A publish failed before the manifest was registered.

    The handle's diff base is rolled back to the last PUBLISHED state (H1),
    so simply retrying ``publish`` with the same tensors is safe.
    """


def _validate_revision(revision: str) -> str:
    r = revision.strip()
    ok = (
        1 <= len(r) <= 128
        and set(r) <= _REV_CHARS
        and r[0] not in "._-"
        and r[-1] not in "._-"
    )
    if not ok:
        raise ValueError(
            f"invalid revision name {revision!r}: 1-128 chars of [a-z0-9._-] "
            "with letter-or-digit edges (the server enforces the same grammar)"
        )
    return r


def _pulse_modules():
    """Lazy-import the vendored codec; name the extras on failure."""
    try:
        from basilica._pulse import anchor, codec, digest  # noqa: PLC0415

        return anchor, codec, digest
    except ImportError as e:
        raise ImportError(
            "the publisher path needs torch, numpy, xxhash, zstandard and "
            "safetensors — install the publisher extra: "
            "pip install 'basilica-sdk[publisher]' "
            f"(missing: {e.name})"
        ) from e


def _boto3():
    try:
        import boto3  # noqa: PLC0415

        return boto3
    except ImportError as e:
        raise ImportError(
            "the publisher uploads with boto3 — install the publisher extra: "
            "pip install 'basilica-sdk[publisher]'"
        ) from e


@dataclass
class PolicyStorage:
    """The CUSTOMER's storage coordinates for direct artifact upload.

    These never travel to the platform from here — the manifest POST carries
    only the artifact URI + digests. Credentials are optional: when omitted,
    boto3's standard resolution chain (env, shared config, instance role)
    applies, which is the recommended shape on a trainer node.
    """

    bucket: str
    endpoint: str
    region: Optional[str] = None
    access_key_id: Optional[str] = None
    secret_access_key: Optional[str] = None

    def __repr__(self) -> str:  # never echo key material
        return (
            f"PolicyStorage(bucket={self.bucket!r}, endpoint={self.endpoint!r}, "
            f"region={self.region!r}, credentials="
            f"{'<explicit>' if self.access_key_id else '<default chain>'})"
        )


def _sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


class RlPolicyHandle:
    """One trainer's publishing session against a policy lineage.

    The handle owns the DIFF BASE: the last successfully PUBLISHED state.
    A failed upload or manifest POST rolls the base back (H1) so the next
    patch never diffs against state the registry has not accepted — the
    exact invariant that keeps a relay hiccup from masquerading as an I3
    digest violation on the fleet.

    Anchor cadence: ``publish`` transparently promotes to an anchor when
    there is no base yet (fresh handle) or ``anchor_every`` patches have
    accumulated — late-join replay cost stays bounded without the trainer
    thinking about it. ``publish_anchor`` remains available for explicit
    checkpoint-style anchoring.
    """

    def __init__(
        self,
        core: Any,
        name: str,
        *,
        storage: PolicyStorage,
        anchor_every: int = DEFAULT_ANCHOR_EVERY,
    ):
        if anchor_every < 1:
            raise ValueError("anchor_every must be >= 1")
        self._core = core
        self.name = name
        self._storage = storage
        self._anchor_every = anchor_every
        meta = json.loads(core.rl_get_policy(name))
        self._effective_prefix: str = meta["effectivePrefix"]
        self._repo: str = meta["repo"]
        self._commit: str = meta["commit"]
        # Publishing state — all of it advances ONLY on registry-accepted
        # publishes (H1).
        self._snapshot = None  # last-published Snapshot (the diff base)
        self._parent: Optional[str] = None
        self._anchor_digest: Optional[str] = None
        self._step = 0
        self._patches_since_anchor = 0

    # -- internals ---------------------------------------------------------

    def _artifact_key(self, revision: str, filename: str) -> str:
        return f"{self._effective_prefix}{revision}/{filename}"

    def _upload(self, key: str, path: Path) -> str:
        """PUT the file to the customer bucket; returns the s3:// URI.

        boto3's managed transfer handles multipart automatically — anchors
        for 7B-class models are ~15 GB and must not go through a single PUT.
        """
        boto3 = _boto3()
        client = boto3.client(
            "s3",
            endpoint_url=self._storage.endpoint,
            region_name=self._storage.region,
            aws_access_key_id=self._storage.access_key_id,
            aws_secret_access_key=self._storage.secret_access_key,
        )
        client.upload_file(str(path), self._storage.bucket, key)
        return f"s3://{self._storage.bucket}/{key}"

    def _register(
        self,
        revision: str,
        parent: Optional[str],
        uri: str,
        sha256: str,
        state_digest: str,
    ) -> dict:
        body = {
            "revision": revision,
            "artifact": {"uri": uri, "sha256": sha256},
            "expectedStateDigest": state_digest,
        }
        if parent is not None:
            body["parentRevision"] = parent
        return json.loads(self._core.rl_create_revision(self.name, json.dumps(body)))

    def _publish_anchor_locked(self, named: Iterable[Tuple[str, Any]], revision: str) -> dict:
        anchor_mod, codec_mod, _digest_mod = _pulse_modules()
        snapshot = codec_mod.Snapshot.from_named_tensors(named)
        self._step += 1
        with tempfile.TemporaryDirectory(prefix="basilica-anchor-") as td:
            path = Path(td) / "anchor.safetensors"
            state_digest = anchor_mod.save_anchor(snapshot, path, step=self._step)
            sha256 = _sha256_file(path)
            try:
                uri = self._upload(self._artifact_key(revision, "anchor.safetensors"), path)
                resp = self._register(revision, None, uri, sha256, state_digest)
            except Exception as e:
                self._step -= 1  # nothing published; the counter must not drift
                raise PublishError(f"anchor publish of {revision!r} failed: {e}") from e
        # Registry accepted: the anchor is the new diff base and chain root.
        self._snapshot = snapshot
        self._parent = revision
        self._anchor_digest = state_digest
        self._patches_since_anchor = 0
        return resp

    def _publish_patch_locked(self, named: Iterable[Tuple[str, Any]], revision: str) -> dict:
        _anchor_mod, codec_mod, _digest_mod = _pulse_modules()
        model = codec_mod.ModelMeta(
            id=f"{self._repo}@{self._commit}",
            digest=self._anchor_digest,
            param_count=self._snapshot.param_count(),
        )
        base_step = self._step
        self._step += 1
        # encode_step stages the advance; commit/rollback below is the H1
        # contract the vendored codec provides.
        patch_bytes, state_digest = self._snapshot.encode_step(
            named,
            step=self._step,
            base_step=base_step,
            model=model,
            base_state_digest=self._snapshot.digest(),
        )
        with tempfile.TemporaryDirectory(prefix="basilica-patch-") as td:
            path = Path(td) / "patch.pulsept"
            path.write_bytes(patch_bytes)
            sha256 = _sha256_file(path)
            try:
                uri = self._upload(self._artifact_key(revision, "patch.pulsept"), path)
                resp = self._register(revision, self._parent, uri, sha256, state_digest)
            except Exception as e:
                self._snapshot.rollback_last_encode()
                self._step -= 1
                raise PublishError(f"patch publish of {revision!r} failed: {e}") from e
        self._snapshot.commit_step()
        self._parent = revision
        self._patches_since_anchor += 1
        return resp

    # -- public surface ----------------------------------------------------

    def publish_anchor(self, named_tensors: Iterable[Tuple[str, Any]], *, revision: str) -> dict:
        """Publish the FULL bf16 state as an anchor (chain root).

        Returns the registry's revision record (state ``Validated``). The
        anchor becomes the handle's diff base; the next ``publish`` diffs
        against exactly these tensors.
        """
        return self._publish_anchor_locked(named_tensors, _validate_revision(revision))

    def publish(self, named_tensors: Iterable[Tuple[str, Any]], *, revision: str) -> dict:
        """Publish the state as a sparse patch over the last published revision.

        Transparently promotes to an anchor when the handle has no diff base
        yet, or when ``anchor_every`` patches have accumulated since the last
        anchor (default 30 — the late-join replay bound).
        """
        rev = _validate_revision(revision)
        if self._snapshot is None or self._patches_since_anchor >= self._anchor_every:
            return self._publish_anchor_locked(named_tensors, rev)
        return self._publish_patch_locked(named_tensors, rev)

    def wait_until_active(
        self,
        revision: str,
        *,
        timeout: float = 1800.0,
        poll_interval: float = 5.0,
    ) -> dict:
        """Block until the fleet confirms the revision (state ``Active``).

        Raises :class:`RevisionRejected` on a fleet digest mismatch and
        :class:`RevisionSuperseded` when a newer revision won the race
        (newest-wins is the platform contract). Times out with
        :class:`BasilicaError` — the revision may still activate later.
        """
        rev = _validate_revision(revision)
        deadline = time.monotonic() + timeout
        while True:
            rec = json.loads(self._core.rl_get_revision(self.name, rev))
            state = rec.get("state")
            if state == "Active":
                return rec
            if state == "Rejected":
                raise RevisionRejected(
                    f"revision {rev!r} was rejected by the fleet: "
                    f"{rec.get('rejectedReason') or 'digest mismatch'}"
                )
            if state == "Superseded":
                raise RevisionSuperseded(
                    f"revision {rev!r} was superseded by a newer publish"
                )
            if time.monotonic() >= deadline:
                raise BasilicaError(
                    f"revision {rev!r} did not activate within {timeout:.0f}s "
                    f"(last state: {state!r})"
                )
            time.sleep(poll_interval)
