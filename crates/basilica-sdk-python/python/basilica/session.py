"""BYOT rollout-session client (#1666, interface doc steps 5+6).

The SERVING half of the session surface: ``generate()`` speaks the T4
training dialect against the session URL — token IDs in and out, the
sampler's own logprobs, ``revision`` as an assertion, ``servedRevision``
on every result. Pure stdlib (urllib): session traffic goes to the
session's own host, not the platform API, and the base SDK stays
zero-dependency.

The doc's step-6 loop runs verbatim when a publisher handle is attached::

    session = client.rl.open_session(url, token, publisher=handle)
    out = session.generate(token_ids=batch, n=8, seed=step)
    ...
    rev = session.publish(trainer.named_bf16_tensors(), revision=f"step-{step}")

``publish`` / ``publish_anchor`` / ``wait_until_active`` delegate to the
attached :class:`basilica.publisher.RlPolicyHandle`; ``generate`` works
without one.
"""

from __future__ import annotations

import json
import logging
import random
import time
import urllib.error
import urllib.request
from dataclasses import dataclass, field
from typing import Any, Optional, Sequence

from basilica.exceptions import BasilicaError


class StaleRevisionError(BasilicaError):
    """The asserted ``revision`` is not what the session serves.

    The doc's contract: re-request on the current revision (the error
    message names it) or omit the assertion to take the newest Active.
    """


class SessionServingError(BasilicaError):
    """The session endpoint refused or failed a generate call."""


class SessionUnavailableError(SessionServingError):
    """The session kept answering with a retryable 503 (a replica draining,
    or an engine fault being repaired) until the retry budget ran out.

    ``error_type`` is the last refusal's type (``SessionDraining`` or
    ``EngineFault``). A subclass of :class:`SessionServingError`, so code
    that catches that keeps working.
    """

    def __init__(self, message: str, error_type: str):
        super().__init__(message)
        self.error_type = error_type


#: 503 refusals the session's consumer uses for conditions that clear on
#: their own: a replica in its preStop drain, and a replica whose engine
#: weights match no revision until it reloads an anchor. Generation is safe
#: to resend (seeded, no side effects, an asserted revision is re-checked).
RETRYABLE_503_TYPES = frozenset({"SessionDraining", "EngineFault"})

_log = logging.getLogger(__name__)


class _Retryable(Exception):
    """A retryable 503 from the session, raised inside ``_post_json``."""

    def __init__(self, error_type: str, message: str, retry_after: Optional[float]):
        super().__init__(message)
        self.error_type = error_type
        self.message = message
        self.retry_after = retry_after


def _retry_after(e: urllib.error.HTTPError) -> Optional[float]:
    """``Retry-After`` in seconds, if the response carries a numeric one."""
    value = e.headers.get("Retry-After") if e.headers is not None else None
    try:
        return max(float(value), 0.0) if value is not None else None
    except ValueError:
        return None


@dataclass
class GenerateResult:
    """One generate call's yield, shaped for the trainer.

    Lists are choice-aligned (``len == n × prompts``, prompt-major — the
    engine's own order): ``token_ids[i]`` are the sampled ids for choice
    ``i``, ``logprobs[i]`` the sampler's logprobs on exactly those ids.
    """

    served_revision: Optional[str]
    token_ids: list = field(default_factory=list)
    logprobs: list = field(default_factory=list)
    prompt_token_ids: list = field(default_factory=list)
    finish_reasons: list = field(default_factory=list)
    texts: list = field(default_factory=list)
    usage: dict = field(default_factory=dict)
    #: The full response body for anything the shaped fields omit.
    raw: dict = field(default_factory=dict)


class RlSessionClient:
    """A rollout session's client: serving always, publishing when a
    :class:`basilica.publisher.RlPolicyHandle` is attached."""

    def __init__(
        self,
        url: str,
        token: str,
        *,
        publisher: Any = None,
        api: Any = None,
        session_uid: Optional[str] = None,
        timeout: float = 1800.0,
        retry_budget_s: float = 300.0,
    ):
        self._base = url.rstrip("/")
        self._token = token
        self._timeout = timeout
        # Seconds to keep retrying a retryable 503 before raising
        # SessionUnavailableError; 0 raises on the first one.
        self._retry_budget_s = retry_budget_s
        self._publisher = publisher
        self._api = api
        self._session_uid = session_uid

    # -- serving (T4 dialect) ---------------------------------------------

    def generate(
        self,
        *,
        token_ids: Optional[Sequence] = None,
        prompt: Optional[str] = None,
        n: int = 1,
        max_tokens: int = 512,
        temperature: Optional[float] = None,
        seed: Optional[int] = None,
        revision: Optional[str] = None,
        logprobs: int = 1,
        **extra: Any,
    ) -> GenerateResult:
        """Sample from the session (interface doc step 5).

        ``token_ids`` is the training path — one prompt (``[int, ...]``)
        or a batch (``[[int, ...], ...]``); the server never tokenizes.
        ``prompt`` (text) exists for eyeballing only. ``revision`` is an
        ASSERTION: mismatch raises :class:`StaleRevisionError` without
        sampling; omitted, the newest Active serves and
        ``result.served_revision`` says which. ``seed`` makes the batch
        replayable; ``n`` is the GRPO group (same prompt, same revision,
        prefix computed once).
        """
        if (token_ids is None) == (prompt is None):
            raise ValueError("pass exactly one of token_ids= or prompt=")
        body: dict = {
            "n": n,
            "max_tokens": max_tokens,
            "logprobs": logprobs,
            "return_token_ids": True,
        }
        if token_ids is not None:
            ids = list(token_ids)
            # Normalize a single prompt to the batch shape the engine takes.
            body["prompt"] = [ids] if ids and isinstance(ids[0], int) else ids
        else:
            body["prompt"] = prompt
        if temperature is not None:
            body["temperature"] = temperature
        if seed is not None:
            body["seed"] = seed
        if revision is not None:
            body["revision"] = revision
        body.update(extra)

        out = self._post_json("/v1/completions", body)
        result = GenerateResult(served_revision=out.get("servedRevision"), raw=out, usage=out.get("usage") or {})
        for choice in out.get("choices") or []:
            result.token_ids.append(choice.get("token_ids"))
            result.logprobs.append(choice.get("logprobs"))
            result.prompt_token_ids.append(choice.get("prompt_token_ids"))
            result.finish_reasons.append(choice.get("finish_reason"))
            result.texts.append(choice.get("text"))
        return result

    def _post_json(self, path: str, body: dict) -> dict:
        """POST with retries on the session's retryable 503s (a draining
        replica, an engine fault); every other failure raises at once."""
        deadline = time.monotonic() + max(self._retry_budget_s, 0.0)
        attempt = 0
        while True:
            attempt += 1
            try:
                return self._post_json_once(path, body)
            except _Retryable as r:
                wait = r.retry_after
                if wait is None:
                    # Exponential backoff with full jitter: 1, 2, 4 ... 15 s.
                    wait = random.uniform(0.5, 1.0) * min(15.0, 2.0 ** (attempt - 1))
                if time.monotonic() + wait > deadline:
                    raise SessionUnavailableError(
                        f"HTTP 503 {r.error_type} after {attempt} attempt(s): {r.message}",
                        r.error_type,
                    ) from r.__cause__
                _log.warning(
                    "session %s (attempt %d): retrying in %.1fs: %s",
                    r.error_type, attempt, wait, r.message,
                )
                time.sleep(wait)

    def _post_json_once(self, path: str, body: dict) -> dict:
        req = urllib.request.Request(
            f"{self._base}{path}",
            data=json.dumps(body).encode(),
            headers={
                "content-type": "application/json",
                "authorization": f"Bearer {self._token}",
            },
            method="POST",
        )
        try:
            with urllib.request.urlopen(req, timeout=self._timeout) as resp:
                return json.loads(resp.read())
        except urllib.error.HTTPError as e:
            payload = e.read()
            try:
                err = json.loads(payload).get("error") or {}
            except ValueError:
                err = {}
            message = err.get("message") or payload.decode(errors="replace")[:256]
            # The ONE failure trainers branch on gets its own type (doc:
            # "When something fails, the reason says who acts").
            if err.get("type") == "StaleRevision" or e.code == 409:
                raise StaleRevisionError(message) from e
            if e.code == 503 and err.get("type") in RETRYABLE_503_TYPES:
                raise _Retryable(err["type"], message, _retry_after(e)) from e
            raise SessionServingError(f"HTTP {e.code}: {message}") from e
        except urllib.error.URLError as e:
            raise SessionServingError(f"session unreachable: {e.reason}") from e

    # -- publishing (delegates to the attached policy handle) --------------

    def _need_publisher(self) -> Any:
        if self._publisher is None:
            raise BasilicaError(
                "this session has no publisher attached — open it with "
                "client.rl.open_session(url, token, publisher=client.rl.policy(...))"
            )
        return self._publisher

    # -- platform surface (delegates to the RL API when identified) --------

    def _need_api(self) -> Any:
        if self._api is None or self._session_uid is None:
            raise BasilicaError(
                "this session is not identified to the platform — open it with "
                "client.rl.open_session(url, token, session_uid=...) to use "
                "usage()/park()/resume()"
            )
        return self._api

    def usage(self) -> dict:
        """Usage & cost for THIS session (interface doc step 7)."""
        return self._need_api().session_usage(self._session_uid)

    def park(self) -> dict:
        """Park this session's fleet; the session identity survives."""
        return self._need_api().park_session(self._session_uid)

    def resume(self) -> dict:
        """Resume this parked session on the same lineage."""
        return self._need_api().resume_session(self._session_uid)

    def publish(self, named_tensors, *, revision: str) -> dict:
        return self._need_publisher().publish(named_tensors, revision=revision)

    def publish_anchor(self, named_tensors, *, revision: str) -> dict:
        return self._need_publisher().publish_anchor(named_tensors, revision=revision)

    def wait_until_active(self, revision: str, **kwargs: Any) -> dict:
        return self._need_publisher().wait_until_active(revision, **kwargs)
