# Basilica CLI to API Refactoring Design Document

## Executive Summary

This document outlines a refactoring strategy to achieve a DRY (Don't Repeat Yourself) codebase by moving core business logic from `basilica-cli` to `basilica-api`, making it reusable for the SDK and other consumers. **We are not maintaining backward compatibility** - this is a clean break refactor.

## Current Architecture Analysis

### Business Logic Currently in CLI

After analyzing the codebase, the following business logic is currently implemented in `basilica-cli` that should be shared:

1. **Authentication & Token Management** (`crates/basilica-cli/src/auth/`)
   - OAuth flow with PKCE implementation
   - Token storage and refresh logic
   - Device flow authentication
   - Auth0 integration specifics

2. **Rental Management Logic** (`crates/basilica-cli/src/cli/handlers/gpu_rental.rs`)
   - Executor selection and filtering
   - SSH key validation and loading
   - Rental status polling
   - Port mapping and environment variable parsing
   - Resource requirement building

3. **SSH Operations** (`crates/basilica-cli/src/ssh/`)
   - SSH connection management
   - Port forwarding logic
   - Interactive session handling
   - File transfer operations

4. **Caching Layer** (`crates/basilica-cli/src/cache/`)
   - Rental information caching
   - Cache invalidation strategies
   - Persistent storage management

5. **Client Creation & Configuration** (`crates/basilica-cli/src/client.rs`)
   - Authenticated client factory
   - Token refresh integration
   - Configuration management

## Proposed Architecture

```
┌────────────────────────────────────────────────────┐
│                    Applications                     │
├──────────────┬────────────────┬────────────────────┤
│ basilica-cli │ basilica-sdk   │ basilica-python    │
├──────────────┴────────────────┴────────────────────┤
│              basilica-api (Core Logic)              │
│  ┌────────────────────────────────────────────┐    │
│  │ • Authentication Manager                    │    │
│  │ • Rental Service                           │    │
│  │ • SSH Service                              │    │
│  │ • Cache Service                            │    │
│  │ • Executor Service                         │    │
│  └────────────────────────────────────────────────┘    │
├─────────────────────────────────────────────────────┤
│            basilica-validator (Internal)            │
└─────────────────────────────────────────────────────┘
```

## Aggressive Refactoring Plan (No Backward Compatibility)

### Phase 1: Complete Service Layer in basilica-api

#### 1.1 Authentication Service
```rust
// crates/basilica-api/src/services/auth.rs
pub struct AuthenticationService {
    config: AuthConfig,
    token_store: Box<dyn TokenStorage>,
}

impl AuthenticationService {
    /// OAuth flow with PKCE
    pub async fn oauth_login(&self) -> Result<TokenSet>;
    
    /// Device flow authentication
    pub async fn device_login(&self) -> Result<TokenSet>;
    
    /// Refresh expired tokens
    pub async fn refresh_token(&self, refresh_token: &str) -> Result<TokenSet>;
    
    /// Get valid access token (with auto-refresh)
    pub async fn get_valid_token(&self) -> Result<String>;
}

/// Token storage trait for different implementations
#[async_trait]
pub trait TokenStorage: Send + Sync {
    async fn store(&self, tokens: &TokenSet) -> Result<()>;
    async fn retrieve(&self) -> Result<Option<TokenSet>>;
    async fn delete(&self) -> Result<()>;
}
```

#### 1.2 Rental Service (Replaces direct API calls)
```rust
// crates/basilica-api/src/services/rental.rs
pub struct RentalService {
    client: BasilicaClient,
    cache: Box<dyn RentalCache>,
}

impl RentalService {
    /// Deploy a container (renamed from create_rental)
    pub async fn deploy(&self, config: DeploymentConfig) -> Result<Deployment>;
    
    /// Wait for deployment to become active
    pub async fn wait_for_active(&self, deployment_id: &str, timeout: Duration) -> Result<bool>;
    
    /// Get deployment with caching
    pub async fn get_deployment(&self, deployment_id: &str) -> Result<Deployment>;
    
    /// List deployments with filters
    pub async fn list_deployments(&self, filters: DeploymentFilters) -> Result<Vec<Deployment>>;
    
    /// Stop deployment and cleanup cache
    pub async fn stop(&self, deployment_id: &str) -> Result<()>;
    
    /// Stream logs from deployment
    pub async fn stream_logs(&self, deployment_id: &str, follow: bool) -> Result<LogStream>;
}

/// Deployment configuration (user-friendly naming)
pub struct DeploymentConfig {
    pub executor_id: String,
    pub image: String,
    pub ssh_key: String,
    pub resources: ResourceRequirements,
    pub environment: HashMap<String, String>,
    pub ports: Vec<PortMapping>,
    pub command: Vec<String>,
    pub volumes: Vec<VolumeMount>,
}
```

#### 1.3 SSH Service
```rust
// crates/basilica-api/src/services/ssh.rs
pub struct SshService {
    config: SshConfig,
}

impl SshService {
    /// Parse SSH credentials from string
    pub fn parse_credentials(creds: &str) -> Result<SshCredentials>;
    
    /// Validate SSH public key
    pub fn validate_public_key(key: &str) -> Result<()>;
    
    /// Load SSH key from file or generate new one
    pub async fn load_or_generate_key(path: Option<&Path>) -> Result<String>;
    
    /// Open interactive SSH session
    pub async fn connect_interactive(&self, access: &SshAccess) -> Result<()>;
    
    /// Execute command over SSH
    pub async fn execute_command(&self, access: &SshAccess, cmd: &str) -> Result<String>;
    
    /// Setup port forwarding
    pub async fn port_forward(&self, access: &SshAccess, forwards: Vec<PortForward>) -> Result<()>;
}
```

#### 1.4 Executor Service
```rust
// crates/basilica-api/src/services/executor.rs
pub struct ExecutorService {
    client: BasilicaClient,
}

impl ExecutorService {
    /// List available executors with smart filtering
    pub async fn list_available(&self, filters: ExecutorFilters) -> Result<Vec<Executor>>;
    
    /// Find best executor for requirements
    pub async fn find_best_match(&self, requirements: &ResourceRequirements) -> Result<Executor>;
    
    /// Get executor details
    pub async fn get_executor(&self, executor_id: &str) -> Result<ExecutorDetails>;
    
    /// Format executor info for display
    pub fn format_executor_info(&self, executor: &Executor) -> String;
}
```

#### 1.5 Cache Service
```rust
// crates/basilica-api/src/services/cache.rs
pub struct CacheService {
    storage: Box<dyn CacheStorage>,
}

impl CacheService {
    /// Cache deployment information
    pub async fn cache_deployment(&self, deployment: &Deployment) -> Result<()>;
    
    /// Get cached deployment
    pub async fn get_cached_deployment(&self, deployment_id: &str) -> Result<Option<Deployment>>;
    
    /// Clear cache
    pub async fn clear(&self) -> Result<()>;
}

/// Cache storage trait for different backends
#[async_trait]
pub trait CacheStorage: Send + Sync {
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>>;
    async fn set(&self, key: &str, value: Vec<u8>, ttl: Option<Duration>) -> Result<()>;
    async fn delete(&self, key: &str) -> Result<()>;
}
```

### Phase 2: New Unified Service Client

```rust
// crates/basilica-api/src/services/mod.rs
pub struct ServiceClient {
    auth: AuthenticationService,
    rental: RentalService,
    ssh: SshService,
    executor: ExecutorService,
    cache: CacheService,
}

impl ServiceClient {
    /// Builder pattern for configuration
    pub fn builder() -> ServiceClientBuilder;
    
    /// Get service references
    pub fn auth(&self) -> &AuthenticationService;
    pub fn deployments(&self) -> &RentalService;
    pub fn ssh(&self) -> &SshService;
    pub fn executors(&self) -> &ExecutorService;
    pub fn cache(&self) -> &CacheService;
}

pub struct ServiceClientBuilder {
    base_url: String,
    auth_config: Option<AuthConfig>,
    ssh_config: Option<SshConfig>,
    cache_backend: Option<Box<dyn CacheStorage>>,
}
```

### Phase 3: Complete CLI Rewrite

```rust
// crates/basilica-cli/src/main.rs
use basilica_api::services::{ServiceClient, FileTokenStorage, FileCacheStorage};

#[tokio::main]
async fn main() -> Result<()> {
    let config = CliConfig::load()?;
    
    // Create unified service client
    let client = ServiceClient::builder()
        .base_url(&config.api.base_url)
        .auth_config(config.auth.clone())
        .ssh_config(config.ssh.clone())
        .token_storage(Box::new(FileTokenStorage::new(&config.token_path)?))
        .cache_storage(Box::new(FileCacheStorage::new(&config.cache_path)?))
        .build()?;
    
    // Execute commands using services
    match args.command {
        Command::Deploy(opts) => handle_deploy(client, opts).await,
        Command::List => handle_list(client).await,
        Command::Stop(id) => handle_stop(client, id).await,
        Command::Ssh(id) => handle_ssh(client, id).await,
        Command::Login => handle_login(client).await,
    }
}

// Simplified handler using services
async fn handle_deploy(client: ServiceClient, opts: DeployOptions) -> Result<()> {
    // Authenticate
    let token = client.auth().get_valid_token().await?;
    
    // Select executor if not specified
    let executor_id = if let Some(id) = opts.executor_id {
        id
    } else {
        let executors = client.executors().list_available(opts.filters).await?;
        prompt_executor_selection(&executors)?
    };
    
    // Deploy using service
    let deployment = client.deployments().deploy(
        DeploymentConfig {
            executor_id,
            image: opts.image,
            ssh_key: client.ssh().load_or_generate_key(opts.ssh_key_path).await?,
            resources: opts.resources,
            environment: opts.environment,
            ports: opts.ports,
            command: opts.command,
            volumes: opts.volumes,
        }
    ).await?;
    
    println!("Deployed: {}", deployment.id);
    
    // Connect SSH if requested
    if !opts.detach {
        client.deployments().wait_for_active(&deployment.id, Duration::from_secs(120)).await?;
        client.ssh().connect_interactive(&deployment.ssh_access).await?;
    }
    
    Ok(())
}
```

### Phase 4: SDK Built on Services

```rust
// crates/basilica-sdk/src/lib.rs
use basilica_api::services::ServiceClient;

pub struct BasilicaSdk {
    client: ServiceClient,
}

impl BasilicaSdk {
    pub fn new(config: SdkConfig) -> Result<Self> {
        let client = ServiceClient::builder()
            .base_url(&config.base_url)
            .auth_token(config.auth_token)
            .build()?;
        
        Ok(Self { client })
    }
    
    pub fn deployments(&self) -> &DeploymentManager {
        &self.client.deployments()
    }
    
    pub fn executors(&self) -> &ExecutorManager {
        &self.client.executors()
    }
}
```

## Breaking Changes (Intentional)

### API Changes
- `start_rental` → `deploy`
- `rental_id` → `deployment_id`
- `RentalResponse` → `Deployment`
- All "rental" terminology → "deployment"

### CLI Changes
- Command structure simplified
- Configuration format updated
- Cache format changed
- Token storage format updated

### Removed Features
- Legacy authentication methods
- Old cache formats
- Deprecated configuration options

## Implementation Strategy (Fast & Clean)

### Week 1: Service Layer
- [ ] Implement all services in basilica-api
- [ ] No concern for backward compatibility
- [ ] Clean, modern API design

### Week 2: CLI Rewrite
- [ ] Complete rewrite using services
- [ ] New command structure
- [ ] Improved UX

### Week 3: SDK & Python Bindings
- [ ] Build SDK on services
- [ ] Create Python bindings
- [ ] Documentation

### Week 4: Testing & Polish
- [ ] Comprehensive testing
- [ ] Performance optimization
- [ ] Final cleanup

## Benefits of Breaking Compatibility

1. **Clean Architecture**: No legacy code or compatibility layers
2. **Better Names**: "deployment" instead of "rental" throughout
3. **Simpler Code**: No need for adapters or converters
4. **Faster Development**: No time spent on migration paths
5. **Modern Patterns**: Use latest Rust patterns and practices

## New File Structure

```
crates/basilica-api/
├── src/
│   ├── services/           # All business logic here
│   │   ├── mod.rs          # ServiceClient
│   │   ├── auth.rs         # Authentication
│   │   ├── deployment.rs   # Deployment management
│   │   ├── executor.rs     # Executor operations
│   │   ├── ssh.rs          # SSH operations
│   │   └── cache.rs        # Caching layer
│   ├── models/             # Shared data models
│   │   ├── deployment.rs
│   │   ├── executor.rs
│   │   └── auth.rs
│   └── client.rs           # Low-level HTTP client

crates/basilica-cli/
├── src/
│   ├── main.rs            # Thin CLI layer
│   ├── commands/          # Command handlers (thin)
│   └── display/           # Output formatting

crates/basilica-sdk/
├── src/
│   └── lib.rs             # Thin wrapper over services
```

## Testing Strategy

### Unit Tests
- Test each service in isolation
- Mock external dependencies
- 100% coverage for services

### Integration Tests
- Test complete workflows
- Use test environment
- Verify all service interactions

### E2E Tests
- Test CLI commands
- Test SDK operations
- Test Python bindings

## Success Metrics

1. **Code Reduction**: 50%+ less code overall
2. **Performance**: 2x faster command execution
3. **Maintainability**: Single source of truth for all logic
4. **Test Coverage**: 90%+ for service layer
5. **Developer Experience**: Simplified API and clear documentation

## Conclusion

By not maintaining backward compatibility, we can create a much cleaner, more maintainable architecture. The service layer in `basilica-api` becomes the single source of truth for all business logic, with CLI, SDK, and Python bindings as thin wrappers. This approach will significantly reduce code duplication and improve the overall quality of the codebase.