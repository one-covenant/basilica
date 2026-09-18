//! Managed agents use their own resource namespace and durable operation API.

use crate::error::CliError;
use crate::interactive::gate;
use crate::output::json_output;
use basilica_sdk::{agents::*, BasilicaClient};
use clap::{Args, Subcommand, ValueEnum};
use std::collections::HashSet;
use std::time::Duration;

#[derive(Debug, Clone, Args)]
pub struct PageOptions {
    #[arg(long)]
    pub cursor: Option<String>,
    #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u32).range(1..=100))]
    pub limit: u32,
}

impl From<PageOptions> for AgentPageQuery {
    fn from(value: PageOptions) -> Self {
        Self {
            cursor: value.cursor,
            limit: Some(value.limit),
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct MutationOptions {
    /// Reuse this key AND the same request after an uncertain response
    #[arg(long, value_parser = parse_key)]
    pub idempotency_key: String,
    /// Return once intent is accepted, without waiting for completion
    #[arg(long)]
    pub detach: bool,
    /// Maximum seconds to wait; timeout does not cancel server work
    #[arg(long, default_value_t = 600, value_parser = clap::value_parser!(u64).range(1..=86400))]
    pub timeout: u64,
}

#[derive(Debug, Clone, Args)]
pub struct QuoteOptions {
    /// Saved model connection ID
    #[arg(long)]
    pub connection: String,
    /// Compute size; defaults to the server's recommendation
    #[arg(long)]
    pub size: Option<String>,
    /// Region; required if the chosen size has multiple regions
    #[arg(long)]
    pub region: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct ExoOptions {
    /// Stable server-stored agent name
    #[arg(long)]
    pub name: String,
    #[command(flatten)]
    pub quote: QuoteOptions,
    /// Use a previously reviewed quote (also required when replaying a launch)
    #[arg(long, conflicts_with_all = ["size", "region"], requires = "yes")]
    pub quote_id: Option<String>,
    #[command(flatten)]
    pub mutation: MutationOptions,
    /// Accept displayed costs, persistence terms and continuing billing
    #[arg(short = 'y', long)]
    pub yes: bool,
    /// Initial supported lifetime; no automatic expiry
    #[arg(long, default_value = "until-deleted", value_parser = ["until-deleted"])]
    pub lifetime: String,
}

#[derive(Debug, Clone, Subcommand)]
pub enum AgentAction {
    /// List tested templates, sizes and persistence contracts
    Templates,
    /// Request a quote without launching a resource
    Quote {
        #[command(flatten)]
        options: QuoteOptions,
        #[arg(long, value_parser = parse_key)]
        idempotency_key: String,
    },
    /// List managed agents only; never search rentals or deployments
    #[command(name = "ls", visible_alias = "list")]
    List(PageOptions),
    /// Read safe metadata; TARGET is an ID, name, id:ID or name:NAME
    Status { target: String },
    /// Read a durable operation, including cleanup and billing state
    Operation { id: String },
    /// Read a bounded log page; pass next_cursor to continue
    Logs {
        target: String,
        #[arg(long)]
        cursor: Option<String>,
        #[arg(long, default_value_t = 100, value_parser = clap::value_parser!(u32).range(1..=1000))]
        limit: u32,
    },
    /// Request a restart through the durable lifecycle worker
    Restart {
        target: String,
        #[command(flatten)]
        mutation: MutationOptions,
    },
    /// Request a consistent encrypted export; operation returns artifact metadata
    Export {
        target: String,
        #[command(flatten)]
        mutation: MutationOptions,
    },
    /// Recover code from a compatible checkpoint while retaining canonical state
    Recover {
        target: String,
        #[arg(long)]
        checkpoint: String,
        #[command(flatten)]
        mutation: MutationOptions,
    },
    /// Delete the agent; completion requires confirmed cleanup and final billing
    Delete {
        target: String,
        #[command(flatten)]
        mutation: MutationOptions,
        #[arg(short = 'y', long)]
        yes: bool,
    },
    /// Manage safe metadata and server-encrypted provider credentials
    Connections {
        #[command(subcommand)]
        action: ConnectionAction,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Provider {
    Openai,
    Openrouter,
}

impl From<Provider> for AgentModelProvider {
    fn from(value: Provider) -> Self {
        match value {
            Provider::Openai => Self::Openai,
            Provider::Openrouter => Self::Openrouter,
        }
    }
}

#[derive(Debug, Clone, Subcommand)]
pub enum ConnectionAction {
    #[command(name = "ls", visible_alias = "list")]
    List(PageOptions),
    Create {
        #[arg(long)]
        name: String,
        #[arg(long, value_enum)]
        provider: Provider,
        #[arg(long)]
        model: String,
        /// Name of the environment variable containing the key; otherwise prompt secretly
        #[arg(long, value_name = "VARIABLE")]
        key_env: Option<String>,
        #[arg(long, value_parser = parse_key)]
        idempotency_key: String,
    },
    Rotate {
        id: String,
        #[arg(long, value_name = "VARIABLE")]
        key_env: Option<String>,
        #[arg(long, value_parser = parse_key)]
        idempotency_key: String,
    },
    Delete {
        id: String,
        #[arg(long, value_parser = parse_key)]
        idempotency_key: String,
        #[arg(short = 'y', long)]
        yes: bool,
    },
}

/// The deploy alias has generic flags before its subcommand. Reject them for
/// Exo rather than silently ignoring GPU, image, storage or lifetime settings.
pub(super) fn validate_parent_options(
    cmd: &crate::cli::commands::DeployCommand,
) -> Result<(), CliError> {
    use clap::Parser;
    let defaults = crate::cli::commands::DeployCommand::try_parse_from(["deploy"])
        .map_err(|_| invalid("Could not validate Exo command options"))?;
    if cmd.source.is_some()
        || cmd.naming != defaults.naming
        || cmd.resources != defaults.resources
        || cmd.gpu != defaults.gpu
        || cmd.storage != defaults.storage
        || cmd.topology_spread != defaults.topology_spread
        || cmd.websocket != defaults.websocket
        || cmd.health != defaults.health
        || cmd.networking != defaults.networking
        || cmd.lifecycle != defaults.lifecycle
    {
        return Err(invalid("Generic deployment options do not apply to Exo. Place managed-agent options after 'exo'; see summon exo --help"));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> CliError {
    CliError::Internal(color_eyre::eyre::eyre!(message.into()))
}

fn parse_key(value: &str) -> Result<String, String> {
    if (16..=128).contains(&value.len())
        && value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        Ok(value.into())
    } else {
        Err("Use 16–128 ASCII letters, digits, hyphens or underscores".into())
    }
}

fn show<T: serde::Serialize>(value: &T, json: bool) -> Result<(), CliError> {
    // Both formats retain the server's cost, persistence, health and pagination fields.
    if json {
        json_output(value).map_err(CliError::Internal)
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(value)
                .map_err(|_| invalid("Could not format agent response"))?
        );
        Ok(())
    }
}

fn confirm(yes: bool, json: bool, prompt: &str) -> Result<(), CliError> {
    if yes {
        return Ok(());
    }
    if json {
        return Err(CliError::MissingInput {
            field: "confirmation".into(),
            hint: "Review terms and pass --yes".into(),
        });
    }
    if gate::ask_confirm("confirmation", prompt, false, "Review terms and pass --yes")? {
        Ok(())
    } else {
        Err(invalid("Cancelled; no mutation submitted"))
    }
}

fn secret(variable: Option<&str>, json: bool) -> Result<AgentSecret, CliError> {
    let value = if let Some(variable) = variable {
        std::env::var(variable).map_err(|_| {
            invalid("Provider key environment variable is missing or is not Unicode")
        })?
    } else {
        if json {
            return Err(CliError::MissingInput {
                field: "provider_key".into(),
                hint: "Pass --key-env VARIABLE (the variable name, not the key)".into(),
            });
        }
        gate::ask_secret(
            "provider_key",
            "Provider API key",
            "Pass --key-env VARIABLE",
        )?
    };
    if value.is_empty() || value.len() > 8192 || value.chars().any(char::is_control) {
        return Err(invalid(
            "Provider key must be nonempty, bounded and contain no control characters",
        ));
    }
    Ok(AgentSecret::new(value))
}

pub async fn handle(
    client: &BasilicaClient,
    action: AgentAction,
    json: bool,
) -> Result<(), CliError> {
    match action {
        AgentAction::Templates => show(&client.list_agent_templates().await?, json),
        AgentAction::Quote {
            options,
            idempotency_key,
        } => show(&quote(client, &options, &idempotency_key).await?, json),
        AgentAction::List(page) => show(&client.list_agents(&page.into()).await?, json),
        AgentAction::Status { target } => show(&resolve(client, &target).await?, json),
        AgentAction::Operation { id } => {
            let operation = client.get_agent_operation(&id).await?;
            show(&operation, json)?;
            operation_result(&operation).map(|_| ())
        }
        AgentAction::Logs {
            target,
            cursor,
            limit,
        } => {
            let agent = resolve(client, &target).await?;
            show(
                &client
                    .get_agent_logs(
                        &agent.id,
                        &AgentPageQuery {
                            cursor,
                            limit: Some(limit),
                        },
                    )
                    .await?,
                json,
            )
        }
        AgentAction::Connections { action } => connection(client, action, json).await,
        action => {
            let (target, mutation) = match &action {
                AgentAction::Restart { target, mutation }
                | AgentAction::Export { target, mutation }
                | AgentAction::Recover {
                    target, mutation, ..
                }
                | AgentAction::Delete {
                    target, mutation, ..
                } => (target, mutation),
                _ => unreachable!(),
            };
            let agent = resolve(client, target).await?;
            // Do not pre-check mutable capabilities: the server must recognize a replay
            // even after the operation changed phase/capabilities or deleted the instance.
            if let AgentAction::Delete { yes, .. } = &action {
                eprintln!("{}", agent.persistence.deletion_policy);
                confirm(
                    *yes,
                    json,
                    "Delete this agent and its declared persistent data?",
                )?;
            }
            eprintln!(
                "Managed agent: {}; idempotency key: {}",
                agent.id, mutation.idempotency_key
            );
            let accepted = match &action {
                AgentAction::Restart { .. } => {
                    client
                        .restart_agent(&agent.id, &mutation.idempotency_key)
                        .await?
                }
                AgentAction::Export { .. } => {
                    client
                        .export_agent(&agent.id, &mutation.idempotency_key)
                        .await?
                }
                AgentAction::Recover { checkpoint, .. } => {
                    client
                        .recover_agent(
                            &agent.id,
                            &RecoverAgentRequest {
                                checkpoint_id: checkpoint.clone(),
                            },
                            &mutation.idempotency_key,
                        )
                        .await?
                }
                AgentAction::Delete { .. } => {
                    client
                        .delete_agent(&agent.id, &mutation.idempotency_key)
                        .await?
                }
                _ => unreachable!(),
            };
            finish(client, accepted, mutation, json, false).await
        }
    }
}

async fn connection(
    client: &BasilicaClient,
    action: ConnectionAction,
    json: bool,
) -> Result<(), CliError> {
    match action {
        ConnectionAction::List(page) => {
            show(&client.list_model_connections(&page.into()).await?, json)
        }
        ConnectionAction::Create {
            name,
            provider,
            model,
            key_env,
            idempotency_key,
        } => {
            let request = CreateModelConnectionRequest {
                name,
                provider: provider.into(),
                model,
                api_key: secret(key_env.as_deref(), json)?,
            };
            show(
                &client
                    .create_model_connection(&request, &idempotency_key)
                    .await?,
                json,
            )
        }
        ConnectionAction::Rotate {
            id,
            key_env,
            idempotency_key,
        } => {
            let request = RotateModelConnectionRequest {
                api_key: secret(key_env.as_deref(), json)?,
            };
            show(
                &client
                    .rotate_model_connection(&id, &request, &idempotency_key)
                    .await?,
                json,
            )
        }
        ConnectionAction::Delete {
            id,
            idempotency_key,
            yes,
        } => {
            confirm(
                yes,
                json,
                "Delete this model connection? Active connections cannot be deleted.",
            )?;
            show(
                &client
                    .delete_model_connection(&id, &idempotency_key)
                    .await?,
                json,
            )
        }
    }
}

fn quote_request(
    template: &AgentTemplate,
    options: &QuoteOptions,
) -> Result<AgentQuoteRequest, CliError> {
    let size_id = options
        .size
        .as_deref()
        .unwrap_or(&template.recommended_size_id);
    let size = template
        .sizes
        .iter()
        .find(|size| size.id == size_id)
        .ok_or_else(|| invalid("Requested or recommended size is unavailable"))?;
    let region = match options.region.as_deref() {
        Some(region) if size.regions.iter().any(|r| r == region) => region.to_string(),
        Some(_) => return Err(invalid("Requested region is unavailable for this size")),
        None if size.regions.len() == 1 => size.regions[0].clone(),
        None => {
            return Err(CliError::MissingInput {
                field: "region".into(),
                hint: "Choose --region from basilica agents templates".into(),
            })
        }
    };
    Ok(AgentQuoteRequest {
        template_id: template.id.clone(),
        connection_id: options.connection.clone(),
        size_id: size.id.clone(),
        region,
        lifetime: AgentLifetime::UntilDeleted,
    })
}

async fn quote(
    client: &BasilicaClient,
    options: &QuoteOptions,
    key: &str,
) -> Result<AgentQuote, CliError> {
    let templates = client.list_agent_templates().await?;
    let template = templates
        .iter()
        .find(|t| t.id == "exo")
        .ok_or_else(|| invalid("Exo is not available in this server's tested catalog"))?;
    let request = quote_request(template, options)?;
    let quote = client.quote_agent(&request, key).await?;
    if quote.template_id != request.template_id
        || quote.template_version != template.version
        || quote.connection_id != request.connection_id
        || quote.size_id != request.size_id
        || quote.region != request.region
        || quote.lifetime != request.lifetime
    {
        return Err(invalid(
            "Server quote does not match the requested configuration",
        ));
    }
    Ok(quote)
}

pub async fn launch(
    client: &BasilicaClient,
    options: ExoOptions,
    json: bool,
    show_phases: bool,
) -> Result<(), CliError> {
    let quote_id = if let Some(id) = options.quote_id {
        confirm(
            options.yes,
            json,
            "Launch using this previously reviewed quote?",
        )?;
        id
    } else {
        let quote = quote(client, &options.quote, &uuid::Uuid::new_v4().to_string()).await?;
        eprintln!("Quote (compute and storage are hourly; model usage is billed separately by your connected provider):\n{}", serde_json::to_string_pretty(&quote).map_err(|_| invalid("Could not format quote"))?);
        if quote.expires_at <= chrono::Utc::now() {
            return Err(invalid("Quote expired; obtain and review a new quote"));
        }
        confirm(
            options.yes,
            json,
            "Accept this quote and persistence contract, billed until deleted?",
        )?;
        quote.id
    };
    let request = CreateAgentRequest {
        template_id: "exo".into(),
        name: options.name,
        connection_id: options.quote.connection,
        quote_id,
        lifetime: AgentLifetime::UntilDeleted,
    };
    // Emit the exact non-secret replay inputs BEFORE sending intent. An interrupted
    // client can reuse them, even after quote expiry. Never requote a submitted intent.
    eprintln!(
        "Launch replay inputs: {}",
        serde_json::json!({ "idempotency_key": options.mutation.idempotency_key, "request": request })
    );
    let accepted = client
        .create_agent(&request, &options.mutation.idempotency_key)
        .await?;
    finish(client, accepted, &options.mutation, json, show_phases).await
}

async fn resolve(client: &BasilicaClient, target: &str) -> Result<AgentInstance, CliError> {
    if let Some(id) = target.strip_prefix("id:") {
        return Ok(client.get_agent(id).await?);
    }
    if !target.starts_with("name:") && uuid::Uuid::parse_str(target).is_ok() {
        return Ok(client.get_agent(target).await?);
    }
    let name = target.strip_prefix("name:").unwrap_or(target);
    let mut cursor = None;
    let mut seen = HashSet::new();
    let mut found = None;
    loop {
        let page = client
            .list_agents(&AgentPageQuery {
                cursor,
                limit: Some(100),
            })
            .await?;
        for agent in page.items.into_iter().filter(|a| a.name == name) {
            if found.is_some() {
                return Err(invalid("Agent name is ambiguous; use id:ID"));
            }
            found = Some(agent);
        }
        match page.next_cursor {
            Some(next) if seen.insert(next.clone()) => cursor = Some(next),
            Some(_) => return Err(invalid("Server repeated an agent page cursor")),
            None => break,
        }
    }
    found.ok_or_else(|| {
        invalid("Managed agent not found; use agents ls, or id:ID for a deleted agent")
    })
}

fn operation_result(operation: &AgentOperation) -> Result<bool, CliError> {
    match operation.state {
        AgentOperationState::Running => Ok(false),
        AgentOperationState::Failed => Err(invalid(format!("Agent operation {} failed (cleanup pending: {}; billing finalized: {}). Inspect agents operation for safe error details", operation.id, operation.cleanup_pending, operation.billing_finalized))),
        AgentOperationState::Succeeded => {
            if operation.error.is_some()
                || operation.cleanup_pending
                || (operation.kind == AgentOperationKind::Export && operation.export.is_none())
                || (operation.kind == AgentOperationKind::Delete && (!operation.billing_finalized || operation.phase != AgentPhase::Deleted)) {
                return Err(invalid("Server reported success without the required artifact or cleanup/billing; inspect the operation"));
            }
            Ok(true)
        }
    }
}

async fn finish(
    client: &BasilicaClient,
    accepted: AgentMutationResponse,
    options: &MutationOptions,
    json: bool,
    show_phases: bool,
) -> Result<(), CliError> {
    if options.detach {
        return show(&accepted, json);
    }
    eprintln!(
        "Accepted operation {}; instance {}. Track with: basilica agents operation {}",
        accepted.operation_id, accepted.instance_id, accepted.operation_id
    );
    let deadline = tokio::time::Instant::now() + Duration::from_secs(options.timeout);
    let mut previous_progress = None;
    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(invalid(format!(
                "Wait timed out; server work continues. Track operation {}",
                accepted.operation_id
            )));
        }
        let operation =
            tokio::time::timeout_at(deadline, client.get_agent_operation(&accepted.operation_id))
                .await
                .map_err(|_| {
                    invalid(format!(
                        "Wait timed out; server work continues. Track operation {}",
                        accepted.operation_id
                    ))
                })??;
        if operation.id != accepted.operation_id || operation.instance_id != accepted.instance_id {
            return Err(invalid("Server returned an unrelated operation"));
        }
        let progress = (
            operation.phase,
            operation.state,
            operation.cleanup_pending,
            operation.billing_finalized,
        );
        if show_phases && previous_progress != Some(progress) {
            eprintln!(
                "Operation {}: {:?}, {:?}; cleanup pending: {}; billing finalized: {}",
                operation.id,
                operation.phase,
                operation.state,
                operation.cleanup_pending,
                operation.billing_finalized
            );
            previous_progress = Some(progress);
        }
        let result = operation_result(&operation);
        if !matches!(result, Ok(false)) {
            show(&operation, json)?;
            return result.map(|_| ());
        }
        tokio::time::sleep_until(std::cmp::min(
            deadline,
            tokio::time::Instant::now() + Duration::from_secs(2),
        ))
        .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::args::Args as CliArgs;
    use crate::cli::commands::{Commands, DeployAction};
    use clap::Parser;
    use serde_json::{json, Value};
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    const KEY: &str = "exo-cli-intent-00000001";
    const ID: &str = "b0000000-0000-4000-8000-000000000001";

    fn template() -> AgentTemplate {
        serde_json::from_value(json!({
            "id":"exo", "version":"1", "name":"Exo", "recommended_size_id":"measured",
            "sizes":[{"id":"measured","label":"Measured size","cpu_cores":4,"memory_mib":8192,"storage_gib":20,"regions":["test-region"]}],
            "models":[], "capabilities":{},
            "persistence":{"description":"Test persistence","preserved_paths":["/workspace"],"limitations":["No memory"],"deletion_policy":"Delete removes state"}
        })).unwrap()
    }
    fn instance() -> Value {
        json!({
            "id":ID,"name":"research","template_id":"exo","template_version":"1","phase":"ready","desired_state":"running",
            "current_operation_id":null,"connection_id":"connection-1","model_label":"Test model",
            "cost":{"currency":"USD","compute_per_hour":"0.10","storage_per_hour":"0.01","model_billing":"connected_provider"},
            "lifetime":"until_deleted","expires_at":null,"persistence":template().persistence,"capabilities":{},
            "health":{"runtime":"healthy","model":"healthy","chat":"healthy"},"checkpoints":[],"error":null,
            "created_at":"2026-09-17T12:00:00Z","updated_at":"2026-09-17T12:00:00Z"
        })
    }
    fn accepted() -> Value {
        json!({"instance_id":ID,"operation_id":"operation-1","status_url":"/agent-operations/operation-1"})
    }
    fn operation(state: &str, kind: &str, phase: &str) -> AgentOperation {
        serde_json::from_value(json!({
            "id":"operation-1","instance_id":ID,"kind":kind,"state":state,"phase":phase,
            "cleanup_pending":false,"billing_finalized":kind=="delete","error":null,"export":null,
            "created_at":"2026-09-17T12:00:00Z","updated_at":"2026-09-17T12:00:00Z"
        }))
        .unwrap()
    }
    fn mutation(detach: bool) -> MutationOptions {
        MutationOptions {
            idempotency_key: KEY.into(),
            detach,
            timeout: 1,
        }
    }

    // Actual SDK requests over local HTTP. Responses are protocol fixtures, not
    // runtime, provider, hosted authentication or product acceptance evidence.
    struct Expected {
        method: &'static str,
        path: String,
        key: Option<&'static str>,
        body: Option<Value>,
        response: Value,
    }
    struct Server {
        client: BasilicaClient,
        pending: Arc<Mutex<VecDeque<Expected>>>,
        task: tokio::task::JoinHandle<()>,
    }
    impl Server {
        async fn start(requests: Vec<Expected>) -> Self {
            use axum::{
                extract::{Request, State},
                response::IntoResponse,
                Json, Router,
            };
            let pending = Arc::new(Mutex::new(VecDeque::from(requests)));
            async fn serve(
                State(pending): State<Arc<Mutex<VecDeque<Expected>>>>,
                request: Request,
            ) -> impl IntoResponse {
                let expected = pending
                    .lock()
                    .unwrap()
                    .pop_front()
                    .expect("unexpected request");
                assert_eq!(request.method().as_str(), expected.method);
                assert_eq!(request.uri().to_string(), expected.path);
                assert_eq!(
                    request.headers()["authorization"],
                    "Bearer synthetic-cli-account"
                );
                assert_eq!(
                    request
                        .headers()
                        .get("idempotency-key")
                        .map(|v| v.to_str().unwrap()),
                    expected.key
                );
                let bytes = axum::body::to_bytes(request.into_body(), 16384)
                    .await
                    .unwrap();
                if let Some(body) = expected.body {
                    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), body);
                }
                Json(expected.response)
            }
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let app = Router::new().fallback(serve).with_state(pending.clone());
            let task = tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            let client = basilica_sdk::ClientBuilder::default()
                .base_url(format!("http://{address}"))
                .with_api_key("synthetic-cli-account")
                .build()
                .unwrap();
            Self {
                client,
                pending,
                task,
            }
        }
        fn verify(&self) {
            assert!(self.pending.lock().unwrap().is_empty());
        }
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.task.abort();
        }
    }
    fn get(path: impl Into<String>, response: Value) -> Expected {
        Expected {
            method: "GET",
            path: path.into(),
            key: None,
            body: None,
            response,
        }
    }

    #[test]
    fn parsing_keeps_exo_separate_from_generic_and_openclaw() {
        for verb in ["summon", "deploy", "d"] {
            let args = CliArgs::try_parse_from([
                "basilica",
                verb,
                "exo",
                "--name",
                "research",
                "--connection",
                "connection-1",
                "--idempotency-key",
                KEY,
                "--detach",
                "--yes",
                "--json",
            ])
            .unwrap();
            assert!(args.command.requires_auth());
            assert!(
                matches!(args.command, Commands::Deploy(cmd) if matches!(cmd.action, Some(DeployAction::Exo(_))))
            );
        }
        for source in ["openclaw", "tau", "vllm", "sglang"] {
            let args = CliArgs::try_parse_from(["basilica", "summon", source]).unwrap();
            assert!(matches!(args.command, Commands::Deploy(cmd) if cmd.action.is_some()));
        }
        let args =
            CliArgs::try_parse_from(["basilica", "summon", "registry.example/exo:1"]).unwrap();
        assert!(
            matches!(args.command, Commands::Deploy(cmd) if cmd.action.is_none() && cmd.source.is_some())
        );
        for flag in ["--gpu", "--replicas", "--no-storage", "--image"] {
            assert!(CliArgs::try_parse_from([
                "basilica",
                "summon",
                "exo",
                "--name",
                "research",
                "--connection",
                "connection-1",
                "--idempotency-key",
                KEY,
                flag,
                "1"
            ])
            .is_err());
        }
        for action in ["pause", "resume", "terminal"] {
            assert!(CliArgs::try_parse_from(["basilica", "agents", action, ID]).is_err());
        }
    }

    #[test]
    fn exo_rejects_generic_parent_flags_without_affecting_other_templates() {
        for (flag, value) in [
            ("--gpu", "1"),
            ("--replicas", "2"),
            ("--image", "other"),
            ("--ttl", "60"),
            ("--env", "NAME=value"),
        ] {
            let args = CliArgs::try_parse_from([
                "basilica",
                "summon",
                flag,
                value,
                "exo",
                "--name",
                "research",
                "--connection",
                "c",
                "--idempotency-key",
                KEY,
            ])
            .unwrap();
            let Commands::Deploy(cmd) = args.command else {
                panic!()
            };
            assert!(validate_parent_options(&cmd).is_err());
        }
        let args = CliArgs::try_parse_from([
            "basilica",
            "summon",
            "exo",
            "--name",
            "research",
            "--connection",
            "c",
            "--idempotency-key",
            KEY,
        ])
        .unwrap();
        let Commands::Deploy(mut cmd) = args.command else {
            panic!()
        };
        validate_parent_options(&cmd).unwrap();
        cmd.show_phases = true;
        validate_parent_options(&cmd).unwrap();
    }

    #[test]
    fn parsing_enforces_replay_and_bounded_options() {
        assert!(parse_key("short").is_err());
        assert!(parse_key(&"a".repeat(129)).is_err());
        assert!(parse_key("not-a-key/000000000").is_err());
        assert!(parse_key(KEY).is_ok());
        for limit in ["0", "101"] {
            assert!(
                CliArgs::try_parse_from(["basilica", "agents", "ls", "--limit", limit]).is_err()
            );
        }
        assert!(
            CliArgs::try_parse_from(["basilica", "agents", "logs", ID, "--limit", "1000"]).is_ok()
        );
        assert!(
            CliArgs::try_parse_from(["basilica", "agents", "logs", ID, "--limit", "1001"]).is_err()
        );
        assert!(CliArgs::try_parse_from([
            "basilica",
            "summon",
            "exo",
            "--name",
            "x",
            "--connection",
            "c",
            "--quote-id",
            "q",
            "--idempotency-key",
            KEY
        ])
        .is_err());
        assert!(CliArgs::try_parse_from([
            "basilica",
            "summon",
            "exo",
            "--name",
            "x",
            "--connection",
            "c",
            "--quote-id",
            "q",
            "--idempotency-key",
            KEY,
            "--yes",
            "--size",
            "other"
        ])
        .is_err());
    }

    #[test]
    fn recommended_size_and_region_are_server_driven() {
        let options = QuoteOptions {
            connection: "c".into(),
            size: None,
            region: None,
        };
        let mut template = template();
        let request = quote_request(&template, &options).unwrap();
        assert_eq!(request.size_id, "measured");
        assert_eq!(request.region, "test-region");
        template.sizes[0].regions.push("other".into());
        assert!(matches!(
            quote_request(&template, &options),
            Err(CliError::MissingInput { .. })
        ));
        template.recommended_size_id = "missing".into();
        assert!(quote_request(&template, &options).is_err());
    }

    #[test]
    fn operation_outcomes_do_not_confuse_intent_with_cleanup() {
        assert!(!operation_result(&operation("running", "create", "starting")).unwrap());
        assert!(operation_result(&operation("succeeded", "create", "ready")).unwrap());
        assert!(operation_result(&operation("failed", "create", "failed")).is_err());
        // Operation state is historical; phase is the instance's CURRENT state.
        // A later restart/deletion must not turn a successful replay into failure.
        assert!(operation_result(&operation("succeeded", "create", "restarting")).unwrap());
        assert!(operation_result(&operation("succeeded", "create", "deleted")).unwrap());
        assert!(operation_result(&operation("succeeded", "export", "ready")).is_err());
        let mut deletion = operation("succeeded", "delete", "deleted");
        assert!(operation_result(&deletion).unwrap());
        deletion.billing_finalized = false;
        assert!(operation_result(&deletion).is_err());
        deletion.billing_finalized = true;
        deletion.cleanup_pending = true;
        assert!(operation_result(&deletion).is_err());
        deletion.cleanup_pending = false;
        deletion.phase = AgentPhase::Deleting;
        assert!(operation_result(&deletion).is_err());
    }

    #[test]
    fn json_never_prompts_for_confirmation_or_secrets() {
        assert!(matches!(
            confirm(false, true, "test"),
            Err(CliError::MissingInput { .. })
        ));
        assert!(matches!(
            secret(None, true),
            Err(CliError::MissingInput { .. })
        ));
        assert!(secret(Some("BASILICA_EXO_TEST_MISSING_SECRET_897324"), true).is_err());
        let args = CliArgs::try_parse_from([
            "basilica",
            "agents",
            "connections",
            "create",
            "--name",
            "work",
            "--provider",
            "openai",
            "--model",
            "tested",
            "--key-env",
            "MY_PROVIDER_KEY",
            "--idempotency-key",
            KEY,
        ])
        .unwrap();
        let debug = format!("{args:?}");
        assert!(debug.contains("MY_PROVIDER_KEY"));
        assert!(!debug.contains("api_key:"));
        assert!(CliArgs::try_parse_from([
            "basilica",
            "agents",
            "connections",
            "create",
            "--api-key",
            "do-not-accept-values"
        ])
        .is_err());
    }

    #[tokio::test]
    async fn resolve_uses_only_agent_routes_with_paginated_names_and_explicit_ids() {
        let server = Server::start(vec![
            get(
                "/agent-instances?limit=100",
                json!({"items":[],"next_cursor":"page-two"}),
            ),
            get(
                "/agent-instances?cursor=page-two&limit=100",
                json!({"items":[instance()],"next_cursor":null}),
            ),
            get(format!("/agent-instances/{ID}"), instance()),
        ])
        .await;
        assert_eq!(
            resolve(&server.client, "name:research").await.unwrap().id,
            ID
        );
        assert_eq!(
            resolve(&server.client, &format!("id:{ID}"))
                .await
                .unwrap()
                .id,
            ID
        );
        server.verify();
    }

    #[tokio::test]
    async fn ambiguity_and_repeated_cursors_fail_without_mutation() {
        let server = Server::start(vec![get(
            "/agent-instances?limit=100",
            json!({"items":[instance(),instance()],"next_cursor":null}),
        )])
        .await;
        assert!(resolve(&server.client, "research")
            .await
            .unwrap_err()
            .to_string()
            .contains("ambiguous"));
        server.verify();
        let server = Server::start(vec![
            get(
                "/agent-instances?limit=100",
                json!({"items":[],"next_cursor":"same"}),
            ),
            get(
                "/agent-instances?cursor=same&limit=100",
                json!({"items":[],"next_cursor":"same"}),
            ),
        ])
        .await;
        assert!(resolve(&server.client, "research")
            .await
            .unwrap_err()
            .to_string()
            .contains("repeated"));
        server.verify();
    }

    #[tokio::test]
    async fn lifecycle_replays_preserve_target_body_and_key_despite_capability_changes() {
        let mut expectations = Vec::new();
        for (method, suffix, body) in [
            ("POST", "/restart", json!({})),
            ("POST", "/export", json!({})),
            ("POST", "/recover", json!({"checkpoint_id":"checkpoint-1"})),
            ("DELETE", "", json!({})),
        ] {
            expectations.push(get(format!("/agent-instances/{ID}"), instance()));
            expectations.push(Expected {
                method,
                path: format!("/agent-instances/{ID}{suffix}"),
                key: Some(KEY),
                body: Some(body),
                response: accepted(),
            });
        }
        let server = Server::start(expectations).await;
        for action in [
            AgentAction::Restart {
                target: ID.into(),
                mutation: mutation(true),
            },
            AgentAction::Export {
                target: ID.into(),
                mutation: mutation(true),
            },
            AgentAction::Recover {
                target: ID.into(),
                mutation: mutation(true),
                checkpoint: "checkpoint-1".into(),
            },
            AgentAction::Delete {
                target: ID.into(),
                mutation: mutation(true),
                yes: true,
            },
        ] {
            handle(&server.client, action, true).await.unwrap();
        }
        server.verify();
    }

    #[tokio::test]
    async fn launch_replay_submits_exact_quote_without_catalog_or_repricing() {
        let mut expectations = Vec::new();
        for _ in 0..2 {
            expectations.push(Expected {method:"POST",path:"/agent-instances".into(),key:Some(KEY),body:Some(json!({"template_id":"exo","name":"research","connection_id":"connection-1","quote_id":"original-quote","lifetime":"until_deleted"})),response:accepted()});
        }
        let server = Server::start(expectations).await;
        for _ in 0..2 {
            launch(
                &server.client,
                ExoOptions {
                    name: "research".into(),
                    quote: QuoteOptions {
                        connection: "connection-1".into(),
                        size: None,
                        region: None,
                    },
                    quote_id: Some("original-quote".into()),
                    mutation: mutation(true),
                    yes: true,
                    lifetime: "until-deleted".into(),
                },
                true,
                false,
            )
            .await
            .unwrap();
        }
        server.verify();
    }

    #[tokio::test]
    async fn wait_reports_failed_and_inconsistent_operation_as_errors() {
        for value in [
            operation("failed", "create", "failed"),
            {
                let mut op = operation("succeeded", "delete", "deleted");
                op.billing_finalized = false;
                op
            },
            {
                let mut op = operation("succeeded", "create", "ready");
                op.instance_id = "another-agent".into();
                op
            },
        ] {
            let server = Server::start(vec![get(
                "/agent-operations/operation-1",
                serde_json::to_value(value).unwrap(),
            )])
            .await;
            assert!(finish(
                &server.client,
                serde_json::from_value(accepted()).unwrap(),
                &mutation(false),
                true,
                false
            )
            .await
            .is_err());
            server.verify();
        }
    }

    #[tokio::test]
    async fn wait_returns_success_only_after_durable_completion() {
        let server = Server::start(vec![get(
            "/agent-operations/operation-1",
            serde_json::to_value(operation("succeeded", "delete", "deleted")).unwrap(),
        )])
        .await;
        finish(
            &server.client,
            serde_json::from_value(accepted()).unwrap(),
            &mutation(false),
            true,
            true,
        )
        .await
        .unwrap();
        server.verify();
    }
    #[tokio::test]
    async fn mismatched_quote_is_rejected_before_launch() {
        let request = QuoteOptions {
            connection: "connection-1".into(),
            size: None,
            region: None,
        };
        let quote = json!({"id":"quote-1","template_id":"exo","template_version":"1","connection_id":"another-connection","size_id":"measured","region":"test-region","lifetime":"until_deleted","cost":instance()["cost"],"persistence":template().persistence,"expires_at":"2099-01-01T00:00:00Z"});
        let server = Server::start(vec![
            get("/agent-templates", json!([template()])),
            Expected {method:"POST",path:"/agent-instances/quote".into(),key:Some(KEY),body:Some(json!({"template_id":"exo","connection_id":"connection-1","size_id":"measured","region":"test-region","lifetime":"until_deleted"})),response:quote}
        ]).await;
        assert!(super::quote(&server.client, &request, KEY)
            .await
            .unwrap_err()
            .to_string()
            .contains("does not match"));
        server.verify();
    }

    #[tokio::test]
    async fn wait_timeout_does_not_cancel_or_resubmit_the_operation() {
        let server = Server::start(vec![get(
            "/agent-operations/operation-1",
            serde_json::to_value(operation("running", "create", "provisioning")).unwrap(),
        )])
        .await;
        let error = finish(
            &server.client,
            serde_json::from_value(accepted()).unwrap(),
            &mutation(false),
            true,
            false,
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("server work continues"));
        assert!(error.to_string().contains("operation-1"));
        server.verify();
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn connection_input_sends_key_only_in_mutation_body_and_returns_safe_metadata() {
        const VARIABLE: &str = "BASILICA_EXO_TEST_PROVIDER_342578";
        const SECRET: &str = "synthetic-exo-test-provider-secret";
        struct Restore(Option<std::ffi::OsString>);
        impl Drop for Restore {
            fn drop(&mut self) {
                match &self.0 {
                    Some(value) => std::env::set_var(VARIABLE, value),
                    None => std::env::remove_var(VARIABLE),
                }
            }
        }
        let _restore = Restore(std::env::var_os(VARIABLE));
        std::env::set_var(VARIABLE, SECRET);
        let metadata = json!({"id":"connection-1","name":"work","provider":"openai","model":"tested","model_label":"Test model","credential_version":1,"validated_at":"2026-09-17T12:00:00Z","created_at":"2026-09-17T12:00:00Z"});
        let server = Server::start(vec![Expected {
            method: "POST",
            path: "/model-connections".into(),
            key: Some(KEY),
            body: Some(
                json!({"name":"work","provider":"openai","model":"tested","api_key":SECRET}),
            ),
            response: metadata.clone(),
        }])
        .await;
        let action = ConnectionAction::Create {
            name: "work".into(),
            provider: Provider::Openai,
            model: "tested".into(),
            key_env: Some(VARIABLE.into()),
            idempotency_key: KEY.into(),
        };
        assert!(!format!("{action:?}").contains(SECRET));
        assert!(!format!("{:?}", secret(Some(VARIABLE), true).unwrap()).contains(SECRET));
        connection(&server.client, action, true).await.unwrap();
        let mut extended_metadata = metadata;
        extended_metadata["api_key"] = json!(SECRET);
        extended_metadata["access_token"] = json!(SECRET);
        let response: ModelConnection = serde_json::from_value(extended_metadata).unwrap();
        assert!(!serde_json::to_string(&response).unwrap().contains(SECRET));
        server.verify();
        std::env::set_var(VARIABLE, "secret\nnot-allowed");
        assert!(!secret(Some(VARIABLE), true)
            .unwrap_err()
            .to_string()
            .contains("not-allowed"));
    }
}
