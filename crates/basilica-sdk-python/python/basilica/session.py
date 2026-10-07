"""BYOT rollout-session client (#1666, interface doc steps 5+6).

The SERVING half of the session surface: ``generate()`` speaks the T4
training dialect against the session URL — token IDs in and out, the
engine's logprobs on the sampled tokens, ``revision`` as an assertion,
``servedRevision`` on every result. Pure stdlib (urllib): session traffic
goes to the session's own host, not the platform API, and the base SDK
stays zero-dependency.

The logprobs are the engine's RAW logprobs unless the session says
otherwise (``GenerateResult.logprobs_mode``): the model's log-softmax
before temperature, top-k/top-p/min-p and penalties. They equal the
distribution the tokens were sampled from only at temperature 1 with no
top-k, top-p or min-p and no penalties; at any other setting a trainer
must apply the same transform itself before using them as the behaviour
policy.

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
    ``i``, ``logprobs[i]`` the engine's logprobs on exactly those ids (raw
    by default, see the module docstring).

    The opt-in fields stay empty (or None) unless the call asked for them
    and the session's server returns them, so an older server yields the
    same result as before.
    """

    served_revision: Optional[str]
    token_ids: list = field(default_factory=list)
    logprobs: list = field(default_factory=list)
    prompt_token_ids: list = field(default_factory=list)
    finish_reasons: list = field(default_factory=list)
    texts: list = field(default_factory=list)
    usage: dict = field(default_factory=dict)
    #: ``top_logprobs=k``: per choice, one row per sampled token of the k
    #: most likely token ids, most likely first, with the sampled token
    #: appended when it is outside the top k (so a row has k or k+1
    #: entries). ``top_logprobs[i][t][j]`` is the logprob of
    #: ``top_logprob_ids[i][t][j]``.
    top_logprob_ids: list = field(default_factory=list)
    top_logprobs: list = field(default_factory=list)
    #: ``return_prompt_logprobs=True``: per choice, the logprob of each
    #: prompt token given the tokens before it; the first entry is None.
    #: Always raw, whatever the session's logprobs mode.
    prompt_logprobs: list = field(default_factory=list)
    #: ``"raw"`` or ``"processed"``: what ``logprobs`` and ``top_logprobs``
    #: hold. Returned with any opt-in field or ``return_serving_info``.
    logprobs_mode: Optional[str] = None
    #: The replica (pod) that answered.
    replica: Optional[str] = None
    #: That replica's acked state digest for ``served_revision``, when it
    #: has one (None between the steps of a multi-step activation).
    served_state_digest: Optional[str] = None
    #: The full response body for anything the shaped fields omit.
    raw: dict = field(default_factory=dict)


@dataclass
class ScoreResult:
    """Per-token logprobs of given sequences, from :meth:`RlSessionClient.score`.

    ``logprobs[i][t]`` is the logprob of ``token_ids[i][t]`` given
    ``token_ids[i][:t]`` under ``served_revision``; ``logprobs[i][0]`` is
    None (the first token has no context). Raw log-softmax, whatever the
    session's logprobs mode.
    """

    served_revision: Optional[str]
    token_ids: list = field(default_factory=list)
    logprobs: list = field(default_factory=list)
    replica: Optional[str] = None
    served_state_digest: Optional[str] = None
    usage: dict = field(default_factory=dict)
    raw: dict = field(default_factory=dict)


def _is_flat_logprobs(value: Any) -> bool:
    return isinstance(value, list) and all(
        v is None or isinstance(v, (int, float)) for v in value
    )


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
        top_logprobs: Optional[int] = None,
        return_prompt_logprobs: bool = False,
        return_serving_info: bool = False,
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

        Opt-in extras (each sent only when set): ``top_logprobs=k`` (0 to
        20) fills ``top_logprob_ids`` / ``top_logprobs``;
        ``return_prompt_logprobs=True`` fills ``prompt_logprobs``, at the
        cost of a full prefill per prompt (the engine skips prefix-cache
        reads for it), so ask once per GRPO group rather than per sample;
        ``return_serving_info=True`` fills only ``logprobs_mode``,
        ``replica`` and ``served_state_digest``, which the other two also
        fill.
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
        if top_logprobs is not None:
            body["top_logprobs"] = top_logprobs
        if return_prompt_logprobs:
            body["return_prompt_logprobs"] = True
        if return_serving_info:
            body["return_serving_info"] = True
        body.update(extra)

        out = self._post_json("/v1/completions", body)
        result = GenerateResult(
            served_revision=out.get("servedRevision"),
            raw=out,
            usage=out.get("usage") or {},
            logprobs_mode=out.get("logprobsMode"),
            replica=out.get("replica"),
            served_state_digest=out.get("servedStateDigest"),
        )
        choices = out.get("choices") or []
        for choice in choices:
            result.token_ids.append(choice.get("token_ids"))
            result.logprobs.append(choice.get("logprobs"))
            result.prompt_token_ids.append(choice.get("prompt_token_ids"))
            result.finish_reasons.append(choice.get("finish_reason"))
            result.texts.append(choice.get("text"))
        # Opt-in columns: only when every choice carries them, so the
        # lists stay choice-aligned or empty.
        tops = [choice.get("top_logprobs") for choice in choices]
        if choices and all(isinstance(t, dict) for t in tops):
            result.top_logprob_ids = [t.get("ids") for t in tops]
            result.top_logprobs = [t.get("logprobs") for t in tops]
        prompt_lps = [choice.get("prompt_logprobs") for choice in choices]
        # A flat list of numbers; vLLM's own per-token dicts (a caller
        # passing ``prompt_logprobs`` straight through) stay in ``raw``.
        if choices and all(_is_flat_logprobs(p) for p in prompt_lps):
            result.prompt_logprobs = prompt_lps
        return result

    def score(
        self,
        token_ids: Sequence,
        *,
        revision: Optional[str] = None,
    ) -> ScoreResult:
        """Per-token logprobs of the given sequences under the revision
        the session serves (or ``revision``, asserted as in
        :meth:`generate`).

        ``token_ids`` is one sequence (``[int, ...]``) or a batch. Each
        sequence is scored as a prompt with one throwaway sampled token
        (the engine needs ``max_tokens >= 1``), so the cost is a full
        prefill per sequence: prompt logprobs make the engine skip
        prefix-cache reads, so nothing is reused from earlier requests.
        The logprobs are raw log-softmax whatever the session's mode,
        which makes them comparable with a trainer's own forward pass at
        temperature 1.
        """
        out = self.generate(
            token_ids=token_ids,
            n=1,
            max_tokens=1,
            revision=revision,
            logprobs=0,
            return_prompt_logprobs=True,
        )
        if len(out.prompt_logprobs) != len(out.prompt_token_ids):
            raise SessionServingError(
                "the session returned no prompt logprobs; its server predates "
                "return_prompt_logprobs"
            )
        return ScoreResult(
            served_revision=out.served_revision,
            token_ids=out.prompt_token_ids,
            logprobs=out.prompt_logprobs,
            replica=out.replica,
            served_state_digest=out.served_state_digest,
            usage=out.usage,
            raw=out.raw,
        )

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

    def trajectory(self, policy: str = "strict") -> "Trajectory":
        """A :class:`Trajectory` over this session."""
        return Trajectory(self, policy=policy)

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
        """Block until ``revision`` is ``Active`` and return its record. The
        record may not carry ``probes`` yet; see :meth:`revision_probes`."""
        return self._need_publisher().wait_until_active(revision, **kwargs)

    def revision_probes(self, revision: str) -> Optional[dict]:
        """The report-only numerics probes (prefill vs decode k3 per
        replica) for ``revision``, or ``None`` until the first arrives. See
        :meth:`basilica.publisher.RlPolicyHandle.revision_probes`."""
        return self._need_publisher().revision_probes(revision)


@dataclass
class TrajectoryTurn:
    """One turn of a :class:`Trajectory`: the sampled tokens sit at
    ``token_range`` (start inclusive, end exclusive) in the turn's full
    sequence (its prompt followed by its completion), and
    ``served_revision`` generated them."""

    turn: int
    token_range: tuple
    served_revision: Optional[str]


class Trajectory:
    """Records which revision served each turn of one multi-turn rollout.

    Each turn's prompt is the whole history so far (one sequence, ``n``
    is 1), so ``token_range`` positions index the final sequence too.
    ``policy`` decides what happens when the session moves to a new
    revision between turns:

    - ``"strict"``: every turn after the first asserts the first turn's
      revision, so a switch raises :class:`StaleRevisionError` (restart
      the trajectory, or keep what ``turns`` holds).
    - ``"allow_switch"``: no assertion; the switch is recorded and
      ``switches`` lists it.

    Example::

        traj = session.trajectory("strict")
        out = traj.generate(token_ids=prompt, max_tokens=256)
        ...
        out = traj.generate(token_ids=prompt + reply + tool_output)
    """

    POLICIES = ("strict", "allow_switch")

    def __init__(self, session: RlSessionClient, policy: str = "strict"):
        if policy not in self.POLICIES:
            raise ValueError(f"policy must be one of {self.POLICIES}, got {policy!r}")
        self._session = session
        self.policy = policy
        self.turns: list = []

    @property
    def pinned_revision(self) -> Optional[str]:
        """The first turn's revision (None before the first turn, or when
        the base model served it)."""
        return self.turns[0].served_revision if self.turns else None

    @property
    def switches(self) -> list:
        """``(turn, from_revision, to_revision)`` for every turn served by
        a different revision than the turn before it."""
        return [
            (cur.turn, prev.served_revision, cur.served_revision)
            for prev, cur in zip(self.turns, self.turns[1:])
            if cur.served_revision != prev.served_revision
        ]

    def generate(self, *, token_ids: Sequence, n: int = 1, **kwargs: Any) -> GenerateResult:
        """One turn: :meth:`RlSessionClient.generate` on one sequence, with
        the pinning policy applied and the turn recorded."""
        ids = list(token_ids)
        if not ids or not all(isinstance(t, int) for t in ids):
            raise ValueError("a trajectory turn takes one sequence of token ids")
        if n != 1:
            raise ValueError("a trajectory is one sequence: n must be 1")
        pinned = self.policy == "strict" and bool(self.turns)
        if pinned:
            want = self.pinned_revision
            if kwargs.get("revision") not in (None, want):
                raise ValueError(
                    f"revision={kwargs['revision']!r} conflicts with the trajectory's "
                    f"pinned revision {want!r}"
                )
            if want is not None:
                kwargs["revision"] = want
        out = self._session.generate(token_ids=ids, n=1, **kwargs)
        if pinned and out.served_revision != self.pinned_revision:
            # Only reachable when the first turn ran on the base model,
            # which no assertion can name.
            raise StaleRevisionError(
                f"trajectory pinned to {self.pinned_revision!r} but turn "
                f"{len(self.turns)} was served by {out.served_revision!r}"
            )
        completion = out.token_ids[0] if out.token_ids else None
        start = len(ids)
        end = start + len(completion or [])
        self.turns.append(TrajectoryTurn(len(self.turns), (start, end), out.served_revision))
        return out
