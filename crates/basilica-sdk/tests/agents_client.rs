use basilica_sdk::{agents::*, ApiError, BasilicaClient, ClientBuilder};
use serde_json::{json, Value};
use wiremock::matchers::{body_json, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "launch-intent-00000001";

fn client(server: &MockServer) -> BasilicaClient {
    ClientBuilder::default()
        .base_url(server.uri())
        .with_api_key("synthetic-account-token")
        .build()
        .unwrap()
}

fn create_request() -> CreateAgentRequest {
    CreateAgentRequest {
        template_id: "exo".into(),
        name: "research".into(),
        connection_id: "connection-1".into(),
        quote_id: "quote-1".into(),
        lifetime: AgentLifetime::UntilDeleted,
    }
}

fn mutation() -> Value {
    json!({"instance_id": "agent-1", "operation_id": "operation-1", "status_url": "/agent-operations/operation-1"})
}

fn connection() -> Value {
    json!({
        "id": "connection-1", "name": "personal", "provider": "openai",
        "model": "tested-model", "model_label": "Tested model", "credential_version": 2,
        "validated_at": "2026-09-17T12:00:00Z", "created_at": "2026-09-17T12:00:00Z"
    })
}

fn persistence() -> Value {
    json!({"description": "Persistent workspace", "preserved_paths": ["/workspace"], "limitations": ["No process memory"], "deletion_policy": "Deletion removes workspace"})
}

fn cost() -> Value {
    json!({"currency": "USD", "compute_per_hour": "0.10", "storage_per_hour": "0.01", "model_billing": "connected_provider"})
}

fn instance() -> Value {
    json!({
        "id": "agent-1", "name": "research", "template_id": "exo", "template_version": "1",
        "phase": "ready", "desired_state": "running", "current_operation_id": null,
        "connection_id": "connection-1", "model_label": "Tested model", "cost": cost(),
        "lifetime": "until_deleted", "expires_at": null, "persistence": persistence(),
        "capabilities": {"chat": true}, "health": {"runtime": "healthy", "model": "healthy", "chat": "healthy"},
        "checkpoints": [], "error": null, "created_at": "2026-09-17T12:00:00Z", "updated_at": "2026-09-17T12:00:00Z"
    })
}

async fn expect_mutation(
    server: &MockServer,
    verb: &str,
    route: &str,
    body: Value,
    response: Value,
) {
    Mock::given(method(verb))
        .and(path(route))
        .and(header("Authorization", "Bearer synthetic-account-token"))
        .and(header("Idempotency-Key", KEY))
        .and(body_json(body))
        .respond_with(ResponseTemplate::new(202).set_body_json(response))
        .expect(1)
        .mount(server)
        .await;
}

#[tokio::test]
async fn launch_retries_preserve_exact_intent_and_return_stable_operation() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/agent-instances"))
        .and(header("Authorization", "Bearer synthetic-account-token"))
        .and(header("Idempotency-Key", KEY))
        .and(body_json(serde_json::to_value(create_request()).unwrap()))
        .respond_with(ResponseTemplate::new(202).set_body_json(mutation()))
        .expect(2)
        .mount(&server)
        .await;
    let api = client(&server);
    let first = api.create_agent(&create_request(), KEY).await.unwrap();
    let retry = api.create_agent(&create_request(), KEY).await.unwrap();
    assert_eq!(first.operation_id, retry.operation_id);
    assert_eq!(first.instance_id, retry.instance_id);
}

#[tokio::test]
async fn lifecycle_routes_use_managed_resource_kind_and_explicit_checkpoint() {
    let server = MockServer::start().await;
    for action in ["restart", "export", "recover"] {
        expect_mutation(
            &server,
            "POST",
            &format!("/agent-instances/agent-1/{action}"),
            if action == "recover" {
                json!({"checkpoint_id": "baseline-1"})
            } else {
                json!({})
            },
            mutation(),
        )
        .await;
    }
    expect_mutation(
        &server,
        "DELETE",
        "/agent-instances/agent-1",
        json!({}),
        mutation(),
    )
    .await;
    let api = client(&server);
    api.restart_agent("agent-1", KEY).await.unwrap();
    api.export_agent("agent-1", KEY).await.unwrap();
    api.recover_agent(
        "agent-1",
        &RecoverAgentRequest {
            checkpoint_id: "baseline-1".into(),
        },
        KEY,
    )
    .await
    .unwrap();
    api.delete_agent("agent-1", KEY).await.unwrap();
}

#[tokio::test]
async fn connection_keys_are_sent_only_in_authenticated_mutation_bodies() {
    let server = MockServer::start().await;
    expect_mutation(&server, "POST", "/model-connections", json!({
        "name": "personal", "provider": "openai", "model": "tested-model", "api_key": "synthetic-provider-key"
    }), connection()).await;
    expect_mutation(
        &server,
        "PATCH",
        "/model-connections/connection-1",
        json!({"api_key": "synthetic-replacement-key"}),
        connection(),
    )
    .await;
    expect_mutation(
        &server,
        "DELETE",
        "/model-connections/connection-1",
        json!({}),
        json!({"id": "connection-1", "deleted": true}),
    )
    .await;
    let api = client(&server);
    let safe = api
        .create_model_connection(
            &CreateModelConnectionRequest {
                name: "personal".into(),
                provider: AgentModelProvider::Openai,
                model: "tested-model".into(),
                api_key: AgentSecret::new("synthetic-provider-key"),
            },
            KEY,
        )
        .await
        .unwrap();
    assert!(!serde_json::to_string(&safe).unwrap().contains("api_key"));
    assert_eq!(
        api.rotate_model_connection(
            "connection-1",
            &RotateModelConnectionRequest {
                api_key: AgentSecret::new("synthetic-replacement-key")
            },
            KEY
        )
        .await
        .unwrap()
        .credential_version,
        2
    );
    assert!(
        api.delete_model_connection("connection-1", KEY)
            .await
            .unwrap()
            .deleted
    );
}

#[tokio::test]
async fn mutation_redirects_never_forward_credentials_or_create_another_resource() {
    let server = MockServer::start().await;
    let destination = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/model-connections"))
        .respond_with(ResponseTemplate::new(307).insert_header("Location", destination.uri()))
        .expect(1)
        .mount(&server)
        .await;
    let result = client(&server)
        .create_model_connection(
            &CreateModelConnectionRequest {
                name: "personal".into(),
                provider: AgentModelProvider::Openai,
                model: "tested-model".into(),
                api_key: AgentSecret::new("synthetic-provider-key"),
            },
            KEY,
        )
        .await;
    assert!(result.is_err());
    assert!(destination.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn invalid_keys_ids_and_limits_fail_before_network_access() {
    let server = MockServer::start().await;
    let api = client(&server);
    for key in [
        "",
        "short",
        "newline\n000000000",
        "spaces 00000000000",
        &"a".repeat(129),
    ] {
        assert!(matches!(
            api.create_agent(&create_request(), key).await,
            Err(ApiError::InvalidRequest { .. })
        ));
    }
    for id in [
        "",
        ".",
        "..",
        "../rentals",
        "a/b",
        "a?x=y",
        "a#f",
        "a%2Fb",
        "\\evil",
        &"a".repeat(129),
    ] {
        assert!(matches!(
            api.get_agent(id).await,
            Err(ApiError::InvalidRequest { .. })
        ));
        assert!(matches!(
            api.delete_agent(id, KEY).await,
            Err(ApiError::InvalidRequest { .. })
        ));
    }
    for limit in [0, 101] {
        assert!(matches!(
            api.list_agents(&AgentPageQuery {
                limit: Some(limit),
                cursor: None
            })
            .await,
            Err(ApiError::InvalidRequest { .. })
        ));
    }
    assert!(matches!(
        api.get_agent_logs(
            "agent-1",
            &AgentPageQuery {
                limit: Some(1001),
                cursor: None
            }
        )
        .await,
        Err(ApiError::InvalidRequest { .. })
    ));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn pagination_encodes_cursor_and_preserves_next_page() {
    let server = MockServer::start().await;
    for route in [
        "/agent-instances",
        "/model-connections",
        "/agent-instances/agent-1/logs",
    ] {
        Mock::given(method("GET"))
            .and(path(route))
            .and(header("Authorization", "Bearer synthetic-account-token"))
            .and(query_param("cursor", "opaque+/=&next"))
            .and(query_param("limit", "25"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"items": [], "next_cursor": "page-2"})),
            )
            .expect(1)
            .mount(&server)
            .await;
    }
    let api = client(&server);
    let query = AgentPageQuery {
        cursor: Some("opaque+/=&next".into()),
        limit: Some(25),
    };
    assert_eq!(
        api.list_agents(&query)
            .await
            .unwrap()
            .next_cursor
            .as_deref(),
        Some("page-2")
    );
    assert_eq!(
        api.list_model_connections(&query)
            .await
            .unwrap()
            .next_cursor
            .as_deref(),
        Some("page-2")
    );
    assert_eq!(
        api.get_agent_logs("agent-1", &query)
            .await
            .unwrap()
            .next_cursor
            .as_deref(),
        Some("page-2")
    );
}

#[tokio::test]
async fn status_operation_and_chat_credentials_are_separate() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/agent-instances/agent-1"))
        .and(header("Authorization", "Bearer synthetic-account-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(instance()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET")).and(path("/agent-operations/operation-1"))
        .and(header("Authorization", "Bearer synthetic-account-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": "operation-1", "instance_id": "agent-1", "kind": "delete", "state": "running",
            "phase": "deleting", "cleanup_pending": true, "billing_finalized": false,
            "error": {"code": "cleanup_pending", "message": "Cleanup is retrying", "retryable": true},
            "export": null, "created_at": "2026-09-17T12:00:00Z", "updated_at": "2026-09-17T12:00:00Z"
        }))).expect(1).mount(&server).await;
    expect_mutation(&server, "POST", "/agent-instances/agent-1/chat-sessions", json!({}), json!({
        "session_id": "session-1", "instance_id": "agent-1", "websocket_url": "wss://api.example/agent-chat",
        "access_token": "synthetic-session-secret", "expires_at": "2026-09-17T12:05:00Z", "protocol_version": "1"
    })).await;
    let api = client(&server);
    let status = api.get_agent("agent-1").await.unwrap();
    assert_eq!(status.phase, AgentPhase::Ready);
    assert!(!serde_json::to_string(&status)
        .unwrap()
        .contains("access_token"));
    let operation = api.get_agent_operation("operation-1").await.unwrap();
    assert!(operation.cleanup_pending && !operation.billing_finalized);
    assert_eq!(operation.state, AgentOperationState::Running);
    let session = api.create_agent_chat_session("agent-1", KEY).await.unwrap();
    assert_eq!(session.access_token.expose(), "synthetic-session-secret");
    assert!(!format!("{session:?}").contains("synthetic-session-secret"));
}

#[tokio::test]
async fn templates_and_quotes_retain_cost_and_persistence_disclosure() {
    let server = MockServer::start().await;
    Mock::given(method("GET")).and(path("/agent-templates"))
        .and(header("Authorization", "Bearer synthetic-account-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([{
            "id": "exo", "version": "1", "name": "Exo", "recommended_size_id": "small",
            "sizes": [{"id": "small", "label": "Small", "cpu_cores": 4, "memory_mib": 8192, "storage_gib": 30, "regions": ["test-region"]}],
            "models": [{"provider": "openai", "model": "tested-model", "label": "Tested model"}],
            "capabilities": {"chat": true}, "persistence": persistence()
        }]))).expect(1).mount(&server).await;
    let request = AgentQuoteRequest {
        template_id: "exo".into(),
        connection_id: "connection-1".into(),
        size_id: "small".into(),
        region: "test-region".into(),
        lifetime: AgentLifetime::UntilDeleted,
    };
    expect_mutation(&server, "POST", "/agent-instances/quote", serde_json::to_value(&request).unwrap(), json!({
        "id": "quote-1", "template_id": "exo", "template_version": "1", "connection_id": "connection-1",
        "size_id": "small", "region": "test-region", "lifetime": "until_deleted", "cost": cost(),
        "persistence": persistence(), "expires_at": "2026-09-17T12:05:00Z"
    })).await;
    let api = client(&server);
    assert_eq!(
        api.list_agent_templates().await.unwrap()[0].sizes[0].cpu_cores,
        4
    );
    let quote = api.quote_agent(&request, KEY).await.unwrap();
    assert_eq!(quote.cost.compute_per_hour, "0.10");
    assert_eq!(quote.cost.storage_per_hour, "0.01");
    assert_eq!(
        quote.cost.model_billing,
        AgentModelBilling::ConnectedProvider
    );
    assert_eq!(quote.persistence.limitations, ["No process memory"]);
}

#[tokio::test]
async fn errors_preserve_auth_conflict_and_rate_limit_semantics_without_retry() {
    for status in [400, 401, 403, 404, 409, 429] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/agent-instances"))
            .respond_with(
                ResponseTemplate::new(status).set_body_json(json!({"error": {
                    "code": "BASILICA_API_CONFLICT", "message": "Safe server explanation",
                    "timestamp": "2026-09-17T12:00:00Z", "retryable": false
                }})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let error = client(&server)
            .create_agent(&create_request(), KEY)
            .await
            .unwrap_err();
        match status {
            400 => assert!(matches!(error, ApiError::BadRequest { .. })),
            401 => assert!(matches!(error, ApiError::Authentication { .. })),
            403 => assert!(matches!(error, ApiError::Authorization { .. })),
            404 => assert!(matches!(error, ApiError::NotFound { .. })),
            409 => assert!(matches!(error, ApiError::Conflict { .. })),
            429 => assert!(matches!(error, ApiError::RateLimitExceeded)),
            _ => unreachable!(),
        }
    }
}

#[tokio::test]
async fn provider_credentials_require_secure_transport_outside_loopback() {
    for base in [
        "http://192.0.2.1",
        "http://api.example",
        "https://user:password@api.example",
        "https://api.example?key=bad",
        "https://api.example#fragment",
    ] {
        let api = ClientBuilder::default()
            .base_url(base)
            .with_api_key("synthetic")
            .build()
            .unwrap();
        let result = api
            .create_model_connection(
                &CreateModelConnectionRequest {
                    name: "personal".into(),
                    provider: AgentModelProvider::Openai,
                    model: "tested".into(),
                    api_key: AgentSecret::new("synthetic-provider-secret"),
                },
                KEY,
            )
            .await;
        assert!(
            matches!(result, Err(ApiError::InvalidRequest { .. })),
            "{base}"
        );
    }
}
