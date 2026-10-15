"""Bounded retries for idempotent platform reads.

A single DNS failure, dropped connection or load-balancer 502 while polling a
session or a revision must not end a training run that has been going for
an hour. Reads (GET a session, a revision, a policy, usage) are safe to
repeat, so they are retried with exponential backoff for up to a budget of
seconds (``BASILICA_RETRY_BUDGET_S``, default 120; 0 disables). Anything
that is not transient (a 4xx, a missing resource, bad credentials) raises
at once, exactly as before.

The compiled core maps a transport failure (connect, DNS, TLS, timeout) to
the builtin :class:`ConnectionError`, and a 5xx or 429 to
:class:`RuntimeError` whose message names the status. Both are recognised
here without a dependency on the core's types.
"""

from __future__ import annotations

import logging
import os
import random
import re
import time
from typing import Callable, Optional, TypeVar

T = TypeVar("T")

RETRY_BUDGET_ENV = "BASILICA_RETRY_BUDGET_S"
DEFAULT_RETRY_BUDGET_S = 120.0
#: Backoff cap between two attempts, in seconds.
MAX_BACKOFF_S = 15.0

_log = logging.getLogger("basilica.retry")

#: RuntimeError messages from the core that mean "try again": a gateway or
#: overload status (the core's fallback message carries the status code), a
#: rate limit, or a request timeout.
_TRANSIENT_RUNTIME = re.compile(
    r"status 50[234]\b|\b50[234] (Bad Gateway|Service Unavailable|Gateway Timeout)"
    r"|Service temporarily unavailable|Request timeout|Rate limit exceeded",
    re.IGNORECASE,
)


def retry_budget_s() -> float:
    """The retry budget in seconds, from ``BASILICA_RETRY_BUDGET_S``."""
    raw = os.environ.get(RETRY_BUDGET_ENV)
    if raw is None or raw.strip() == "":
        return DEFAULT_RETRY_BUDGET_S
    try:
        value = float(raw)
    except ValueError:
        return DEFAULT_RETRY_BUDGET_S
    return max(value, 0.0)


def is_transient(exc: BaseException) -> bool:
    """Whether ``exc`` is a failure that a repeat of the same read can fix."""
    if isinstance(exc, (ConnectionError, TimeoutError)):
        return True
    if isinstance(exc, RuntimeError):
        return bool(_TRANSIENT_RUNTIME.search(str(exc)))
    return False


def backoff_s(attempt: int) -> float:
    """Seconds to wait after failed attempt ``attempt`` (1-based): about
    1, 2, 4, 8 and then 15 seconds, with jitter so that many clients that
    lost the same connection do not come back in lockstep."""
    return random.uniform(0.5, 1.0) * min(MAX_BACKOFF_S, 2.0 ** (attempt - 1))


def call_with_retry(
    fn: Callable[[], T],
    what: str,
    *,
    budget_s: Optional[float] = None,
    sleep: Optional[Callable[[float], None]] = None,
    clock: Optional[Callable[[], float]] = None,
) -> T:
    """Call ``fn`` and return its result, retrying transient failures.

    ``what`` names the read in the warning logged before each retry. The
    last error is raised unchanged once the next wait would pass the budget,
    so a caller that already handles ``ConnectionError`` keeps working.
    """
    sleep = sleep or time.sleep
    clock = clock or time.monotonic
    budget = retry_budget_s() if budget_s is None else max(budget_s, 0.0)
    deadline = clock() + budget
    attempt = 0
    while True:
        attempt += 1
        try:
            return fn()
        except Exception as e:  # noqa: BLE001 - filtered by is_transient
            if not is_transient(e):
                raise
            wait = backoff_s(attempt)
            if clock() + wait > deadline:
                raise
            _log.warning(
                "%s failed (attempt %d), retrying in %.1fs: %s", what, attempt, wait, e
            )
            sleep(wait)
