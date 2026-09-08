"""basilica.rl policy-registry contract tests (#1666), run against the
COMPILED core transport — the same Recorder pattern as test_rl_client.py:
a real stdlib HTTP server receives what the Rust client actually sends,
asserting the exact wire shapes the server's deny-unknown-fields DTOs
enforce (key casing, credential passthrough, omitted-vs-null parent, auth
header, path interpolation).

Requires the built extension (maturin develop / the installed wheel);
skipped cleanly where only the pure-python tree is on the path.
"""

import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest

basilica = pytest.importorskip("basilica")
pytest.importorskip("basilica._basilica")


class _Recorder(BaseHTTPRequestHandler):
    requests: list = []
    responses: list = []

    def _handle(self):
        length = int(self.headers.get("Content-Length") or 0)
        body = json.loads(self.rfile.read(length)) if length else None
        _Recorder.requests.append(
            {
                "method": self.command,
                "path": self.path,
                "auth": self.headers.get("Authorization"),
                "body": body,
            }
        )
        status, resp = (
            _Recorder.responses.pop(0) if _Recorder.responses else (200, {})
        )
        payload = json.dumps(resp).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    do_GET = do_POST = do_DELETE = _handle

    def log_message(self, *_):
        pass


@pytest.fixture()
def server():
    _Recorder.requests = []
    _Recorder.responses = []
    httpd = ThreadingHTTPServer(("127.0.0.1", 0), _Recorder)
    t = threading.Thread(target=httpd.serve_forever, daemon=True)
    t.start()
    yield f"http://127.0.0.1:{httpd.server_address[1]}", _Recorder
    httpd.shutdown()
    httpd.server_close()


def rl(base):
    return basilica.BasilicaClient(base_url=base, api_key="test-key").rl


_POLICY_RESP = {
    "policyUid": "uid-1",
    "effectivePrefix": "policies/uid-1/",
}


def test_create_policy_wire_shape(server):
    base, rec = server
    rec.responses = [(200, _POLICY_RESP)]
    out = rl(base).create_policy(
        "math-policy",
        repo="Qwen/Qwen2.5-7B-Instruct",
        commit="a" * 40,
        tokenizer_digest="sha256:" + "b" * 64,
        bucket="my-weights",
        endpoint="https://acc.r2.cloudflarestorage.com",
        access_key_id="AK",
        secret_access_key="SK",
    )
    (r,) = rec.requests
    assert (r["method"], r["path"]) == ("POST", "/rl/policies")
    assert r["auth"] == "Bearer test-key"
    assert r["body"] == {
        "name": "math-policy",
        "baseModel": {
            "repo": "Qwen/Qwen2.5-7B-Instruct",
            "commit": "a" * 40,
            "tokenizerDigest": "sha256:" + "b" * 64,
        },
        "updateFormat": "pulse-bf16-v1",
        "storage": {
            "backend": "r2",
            "bucket": "my-weights",
            "endpoint": "https://acc.r2.cloudflarestorage.com",
            "accessKeyId": "AK",
            "secretAccessKey": "SK",
        },
    }
    assert out["effectivePrefix"] == "policies/uid-1/"


def test_create_policy_with_referenced_secret_omits_inline_keys(server):
    base, rec = server
    rec.responses = [(200, _POLICY_RESP)]
    rl(base).create_policy(
        "math-policy",
        repo="r",
        commit="a" * 40,
        tokenizer_digest="sha256:" + "b" * 64,
        bucket="b",
        endpoint="https://e.example",
        credentials_secret="my-secret",
    )
    body = rec.requests[0]["body"]
    assert body["storage"]["credentialsSecret"] == "my-secret"
    assert "accessKeyId" not in body["storage"], "absent keys must be OMITTED"
    assert "secretAccessKey" not in body["storage"]


def test_create_revision_wire_shape_anchor_omits_parent(server):
    base, rec = server
    rec.responses = [
        (200, {"revision": "step-0000", "state": "Validated", "submittedAt": "t"})
    ]
    core_body = {
        "revision": "step-0000",
        "artifact": {
            "uri": "s3://my-weights/policies/uid-1/step-0000/anchor.safetensors",
            "sha256": "c" * 64,
        },
        "expectedStateDigest": "xxh3_128:" + "d" * 32,
    }
    client = basilica.BasilicaClient(base_url=base, api_key="test-key")
    json.loads(client.rl._core.rl_create_revision("math-policy", json.dumps(core_body)))
    (r,) = rec.requests
    assert (r["method"], r["path"]) == ("POST", "/rl/policies/math-policy/revisions")
    assert r["body"] == core_body
    assert "parentRevision" not in r["body"]


def test_get_revision_path_and_parse(server):
    base, rec = server
    rec.responses = [
        (
            200,
            {
                "revision": "step-0001",
                "parentRevision": "step-0000",
                "state": "Loading",
                "submittedAt": "t",
            },
        )
    ]
    out = rl(base).get_revision("math-policy", "step-0001")
    (r,) = rec.requests
    assert (r["method"], r["path"]) == (
        "GET",
        "/rl/policies/math-policy/revisions/step-0001",
    )
    assert out["state"] == "Loading"
    assert out["parentRevision"] == "step-0000"


def test_revision_grammar_refused_client_side(server):
    base, rec = server
    client = basilica.BasilicaClient(base_url=base, api_key="test-key")
    bad = {
        "revision": "../escape",
        "artifact": {"uri": "s3://b/k", "sha256": "c" * 64},
        "expectedStateDigest": "xxh3_128:" + "d" * 32,
    }
    with pytest.raises(ValueError, match="letter-or-digit edges"):
        client.rl._core.rl_create_revision("math-policy", json.dumps(bad))
    assert rec.requests == [], "an invalid revision must never reach the wire"


def test_delete_policy_wire_shape(server):
    base, rec = server
    rec.responses = [(200, {"name": "math-policy"})]
    rl(base).delete_policy("math-policy")
    (r,) = rec.requests
    assert (r["method"], r["path"]) == ("DELETE", "/rl/policies/math-policy")


def test_get_policy_wire_shape(server):
    base, rec = server
    rec.responses = [
        (
            200,
            {
                "name": "math-policy",
                "policyUid": "uid-1",
                "effectivePrefix": "policies/uid-1/",
                "repo": "Qwen/Qwen2.5-7B-Instruct",
                "commit": "a" * 40,
                "updateFormat": "pulse-bf16-v1",
                "totalRevisions": 2,
                "latestRevision": "step-0001",
            },
        )
    ]
    out = rl(base).get_policy("math-policy")
    (r,) = rec.requests
    assert (r["method"], r["path"]) == ("GET", "/rl/policies/math-policy")
    assert r["auth"] == "Bearer test-key"
    assert out["effectivePrefix"] == "policies/uid-1/"
    assert out["latestRevision"] == "step-0001"


_SESSION_RESP = {
    "sessionUid": "0b5e7a2e-1c1c-4a6c-9f6d-2f9f6b1e7a10",
    "url": "https://s-01bc.rollouts.basilica.ai",
    "token": "one-time-token",
    "state": "starting",
}


def test_create_session_wire_shape(server):
    base, rec = server
    rec.responses = [(200, _SESSION_RESP)]
    out = rl(base).create_session(
        "math-policy", gpu_model="H100", gpu_count=4, replicas=2
    )
    (r,) = rec.requests
    assert (r["method"], r["path"]) == ("POST", "/rl/rollout-sessions")
    assert r["body"] == {
        "policy": "math-policy",
        "fleet": {"replicas": 2, "gpu": {"model": "H100", "count": 4}},
    }
    assert "activation" not in r["body"], "default async = OMITTED, not sent"
    assert out["token"] == "one-time-token"


def test_get_and_delete_session_paths(server):
    base, rec = server
    uid = _SESSION_RESP["sessionUid"]
    rec.responses = [
        (200, {"sessionUid": uid, "state": "active", "policy": "math-policy",
               "url": _SESSION_RESP["url"]}),
        (200, {"sessionUid": uid}),
    ]
    assert rl(base).get_session(uid)["state"] == "active"
    rl(base).delete_session(uid)
    paths = [(r["method"], r["path"]) for r in rec.requests]
    assert paths == [
        ("GET", f"/rl/rollout-sessions/{uid}"),
        ("DELETE", f"/rl/rollout-sessions/{uid}"),
    ]


def test_session_id_grammar_refused_client_side(server):
    base, rec = server
    with pytest.raises(ValueError, match="session id"):
        rl(base).get_session("NOT_A_VALID/ID")
    assert rec.requests == [], "an invalid id never reaches the wire"
