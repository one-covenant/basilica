"""Transient-failure retries on the BYOT path: platform reads, deletes,
session generation and artifact uploads.

A DNS error or a dropped connection while polling a session used to end a
run (and its cleanup) on the spot; these tests pin the bounded retries and,
just as much, that permanent failures still raise at once. Pure python
fakes, except the last group, which runs the compiled core against a local
HTTP server so the core's real error mapping is what gets classified.
"""

import json
import socket
import threading
import urllib.error
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest

from basilica import _retry
from basilica import session as session_mod
from basilica._retry import call_with_retry, is_transient, retry_budget_s
from basilica.rl import RlNamespace
from basilica.session import RlSessionClient, SessionServingError


@pytest.fixture(autouse=True)
def _no_sleep(monkeypatch):
    slept = []
    monkeypatch.setattr(_retry.time, "sleep", slept.append)
    return slept


# -- classification ------------------------------------------------------------


@pytest.mark.parametrize(
    "exc",
    [
        ConnectionError("HTTP client error: error sending request for url: dns error"),
        ConnectionResetError("reset by peer"),
        TimeoutError("timed out"),
        RuntimeError("Request failed with status 502 Bad Gateway: <html>"),
        RuntimeError("Request failed with status 503 Service Unavailable: "),
        RuntimeError("Request failed with status 504 Gateway Timeout: upstream"),
        RuntimeError("Rate limit exceeded"),
        RuntimeError("Service temporarily unavailable"),
    ],
)
def test_transient_failures(exc):
    assert is_transient(exc)


@pytest.mark.parametrize(
    "exc",
    [
        KeyError("rollout session not found"),
        ValueError("Bad request: SessionGpuInvalid"),
        PermissionError("Authorization error"),
        RuntimeError("Request failed with status 500 Internal Server Error: bug"),
        RuntimeError("relay credentials rejected"),
    ],
)
def test_permanent_failures(exc):
    assert not is_transient(exc)


@pytest.mark.parametrize(
    "raw, want", [(None, 120.0), ("", 120.0), ("30", 30.0), ("-5", 0.0), ("x", 120.0)]
)
def test_budget_from_env(monkeypatch, raw, want):
    if raw is None:
        monkeypatch.delenv(_retry.RETRY_BUDGET_ENV, raising=False)
    else:
        monkeypatch.setenv(_retry.RETRY_BUDGET_ENV, raw)
    assert retry_budget_s() == want


# -- call_with_retry -------------------------------------------------------------


def _flaky(errors, result="ok"):
    calls = []

    def fn():
        calls.append(1)
        if errors:
            raise errors.pop(0)
        return result

    return fn, calls


def test_transient_errors_are_retried_until_success(_no_sleep):
    fn, calls = _flaky([ConnectionError("dns error"), RuntimeError("status 503 x")])
    assert call_with_retry(fn, "get session") == "ok"
    assert len(calls) == 3
    assert len(_no_sleep) == 2 and all(0 < s <= 15 for s in _no_sleep)


def test_permanent_errors_raise_at_once(_no_sleep):
    fn, calls = _flaky([KeyError("gone")])
    with pytest.raises(KeyError):
        call_with_retry(fn, "get session")
    assert len(calls) == 1 and not _no_sleep


def test_budget_exhaustion_raises_the_last_error():
    now = [0.0]
    fn, calls = _flaky([ConnectionError(f"down {i}") for i in range(100)])

    def sleep(s):
        now[0] += s

    with pytest.raises(ConnectionError, match="down"):
        call_with_retry(fn, "get session", budget_s=30, sleep=sleep, clock=lambda: now[0])
    assert 2 < len(calls) < 100
    assert now[0] <= 30


def test_zero_budget_disables_retries(monkeypatch):
    monkeypatch.setenv(_retry.RETRY_BUDGET_ENV, "0")
    fn, calls = _flaky([ConnectionError("down")])
    with pytest.raises(ConnectionError):
        call_with_retry(fn, "get session")
    assert len(calls) == 1


# -- RlNamespace over a fake core -----------------------------------------------


class FlakyCore:
    """Each rl_* method fails with the queued errors, then answers."""

    def __init__(self, **errors):
        self.errors = {k: list(v) for k, v in errors.items()}
        self.calls = []

    def _answer(self, name, payload):
        self.calls.append(name)
        queue = self.errors.get(name) or []
        if queue:
            raise queue.pop(0)
        return json.dumps(payload)

    def rl_get_session(self, uid):
        return self._answer("rl_get_session", {"sessionUid": uid, "state": "active"})

    def rl_get_session_usage(self, uid):
        return self._answer("rl_get_session_usage", {"gpuHours": 0.5})

    def rl_get_revision(self, policy, rev):
        return self._answer("rl_get_revision", {"revision": rev, "state": "Active"})

    def rl_get_policy(self, name):
        return self._answer(
            "rl_get_policy",
            {"name": name, "effectivePrefix": "p/", "repo": "r", "commit": "c" * 40},
        )

    def rl_delete_session(self, uid):
        return self._answer("rl_delete_session", {"sessionUid": uid})

    def rl_delete_policy(self, name):
        return self._answer("rl_delete_policy", {"name": name})

    def rl_create_session(self, body):
        return self._answer("rl_create_session", {"sessionUid": "s-1"})


DNS = ConnectionError("HTTP client error: error sending request: dns error")


def test_reads_survive_a_dns_blip():
    core = FlakyCore(
        rl_get_session=[DNS],
        rl_get_session_usage=[DNS, DNS],
        rl_get_revision=[RuntimeError("Request failed with status 502 Bad Gateway: ")],
        rl_get_policy=[DNS],
    )
    rl = RlNamespace(core)
    assert rl.get_session("s-1")["state"] == "active"
    assert rl.session_usage("s-1") == {"gpuHours": 0.5}
    assert rl.get_revision("pol", "step-1")["state"] == "Active"
    assert rl.get_policy("pol")["effectivePrefix"] == "p/"
    assert core.calls.count("rl_get_session_usage") == 3


def test_a_missing_session_is_not_retried():
    core = FlakyCore(rl_get_session=[KeyError("not found")])
    with pytest.raises(KeyError):
        RlNamespace(core).get_session("s-1")
    assert core.calls == ["rl_get_session"]


def test_delete_retries_and_a_retry_finding_it_gone_is_success():
    # The first DELETE landed but its response was lost.
    core = FlakyCore(rl_delete_session=[DNS, KeyError("not found")])
    assert RlNamespace(core).delete_session("s-1") == {}
    assert core.calls == ["rl_delete_session"] * 2


def test_delete_of_a_session_that_never_existed_still_raises():
    core = FlakyCore(rl_delete_policy=[KeyError("not found")])
    with pytest.raises(KeyError):
        RlNamespace(core).delete_policy("nope")


def test_creates_are_never_retried():
    core = FlakyCore(rl_create_session=[DNS])
    with pytest.raises(ConnectionError):
        RlNamespace(core).create_session("pol", gpu_model="A100", gpu_count=1)
    assert core.calls == ["rl_create_session"]


# -- publisher: wait_until_active and uploads -------------------------------------


def _handle(core, s3=None):
    from basilica.publisher import PolicyStorage, RlPolicyHandle

    h = RlPolicyHandle(
        core, "pol", storage=PolicyStorage(bucket="b", endpoint="https://acc.example")
    )
    if s3 is not None:
        h._s3 = s3
    return h


def test_handle_and_wait_until_active_survive_a_blip():
    core = FlakyCore(rl_get_policy=[DNS], rl_get_revision=[DNS, DNS])
    h = _handle(core)
    assert h.wait_until_active("step-0001", poll_interval=0)["state"] == "Active"
    assert h.revision_probes("step-0001") is None


def test_wait_until_active_raises_after_the_budget(monkeypatch):
    monkeypatch.setenv(_retry.RETRY_BUDGET_ENV, "0")
    core = FlakyCore(rl_get_revision=[DNS])
    with pytest.raises(ConnectionError):
        _handle(core).wait_until_active("step-0001", poll_interval=0)


class ScriptedS3:
    def __init__(self, errors):
        self.errors = list(errors)
        self.uploads = 0

    def upload_file(self, filename, bucket, key, Config=None):
        self.uploads += 1
        if self.errors:
            raise self.errors.pop(0)

    def head_object(self, Bucket, Key):
        raise RuntimeError("404")


def _upload_failures():
    botocore = pytest.importorskip("botocore.exceptions")
    s3t = pytest.importorskip("boto3.exceptions")
    return botocore, s3t


@pytest.fixture()
def no_upload_sleep(monkeypatch):
    from basilica import publisher

    slept = []
    monkeypatch.setattr(publisher.time, "sleep", slept.append)
    return slept


def test_upload_restarts_after_a_closed_connection(tmp_path, no_upload_sleep):
    botocore, s3t = _upload_failures()
    f = tmp_path / "anchor.safetensors"
    f.write_bytes(b"x" * 10)
    s3 = ScriptedS3(
        [
            botocore.ConnectionClosedError(endpoint_url="https://acc.example"),
            s3t.S3UploadFailedError(
                "Failed to upload a to b: An error occurred (InvalidPart) when calling "
                "the CompleteMultipartUpload operation"
            ),
        ]
    )
    assert _handle(FlakyCore(), s3)._upload("k/a", f) == "s3://b/k/a"
    assert s3.uploads == 3 and len(no_upload_sleep) == 2


def test_upload_gives_up_after_the_restarts(tmp_path, monkeypatch, no_upload_sleep):
    botocore, _ = _upload_failures()
    monkeypatch.setenv("BASILICA_UPLOAD_RESTARTS", "2")
    f = tmp_path / "anchor.safetensors"
    f.write_bytes(b"x" * 10)
    s3 = ScriptedS3(
        [botocore.EndpointConnectionError(endpoint_url="https://acc.example")] * 5
    )
    with pytest.raises(botocore.EndpointConnectionError):
        _handle(FlakyCore(), s3)._upload("k/a", f)
    assert s3.uploads == 3


def test_a_refused_upload_is_not_restarted(tmp_path, no_upload_sleep):
    _, s3t = _upload_failures()
    f = tmp_path / "anchor.safetensors"
    f.write_bytes(b"x" * 10)
    s3 = ScriptedS3([s3t.S3UploadFailedError("An error occurred (AccessDenied)")])
    with pytest.raises(s3t.S3UploadFailedError):
        _handle(FlakyCore(), s3)._upload("k/a", f)
    assert s3.uploads == 1 and not no_upload_sleep


def test_upload_client_retries_each_request():
    pytest.importorskip("boto3")
    s3 = _handle(FlakyCore())._s3_client()
    retries = s3.meta.config.retries
    assert retries == {"mode": "standard", "total_max_attempts": 10}


# -- session generate over an unreachable endpoint --------------------------------


def _session_ok():
    return {
        "servedRevision": "step-0001",
        "choices": [{"token_ids": [1], "logprobs": [-0.1], "text": "a"}],
    }


def test_generate_retries_a_dns_failure(monkeypatch):
    slept = []
    monkeypatch.setattr(session_mod.time, "sleep", slept.append)
    calls = []

    def once(self, path, body):
        calls.append(path)
        if len(calls) == 1:
            raise session_mod._Unreachable("session unreachable: dns") from urllib.error.URLError(
                socket.gaierror(8, "nodename nor servname provided")
            )
        return _session_ok()

    monkeypatch.setattr(RlSessionClient, "_post_json_once", once)
    out = RlSessionClient("https://s.example", "t").generate(token_ids=[1])
    assert out.served_revision == "step-0001" and len(calls) == 2 and len(slept) == 1


def test_unreachable_session_raises_a_serving_error_after_the_budget():
    # Nothing listens on this port: connection refused, retried, then the
    # same SessionServingError callers already catch.
    probe = socket.socket()
    probe.bind(("127.0.0.1", 0))
    port = probe.getsockname()[1]
    probe.close()
    client = RlSessionClient(f"http://127.0.0.1:{port}", "t", retry_budget_s=0)
    with pytest.raises(SessionServingError, match="session unreachable"):
        client.generate(token_ids=[1])


# -- the compiled core's real error mapping -------------------------------------


class _Gateway(BaseHTTPRequestHandler):
    script: list = []  # (status, body bytes) popped per request
    hits = 0

    def do_GET(self):  # noqa: N802
        _Gateway.hits += 1
        ok = {"sessionUid": "s-1", "state": "active", "policy": "p", "url": "u"}
        status, body = (
            _Gateway.script.pop(0) if _Gateway.script else (200, json.dumps(ok).encode())
        )
        self.send_response(status)
        self.send_header("Content-Type", "text/html" if status >= 500 else "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_):
        pass


@pytest.fixture()
def core_rl():
    pytest.importorskip("basilica._basilica")
    import basilica

    _Gateway.script, _Gateway.hits = [], 0
    httpd = ThreadingHTTPServer(("127.0.0.1", 0), _Gateway)
    threading.Thread(target=httpd.serve_forever, daemon=True).start()
    yield basilica.BasilicaClient(
        base_url=f"http://127.0.0.1:{httpd.server_address[1]}", api_key="k"
    ).rl
    httpd.shutdown()
    httpd.server_close()


def test_core_gateway_502_is_retried(core_rl):
    _Gateway.script = [(502, b"<html>bad gateway</html>")]
    assert core_rl.get_session("s-1")["state"] == "active"
    assert _Gateway.hits == 2


def test_core_dns_failure_is_transient(monkeypatch):
    pytest.importorskip("basilica._basilica")
    import basilica

    monkeypatch.setenv(_retry.RETRY_BUDGET_ENV, "0")
    rl = basilica.BasilicaClient(base_url="http://byot-retry-test.invalid", api_key="k").rl
    with pytest.raises(ConnectionError) as ei:
        rl.get_session("s-1")
    assert is_transient(ei.value)
