# Basilica Refactoring Implementation Plan

## Overview
This document provides a detailed, test-first implementation plan for refactoring the Basilica codebase. Each phase must achieve **100% test coverage** before proceeding to the next phase.

## Testing Requirements
- **Unit Tests**: Every public method must have tests
- **Integration Tests**: Every service interaction must be tested
- **Coverage Gate**: Minimum 95% coverage per module
- **CI Pipeline**: Tests must pass before merging

---

## Phase 1: Core Models & Types (Week 1, Days 1-2)

### 1.1 Create Shared Models
**Location**: `crates/basilica-api/src/models/`

#### Tasks:
```rust
// models/deployment.rs
- [ ] Define Deployment struct
- [ ] Define DeploymentConfig struct
- [ ] Define DeploymentStatus enum
- [ ] Define DeploymentFilters struct
- [ ] Implement Display traits
- [ ] Implement conversion traits (From/Into)

// models/executor.rs
- [ ] Define Executor struct
- [ ] Define ExecutorDetails struct
- [ ] Define ExecutorFilters struct
- [ ] Define ResourceRequirements struct

// models/auth.rs
- [ ] Define TokenSet struct
- [ ] Define AuthConfig struct
- [ ] Define AuthError enum

// models/ssh.rs
- [ ] Define SshAccess struct
- [ ] Define SshCredentials struct
- [ ] Define PortForward struct
```

#### Tests Required:
```rust
// tests/models/deployment_test.rs
#[test]
fn test_deployment_creation()
fn test_deployment_config_validation()
fn test_deployment_status_transitions()
fn test_deployment_filters_serialization()
fn test_deployment_display_format()

// tests/models/executor_test.rs
#[test]
fn test_executor_resource_matching()
fn test_executor_filters_application()
fn test_resource_requirements_validation()

// tests/models/auth_test.rs
#[test]
fn test_token_expiry_calculation()
fn test_auth_config_validation()
fn test_auth_error_messages()
```

**Deliverable**: All models with 100% test coverage
**Time**: 2 days

---

## Phase 2: Cache Service (Week 1, Days 3-4)

### 2.1 Implement Cache Service
**Location**: `crates/basilica-api/src/services/cache.rs`

#### Tasks:
```rust
- [ ] Define CacheStorage trait
- [ ] Implement MemoryCacheStorage
- [ ] Implement FileCacheStorage
- [ ] Implement RedisCacheStorage (optional)
- [ ] Create CacheService with TTL support
- [ ] Add cache invalidation strategies
- [ ] Add cache statistics/metrics
```

#### Tests Required:
```rust
// tests/services/cache_test.rs
#[tokio::test]
async fn test_memory_cache_storage_set_get()
async fn test_memory_cache_storage_expiry()
async fn test_file_cache_storage_persistence()
async fn test_file_cache_storage_concurrent_access()
async fn test_cache_service_ttl()
async fn test_cache_service_invalidation()
async fn test_cache_service_stats()

// Integration tests
#[tokio::test]
async fn test_cache_service_with_different_backends()
async fn test_cache_service_error_handling()
```

**Deliverable**: Complete cache service with pluggable backends
**Test Coverage Target**: 100%
**Time**: 2 days

---

## Phase 3: Authentication Service (Week 1, Days 5-7)

### 3.1 Implement Auth Service
**Location**: `crates/basilica-api/src/services/auth.rs`

#### Tasks:
```rust
- [ ] Define TokenStorage trait
- [ ] Implement FileTokenStorage
- [ ] Implement MemoryTokenStorage
- [ ] Create AuthenticationService
- [ ] Implement OAuth with PKCE flow
- [ ] Implement device flow
- [ ] Implement token refresh logic
- [ ] Add token validation
- [ ] Add multi-tenant support
```

#### Tests Required:
```rust
// tests/services/auth_test.rs
#[tokio::test]
async fn test_oauth_pkce_flow_success()
async fn test_oauth_pkce_flow_invalid_code()
async fn test_device_flow_success()
async fn test_device_flow_timeout()
async fn test_token_refresh_success()
async fn test_token_refresh_expired_refresh_token()
async fn test_token_validation()
async fn test_token_storage_file_persistence()
async fn test_token_storage_concurrent_access()

// Mock server tests
#[tokio::test]
async fn test_auth_service_with_mock_oauth_provider()
async fn test_auth_service_network_errors()
async fn test_auth_service_rate_limiting()
```

**Deliverable**: Complete authentication service with OAuth and device flow
**Test Coverage Target**: 100%
**Time**: 3 days

---

## Phase 4: Executor Service (Week 2, Days 1-2)

### 4.1 Implement Executor Service
**Location**: `crates/basilica-api/src/services/executor.rs`

#### Tasks:
```rust
- [ ] Create ExecutorService
- [ ] Implement list_available with filters
- [ ] Implement find_best_match algorithm
- [ ] Add executor scoring logic
- [ ] Implement get_executor_details
- [ ] Add executor health checks
- [ ] Add capacity planning helpers
```

#### Tests Required:
```rust
// tests/services/executor_test.rs
#[tokio::test]
async fn test_list_executors_no_filters()
async fn test_list_executors_gpu_filter()
async fn test_list_executors_memory_filter()
async fn test_find_best_match_cpu_only()
async fn test_find_best_match_gpu_required()
async fn test_find_best_match_no_matches()
async fn test_executor_scoring_algorithm()
async fn test_executor_health_check()

// Mock client tests
#[tokio::test]
async fn test_executor_service_api_errors()
async fn test_executor_service_timeout()
async fn test_executor_service_caching()
```

**Deliverable**: Complete executor service with smart matching
**Test Coverage Target**: 100%
**Time**: 2 days

---

## Phase 5: SSH Service (Week 2, Days 3-4)

### 5.1 Implement SSH Service
**Location**: `crates/basilica-api/src/services/ssh.rs`

#### Tasks:
```rust
- [ ] Create SshService
- [ ] Implement SSH key validation
- [ ] Implement SSH key generation
- [ ] Add credential parsing
- [ ] Implement connection builder
- [ ] Add port forwarding support
- [ ] Add file transfer support
- [ ] Add connection pooling
```

#### Tests Required:
```rust
// tests/services/ssh_test.rs
#[test]
fn test_ssh_key_validation_valid_rsa()
fn test_ssh_key_validation_valid_ed25519()
fn test_ssh_key_validation_invalid_format()
fn test_ssh_key_generation()
fn test_credential_parsing()
fn test_connection_builder()

#[tokio::test]
async fn test_ssh_connection_mock()
async fn test_ssh_port_forwarding()
async fn test_ssh_file_transfer()
async fn test_ssh_connection_pool()

// Integration tests with container
#[tokio::test]
async fn test_ssh_real_connection() // Use test container
async fn test_ssh_command_execution()
async fn test_ssh_connection_timeout()
```

**Deliverable**: Complete SSH service with all features
**Test Coverage Target**: 100%
**Time**: 2 days

---

## Phase 6: Deployment Service (Week 2, Days 5-7)

### 6.1 Implement Deployment Service
**Location**: `crates/basilica-api/src/services/deployment.rs`

#### Tasks:
```rust
- [ ] Create DeploymentService (renamed from RentalService)
- [ ] Implement deploy method with validation
- [ ] Implement wait_for_active with polling
- [ ] Add deployment status tracking
- [ ] Implement list_deployments with filters
- [ ] Add stop deployment with cleanup
- [ ] Implement log streaming
- [ ] Add deployment metrics
- [ ] Add rollback support
```

#### Tests Required:
```rust
// tests/services/deployment_test.rs
#[tokio::test]
async fn test_deploy_success()
async fn test_deploy_invalid_image()
async fn test_deploy_invalid_ssh_key()
async fn test_deploy_no_available_executors()
async fn test_wait_for_active_success()
async fn test_wait_for_active_timeout()
async fn test_list_deployments_all()
async fn test_list_deployments_filtered()
async fn test_stop_deployment_success()
async fn test_stop_deployment_not_found()
async fn test_log_streaming()

// Integration tests
#[tokio::test]
async fn test_deployment_lifecycle() // Full deploy -> wait -> stop
async fn test_deployment_with_cache()
async fn test_deployment_concurrent_operations()
async fn test_deployment_rollback()
```

**Deliverable**: Complete deployment service with all operations
**Test Coverage Target**: 100%
**Time**: 3 days

---

## Phase 7: Service Client Integration (Week 3, Days 1-2)

### 7.1 Create Unified Service Client
**Location**: `crates/basilica-api/src/services/mod.rs`

#### Tasks:
```rust
- [ ] Create ServiceClient struct
- [ ] Implement ServiceClientBuilder
- [ ] Add service initialization
- [ ] Add dependency injection
- [ ] Implement service lifecycle management
- [ ] Add health checks for all services
- [ ] Add metrics aggregation
- [ ] Add circuit breaker pattern
```

#### Tests Required:
```rust
// tests/services/client_test.rs
#[tokio::test]
async fn test_service_client_builder()
async fn test_service_client_initialization()
async fn test_service_client_with_custom_backends()
async fn test_service_client_health_check()
async fn test_service_client_shutdown()

// Integration tests
#[tokio::test]
async fn test_service_client_all_services()
async fn test_service_client_error_propagation()
async fn test_service_client_concurrent_usage()
async fn test_circuit_breaker_activation()
```

**Deliverable**: Unified service client with all services integrated
**Test Coverage Target**: 100%
**Time**: 2 days

---

## Phase 8: CLI Refactoring (Week 3, Days 3-5)

### 8.1 Refactor CLI to Use Services
**Location**: `crates/basilica-cli/src/`

#### Tasks:
```rust
- [ ] Remove all business logic from CLI
- [ ] Update main.rs to use ServiceClient
- [ ] Refactor deploy command
- [ ] Refactor list command
- [ ] Refactor stop command
- [ ] Refactor ssh command
- [ ] Refactor login/logout commands
- [ ] Update configuration handling
- [ ] Add new output formatters
- [ ] Update error handling
```

#### Tests Required:
```rust
// tests/cli/commands_test.rs
#[tokio::test]
async fn test_deploy_command_basic()
async fn test_deploy_command_with_options()
async fn test_list_command()
async fn test_stop_command()
async fn test_ssh_command()
async fn test_login_command()

// Integration tests
#[tokio::test]
async fn test_cli_deploy_workflow()
async fn test_cli_error_handling()
async fn test_cli_output_formats()

// E2E tests with mock services
#[tokio::test]
async fn test_cli_e2e_deploy_stop()
async fn test_cli_e2e_with_mock_api()
```

**Deliverable**: Refactored CLI using services
**Test Coverage Target**: 95% (UI code may be lower)
**Time**: 3 days

---

## Phase 9: SDK Implementation (Week 3, Days 6-7 + Week 4, Day 1)

### 9.1 Build SDK on Services
**Location**: `crates/basilica-sdk/src/`

#### Tasks:
```rust
- [ ] Create BasilicaSdk struct
- [ ] Implement SDK builder
- [ ] Add deployment manager
- [ ] Add executor manager
- [ ] Add authentication helpers
- [ ] Add async/sync interfaces
- [ ] Add retry logic
- [ ] Add SDK examples
- [ ] Add comprehensive documentation
```

#### Tests Required:
```rust
// tests/sdk/sdk_test.rs
#[tokio::test]
async fn test_sdk_initialization()
async fn test_sdk_deploy()
async fn test_sdk_list_deployments()
async fn test_sdk_stop_deployment()
async fn test_sdk_executor_operations()

// Integration tests
#[tokio::test]
async fn test_sdk_full_workflow()
async fn test_sdk_error_handling()
async fn test_sdk_concurrent_operations()

// Doc tests
/// Examples in documentation must compile and run
```

**Deliverable**: Complete SDK with examples
**Test Coverage Target**: 100%
**Time**: 3 days

---

## Phase 10: Python Bindings (Week 4, Days 2-4)

### 10.1 Create PyO3 Bindings
**Location**: `crates/basilica-python/src/`

#### Tasks:
```rust
- [ ] Set up PyO3 project structure
- [ ] Create Python client wrapper
- [ ] Add deployment operations
- [ ] Add executor operations
- [ ] Add authentication
- [ ] Implement async support
- [ ] Add type stubs
- [ ] Create Python package
- [ ] Add Python examples
```

#### Tests Required:
```python
# tests/test_basilica.py
def test_client_creation()
def test_deploy()
def test_list_deployments()
def test_stop_deployment()
async def test_async_operations()
def test_error_handling()

# Integration tests
def test_full_workflow()
def test_concurrent_operations()
```

**Deliverable**: Python package with bindings
**Test Coverage Target**: 95%
**Time**: 3 days

---

## Phase 11: Performance & Polish (Week 4, Days 5-7)

### 11.1 Optimization and Cleanup
#### Tasks:
- [ ] Performance profiling
- [ ] Optimize hot paths
- [ ] Add connection pooling
- [ ] Implement caching strategies
- [ ] Remove old code
- [ ] Update all documentation
- [ ] Add migration guide
- [ ] Create benchmarks

#### Tests Required:
```rust
// benches/performance.rs
#[bench]
fn bench_deployment_creation()
fn bench_executor_matching()
fn bench_cache_operations()
fn bench_auth_token_refresh()

// Load tests
#[tokio::test]
async fn test_load_concurrent_deployments()
async fn test_load_service_client()
```

**Deliverable**: Optimized codebase
**Time**: 3 days

---

## Quality Gates

### Per-Phase Requirements:
1. **Code Review**: All code must be reviewed before merging
2. **Test Coverage**: 95% minimum (100% for services)
3. **Documentation**: All public APIs must be documented
4. **CI Pass**: All tests must pass in CI
5. **Benchmarks**: No performance regression

### Final Acceptance Criteria:
- [ ] All services have 100% test coverage
- [ ] CLI works with new services
- [ ] SDK is fully functional
- [ ] Python bindings work correctly
- [ ] Performance is improved or equal
- [ ] Documentation is complete
- [ ] No deprecated code remains

---

## Risk Mitigation

### Contingency Plans:
1. **If tests fail**: Block progress until fixed
2. **If coverage drops**: Add tests before continuing
3. **If performance degrades**: Profile and optimize before next phase
4. **If integration fails**: Create compatibility layer temporarily

### Rollback Strategy:
- Keep old code in separate branch
- Feature flag new implementation
- Gradual rollout with monitoring

---

## Timeline Summary

```
Week 1: Core Models, Cache, Auth Services (100% tested)
Week 2: Executor, SSH, Deployment Services (100% tested)  
Week 3: Service Client, CLI Refactor, SDK Start (95%+ tested)
Week 4: SDK Complete, Python Bindings, Performance (95%+ tested)
```

## Success Metrics

1. **Test Coverage**: Overall >95%, Services 100%
2. **Performance**: 2x faster operations
3. **Code Reduction**: 50% less duplicated code
4. **API Simplification**: 30% fewer public methods
5. **Documentation**: 100% public API documented

---

## Monitoring & Validation

### Daily Checks:
- [ ] Test coverage report
- [ ] CI pipeline status
- [ ] Code review status
- [ ] Performance benchmarks

### Weekly Reviews:
- [ ] Phase completion status
- [ ] Coverage trends
- [ ] Performance metrics
- [ ] Risk assessment

---

## Notes

- **Test First**: Write tests before implementation
- **Coverage Gate**: Cannot proceed without meeting coverage targets
- **Documentation**: Update as you code, not after
- **Review Early**: Get feedback on design before full implementation