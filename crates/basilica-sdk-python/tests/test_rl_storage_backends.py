"""Generic S3 storage (AWS S3 and S3-compatible stores such as MinIO):
the policy registration body and the publisher's upload client per
backend. Fake cores and a fake boto3: no network, no credentials, no
torch (botocore is needed for its Config).
"""

import json

import pytest
from basilica.publisher import PolicyStorage, RlPolicyHandle
from basilica.rl import RlNamespace


class _PolicyCore:
    """Just enough core for the handle constructor and create_policy."""

    def __init__(self):
        self.created = []

    def rl_get_policy(self, name):
        return json.dumps(
            {
                "name": name,
                "policyUid": "uid-1",
                "effectivePrefix": "policies/uid-1/",
                "repo": "Qwen/Qwen2.5-7B-Instruct",
                "commit": "a" * 40,
            }
        )

    def rl_create_policy(self, body_json):
        self.created.append(json.loads(body_json))
        return json.dumps({"policyUid": "uid-1", "effectivePrefix": "policies/uid-1/"})


def _client_kwargs(monkeypatch, storage):
    """Build the handle's upload client through a fake boto3 and return
    the kwargs it was constructed with."""
    pytest.importorskip("botocore")
    captured = {}

    class _FakeBoto3:
        def client(self, *args, **kwargs):
            captured["args"] = args
            captured["kwargs"] = kwargs
            return object()

    monkeypatch.setattr("basilica.publisher._boto3", lambda: _FakeBoto3())
    handle = RlPolicyHandle(_PolicyCore(), "p", storage=storage)
    handle._s3_client()
    assert captured["args"] == ("s3",)
    kw = captured["kwargs"]
    # The checksum settings hold for every backend (#1866).
    assert kw["config"].request_checksum_calculation == "when_required"
    assert kw["config"].response_checksum_validation == "when_required"
    return kw


def test_r2_client_is_unchanged(monkeypatch):
    kw = _client_kwargs(
        monkeypatch,
        PolicyStorage(bucket="my-weights", endpoint="https://acc.r2.cloudflarestorage.com"),
    )
    assert kw["endpoint_url"] == "https://acc.r2.cloudflarestorage.com"
    assert kw["region_name"] is None
    # No addressing style is forced on R2 without an override: boto3's
    # default applies exactly as before.
    assert kw["config"].s3 is None


def test_r2_explicit_addressing_is_applied(monkeypatch):
    kw = _client_kwargs(
        monkeypatch,
        PolicyStorage(
            bucket="my-weights",
            endpoint="https://acc.r2.cloudflarestorage.com",
            addressing="path",
        ),
    )
    assert kw["config"].s3 == {"addressing_style": "path"}


@pytest.mark.parametrize("region", ["us-east-1", "eu-central-1"])
def test_aws_s3_client_uses_region_and_virtual_hosted(monkeypatch, region):
    kw = _client_kwargs(
        monkeypatch,
        PolicyStorage(bucket="my-weights", backend="s3", region=region),
    )
    assert kw["endpoint_url"] is None, "boto3 derives the AWS endpoint"
    assert kw["region_name"] == region
    assert kw["config"].s3 == {"addressing_style": "virtual"}


def test_s3_compatible_client_defaults_path_style_and_us_east_1(monkeypatch):
    kw = _client_kwargs(
        monkeypatch,
        PolicyStorage(
            bucket="my-weights",
            endpoint="https://minio.example.com:9000",
            backend="s3-compatible",
        ),
    )
    assert kw["endpoint_url"] == "https://minio.example.com:9000"
    assert kw["region_name"] == "us-east-1"
    assert kw["config"].s3 == {"addressing_style": "path"}


def test_aws_s3_explicit_endpoint_is_passed_through(monkeypatch):
    kw = _client_kwargs(
        monkeypatch,
        PolicyStorage(
            bucket="my-weights",
            endpoint="https://s3.eu-central-1.amazonaws.com",
            backend="s3",
            region="eu-central-1",
        ),
    )
    assert kw["endpoint_url"] == "https://s3.eu-central-1.amazonaws.com"


def test_policy_storage_validation():
    with pytest.raises(ValueError, match="backend must be one of"):
        PolicyStorage(bucket="b", endpoint="https://e.example", backend="gcs")
    with pytest.raises(ValueError, match="addressing must be"):
        PolicyStorage(bucket="b", endpoint="https://e.example", addressing="vhost")
    with pytest.raises(ValueError, match="endpoint is required"):
        PolicyStorage(bucket="b")
    with pytest.raises(ValueError, match="endpoint is required"):
        PolicyStorage(bucket="b", backend="s3-compatible")
    # The upload must sign for the region the policy was registered with.
    with pytest.raises(ValueError, match="needs region"):
        PolicyStorage(bucket="b", backend="s3")
    # Signed uploads never go over cleartext, except to a local test store.
    for plain in ("http://minio.example.com:9000", "http://10.0.0.7", "ftp://e.example"):
        with pytest.raises(ValueError, match="https://"):
            PolicyStorage(bucket="b", endpoint=plain, backend="s3-compatible")
    for local in ("http://localhost:9000", "http://127.0.0.1:9000", "http://[::1]:9000"):
        PolicyStorage(bucket="b", endpoint=local, backend="s3-compatible")
    # AWS needs no endpoint; repr never echoes key material.
    s = PolicyStorage(
        bucket="b", backend="s3", region="us-east-1",
        access_key_id="AKIALIVE", secret_access_key="SECRETLIVE",
    )
    assert "AKIALIVE" not in repr(s) and "SECRETLIVE" not in repr(s)
    assert "backend='s3'" in repr(s)


_COMMON = dict(
    repo="Qwen/Qwen2.5-7B-Instruct",
    commit="a" * 40,
    tokenizer_digest="sha256:" + "b" * 64,
    bucket="my-weights",
    access_key_id="AK",
    secret_access_key="SK",
)


def test_create_policy_r2_body_is_unchanged():
    core = _PolicyCore()
    RlNamespace(core).create_policy(
        "p", endpoint="https://acc.r2.cloudflarestorage.com", **_COMMON
    )
    (body,) = core.created
    assert body["storage"] == {
        "backend": "r2",
        "bucket": "my-weights",
        "endpoint": "https://acc.r2.cloudflarestorage.com",
        "accessKeyId": "AK",
        "secretAccessKey": "SK",
    }


def test_create_policy_r2_passes_explicit_addressing():
    core = _PolicyCore()
    RlNamespace(core).create_policy(
        "p", endpoint="https://acc.r2.cloudflarestorage.com", addressing="path", **_COMMON
    )
    assert core.created[0]["storage"]["addressing"] == "path"


def test_create_policy_s3_omits_endpoint_and_passes_region():
    core = _PolicyCore()
    RlNamespace(core).create_policy("p", backend="s3", region="eu-central-1", **_COMMON)
    storage = core.created[0]["storage"]
    assert storage["backend"] == "s3"
    assert storage["region"] == "eu-central-1"
    assert "endpoint" not in storage, "the server derives the AWS endpoint"
    assert "addressing" not in storage


def test_create_policy_s3_compatible_passes_addressing():
    core = _PolicyCore()
    RlNamespace(core).create_policy(
        "p",
        backend="s3-compatible",
        endpoint="https://minio.example.com:9000",
        addressing="virtual",
        **_COMMON,
    )
    storage = core.created[0]["storage"]
    assert storage["backend"] == "s3-compatible"
    assert storage["endpoint"] == "https://minio.example.com:9000"
    assert storage["addressing"] == "virtual"
    assert "region" not in storage


def test_create_policy_refuses_bad_storage_client_side():
    ns = RlNamespace(object())  # any core access would explode
    with pytest.raises(ValueError, match="needs region"):
        ns.create_policy("p", backend="s3", **_COMMON)
    with pytest.raises(ValueError, match="endpoint is required"):
        ns.create_policy("p", backend="s3-compatible", **_COMMON)
    with pytest.raises(ValueError, match="endpoint is required"):
        ns.create_policy("p", **_COMMON)
    with pytest.raises(ValueError, match="backend must be one of"):
        ns.create_policy("p", backend="gcs", endpoint="https://e.example", **_COMMON)
    with pytest.raises(ValueError, match="addressing must be"):
        ns.create_policy(
            "p", endpoint="https://e.example", addressing="dns", **_COMMON
        )
    with pytest.raises(ValueError, match="https://"):
        ns.create_policy(
            "p", backend="s3-compatible", endpoint="http://minio.example.com", **_COMMON
        )
