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

Disk contract: anchors serialize to a temporary file before upload —
~2 bytes/param, so ~15 GB for a 7B model. The staging directory is
``work_dir`` (or ``$TMPDIR`` when unset); on trainer nodes where /tmp is
tmpfs or a small root partition, point ``work_dir`` at real disk or a
publish can OOM the node / ENOSPC mid-upload.
"""

from __future__ import annotations

import hashlib
import json
import os
import tempfile
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable, Iterator, Optional, Tuple, Union

from basilica.exceptions import BasilicaError

#: Server grammar for revision names (rl.rs RevisionNameInvalid) — validated
#: client-side so a typo fails before tensors are diffed and bytes uploaded.
_REV_CHARS = set("abcdefghijklmnopqrstuvwxyz0123456789._-")

#: T5 anchor-cadence guidance: a fresh consumer replays the whole window
#: parent-first, so unbounded patch chains make late-join cost unbounded.
DEFAULT_ANCHOR_EVERY = 30


class RevisionRejected(BasilicaError):
    """The fleet refused this revision.

    The platform records why on the revision status as a class token
    (``rejectedReason``) plus human text (``rejectedDetail``): an integrity
    failure (a state or artifact digest mismatch) or an apply failure with no
    integrity implication (for example an unreachable serving endpoint). This
    exception surfaces that reason and detail verbatim, so the report points
    at the subsystem that actually failed rather than always at the integrity
    gate.

    The previous revision keeps serving everywhere; the artifact and manifest
    remain for post-mortem. Publishing a corrected revision is the way
    forward: a Rejected record is terminal.
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


class NonFiniteWeights(BasilicaError, ValueError):
    """The state handed to ``publish`` contains NaN or Inf values.

    Refused before anything is uploaded: the handle, its diff base and the
    bucket are untouched. A fleet digest-verifies exactly what
    was published, so non-finite weights would otherwise be accepted and
    served by every replica. The cause is upstream, usually a training step
    whose loss or gradients went non-finite; retrying with the same tensors
    fails the same way.
    """


def _count_non_finite(tensor: Any) -> Tuple[int, int]:
    """(non-finite count, element count) for a floating or complex tensor
    or array; (0, n) for anything that cannot hold NaN/Inf."""
    try:
        import torch  # noqa: PLC0415

        if isinstance(tensor, torch.Tensor):
            if not (tensor.is_floating_point() or tensor.is_complex()):
                return 0, tensor.numel()
            return int((~torch.isfinite(tensor)).sum()), tensor.numel()
    except ImportError:
        pass
    import numpy as np  # noqa: PLC0415

    arr = np.asarray(tensor)
    if not np.issubdtype(arr.dtype, np.inexact):
        return 0, arr.size
    return int((~np.isfinite(arr)).sum()), arr.size


def _refuse_non_finite(
    named: Iterable[Tuple[str, Any]], revision: str
) -> Iterator[Tuple[str, Any]]:
    """Pass tensors through, raising on the first one with NaN/Inf. It is
    consumed lazily inside the atomic encode, so tensors before the bad one
    may already be encoded; the refusal still uploads nothing and leaves
    the handle and its diff base unchanged."""
    for name, tensor in named:
        bad, total = _count_non_finite(tensor)
        if bad:
            raise NonFiniteWeights(
                f"refusing to publish revision {revision!r}: tensor {name!r} has "
                f"{bad} NaN/Inf value(s) of {total}. Nothing was uploaded and the "
                "handle is unchanged; check the training step that produced it "
                "(a non-finite loss or gradient norm)."
            )
        yield name, tensor


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


#: Storage backends a policy can live on. ``r2`` is the default and keeps
#: the original behaviour; ``s3`` is AWS S3; ``s3-compatible`` covers
#: self-hosted or third-party stores such as MinIO.
STORAGE_BACKENDS = ("r2", "s3", "s3-compatible")
#: S3 addressing styles: ``virtual`` (``<bucket>.<host>``) or ``path``
#: (``<host>/<bucket>``).
ADDRESSING_STYLES = ("virtual", "path")


def _check_storage_choice(backend: str, addressing: Optional[str]) -> None:
    if backend not in STORAGE_BACKENDS:
        raise ValueError(
            f"backend must be one of {', '.join(STORAGE_BACKENDS)} (got {backend!r})"
        )
    if addressing is not None and addressing not in ADDRESSING_STYLES:
        raise ValueError(
            f"addressing must be 'virtual' or 'path' (got {addressing!r})"
        )


@dataclass
class PolicyStorage:
    """The CUSTOMER's storage coordinates for direct artifact upload.

    These never travel to the platform from here — the manifest POST carries
    only the artifact URI + digests. Credentials are optional: when omitted,
    boto3's standard resolution chain (env, shared config, instance role)
    applies, which is the recommended shape on a trainer node.

    ``backend`` mirrors the policy's: ``r2`` (default), ``s3`` (AWS; the
    endpoint may be omitted and boto3 derives it from ``region``) or
    ``s3-compatible`` (endpoint required; region defaults to
    ``us-east-1``). ``addressing`` overrides the backend's addressing
    style: ``s3`` defaults to virtual-hosted, ``s3-compatible`` to path
    style, and ``r2`` keeps boto3's own default.
    """

    bucket: str
    endpoint: Optional[str] = None
    region: Optional[str] = None
    access_key_id: Optional[str] = None
    secret_access_key: Optional[str] = None
    backend: str = "r2"
    addressing: Optional[str] = None

    def __post_init__(self) -> None:
        _check_storage_choice(self.backend, self.addressing)
        if not self.endpoint and self.backend != "s3":
            raise ValueError(f"endpoint is required for backend {self.backend!r}")

    def effective_region(self) -> Optional[str]:
        """Region for the upload client. ``None`` leaves boto3's own
        resolution (env, shared config) in charge."""
        if self.region:
            return self.region
        return "us-east-1" if self.backend == "s3-compatible" else None

    def effective_addressing(self) -> Optional[str]:
        """Addressing style for the upload client. ``None`` (R2 without
        an override) keeps boto3's default, exactly as before."""
        if self.addressing:
            return self.addressing
        return {"s3": "virtual", "s3-compatible": "path"}.get(self.backend)

    def __repr__(self) -> str:  # never echo key material
        return (
            f"PolicyStorage(bucket={self.bucket!r}, endpoint={self.endpoint!r}, "
            f"region={self.region!r}, backend={self.backend!r}, "
            f"addressing={self.addressing!r}, credentials="
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
        work_dir: Optional[Union[str, Path]] = None,
    ):
        if anchor_every < 1:
            raise ValueError("anchor_every must be >= 1")
        self._core = core
        self.name = name
        self._storage = storage
        self._anchor_every = anchor_every
        # Artifact staging directory (see the module's disk contract):
        # anchors are ~15 GB for 7B — the platform default tmpdir is often
        # tmpfs on trainer nodes, so this must be overridable.
        self._work_dir = str(work_dir) if work_dir is not None else None
        # Created eagerly: tempfile.mkdtemp(dir=...) does NOT create
        # parents, and "your staging dir is missing" should fail here, at
        # configuration time, not minutes later mid-publish.
        if self._work_dir is not None:
            os.makedirs(self._work_dir, exist_ok=True)
        meta = json.loads(core.rl_get_policy(name))
        self._effective_prefix: str = meta["effectivePrefix"]
        self._repo: str = meta["repo"]
        self._commit: str = meta["commit"]
        # One publish at a time: the diff base + step counter + staged
        # commit/rollback pair are a single unit of state; interleaved
        # publishes could roll back each other's staged encode.
        self._lock = threading.Lock()
        self._s3 = None  # lazy, cached across uploads (connection pool)
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

    def _s3_client(self):
        """The upload client, built once and reused: publishes are frequent
        (every training step in the tightest loop) and rebuilding the client
        re-resolves credentials and discards the connection pool."""
        if self._s3 is None:
            from botocore.config import Config  # noqa: PLC0415

            # Only set an addressing style when the backend (or the caller)
            # names one: R2 without an override builds the same client as
            # before this option existed.
            addressing = self._storage.effective_addressing()
            extra = {"s3": {"addressing_style": addressing}} if addressing else {}
            self._s3 = _boto3().client(
                "s3",
                endpoint_url=self._storage.endpoint or None,
                region_name=self._storage.effective_region(),
                aws_access_key_id=self._storage.access_key_id,
                aws_secret_access_key=self._storage.secret_access_key,
                # boto3 >= 1.36 attaches integrity checksums by default
                # (request_checksum_calculation="when_supported"), which R2
                # and other S3-compatible stores reject on multipart
                # completion with InvalidPart — the anchor upload is always
                # multipart at model scale. Only add checksums when the
                # operation requires them: correct against AWS S3, compatible
                # with R2. These Config keys exist in the botocore versions
                # (>= 1.36) that have the default, so no version guard is
                # needed for the publisher extra's pinned boto3.
                config=Config(
                    request_checksum_calculation="when_required",
                    response_checksum_validation="when_required",
                    **extra,
                ),
            )
        return self._s3

    def _upload(self, key: str, path: Path) -> str:
        """PUT the file to the customer bucket; returns the s3:// URI.

        boto3's managed transfer handles multipart automatically — anchors
        for 7B-class models are ~15 GB and must not go through a single PUT.
        """
        self._s3_client().upload_file(str(path), self._storage.bucket, key)
        return f"s3://{self._storage.bucket}/{key}"

    def _delete_orphan(self, key: str) -> None:
        """Best-effort cleanup of an uploaded artifact whose manifest POST
        failed — nothing references it, and 15 GB orphans add up. Failure
        here is swallowed: the publish error the caller sees must be the
        REGISTER failure, not the cleanup's."""
        try:
            self._s3_client().delete_object(Bucket=self._storage.bucket, Key=key)
        except Exception:  # noqa: BLE001 — deliberately best-effort
            pass

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

    # The `_locked` suffix is a contract, not a hope: both methods run only
    # under `self._lock` (acquired by the public surface below).

    def _publish_anchor_locked(self, named: Iterable[Tuple[str, Any]], revision: str) -> dict:
        anchor_mod, codec_mod, _digest_mod = _pulse_modules()
        snapshot = codec_mod.Snapshot.from_named_tensors(named)
        # Handle state (step included) advances ONLY at the success point at
        # the bottom — an encode/save/upload failure leaves the handle
        # exactly as it was, so a retry is always safe and step numbers
        # never drift (the patch path is deliberately symmetric).
        step = self._step + 1
        key = self._artifact_key(revision, "anchor.safetensors")
        uploaded = False
        with tempfile.TemporaryDirectory(
            prefix="basilica-anchor-", dir=self._work_dir
        ) as td:
            path = Path(td) / "anchor.safetensors"
            state_digest = anchor_mod.save_anchor(snapshot, path, step=step)
            sha256 = _sha256_file(path)
            try:
                uri = self._upload(key, path)
                uploaded = True
                resp = self._register(revision, None, uri, sha256, state_digest)
            except Exception as e:
                if uploaded:
                    # Registered nowhere, referenced by nothing — don't
                    # leave a 15 GB orphan in the customer's bucket.
                    self._delete_orphan(key)
                raise PublishError(f"anchor publish of {revision!r} failed: {e}") from e
        # Registry accepted: the anchor is the new diff base and chain root.
        self._step = step
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
        step = base_step + 1
        # encode_step is atomic (a failure leaves the snapshot untouched)
        # and stages the advance; commit/rollback below is the H1 contract
        # the vendored codec provides. `self._step` moves only on success —
        # symmetric with the anchor path.
        patch_bytes, state_digest = self._snapshot.encode_step(
            named,
            step=step,
            base_step=base_step,
            model=model,
            base_state_digest=self._snapshot.digest(),
        )
        key = self._artifact_key(revision, "patch.pulsept")
        uploaded = False
        try:
            with tempfile.TemporaryDirectory(
                prefix="basilica-patch-", dir=self._work_dir
            ) as td:
                path = Path(td) / "patch.pulsept"
                path.write_bytes(patch_bytes)
                sha256 = _sha256_file(path)
                uri = self._upload(key, path)
                uploaded = True
                resp = self._register(revision, self._parent, uri, sha256, state_digest)
        except Exception as e:
            self._snapshot.rollback_last_encode()
            if uploaded:
                self._delete_orphan(key)
            raise PublishError(f"patch publish of {revision!r} failed: {e}") from e
        self._snapshot.commit_step()
        self._step = step
        self._parent = revision
        self._patches_since_anchor += 1
        return resp

    # -- public surface ----------------------------------------------------

    def publish_anchor(self, named_tensors: Iterable[Tuple[str, Any]], *, revision: str) -> dict:
        """Publish the FULL bf16 state as an anchor (chain root).

        Returns the registry's revision record (state ``Validated``).
        Raises :class:`NonFiniteWeights` (nothing uploaded) if any tensor
        holds NaN or Inf. The anchor becomes the handle's diff base; the
        next ``publish`` diffs against exactly these tensors. Thread-safe:
        publishes on one handle serialize (the diff base is a single unit of
        state).
        """
        rev = _validate_revision(revision)
        with self._lock:
            named = _refuse_non_finite(named_tensors, rev)
            return self._publish_anchor_locked(named, rev)

    def publish(self, named_tensors: Iterable[Tuple[str, Any]], *, revision: str) -> dict:
        """Publish the state as a sparse patch over the last published revision.

        Transparently promotes to an anchor when the handle has no diff base
        yet, or when ``anchor_every`` patches have accumulated since the last
        anchor (default 30 — the late-join replay bound). Raises
        :class:`NonFiniteWeights` (nothing uploaded, handle unchanged) if any
        tensor holds NaN or Inf. Thread-safe: publishes on one handle
        serialize.
        """
        rev = _validate_revision(revision)
        with self._lock:
            named = _refuse_non_finite(named_tensors, rev)
            if self._snapshot is None or self._patches_since_anchor >= self._anchor_every:
                return self._publish_anchor_locked(named, rev)
            return self._publish_patch_locked(named, rev)

    def wait_until_active(
        self,
        revision: str,
        *,
        timeout: float = 1800.0,
        poll_interval: float = 5.0,
    ) -> dict:
        """Block until the fleet confirms the revision (state ``Active``).

        Raises :class:`RevisionRejected` when the fleet refuses the revision
        (for the reason the platform reported) and
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
                # Surface the platform's own classification verbatim. The
                # operator records a reason class token (RevisionApplyFailed,
                # RevisionStateMismatch, RevisionArtifactMismatch, ...) plus a
                # human detail. A non-integrity reject (e.g. a shim 502) is
                # RevisionApplyFailed, NOT a digest mismatch, so never assume
                # one: fall back to a neutral message when neither is present.
                reason = rec.get("rejectedReason")
                detail = rec.get("rejectedDetail")
                why = ": ".join(p for p in (reason, detail) if p)
                raise RevisionRejected(
                    f"revision {rev!r} was rejected by the fleet"
                    + (f": {why}" if why else "")
                )
            if state == "Superseded":
                raise RevisionSuperseded(
                    f"revision {rev!r} was superseded by a newer publish"
                )
            if state not in ("Validated", "Loading"):
                # An unrecognized state is contract drift between this SDK
                # and the server — fail LOUD now, not after a 30-minute
                # spin ending in a generic timeout.
                raise BasilicaError(
                    f"revision {rev!r} reports unrecognized state {state!r} — "
                    "the server speaks a newer revision protocol than this "
                    "SDK; upgrade basilica-sdk"
                )
            if time.monotonic() >= deadline:
                raise BasilicaError(
                    f"revision {rev!r} did not activate within {timeout:.0f}s "
                    f"(last state: {state!r})"
                )
            time.sleep(poll_interval)
