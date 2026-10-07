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
    ScoreResult,
    SessionServingError,
    SessionUnavailableError,
    StaleRevisionError,
    Trajectory,
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
    with pytest.raises(Exception, match="no publisher attached"):
        client.revision_probes("r0")


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

        def revision_probes(self, revision):
            self.calls.append(("probes", revision))
            return {"status": "ok", "k3Max": 0.0001} if revision == "r1" else None

    pub = FakePublisher()
    client = RlSessionClient("http://127.0.0.1:9", "t", publisher=pub)
    assert client.publish({}, revision="r1")["state"] == "Validated"
    assert client.wait_until_active("r1")["state"] == "Active"
    assert client.revision_probes("r1") == {"status": "ok", "k3Max": 0.0001}
    assert client.revision_probes("r2") is None
    assert pub.calls == [
        ("publish", "r1"),
        ("wait", "r1"),
        ("probes", "r1"),
        ("probes", "r2"),
    ]


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


# -- opt-in serving data: top-k, prompt logprobs, score(), Trajectory -------


def test_default_requests_carry_none_of_the_opt_in_flags(session):
    client, fake = session
    fake.responses = [(200, _t4_response())]
    out = client.generate(token_ids=[[1, 2]])
    body = fake.requests[0]["body"]
    for flag in ("top_logprobs", "return_prompt_logprobs", "return_serving_info"):
        assert flag not in body, flag
    assert out.top_logprob_ids == [] and out.top_logprobs == []
    assert out.prompt_logprobs == []
    assert out.logprobs_mode is None and out.replica is None
    assert out.served_state_digest is None


def test_top_logprobs_and_serving_fields_are_parsed(session):
    client, fake = session
    resp = _t4_response()
    resp["choices"][0]["top_logprobs"] = {
        "ids": [[785, 11], [7, 9, 389], [264, 13]],
        "logprobs": [[-0.12, -2.5], [-0.2, -1.9, -3.5], [-0.03, -4.0]],
    }
    resp.update(logprobsMode="raw", replica="sess-abc-0", servedStateDigest="xxh3_128:ab")
    fake.responses = [(200, resp)]
    out = client.generate(token_ids=[[151644, 8948, 198]], top_logprobs=2)
    assert fake.requests[0]["body"]["top_logprobs"] == 2
    assert out.top_logprob_ids == [[[785, 11], [7, 9, 389], [264, 13]]]
    assert out.top_logprobs[0][1] == [-0.2, -1.9, -3.5]
    assert out.logprobs_mode == "raw"
    assert out.replica == "sess-abc-0"
    assert out.served_state_digest == "xxh3_128:ab"


def test_prompt_logprobs_flat_only(session):
    client, fake = session
    resp = _t4_response()
    resp["choices"][0]["prompt_logprobs"] = [None, -7.25, -0.01]
    fake.responses = [(200, resp)]
    out = client.generate(token_ids=[[151644, 8948, 198]], return_prompt_logprobs=True)
    assert fake.requests[0]["body"]["return_prompt_logprobs"] is True
    assert out.prompt_logprobs == [[None, -7.25, -0.01]]
    # vLLM's own verbose shape (prompt_logprobs passed straight through via
    # **extra, as before) is left in raw, not misread as flat.
    resp = _t4_response()
    resp["choices"][0]["prompt_logprobs"] = [None, {"8948": {"logprob": -7.25}}]
    fake.responses = [(200, resp)]
    out = client.generate(token_ids=[[1]], prompt_logprobs=0)
    assert fake.requests[1]["body"]["prompt_logprobs"] == 0
    assert "return_prompt_logprobs" not in fake.requests[1]["body"]
    assert out.prompt_logprobs == []
    assert out.raw["choices"][0]["prompt_logprobs"][1]["8948"]["logprob"] == -7.25


def test_serving_info_flag(session):
    client, fake = session
    fake.responses = [(200, dict(_t4_response(), logprobsMode="processed"))]
    out = client.generate(token_ids=[[1]], return_serving_info=True)
    assert fake.requests[0]["body"]["return_serving_info"] is True
    assert out.logprobs_mode == "processed"


def test_score_asks_for_prompt_logprobs_with_one_token(session):
    client, fake = session
    resp = {
        "servedRevision": "step-0042",
        "replica": "sess-abc-1",
        "logprobsMode": "raw",
        "choices": [
            {"prompt_token_ids": [5, 6, 7], "token_ids": [9], "logprobs": [-1.0],
             "prompt_logprobs": [None, -2.0, -0.5]},
            {"prompt_token_ids": [5, 8], "token_ids": [9], "logprobs": [-1.0],
             "prompt_logprobs": [None, -3.0]},
        ],
        "usage": {"prompt_tokens": 5, "completion_tokens": 2},
    }
    fake.responses = [(200, resp)]
    out = client.score([[5, 6, 7], [5, 8]], revision="step-0042")
    body = fake.requests[0]["body"]
    assert body["prompt"] == [[5, 6, 7], [5, 8]]
    assert body["max_tokens"] == 1 and body["n"] == 1
    assert body["logprobs"] == 0
    assert body["return_prompt_logprobs"] is True
    assert body["return_token_ids"] is True
    assert body["revision"] == "step-0042"
    assert isinstance(out, ScoreResult)
    assert out.served_revision == "step-0042"
    assert out.token_ids == [[5, 6, 7], [5, 8]]
    assert out.logprobs == [[None, -2.0, -0.5], [None, -3.0]]
    assert out.replica == "sess-abc-1"


def test_score_on_a_server_without_prompt_logprobs_fails_loudly(session):
    client, fake = session
    fake.responses = [(200, _t4_response())]
    with pytest.raises(SessionServingError, match="predates"):
        client.score([151644, 8948, 198])
    assert fake.requests[0]["body"]["prompt"] == [[151644, 8948, 198]]


def _turn(revision, completion):
    return {
        "servedRevision": revision,
        "choices": [{"token_ids": completion, "logprobs": [-0.1] * len(completion)}],
    }


def test_strict_trajectory_pins_the_first_revision(session):
    client, fake = session
    fake.responses = [
        (200, _turn("step-0042", [10, 11])),
        (200, _turn("step-0042", [12])),
    ]
    traj = client.trajectory()
    assert isinstance(traj, Trajectory) and traj.policy == "strict"
    traj.generate(token_ids=[1, 2, 3], max_tokens=8)
    assert "revision" not in fake.requests[0]["body"], "turn 1 takes the newest"
    traj.generate(token_ids=[1, 2, 3, 10, 11, 4])
    assert fake.requests[1]["body"]["revision"] == "step-0042", "later turns assert it"
    assert fake.requests[1]["body"]["max_tokens"] == 512
    assert [(t.turn, t.token_range, t.served_revision) for t in traj.turns] == [
        (0, (3, 5), "step-0042"),
        (1, (6, 7), "step-0042"),
    ]
    assert traj.pinned_revision == "step-0042" and traj.switches == []


def test_strict_trajectory_surfaces_a_revision_switch(session):
    client, fake = session
    fake.responses = [
        (200, _turn("step-0042", [10])),
        (409, {"error": {"type": "StaleRevision", "message": "serving: step-0043"}}),
    ]
    traj = Trajectory(client, policy="strict")
    traj.generate(token_ids=[1])
    with pytest.raises(StaleRevisionError, match="step-0043"):
        traj.generate(token_ids=[1, 10, 2])
    assert len(traj.turns) == 1, "the refused turn is not recorded"
    with pytest.raises(ValueError, match="conflicts"):
        traj.generate(token_ids=[1, 10, 2], revision="step-0043")


def test_strict_trajectory_from_the_base_model_checks_after_the_fact(session):
    # No assertion can name the base model, so a later turn on a real
    # revision is caught from its stamp.
    client, fake = session
    fake.responses = [(200, _turn(None, [10])), (200, _turn("step-0001", [11]))]
    traj = client.trajectory("strict")
    traj.generate(token_ids=[1])
    with pytest.raises(StaleRevisionError):
        traj.generate(token_ids=[1, 10])
    assert "revision" not in fake.requests[1]["body"]


def test_allow_switch_trajectory_records_switches(session):
    client, fake = session
    fake.responses = [
        (200, _turn("step-0042", [10, 11])),
        (200, _turn("step-0043", [12])),
        (200, _turn("step-0043", [13, 14, 15])),
    ]
    traj = client.trajectory("allow_switch")
    traj.generate(token_ids=[1, 2])
    traj.generate(token_ids=[1, 2, 10, 11])
    traj.generate(token_ids=[1, 2, 10, 11, 12, 3])
    assert all("revision" not in r["body"] for r in fake.requests)
    assert traj.switches == [(1, "step-0042", "step-0043")]
    assert [t.token_range for t in traj.turns] == [(2, 4), (4, 5), (6, 9)]


def test_trajectory_rejects_batches_and_bad_policies(session):
    client, _fake = session
    with pytest.raises(ValueError, match="policy"):
        client.trajectory("loose")
    traj = client.trajectory()
    with pytest.raises(ValueError, match="one sequence"):
        traj.generate(token_ids=[[1], [2]])
    with pytest.raises(ValueError, match="n must be 1"):
        traj.generate(token_ids=[1], n=4)
