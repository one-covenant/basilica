we # Basilica SDK Design Document

## Executive Summary

This document outlines the design for a Rust SDK for Basilica API and its Python bindings via PyO3. The architecture follows a two-layer approach:
1. **basilica-sdk**: Pure Rust SDK providing high-level API client functionality
2. **basilica-python**: PyO3 bindings wrapping the Rust SDK for Python users

## Architecture Overview

```
┌─────────────────────────────────────┐
│         Python Application          │
├─────────────────────────────────────┤
│     basilica-python (PyO3)          │
├─────────────────────────────────────┤
│       basilica-sdk (Rust)           │
├─────────────────────────────────────┤
│      basilica-api (Internal)        │
├─────────────────────────────────────┤
│    basilica-validator (Internal)    │
└─────────────────────────────────────┘
```

## Part 1: Rust SDK (`basilica-sdk`)

### 1.1 Crate Structure

```
crates/basilica-sdk/
├── Cargo.toml
├── README.md
├── src/
│   ├── lib.rs
│   ├── client.rs          # Main SDK client
│   ├── deployment.rs       # Deployment management
│   ├── executor.rs         # Executor/compute resource management
│   ├── models/
│   │   ├── mod.rs
│   │   ├── deployment.rs   # Deployment models
│   │   ├── executor.rs     # Executor models
│   │   └── resources.rs    # Resource specifications
│   ├── error.rs           # Error types
│   ├── auth.rs            # Authentication handling
│   └── streaming.rs       # Log streaming utilities
```

### 1.2 Core Components

#### 1.2.1 Main Client

```rust
// src/client.rs
pub struct BasilicaSdk {
    inner: basilica_api::client::BasilicaClient,
    config: SdkConfig,
}

pub struct SdkConfig {
    pub base_url: String,
    pub timeout: Duration,
    pub retry_policy: RetryPolicy,
    pub auth: AuthConfig,
}

impl BasilicaSdk {
    /// Create a new SDK instance
    pub fn new(config: SdkConfig) -> Result<Self>;
    
    /// Builder pattern for construction
    pub fn builder() -> SdkBuilder;
    
    /// Get deployment manager
    pub fn deployments(&self) -> DeploymentManager;
    
    /// Get executor manager  
    pub fn executors(&self) -> ExecutorManager;
    
    /// Health check
    pub async fn health(&self) -> Result<HealthStatus>;
}
```

#### 1.2.2 Deployment Manager

```rust
// src/deployment.rs
pub struct DeploymentManager<'a> {
    client: &'a BasilicaSdk,
}

impl<'a> DeploymentManager<'a> {
    /// Deploy a container image
    pub async fn deploy(&self, config: DeploymentConfig) -> Result<Deployment>;
    
    /// Get deployment by ID
    pub async fn get(&self, id: &str) -> Result<Deployment>;
    
    /// List deployments with filters
    pub async fn list(&self, filters: ListFilters) -> Result<Vec<Deployment>>;
    
    /// Stop a deployment
    pub async fn stop(&self, id: &str) -> Result<()>;
    
    /// Stream logs from a deployment
    pub async fn logs(&self, id: &str, follow: bool) -> Result<LogStream>;
    
    /// Get deployment status
    pub async fn status(&self, id: &str) -> Result<DeploymentStatus>;
}
```

### 1.3 Models

#### 1.3.1 Deployment Configuration

```rust
// src/models/deployment.rs
#[derive(Debug, Clone, Builder)]
pub struct DeploymentConfig {
    /// Executor ID to deploy on
    pub executor_id: String,
    
    /// Container image (e.g., "nginx:latest")
    pub image: String,
    
    /// SSH public key for access
    pub ssh_key: String,
    
    /// Resource requirements
    pub resources: ResourceRequirements,
    
    /// Environment variables
    #[builder(default)]
    pub environment: HashMap<String, String>,
    
    /// Port mappings
    #[builder(default)]
    pub ports: Vec<PortMapping>,
    
    /// Volume mounts
    #[builder(default)]
    pub volumes: Vec<VolumeMount>,
    
    /// Command to run
    #[builder(default)]
    pub command: Option<Vec<String>>,
    
    /// Disable SSH access
    #[builder(default)]
    pub no_ssh: bool,
}

#[derive(Debug, Clone)]
pub struct Deployment {
    pub id: String,
    pub status: DeploymentStatus,
    pub ssh_access: Option<SshAccess>,
    pub created_at: DateTime<Utc>,
    pub executor_id: String,
    pub image: String,
    pub resources: ResourceRequirements,
}

#[derive(Debug, Clone)]
pub enum DeploymentStatus {
    Pending,
    Running,
    Stopped,
    Failed(String),
}
```

#### 1.3.2 Resource Specifications

```rust
// src/models/resources.rs
#[derive(Debug, Clone, Builder)]
pub struct ResourceRequirements {
    #[builder(default = "1.0")]
    pub cpu_cores: f64,
    
    #[builder(default = "1024")]
    pub memory_mb: i64,
    
    #[builder(default = "10240")]
    pub storage_mb: i64,
    
    #[builder(default = "0")]
    pub gpu_count: u32,
    
    #[builder(default)]
    pub gpu_types: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PortMapping {
    pub container_port: u32,
    pub host_port: u32,
    pub protocol: Protocol,
}

#[derive(Debug, Clone)]
pub enum Protocol {
    Tcp,
    Udp,
}
```

### 1.4 Error Handling

```rust
// src/error.rs
#[derive(Debug, thiserror::Error)]
pub enum SdkError {
    #[error("Authentication failed: {0}")]
    AuthenticationError(String),
    
    #[error("Deployment failed: {0}")]
    DeploymentError(String),
    
    #[error("Resource not found: {0}")]
    NotFound(String),
    
    #[error("Invalid configuration: {0}")]
    ConfigurationError(String),
    
    #[error("Network error: {0}")]
    NetworkError(#[from] reqwest::Error),
    
    #[error("API error: {0}")]
    ApiError(#[from] basilica_api::error::ApiError),
}

pub type Result<T> = std::result::Result<T, SdkError>;
```

### 1.5 Usage Examples (Rust)

```rust
use basilica_sdk::{BasilicaSdk, DeploymentConfig, ResourceRequirements};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize SDK
    let sdk = BasilicaSdk::builder()
        .base_url("https://api.basilica.ai")
        .auth_token("your_auth_token")
        .build()?;
    
    // List available executors
    let executors = sdk.executors().list_available().await?;
    println!("Available executors: {}", executors.len());
    
    // Deploy a container
    let deployment = sdk.deployments()
        .deploy(
            DeploymentConfig::builder()
                .executor_id(executors[0].id.clone())
                .image("nginx:latest")
                .ssh_key("ssh-rsa ...")
                .resources(
                    ResourceRequirements::builder()
                        .cpu_cores(2.0)
                        .memory_mb(4096)
                        .gpu_count(1)
                        .build()
                )
                .ports(vec![
                    PortMapping::tcp(80, 8080),
                ])
                .build()?
        )
        .await?;
    
    println!("Deployed: {}", deployment.id);
    println!("SSH: {:?}", deployment.ssh_access);
    
    // Stream logs
    let mut log_stream = sdk.deployments()
        .logs(&deployment.id, true)
        .await?;
    
    while let Some(log) = log_stream.next().await {
        println!("{}", log?);
    }
    
    // Stop deployment
    sdk.deployments().stop(&deployment.id).await?;
    
    Ok(())
}
```

## Part 2: Python Bindings (`basilica-python`)

### 2.1 Crate Structure

```
crates/basilica-python/
├── Cargo.toml
├── pyproject.toml
├── README.md
├── src/
│   ├── lib.rs
│   ├── client.rs          # PyO3 wrapper for SDK client
│   ├── deployment.rs       # PyO3 wrapper for deployments
│   ├── executor.rs         # PyO3 wrapper for executors
│   ├── models.rs          # Python model classes
│   ├── error.rs           # Python exception types
│   └── async_runtime.rs   # Async/await support
├── python/
│   ├── basilica/
│   │   ├── __init__.py
│   │   ├── _native.pyi    # Type stubs
│   │   └── py.typed       # PEP 561 marker
└── tests/
    └── test_sdk.py
```

### 2.2 PyO3 Implementation

#### 2.2.1 Main Client Wrapper

```rust
// src/client.rs
use pyo3::prelude::*;
use pyo3_asyncio;

#[pyclass]
pub struct BasilicaClient {
    inner: Arc<basilica_sdk::BasilicaSdk>,
    runtime: Arc<tokio::runtime::Runtime>,
}

#[pymethods]
impl BasilicaClient {
    #[new]
    #[pyo3(signature = (base_url, token, **kwargs))]
    fn new(base_url: String, token: String, kwargs: Option<&PyDict>) -> PyResult<Self> {
        let sdk = basilica_sdk::BasilicaSdk::builder()
            .base_url(base_url)
            .auth_token(token)
            .build()
            .map_err(|e| PyException::new_err(e.to_string()))?;
        
        Ok(Self {
            inner: Arc::new(sdk),
            runtime: Arc::new(tokio::runtime::Runtime::new()?),
        })
    }
    
    /// Deploy a container (async)
    fn deploy<'py>(&self, py: Python<'py>, config: PyDeploymentConfig) -> PyResult<&'py PyAny> {
        let sdk = self.inner.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let deployment = sdk.deployments()
                .deploy(config.into())
                .await
                .map_err(to_py_err)?;
            Ok(PyDeployment::from(deployment))
        })
    }
    
    /// Stop a deployment (async)
    fn stop<'py>(&self, py: Python<'py>, deployment_id: String) -> PyResult<&'py PyAny> {
        let sdk = self.inner.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            sdk.deployments()
                .stop(&deployment_id)
                .await
                .map_err(to_py_err)?;
            Ok(())
        })
    }
    
    /// List deployments (async)
    fn list_deployments<'py>(&self, py: Python<'py>) -> PyResult<&'py PyAny> {
        let sdk = self.inner.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let deployments = sdk.deployments()
                .list(Default::default())
                .await
                .map_err(to_py_err)?;
            Ok(deployments.into_iter()
                .map(PyDeployment::from)
                .collect::<Vec<_>>())
        })
    }
}
```

#### 2.2.2 Python Models

```rust
// src/models.rs
#[pyclass]
#[derive(Clone)]
pub struct PyDeploymentConfig {
    #[pyo3(get, set)]
    pub executor_id: String,
    #[pyo3(get, set)]
    pub image: String,
    #[pyo3(get, set)]
    pub ssh_key: String,
    #[pyo3(get, set)]
    pub cpu_cores: f64,
    #[pyo3(get, set)]
    pub memory_mb: i64,
    #[pyo3(get, set)]
    pub gpu_count: u32,
    #[pyo3(get, set)]
    pub environment: HashMap<String, String>,
    #[pyo3(get, set)]
    pub ports: Vec<PyPortMapping>,
}

#[pymethods]
impl PyDeploymentConfig {
    #[new]
    fn new(executor_id: String, image: String, ssh_key: String) -> Self {
        Self {
            executor_id,
            image,
            ssh_key,
            cpu_cores: 1.0,
            memory_mb: 1024,
            gpu_count: 0,
            environment: HashMap::new(),
            ports: Vec::new(),
        }
    }
}
```

### 2.3 Python API Design

#### 2.3.1 Synchronous Wrapper

```python
# python/basilica/__init__.py
from basilica._native import BasilicaClient as _BasilicaClient
from basilica._native import DeploymentConfig, Deployment
import asyncio
from typing import Optional, List, Dict, Any

class BasilicaClient:
    """Synchronous wrapper for the Basilica SDK"""
    
    def __init__(self, base_url: str, token: str, **kwargs):
        self._async_client = _BasilicaClient(base_url, token, **kwargs)
        self._loop = asyncio.new_event_loop()
    
    def deploy(self, config: DeploymentConfig) -> Deployment:
        """Deploy a container image"""
        return self._loop.run_until_complete(
            self._async_client.deploy(config)
        )
    
    def stop(self, deployment_id: str) -> None:
        """Stop a deployment"""
        self._loop.run_until_complete(
            self._async_client.stop(deployment_id)
        )
    
    def list_deployments(self) -> List[Deployment]:
        """List all deployments"""
        return self._loop.run_until_complete(
            self._async_client.list_deployments()
        )

class AsyncBasilicaClient:
    """Async version of the Basilica SDK"""
    
    def __init__(self, base_url: str, token: str, **kwargs):
        self._client = _BasilicaClient(base_url, token, **kwargs)
    
    async def deploy(self, config: DeploymentConfig) -> Deployment:
        """Deploy a container image"""
        return await self._client.deploy(config)
    
    async def stop(self, deployment_id: str) -> None:
        """Stop a deployment"""
        await self._client.stop(deployment_id)
    
    async def list_deployments(self) -> List[Deployment]:
        """List all deployments"""
        return await self._client.list_deployments()
    
    async def stream_logs(self, deployment_id: str):
        """Stream logs from a deployment"""
        async for log in self._client.stream_logs(deployment_id):
            yield log
```

### 2.4 Python Usage Examples

```python
# Synchronous usage
from basilica import BasilicaClient, DeploymentConfig

client = BasilicaClient(
    base_url="https://api.basilica.ai",
    token="your_auth_token"
)

# List available executors
executors = client.list_executors()
print(f"Available executors: {len(executors)}")

# Deploy a container
config = DeploymentConfig(
    executor_id=executors[0].id,
    image="nginx:latest",
    ssh_key="ssh-rsa ..."
)
config.cpu_cores = 2.0
config.memory_mb = 4096
config.gpu_count = 1
config.ports = [{"container": 80, "host": 8080}]

deployment = client.deploy(config)
print(f"Deployed: {deployment.id}")
print(f"SSH: {deployment.ssh_access}")

# Stop deployment
client.stop(deployment.id)
```

```python
# Async usage
import asyncio
from basilica import AsyncBasilicaClient, DeploymentConfig

async def main():
    client = AsyncBasilicaClient(
        base_url="https://api.basilica.ai", 
        token="your_auth_token"
    )
    
    # Deploy
    config = DeploymentConfig(
        executor_id="executor-123",
        image="python:3.11",
        ssh_key="ssh-rsa ..."
    )
    deployment = await client.deploy(config)
    
    # Stream logs
    async for log in client.stream_logs(deployment.id):
        print(f"[{log.timestamp}] {log.message}")
    
    # Cleanup
    await client.stop(deployment.id)

asyncio.run(main())
```

## Part 3: Build & Distribution

### 3.1 Rust SDK

**Cargo.toml**:
```toml
[package]
name = "basilica-sdk"
version = "0.1.0"
edition = "2021"

[dependencies]
basilica-api = { path = "../basilica-api" }
basilica-validator = { path = "../basilica-validator" }
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.11", features = ["json", "stream"] }
serde = { version = "1", features = ["derive"] }
thiserror = "1"
derive_builder = "0.12"
async-trait = "0.1"
futures = "0.3"
chrono = { version = "0.4", features = ["serde"] }

[dev-dependencies]
tokio-test = "0.4"
mockito = "1"
```

### 3.2 Python Bindings

**Cargo.toml**:
```toml
[package]
name = "basilica-python"
version = "0.1.0"
edition = "2021"

[lib]
name = "basilica"
crate-type = ["cdylib"]

[dependencies]
basilica-sdk = { path = "../basilica-sdk" }
pyo3 = { version = "0.20", features = ["extension-module", "abi3-py38"] }
pyo3-asyncio = { version = "0.20", features = ["tokio-runtime"] }
tokio = { version = "1", features = ["full"] }
futures = "0.3"

[build-dependencies]
pyo3-build-config = "0.20"
```

**pyproject.toml**:
```toml
[build-system]
requires = ["maturin>=1.0,<2.0"]
build-backend = "maturin"

[project]
name = "basilica"
version = "0.1.0"
requires-python = ">=3.8"
description = "Python SDK for Basilica API"
classifiers = [
    "Development Status :: 4 - Beta",
    "Programming Language :: Python :: 3",
    "Programming Language :: Python :: 3.8",
    "Programming Language :: Python :: 3.9",
    "Programming Language :: Python :: 3.10",
    "Programming Language :: Python :: 3.11",
    "Programming Language :: Python :: 3.12",
    "Programming Language :: Rust",
]

[project.optional-dependencies]
dev = [
    "pytest>=7.0",
    "pytest-asyncio>=0.21",
    "black>=23.0",
    "mypy>=1.0",
]
```

### 3.3 CI/CD Pipeline

```yaml
# .github/workflows/sdk.yml
name: SDK Build & Test

on:
  push:
    paths:
      - 'crates/basilica-sdk/**'
      - 'crates/basilica-python/**'

jobs:
  rust-sdk:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test -p basilica-sdk
      - run: cargo doc -p basilica-sdk --no-deps

  python-sdk:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        python-version: ['3.8', '3.9', '3.10', '3.11', '3.12']
    steps:
      - uses: actions/checkout@v3
      - uses: actions/setup-python@v4
        with:
          python-version: ${{ matrix.python-version }}
      - run: pip install maturin pytest pytest-asyncio
      - run: maturin develop -m crates/basilica-python/Cargo.toml
      - run: pytest crates/basilica-python/tests

  publish:
    if: startsWith(github.ref, 'refs/tags/')
    needs: [rust-sdk, python-sdk]
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - run: cargo publish -p basilica-sdk
      - uses: pypa/gh-action-pypi-publish@release/v1
        with:
          maturin-version: latest
          command: publish
          args: -m crates/basilica-python/Cargo.toml
```

## Part 4: Testing Strategy

### 4.1 Rust SDK Tests

```rust
// crates/basilica-sdk/tests/integration.rs
#[tokio::test]
async fn test_deployment_lifecycle() {
    let mock_server = mockito::Server::new();
    let sdk = BasilicaSdk::builder()
        .base_url(mock_server.url())
        .auth_token("test_token")
        .build()
        .unwrap();
    
    // Mock endpoints
    let _m = mock_server.mock("POST", "/rentals")
        .with_status(200)
        .with_body(r#"{"rental_id": "test-123"}"#)
        .create();
    
    // Test deployment
    let config = DeploymentConfig::builder()
        .executor_id("executor-1")
        .image("nginx")
        .ssh_key("ssh-rsa test")
        .build()
        .unwrap();
    
    let deployment = sdk.deployments().deploy(config).await.unwrap();
    assert_eq!(deployment.id, "test-123");
}
```

### 4.2 Python Tests

```python
# crates/basilica-python/tests/test_sdk.py
import pytest
from basilica import BasilicaClient, DeploymentConfig

@pytest.fixture
def client():
    return BasilicaClient(
        base_url="http://localhost:8080",
        token="test_token"
    )

def test_deployment_config():
    config = DeploymentConfig(
        executor_id="test",
        image="nginx",
        ssh_key="ssh-rsa test"
    )
    assert config.executor_id == "test"
    assert config.cpu_cores == 1.0  # default

@pytest.mark.asyncio
async def test_async_client():
    from basilica import AsyncBasilicaClient
    client = AsyncBasilicaClient(
        base_url="http://localhost:8080",
        token="test_token"
    )
    # Add async tests
```

## Part 5: Documentation

### 5.1 Rust SDK Documentation

- Generate with `cargo doc`
- Publish to docs.rs
- Include examples in doc comments
- README with quickstart guide

### 5.2 Python SDK Documentation

- Type stubs for IDE support
- Sphinx documentation
- Jupyter notebook examples
- README with installation and usage

## Part 6: Versioning & Release Strategy

1. **Semantic Versioning**: Follow semver for both crates
2. **Release Process**:
   - Tag releases with `v{version}`
   - Publish Rust SDK to crates.io
   - Build Python wheels for multiple platforms
   - Publish to PyPI
3. **Compatibility**: Maintain backward compatibility within major versions

## Part 7: Key Design Decisions

### 7.1 Why Two Layers?

- **Separation of Concerns**: Rust SDK can be used independently
- **Reusability**: Other language bindings can use the same Rust SDK
- **Performance**: Core logic in Rust, thin Python wrapper
- **Maintenance**: Single source of truth for business logic

### 7.2 Naming Conventions

- **User-Friendly**: `deploy()` instead of `start_rental()`
- **Consistent**: Similar naming across Rust and Python
- **Descriptive**: `DeploymentConfig` clearly indicates purpose

### 7.3 Authentication Strategy

- **Token-based**: Auth0 JWT tokens
- **Refresh Support**: Automatic token refresh on 401
- **Environment Variables**: Support for `BASILICA_API_TOKEN`
- **Secure**: No hardcoded credentials

### 7.4 Error Handling

- **Type-safe**: Custom error types in Rust
- **Python Exceptions**: Map to appropriate Python exceptions
- **Detailed Messages**: Include context for debugging
- **Recovery**: Automatic retry for transient failures

### 7.5 Async Support

- **Native Async**: Both Rust and Python support async/await
- **Streaming**: Efficient log streaming with backpressure
- **Sync Wrapper**: Convenience wrapper for simple scripts
- **Performance**: Non-blocking I/O throughout