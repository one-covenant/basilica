//! Managed-agent API v1 shared by the gateway, CLI, and website contract.
//!
//! Provider credentials and chat credentials are deliberately absent from
//! instance metadata. Secret-bearing request/session types redact `Debug`, but
//! serialize their secrets for authenticated transport; never log their JSON.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

/// A transport secret. Explicit exposure is required for use outside serde.
#[derive(Clone, Serialize, Deserialize)]
#[serde(transparent)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentSecret(String);

impl AgentSecret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AgentSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AgentPhase {
    Queued,
    Provisioning,
    Configuring,
    Starting,
    Ready,
    Restarting,
    Failed,
    Deleting,
    Deleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AgentDesiredState {
    Running,
    Deleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AgentOperationKind {
    Create,
    Restart,
    Export,
    Recover,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AgentOperationState {
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AgentHealthState {
    Unknown,
    Healthy,
    Unavailable,
}

/// Unrecognized or omitted capabilities do not enable actions.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentCapabilities {
    pub chat: bool,
    pub restart: bool,
    pub export: bool,
    pub recover: bool,
    pub pause: bool,
    pub resume: bool,
    pub terminal: bool,
    pub files: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentHealth {
    pub runtime: AgentHealthState,
    pub model: AgentHealthState,
    pub chat: AgentHealthState,
}

/// Safe, actionable failure; never a raw upstream body or provider error.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentFailure {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

/// Exact decimal prices. Model usage is billed by the connected provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentCost {
    pub currency: String,
    pub compute_per_hour: String,
    pub storage_per_hour: String,
    pub model_billing: AgentModelBilling,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AgentModelBilling {
    ConnectedProvider,
}

/// The only supported initial lifetime; no implicit destructive expiry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AgentLifetime {
    UntilDeleted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentPersistence {
    pub description: String,
    pub preserved_paths: Vec<String>,
    pub limitations: Vec<String>,
    pub deletion_policy: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentCheckpoint {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub code_version: String,
    pub state_schema_version: String,
    pub compatible: bool,
    pub effect: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentInstance {
    pub id: String,
    pub name: String,
    pub template_id: String,
    pub template_version: String,
    pub phase: AgentPhase,
    pub desired_state: AgentDesiredState,
    pub current_operation_id: Option<String>,
    pub connection_id: String,
    pub model_label: String,
    pub cost: AgentCost,
    pub lifetime: AgentLifetime,
    pub expires_at: Option<DateTime<Utc>>,
    pub persistence: AgentPersistence,
    pub capabilities: AgentCapabilities,
    pub health: AgentHealth,
    pub checkpoints: Vec<AgentCheckpoint>,
    pub error: Option<AgentFailure>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentOperation {
    pub id: String,
    pub instance_id: String,
    pub kind: AgentOperationKind,
    pub state: AgentOperationState,
    pub phase: AgentPhase,
    pub cleanup_pending: bool,
    pub billing_finalized: bool,
    pub error: Option<AgentFailure>,
    pub export: Option<AgentExport>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Artifact metadata only. Download requires a separate authenticated request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentExport {
    pub id: String,
    pub manifest_version: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentMutationResponse {
    pub instance_id: String,
    pub operation_id: String,
    /// Same-origin relative path; clients may instead fetch by operation_id.
    pub status_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateAgentRequest {
    pub template_id: String,
    pub name: String,
    pub connection_id: String,
    pub quote_id: String,
    pub lifetime: AgentLifetime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentQuoteRequest {
    pub template_id: String,
    pub connection_id: String,
    pub size_id: String,
    pub region: String,
    pub lifetime: AgentLifetime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentQuote {
    pub id: String,
    pub template_id: String,
    pub template_version: String,
    pub connection_id: String,
    pub size_id: String,
    pub region: String,
    pub lifetime: AgentLifetime,
    pub cost: AgentCost,
    pub persistence: AgentPersistence,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RecoverAgentRequest {
    pub checkpoint_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentSize {
    pub id: String,
    pub label: String,
    pub cpu_cores: u32,
    pub memory_mib: u32,
    pub storage_gib: u32,
    pub regions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentTemplate {
    pub id: String,
    pub version: String,
    pub name: String,
    pub recommended_size_id: String,
    pub sizes: Vec<AgentSize>,
    pub models: Vec<AgentModel>,
    pub capabilities: AgentCapabilities,
    pub persistence: AgentPersistence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub enum AgentModelProvider {
    Openai,
    Openrouter,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentModel {
    pub provider: AgentModelProvider,
    pub model: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CreateModelConnectionRequest {
    pub name: String,
    pub provider: AgentModelProvider,
    pub model: String,
    pub api_key: AgentSecret,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RotateModelConnectionRequest {
    pub api_key: AgentSecret,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelConnection {
    pub id: String,
    pub name: String,
    pub provider: AgentModelProvider,
    pub model: String,
    pub model_label: String,
    pub credential_version: u64,
    pub validated_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DeleteModelConnectionResponse {
    pub id: String,
    pub deleted: bool,
}

/// Credentials for one browser session. Authenticate in the first WS frame.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentChatSession {
    pub session_id: String,
    pub instance_id: String,
    /// Approved wss URL without access credentials in the URL.
    pub websocket_url: String,
    pub access_token: AgentSecret,
    pub expires_at: DateTime<Utc>,
    pub protocol_version: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentPageQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentPage<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentLogEntry {
    pub timestamp: DateTime<Utc>,
    pub level: String,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn secrets_are_redacted_in_nested_debug_but_transmitted_explicitly() {
        let request = CreateModelConnectionRequest {
            name: "personal".into(),
            provider: AgentModelProvider::Openai,
            model: "tested-model".into(),
            api_key: AgentSecret::new("synthetic-provider-secret"),
        };
        let debug = format!("{request:?}");
        assert!(!debug.contains("synthetic-provider-secret"));
        assert!(debug.contains("[REDACTED]"));
        let wire = serde_json::to_value(&request).unwrap();
        assert_eq!(wire["api_key"], "synthetic-provider-secret");
        let restored: CreateModelConnectionRequest = serde_json::from_value(wire).unwrap();
        assert_eq!(restored.api_key.expose(), "synthetic-provider-secret");
    }

    #[test]
    fn missing_and_future_capabilities_do_not_enable_actions() {
        let caps: AgentCapabilities = serde_json::from_value(json!({"teleport": true})).unwrap();
        assert_eq!(caps, AgentCapabilities::default());
        let caps: AgentCapabilities = serde_json::from_value(json!({"chat": true})).unwrap();
        assert!(caps.chat);
        assert!(!caps.pause && !caps.resume && !caps.recover && !caps.terminal);
    }

    #[test]
    fn create_contract_rejects_owner_override_and_unsupported_lifetime() {
        let mut wire = json!({
            "template_id": "exo", "name": "my-agent", "connection_id": "conn-1",
            "quote_id": "quote-1", "lifetime": "until_deleted"
        });
        let request: CreateAgentRequest = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(request).unwrap(), wire);
        wire["owner_id"] = json!("someone-else");
        assert!(serde_json::from_value::<CreateAgentRequest>(wire.clone()).is_err());
        wire.as_object_mut().unwrap().remove("owner_id");
        wire["lifetime"] = json!("trial");
        assert!(serde_json::from_value::<CreateAgentRequest>(wire).is_err());
    }

    #[test]
    fn connection_contract_rejects_arbitrary_destinations() {
        let wire = json!({
            "name": "personal", "provider": "openai", "model": "tested-model",
            "api_key": "synthetic", "base_url": "https://untrusted.example"
        });
        assert!(serde_json::from_value::<CreateModelConnectionRequest>(wire).is_err());
    }

    #[test]
    fn unknown_phase_is_not_silently_reported_as_ready() {
        assert!(serde_json::from_str::<AgentPhase>("\"future_state\"").is_err());
        assert_eq!(
            serde_json::to_string(&AgentPhase::Deleting).unwrap(),
            "\"deleting\""
        );
        assert_eq!(
            serde_json::to_string(&AgentOperationKind::Recover).unwrap(),
            "\"recover\""
        );
    }

    #[test]
    fn prices_preserve_decimal_precision() {
        let wire = json!({
            "currency": "USD", "compute_per_hour": "0.000000123456789",
            "storage_per_hour": "0.001", "model_billing": "connected_provider"
        });
        let cost: AgentCost = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(cost).unwrap(), wire);
    }

    #[test]
    fn chat_debug_redacts_bearer_credential() {
        let session: AgentChatSession = serde_json::from_value(json!({
            "session_id": "session-1", "instance_id": "agent-1",
            "websocket_url": "wss://api.example/agent-chat",
            "access_token": "synthetic-session-secret",
            "expires_at": "2026-09-17T12:00:00Z", "protocol_version": "1"
        }))
        .unwrap();
        assert!(!format!("{session:?}").contains("synthetic-session-secret"));
    }
}
