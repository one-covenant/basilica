//! RL Training API client: GRPO post-training on the Basilica platform.
//!
//! Warm GPU clusters, GRPO jobs with custom rewards / datasets / in-cluster
//! LLM-judge, and the declarative manifest surface. The request DTOs here
//! mirror `basilica-api`'s `/rl/*` route DTOs field-for-field (the server
//! uses `deny_unknown_fields`, so the wire shape is the contract); keeping
//! them as shared serde types is what lets the compiler — not a runtime
//! test — catch drift from the server.

use serde::{Deserialize, Serialize};

// Client methods live in `client.rs` (the crate convention: DTOs here, the
// `impl BasilicaClient` beside the private get/post transport helpers).

// ---------------------------------------------------------------------------
// Cluster request DTOs
// ---------------------------------------------------------------------------

/// A trainer/rollout fleet's per-pod GPU shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RlGpuRequest {
    /// GPU model name, e.g. `H100`, `H200`.
    pub model: String,
    /// GPUs per pod. The trainer total (`count * replicas`) must divide the
    /// GRPO train batch — admission rejects shapes that cannot (valid: 1/2/4/8).
    pub count: u32,
    /// Optional minimum VRAM per GPU in GB. >=140 marks an H200-class fleet,
    /// which the >=16B recipe requires.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_memory_gb: Option<u32>,
}

/// One fleet (trainer or rollout).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RlFleetRequest {
    /// Pod replicas (v0 admits exactly 1 for the trainer).
    pub replicas: u32,
    /// Per-pod GPU shape.
    pub gpu: RlGpuRequest,
}

/// Create a warm RL cluster (`POST /rl/clusters`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRlClusterRequest {
    /// Optional cluster name (DNS-1035; generated when omitted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Base model to pin, e.g. `Qwen/Qwen2.5-7B-Instruct`. Must be on the
    /// platform allowlist; >=16B models require H200-class trainers.
    pub base_model: String,
    /// Trainer fleet (FSDP).
    pub trainer: RlFleetRequest,
    /// Rollout fleet (vLLM).
    pub rollout: RlFleetRequest,
    /// Optional idle-TTL (e.g. `30m`) after which an idle cluster reaps itself.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idle_ttl: Option<String>,
    /// Bring-your-own relay storage (#1578; server #1574). Omit for the
    /// platform relay — today's behavior, byte-identical on the wire.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relay: Option<RlRelayRequest>,
    /// Forward-compat catch-all: fields this SDK version doesn't know are
    /// preserved verbatim on the wire (the `body=` escape hatch depends on
    /// this — server-side schema additions must never be silently dropped).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Relay storage mode. Serialized lowercase (`byo` | `platform`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RlRelayMode {
    /// User-supplied S3-compatible storage.
    Byo,
    /// The platform-managed relay (the default when the block is omitted).
    Platform,
}

/// BYO relay block (mirrors the server's `RlRelayRequest`, #1574).
///
/// Credentials are WRITE-ONLY on the platform: they become a namespaced
/// secret at create and are never echoed by any response or log. The
/// hand-written `Debug` below redacts them on the CLIENT side too, so an
/// application that traces its requests cannot leak them either.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RlRelayRequest {
    /// Storage mode.
    pub mode: RlRelayMode,
    /// S3-compatible endpoint URL — https with a public-CA cert and a DNS
    /// hostname (IP literals are rejected).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// Bucket name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bucket: Option<String>,
    /// Region (S3 dialects; R2 uses `auto`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// Organizational prefix inside the bucket; the platform appends
    /// `<cluster-uid>/` — see `effectivePrefix` on the create response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_prefix: Option<String>,
    /// Name of an existing namespaced secret you manage (keys
    /// `access_key_id` / `secret_access_key`). Mutually exclusive with the
    /// inline pair.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credentials_secret: Option<String>,
    /// Inline access key id (write-only; becomes a platform-managed secret).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_key_id: Option<String>,
    /// Inline secret access key (write-only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_access_key: Option<String>,
}

// Redaction policy (deliberate asymmetry): only the two key-material
// fields are redacted. The remaining fields — endpoint, bucket, region,
// basePrefix, and the credentialsSecret NAME — are non-secret storage
// coordinates (the same values appear in the CR spec and cluster status),
// and printing them is what makes a traced request debuggable.
impl std::fmt::Debug for RlRelayRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RlRelayRequest")
            .field("mode", &self.mode)
            .field("endpoint", &self.endpoint)
            .field("bucket", &self.bucket)
            .field("region", &self.region)
            .field("base_prefix", &self.base_prefix)
            .field("credentials_secret", &self.credentials_secret)
            .field(
                "access_key_id",
                &self.access_key_id.as_ref().map(|_| "<redacted>"),
            )
            .field(
                "secret_access_key",
                &self.secret_access_key.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

/// Rotate a BYO cluster's relay credentials
/// (`POST /rl/clusters/{name}/credentials`, #1577). Applies only to
/// clusters created with the inline pair (platform-managed secret).
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RotateRelayCredentialsRequest {
    /// Replacement access key id (write-only).
    pub access_key_id: String,
    /// Replacement secret access key (write-only).
    pub secret_access_key: String,
}

impl std::fmt::Debug for RotateRelayCredentialsRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RotateRelayCredentialsRequest")
            .field("access_key_id", &"<redacted>")
            .field("secret_access_key", &"<redacted>")
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Job request DTOs
// ---------------------------------------------------------------------------

/// In-cluster LLM-judge opt-in (WS-G v0.5). The judge is a platform-owned
/// vLLM pod serving an allowlisted open model; the reward reaches it via
/// `ctx["judge"](prompt, ...) -> str`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RlJudgeRequest {
    /// Judge model id; omit for the platform default (must be on the judge
    /// allowlist).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Custom reward: a user Python scoring function run in the isolated,
/// credential-free, zero-egress executor pod.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RlRewardRequest {
    /// Reward ref, `user:<dns-1035-name>`.
    #[serde(rename = "ref")]
    pub reward_ref: String,
    /// Reward source: stdlib Python defining
    /// `reward(prompt, completion, **ctx) -> float` (<=64 KiB).
    pub source: String,
    /// Optional in-cluster LLM-judge.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub judge: Option<RlJudgeRequest>,
}

/// A public Hugging Face dataset source + column mapping.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RlHfDatasetSource {
    /// HF dataset id, `org/name` (public datasets only).
    pub repo: String,
    /// Optional HF config (subset) name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<String>,
    /// Split to load, e.g. `train`.
    pub split: String,
    /// Column holding the prompt text.
    pub prompt_column: String,
    /// Column handed to the reward as `ground_truth`.
    pub answer_column: String,
}

/// Custom dataset: platform code fetches and renders it (dataset-as-data;
/// no user code touches the data path).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RlDatasetRequest {
    /// Dataset ref, `user:<dns-1035-name>`.
    #[serde(rename = "ref")]
    pub dataset_ref: String,
    /// The public HF source + column mapping.
    pub hf: RlHfDatasetSource,
}

/// Create a GRPO training job on a Ready cluster (`POST /rl/jobs`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRlJobRequest {
    /// Name of the warm cluster to bind to.
    pub cluster_ref: String,
    /// Optional job name (generated when omitted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Algorithm, e.g. `grpo`.
    pub algorithm: String,
    /// Custom reward; omit for the builtin reward.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reward: Option<RlRewardRequest>,
    /// Custom dataset; omit for the builtin GSM8K dataset.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataset: Option<RlDatasetRequest>,
    /// Training steps (<=3000 on custom datasets).
    pub max_steps: u32,
    /// Optional learning-rate override (string, e.g. `3.0e-6`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lr: Option<String>,
    /// Forward-compat catch-all: fields this SDK version doesn't know are
    /// preserved verbatim on the wire (the `body=` escape hatch depends on
    /// this — server-side schema additions must never be silently dropped).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Declarative manifest: one document that renders a cluster and/or a job
/// (`POST /rl/manifest`). Freeform to mirror the server's document surface.
pub type RlManifestRequest = serde_json::Value;

// ---------------------------------------------------------------------------
// Response DTOs
// ---------------------------------------------------------------------------

/// Response after creating a cluster.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRlClusterResponse {
    /// The cluster's name (its `clusterRef`).
    pub name: String,
    /// Unique identifier.
    pub uid: String,
    /// Lifecycle phase at creation (`Provisioning`).
    pub phase: String,
    /// BYO only: the uid-scoped storage key prefix
    /// (`<basePrefix><cluster-uid>/`) everything the cluster stores lands
    /// under — tighten your IAM grant to it. Absent for platform-managed
    /// storage and on servers predating #1574.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effective_prefix: Option<String>,
}

/// Response after rotating a cluster's storage credentials (#1577).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RotateRlCredentialsResponse {
    /// The cluster whose credentials were rotated.
    pub name: String,
    /// When the rotation was applied (RFC 3339). The relay daemon restarts
    /// with the new key material within seconds of this instant — keep the
    /// OLD key valid until then, then revoke it.
    pub rotated_at: String,
}

/// Cluster status (`GET /rl/clusters/{name}`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RlClusterStatusResponse {
    /// `Provisioning` | `Warming` | `Ready` | `Degraded` | `Terminating`.
    pub phase: String,
    /// Whether every fleet pod verified the pinned base model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_loaded: Option<bool>,
    /// The bound job's name, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_job_name: Option<String>,
}

/// Response after creating a job.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRlJobResponse {
    /// The job's name (its identifier for status).
    pub name: String,
    /// Unique identifier.
    pub uid: String,
    /// Lifecycle phase at creation (`Pending`).
    pub phase: String,
    /// sha256 of the admitted reward source (absent for builtin-reward jobs).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reward_sha256: Option<String>,
}

/// Latest training metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RlJobMetrics {
    /// Latest training loss.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub loss: Option<f64>,
    /// Latest mean reward.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reward_mean: Option<f64>,
    /// Latest KL divergence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kl: Option<f64>,
}

/// Job status (`GET /rl/jobs/{name}`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RlJobStatusResponse {
    /// `Pending` | `Binding` | `Running` | `Succeeded` | `Failed` | `TimedOut`.
    pub phase: String,
    /// Best-effort latest training step.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<i64>,
    /// Latest training metrics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<RlJobMetrics>,
    /// Object-store location of the trained model (null until bound).
    #[serde(rename = "artifactURI", skip_serializing_if = "Option::is_none")]
    pub artifact_uri: Option<String>,
}

/// Manifest submission result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RlManifestResponse {
    /// The created cluster, when the manifest carried a `cluster` block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cluster: Option<CreateRlClusterResponse>,
    /// The created job, when the manifest carried a `job` block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job: Option<CreateRlJobResponse>,
}

/// Response after deleting a cluster (`DELETE /rl/clusters/{name}`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteRlClusterResponse {
    /// The deleted cluster's name.
    pub name: String,
}

/// Response after deleting a job (`DELETE /rl/jobs/{name}`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteRlJobResponse {
    /// The deleted job's name.
    pub name: String,
}

// ---------------------------------------------------------------------------
// BYOT policy registry DTOs (#1666; server routes #1662)
// ---------------------------------------------------------------------------

/// Pinned base-model identity for a policy lineage (interface doc step 1).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RlPolicyBaseModel {
    /// HF-style repo, e.g. `Qwen/Qwen2.5-7B-Instruct`.
    pub repo: String,
    /// Immutable HF commit SHA (40 or 64 hex; a branch/tag is refused).
    pub commit: String,
    /// `sha256:<hex>` of the tokenizer the trainer tokenizes with.
    pub tokenizer_digest: String,
}

/// The CUSTOMER's storage the lineage lives in. Credentials are WRITE-ONLY
/// server-side: pass EITHER the inline pair OR `credentials_secret`,
/// exactly one — the server refuses both and neither.
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RlPolicyStorage {
    /// Storage backend. v1: `r2` (any S3-compatible endpoint).
    pub backend: String,
    pub bucket: String,
    /// https + DNS hostname only (server-side §6.1 hygiene).
    pub endpoint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// Referenced namespaced Secret (one credential form).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credentials_secret: Option<String>,
    /// Inline access key id (the other credential form).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_key_id: Option<String>,
    /// Inline secret access key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_access_key: Option<String>,
}

// §7a: key material must not leak through a traced request's Debug —
// same hand-written redaction as `RlRelayRequest`.
impl std::fmt::Debug for RlPolicyStorage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RlPolicyStorage")
            .field("backend", &self.backend)
            .field("bucket", &self.bucket)
            .field("endpoint", &self.endpoint)
            .field("region", &self.region)
            .field("credentials_secret", &self.credentials_secret)
            .field(
                "access_key_id",
                &self.access_key_id.as_ref().map(|_| "<redacted>"),
            )
            .field(
                "secret_access_key",
                &self.secret_access_key.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

/// Register a model lineage (`POST /rl/policies`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRlPolicyRequest {
    /// DNS-1123 label; becomes the CR and platform Secret name.
    pub name: String,
    pub base_model: RlPolicyBaseModel,
    /// Weight-update wire format. v1 admits `pulse-bf16-v1` only.
    pub update_format: String,
    pub storage: RlPolicyStorage,
    /// Forward-compat catch-all (same contract as the cluster request).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Response to a policy create.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRlPolicyResponse {
    pub policy_uid: String,
    /// `policies/<policy-uid>/` — scope your fleet's read-only IAM grant to
    /// this, and upload every artifact under it.
    pub effective_prefix: String,
}

/// One policy's registry view (`GET /rl/policies/{name}`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RlPolicyResponse {
    pub name: String,
    pub policy_uid: String,
    pub effective_prefix: String,
    pub repo: String,
    pub commit: String,
    pub update_format: String,
    pub total_revisions: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_revision: Option<String>,
}

/// Response after deleting a policy (`DELETE /rl/policies/{name}`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteRlPolicyResponse {
    pub name: String,
}

/// The artifact coordinates of one revision manifest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RlRevisionArtifact {
    /// `s3://<bucket>/<key>` — must live under the policy's
    /// `effective_prefix`.
    pub uri: String,
    /// 64 lowercase hex chars of the artifact bytes.
    pub sha256: String,
}

/// Register a revision manifest (`POST /rl/policies/{name}/revisions`).
/// No credentials here: the artifact already lives in the customer's
/// bucket; this only registers + verifies it (interface doc step 3).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRlRevisionRequest {
    /// 1-128 chars of `[a-z0-9._-]` with letter-or-digit edges.
    pub revision: String,
    /// Absent = anchor (chain root); present = patch over that revision.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_revision: Option<String>,
    pub artifact: RlRevisionArtifact,
    /// `xxh3_128:<32 lowercase hex>` — the whole-state digest every serving
    /// replica must match post-apply.
    pub expected_state_digest: String,
}

/// One revision's registry state
/// (`GET /rl/policies/{name}/revisions/{revision}` and the create response).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RlRevisionResponse {
    pub revision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_revision: Option<String>,
    /// `Validated` | `Loading` | `Active` | `Rejected` | `Superseded`.
    pub state: String,
    pub submitted_at: String,
}

// ---------------------------------------------------------------------------
// BYOT rollout-session DTOs (#1666; server routes #1663)
// ---------------------------------------------------------------------------

/// Start a rollout session (`POST /rl/rollout-sessions`): a PRIVATE,
/// token-gated vLLM fleet serving one policy. NOT idempotent — a retry
/// after a lost response creates a second fleet (bounded by the
/// per-tenant cap); list deployments before retrying.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRlSessionRequest {
    /// The policy this fleet serves (must exist).
    pub policy: String,
    /// Fleet shape — same type and bounds as the managed RL fleets.
    pub fleet: RlFleetRequest,
    /// `async` (default) | `sync`. v1 refuses `sync` at admission
    /// (SessionSyncUnavailable) until the activation barrier ships.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activation: Option<String>,
    /// Forward-compat catch-all (the cluster-request contract).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// The create response. `token` is shown ONCE — the platform stores only
/// its hash; losing it means recreating the session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRlSessionResponse {
    pub session_uid: String,
    /// Where `/v1/completions` terminates (the T4 serving surface).
    pub url: String,
    pub token: String,
    pub state: String,
}

/// One session's lifecycle view (`GET /rl/rollout-sessions/{id}`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RlSessionStatusResponse {
    pub session_uid: String,
    /// `starting` | `active` | `failed` | `terminating`.
    pub state: String,
    pub policy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_head: Option<String>,
    pub url: String,
}

/// Response after deleting a session (`DELETE /rl/rollout-sessions/{id}`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteRlSessionResponse {
    pub session_uid: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    // The SDK request DTOs must serialize to the exact wire shape the server's
    // deny_unknown_fields DTOs accept. These pin the field renames and casing
    // that the server contract depends on.
    //
    // Coverage split (deliberate): these are SERIALIZATION tests only. The
    // HTTP-level contract — method, path, auth header, body-on-the-wire,
    // and error mapping for every rl_* call including credential rotation —
    // is pinned by the Python suite's recorded-HTTP harness
    // (crates/basilica-sdk-python/tests/test_rl_client.py), which exercises
    // the COMPILED core transport end to end. A Rust-side HTTP harness
    // would duplicate that coverage one layer lower.
    #[test]
    fn job_request_wire_shape() {
        let req = CreateRlJobRequest {
            cluster_ref: "my-pool".into(),
            name: None,
            algorithm: "grpo".into(),
            reward: Some(RlRewardRequest {
                reward_ref: "user:my-reward".into(),
                source: "def reward(p, c, **k):\n    return 1.0\n".into(),
                judge: Some(RlJudgeRequest { model: None }),
            }),
            dataset: Some(RlDatasetRequest {
                dataset_ref: "user:my-data".into(),
                hf: RlHfDatasetSource {
                    repo: "openai/gsm8k".into(),
                    config: Some("main".into()),
                    split: "train".into(),
                    prompt_column: "question".into(),
                    answer_column: "answer".into(),
                },
            }),
            max_steps: 50,
            lr: None,
            extra: Default::default(),
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["clusterRef"], "my-pool");
        assert_eq!(v["maxSteps"], 50);
        // the serde renames: nested identity fields are `ref`
        assert_eq!(v["reward"]["ref"], "user:my-reward");
        assert_eq!(v["dataset"]["ref"], "user:my-data");
        assert_eq!(v["dataset"]["hf"]["promptColumn"], "question");
        // judge with no model serializes as an empty object (not null)
        assert!(v["reward"]["judge"].is_object());
        assert_eq!(v["reward"]["judge"].as_object().unwrap().len(), 0);
        // None optionals are ABSENT (deny_unknown_fields tolerates absence,
        // but a null in a non-Option server slot would 400)
        assert!(v.get("name").is_none());
        assert!(v.get("lr").is_none());
    }

    #[test]
    fn delete_response_wire_shape() {
        // The server serializes camelCase; both delete responses carry only
        // `name`. Pin the deserialization so a server-side field rename is
        // caught here, not by a user.
        let c: DeleteRlClusterResponse = serde_json::from_str(r#"{"name":"my-pool"}"#).unwrap();
        assert_eq!(c.name, "my-pool");
        let j: DeleteRlJobResponse = serde_json::from_str(r#"{"name":"my-pool-job"}"#).unwrap();
        assert_eq!(j.name, "my-pool-job");
    }

    #[test]
    fn cluster_request_wire_shape() {
        let req = CreateRlClusterRequest {
            name: Some("my-pool".into()),
            base_model: "Qwen/Qwen2.5-7B-Instruct".into(),
            trainer: RlFleetRequest {
                replicas: 1,
                gpu: RlGpuRequest {
                    model: "H100".into(),
                    count: 4,
                    min_memory_gb: None,
                },
            },
            rollout: RlFleetRequest {
                replicas: 1,
                gpu: RlGpuRequest {
                    model: "H100".into(),
                    count: 4,
                    min_memory_gb: None,
                },
            },
            idle_ttl: Some("30m".into()),
            relay: None,
            extra: Default::default(),
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["baseModel"], "Qwen/Qwen2.5-7B-Instruct");
        assert_eq!(v["trainer"]["gpu"]["count"], 4);
        assert_eq!(v["idleTtl"], "30m");
        assert!(v["trainer"]["gpu"].get("minMemoryGb").is_none());
        // No relay block => byte-identical to the pre-#1578 wire shape.
        assert!(v.get("relay").is_none());
    }

    #[test]
    fn relay_request_wire_shape() {
        let relay = RlRelayRequest {
            mode: RlRelayMode::Byo,
            endpoint: Some("https://acc.r2.cloudflarestorage.com".into()),
            bucket: Some("my-weights".into()),
            region: None,
            base_prefix: Some("teams/rl/".into()),
            credentials_secret: None,
            access_key_id: Some("AK".into()),
            secret_access_key: Some("SK".into()),
        };
        let v = serde_json::to_value(&relay).unwrap();
        // The server's deny_unknown_fields DTO expects exactly these names.
        assert_eq!(v["mode"], "byo");
        assert_eq!(v["basePrefix"], "teams/rl/");
        assert_eq!(v["accessKeyId"], "AK");
        assert_eq!(v["secretAccessKey"], "SK");
        assert!(v.get("region").is_none());
        assert!(v.get("credentialsSecret").is_none());
    }

    #[test]
    fn relay_debug_redacts_credentials() {
        let relay = RlRelayRequest {
            mode: RlRelayMode::Byo,
            endpoint: None,
            bucket: None,
            region: None,
            base_prefix: None,
            credentials_secret: None,
            access_key_id: Some("SUPERSECRETAK".into()),
            secret_access_key: Some("SUPERSECRETSK".into()),
        };
        let dbg = format!("{relay:?}");
        assert!(!dbg.contains("SUPERSECRET"), "Debug must redact: {dbg}");
        assert!(dbg.contains("<redacted>"));
        let rot = RotateRelayCredentialsRequest {
            access_key_id: "SUPERSECRETAK".into(),
            secret_access_key: "SUPERSECRETSK".into(),
        };
        let dbg = format!("{rot:?}");
        assert!(!dbg.contains("SUPERSECRET"), "Debug must redact: {dbg}");
    }

    #[test]
    fn cluster_response_effective_prefix_is_optional() {
        // Old servers (pre-#1574) omit it; BYO creates carry it.
        let old: CreateRlClusterResponse =
            serde_json::from_str(r#"{"name":"p","uid":"u","phase":"Provisioning"}"#).unwrap();
        assert_eq!(old.effective_prefix, None);
        let byo: CreateRlClusterResponse = serde_json::from_str(
            r#"{"name":"p","uid":"u","phase":"Provisioning","effectivePrefix":"teams/rl/u/"}"#,
        )
        .unwrap();
        assert_eq!(byo.effective_prefix.as_deref(), Some("teams/rl/u/"));
    }

    #[test]
    fn rotation_wire_shapes() {
        let req = RotateRelayCredentialsRequest {
            access_key_id: "AK".into(),
            secret_access_key: "SK".into(),
        };
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["accessKeyId"], "AK");
        assert_eq!(v["secretAccessKey"], "SK");
        let resp: RotateRlCredentialsResponse =
            serde_json::from_str(r#"{"name":"p","rotatedAt":"2026-08-31T12:00:00+00:00"}"#)
                .unwrap();
        assert_eq!(resp.name, "p");
        assert!(resp.rotated_at.starts_with("2026-08-31"));
    }

    #[test]
    fn job_status_parses_artifact_uri_rename() {
        let body = r#"{"phase":"Succeeded","step":50,"artifactURI":"s3://x/uid"}"#;
        let s: RlJobStatusResponse = serde_json::from_str(body).unwrap();
        assert_eq!(s.phase, "Succeeded");
        assert_eq!(s.artifact_uri.as_deref(), Some("s3://x/uid"));
        assert_eq!(s.step, Some(50));
    }

    #[test]
    fn policy_request_wire_shape_and_debug_redaction() {
        let req = CreateRlPolicyRequest {
            name: "math-policy".into(),
            base_model: RlPolicyBaseModel {
                repo: "Qwen/Qwen2.5-7B-Instruct".into(),
                commit: "a".repeat(40),
                tokenizer_digest: format!("sha256:{}", "b".repeat(64)),
            },
            update_format: "pulse-bf16-v1".into(),
            storage: RlPolicyStorage {
                backend: "r2".into(),
                bucket: "my-weights".into(),
                endpoint: "https://acc.r2.cloudflarestorage.com".into(),
                region: None,
                credentials_secret: None,
                access_key_id: Some("AKIALIVEKEY".into()),
                secret_access_key: Some("SECRETSECRET".into()),
            },
            extra: Default::default(),
        };
        let v = serde_json::to_value(&req).unwrap();
        // The server DTO is deny_unknown_fields camelCase — the exact keys
        // are the contract.
        assert_eq!(
            v["baseModel"]["tokenizerDigest"],
            req.base_model.tokenizer_digest
        );
        assert_eq!(v["updateFormat"], "pulse-bf16-v1");
        assert_eq!(v["storage"]["accessKeyId"], "AKIALIVEKEY");
        assert!(
            v["storage"].get("region").is_none(),
            "None must be OMITTED, not null"
        );
        // §7a: a traced request must not print key material.
        let dbg = format!("{req:?}");
        assert!(!dbg.contains("AKIALIVEKEY"), "{dbg}");
        assert!(!dbg.contains("SECRETSECRET"), "{dbg}");
        assert!(dbg.contains("<redacted>"), "{dbg}");
    }

    #[test]
    fn revision_request_omits_absent_parent() {
        // An anchor's manifest must not carry `"parentRevision": null` — the
        // server treats presence itself as "this is a patch".
        let anchor = CreateRlRevisionRequest {
            revision: "step-0000".into(),
            parent_revision: None,
            artifact: RlRevisionArtifact {
                uri: "s3://my-weights/policies/u/step-0000/anchor.safetensors".into(),
                sha256: "c".repeat(64),
            },
            expected_state_digest: format!("xxh3_128:{}", "d".repeat(32)),
        };
        let v = serde_json::to_value(&anchor).unwrap();
        assert!(v.get("parentRevision").is_none());
        assert_eq!(v["artifact"]["sha256"], anchor.artifact.sha256);
        assert_eq!(v["expectedStateDigest"], anchor.expected_state_digest);

        let rec: RlRevisionResponse = serde_json::from_str(
            r#"{"revision":"step-0001","parentRevision":"step-0000",
                "state":"Validated","submittedAt":"2026-09-07T16:00:00Z"}"#,
        )
        .unwrap();
        assert_eq!(rec.parent_revision.as_deref(), Some("step-0000"));
        assert_eq!(rec.state, "Validated");
    }

    #[test]
    fn policy_response_parses() {
        let p: RlPolicyResponse = serde_json::from_str(
            r#"{"name":"math-policy","policyUid":"u-1","effectivePrefix":"policies/u-1/",
                "repo":"Qwen/Qwen2.5-7B-Instruct","commit":"deadbeef","updateFormat":"pulse-bf16-v1",
                "totalRevisions":3,"latestRevision":"step-0002"}"#,
        )
        .unwrap();
        assert_eq!(p.effective_prefix, "policies/u-1/");
        assert_eq!(p.total_revisions, 3);
        assert_eq!(p.latest_revision.as_deref(), Some("step-0002"));
    }
}
