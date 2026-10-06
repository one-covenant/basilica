"""RlSessionClient contract tests (#1666, interface doc steps 5+6):
generate() against a scripted session endpoint speaking the T4 serving
dialect — pure stdlib on both sides (the session client is deliberately
zero-dependency; session traffic never touches the compiled core)."""

import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest

from basilica import session as session_mod
from basilica.session import (
    GenerateResult,
    RlSessionClient,
    SessionServingError,
    SessionUnavailableError,
    StaleRevisionError,
)


class _FakeSession(BaseHTTPRequestHandler):
    requests: list = []
    responses: list = []  # (status, dict[, headers]) popped per request

    def do_POST(self):  # noqa: N802
        length = int(self.headers.get("Content-Length") or 0)
        _FakeSession.requests.append(
            {
                "path": self.path,
                "auth": self.headers.get("Authorization"),
                "body": json.loads(self.rfile.read(length)),
            }
        )
        status, resp, *rest = (
            _FakeSession.responses.pop(0) if _FakeSession.responses else (200, {})
        )
        payload = json.dumps(resp).encode()
        self.send_response(status)
        for name, value in (rest[0] if rest else {}).items():
            self.send_header(name, value)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, *_):
        pass


@pytest.fixture()
def session():
    _FakeSession.requests = []
    _FakeSession.responses = []
    httpd = ThreadingHTTPServer(("127.0.0.1", 0), _FakeSession)
    threading.Thread(target=httpd.serve_forever, daemon=True).start()
    client = RlSessionClient(
        f"http://127.0.0.1:{httpd.server_address[1]}", "sess-token"
    )
    yield client, _FakeSession
    httpd.shutdown()
    httpd.server_close()


def _t4_response():
    return {
        "servedRevision": "step-0042",
        "choices": [
            {
                "prompt_token_ids": [151644, 8948, 198],
                "token_ids": [785, 389, 264],
                "logprobs": [-0.12, -0.44, -0.03],
                "finish_reason": "stop",
                "text": " the answer",
            }
        ],
        "usage": {"prompt_tokens": 3, "completion_tokens": 3, "total_tokens": 6},
    }


def test_generate_speaks_the_training_dialect(session):
    client, fake = session
    fake.responses = [(200, _t4_response())]
    out = client.generate(
        token_ids=[[151644, 8948, 198]], n=8, seed=19, temperature=0.8,
        revision="step-0042",
    )
    (r,) = fake.requests
    assert r["path"] == "/v1/completions"
    assert r["auth"] == "Bearer sess-token"
    assert r["body"]["prompt"] == [[151644, 8948, 198]]
    assert r["body"]["return_token_ids"] is True, "the dialect switch always rides"
    assert r["body"]["logprobs"] == 1
    assert r["body"]["revision"] == "step-0042"
    assert r["body"]["seed"] == 19
    assert isinstance(out, GenerateResult)
    assert out.served_revision == "step-0042"
    assert out.token_ids == [[785, 389, 264]]
    assert out.logprobs == [[-0.12, -0.44, -0.03]]
    assert out.prompt_token_ids == [[151644, 8948, 198]]
    assert out.finish_reasons == ["stop"]
    assert out.usage["completion_tokens"] == 3


def test_single_prompt_normalizes_to_the_batch_shape(session):
    client, fake = session
    fake.responses = [(200, _t4_response())]
    client.generate(token_ids=[151644, 8948, 198])
    assert fake.requests[0]["body"]["prompt"] == [[151644, 8948, 198]], (
        "a bare token list is ONE prompt, wrapped — not three one-token prompts"
    )


def test_omitted_revision_is_omitted_on_the_wire(session):
    client, fake = session
    fake.responses = [(200, _t4_response())]
    out = client.generate(token_ids=[[1]])
    assert "revision" not in fake.requests[0]["body"], "the async idiom: no assertion"
    assert out.served_revision == "step-0042", "still told which weights answered"


def test_stale_revision_is_a_typed_refusal(session):
    client, fake = session
    fake.responses = [
        (
            409,
            {
                "error": {
                    "message": 'revision "step-0042" is not what this session serves',
                    "type": "StaleRevision",
                    "code": "StaleRevision",
                }
            },
        )
    ]
    with pytest.raises(StaleRevisionError, match="not what this session serves"):
        client.generate(token_ids=[[1]], revision="step-0042")


def test_other_failures_are_serving_errors(session):
    client, fake = session
    fake.responses = [(502, {"error": {"message": "engine: connect refused", "type": "upstream_error"}})]
    with pytest.raises(SessionServingError, match="HTTP 502"):
        client.generate(token_ids=[[1]])


def test_exactly_one_prompt_form(session):
    client, _fake = session
    with pytest.raises(ValueError, match="exactly one"):
        client.generate()
    with pytest.raises(ValueError, match="exactly one"):
        client.generate(token_ids=[[1]], prompt="hi")


def test_publishing_without_a_publisher_is_refused():
    client = RlSessionClient("http://127.0.0.1:9", "t")
    with pytest.raises(Exception, match="no publisher attached"):
        client.publish({}, revision="r0")


def test_publishing_delegates_to_the_attached_handle():
    class FakePublisher:
        def __init__(self):
            self.calls = []

        def publish(self, named, *, revision):
            self.calls.append(("publish", revision))
            return {"revision": revision, "state": "Validated"}

        def wait_until_active(self, revision, **kw):
            self.calls.append(("wait", revision))
            return {"revision": revision, "state": "Active"}

    pub = FakePublisher()
    client = RlSessionClient("http://127.0.0.1:9", "t", publisher=pub)
    assert client.publish({}, revision="r1")["state"] == "Validated"
    assert client.wait_until_active("r1")["state"] == "Active"
    assert pub.calls == [("publish", "r1"), ("wait", "r1")]


def test_platform_calls_without_identification_are_refused():
    client = RlSessionClient("http://127.0.0.1:9", "t")
    for call in (client.usage, client.park, client.resume):
        with pytest.raises(Exception, match="not identified to the platform"):
            call()


def test_platform_calls_delegate_to_the_rl_api():
    class FakeRlApi:
        def __init__(self):
            self.calls = []

        def session_usage(self, uid):
            self.calls.append(("usage", uid))
            return {"gpuHours": 1.0}

        def park_session(self, uid):
            self.calls.append(("park", uid))
            return {"state": "parked"}

        def resume_session(self, uid):
            self.calls.append(("resume", uid))
            return {"state": "starting"}

    api = FakeRlApi()
    client = RlSessionClient(
        "http://127.0.0.1:9", "t", api=api, session_uid="0b1c"
    )
    assert client.usage() == {"gpuHours": 1.0}
    assert client.park()["state"] == "parked"
    assert client.resume()["state"] == "starting"
    assert api.calls == [("usage", "0b1c"), ("park", "0b1c"), ("resume", "0b1c")]


# -- retryable 503s (#598) ---------------------------------------------------


def _refusal(kind):
    return (503, {"error": {"message": f"{kind}: retry the request", "type": kind, "code": kind}})


_OK = (200, _t4_response())


@pytest.fixture()
def no_sleep(monkeypatch):
    slept = []
    monkeypatch.setattr(session_mod.time, "sleep", slept.append)
    return slept


def test_engine_fault_is_retried_until_it_clears(session, no_sleep):
    client, fake = session
    fake.responses = [_refusal("EngineFault"), _refusal("EngineFault"), _OK]
    out = client.generate(token_ids=[[1]], revision="step-0042")
    assert out.served_revision == "step-0042"
    assert len(fake.requests) == 3
    # The same request, revision assertion included, is resent each time.
    assert all(r["body"] == fake.requests[0]["body"] for r in fake.requests)
    assert len(no_sleep) == 2 and all(0 < s <= 15 for s in no_sleep)


def test_a_draining_replica_is_retried(session, no_sleep):
    client, fake = session
    fake.responses = [_refusal("SessionDraining"), _OK]
    assert client.generate(token_ids=[[1]]).served_revision == "step-0042"
    assert len(fake.requests) == 2


def test_retry_after_is_honoured(session, no_sleep):
    client, fake = session
    status, body = _refusal("EngineFault")
    fake.responses = [(status, body, {"Retry-After": "3"}), _OK]
    client.generate(token_ids=[[1]])
    assert no_sleep == [3.0]


def test_an_exhausted_budget_raises_a_typed_serving_error(session, no_sleep):
    client, fake = session
    client._retry_budget_s = 2.0
    fake.responses = [_refusal("EngineFault")] * 10
    with pytest.raises(SessionUnavailableError, match="EngineFault") as exc:
        client.generate(token_ids=[[1]])
    assert exc.value.error_type == "EngineFault"
    assert isinstance(exc.value, SessionServingError)
    assert 1 < len(fake.requests) < 10


def test_a_zero_budget_raises_on_the_first_refusal(session, no_sleep):
    client, fake = session
    client._retry_budget_s = 0
    fake.responses = [_refusal("SessionDraining"), _OK]
    with pytest.raises(SessionUnavailableError, match="SessionDraining"):
        client.generate(token_ids=[[1]])
    assert len(fake.requests) == 1 and no_sleep == []


def test_other_503s_are_not_retried(session, no_sleep):
    client, fake = session
    fake.responses = [
        (503, {"error": {"message": "gateway overloaded", "type": "upstream_error"}}),
        _OK,
    ]
    with pytest.raises(SessionServingError, match="HTTP 503") as exc:
        client.generate(token_ids=[[1]])
    assert not isinstance(exc.value, SessionUnavailableError)
    assert len(fake.requests) == 1


def test_a_stale_revision_is_still_raised_at_once(session, no_sleep):
    client, fake = session
    fake.responses = [
        (409, {"error": {"message": "not served", "type": "StaleRevision"}}),
        _OK,
    ]
    with pytest.raises(StaleRevisionError):
        client.generate(token_ids=[[1]], revision="step-0009")
    assert len(fake.requests) == 1
