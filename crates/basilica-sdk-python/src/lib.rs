//! Python bindings for the Basilica SDK

use basilica_sdk::{
    auth::types::AuthConfig,
    BasilicaClient as RustClient, ClientBuilder,
};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use pythonize::{depythonize, pythonize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;

/// Python wrapper for BasilicaClient
#[pyclass]
struct BasilicaClient {
    inner: Arc<RustClient>,
    runtime: Runtime,
}

#[pymethods]
impl BasilicaClient {
    /// Create a new BasilicaClient
    ///
    /// Args:
    ///     base_url: The base URL of the Basilica API
    ///     token: Optional authentication token
    ///     timeout_secs: Request timeout in seconds (default: 30)
    #[new]
    #[pyo3(signature = (base_url, token=None, timeout_secs=30))]
    fn new(base_url: String, token: Option<String>, timeout_secs: u64) -> PyResult<Self> {
        let runtime = Runtime::new()
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create runtime: {}", e)))?;
        
        let client = runtime.block_on(async {
            let mut builder = ClientBuilder::default()
                .base_url(base_url)
                .timeout(Duration::from_secs(timeout_secs));
            
            if let Some(t) = token {
                builder = builder.with_bearer_token(t);
            }
            
            builder.build()
        })
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to create client: {}", e)))?;
        
        Ok(Self {
            inner: Arc::new(client),
            runtime,
        })
    }

    /// Create a new client with OAuth authentication
    ///
    /// Args:
    ///     base_url: The base URL of the Basilica API
    ///     client_id: Auth0 client ID
    ///     timeout_secs: Request timeout in seconds (default: 30)
    #[staticmethod]
    #[pyo3(signature = (base_url, client_id, timeout_secs=30))]
    fn with_oauth_auth(base_url: String, client_id: String, timeout_secs: u64) -> PyResult<Self> {
        let runtime = Runtime::new()
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create runtime: {}", e)))?;

        let auth_config = AuthConfig {
            client_id,
            auth_endpoint: "https://auth.basilica.ai/authorize".to_string(),
            token_endpoint: "https://auth.basilica.ai/oauth/token".to_string(),
            device_auth_endpoint: Some("https://auth.basilica.ai/oauth/device/code".to_string()),
            revoke_endpoint: Some("https://auth.basilica.ai/oauth/revoke".to_string()),
            redirect_uri: "http://localhost:8080/callback".to_string(),
            scopes: vec![
                "openid".to_string(),
                "profile".to_string(),
                "offline_access".to_string(),
            ],
            additional_params: HashMap::new(),
        };

        let client = runtime.block_on(async {
            ClientBuilder::default()
                .base_url(base_url)
                .timeout(Duration::from_secs(timeout_secs))
                .with_auth_config(auth_config)
                .build_with_oauth_auth()
                .await
        })
        .map_err(|e| PyRuntimeError::new_err(format!("OAuth authentication failed: {}", e)))?;

        Ok(Self {
            inner: Arc::new(client),
            runtime,
        })
    }

    /// Create a new client with device flow authentication
    ///
    /// Args:
    ///     base_url: The base URL of the Basilica API
    ///     client_id: Auth0 client ID
    ///     timeout_secs: Request timeout in seconds (default: 30)
    #[staticmethod]
    #[pyo3(signature = (base_url, client_id, timeout_secs=30))]
    fn with_device_auth(base_url: String, client_id: String, timeout_secs: u64) -> PyResult<Self> {
        let runtime = Runtime::new()
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create runtime: {}", e)))?;

        let auth_config = AuthConfig {
            client_id,
            auth_endpoint: "https://auth.basilica.ai/authorize".to_string(),
            token_endpoint: "https://auth.basilica.ai/oauth/token".to_string(),
            device_auth_endpoint: Some("https://auth.basilica.ai/oauth/device/code".to_string()),
            revoke_endpoint: Some("https://auth.basilica.ai/oauth/revoke".to_string()),
            redirect_uri: "http://localhost:8080/callback".to_string(),
            scopes: vec![
                "openid".to_string(),
                "profile".to_string(),
                "offline_access".to_string(),
            ],
            additional_params: HashMap::new(),
        };

        let client = runtime.block_on(async {
            ClientBuilder::default()
                .base_url(base_url)
                .timeout(Duration::from_secs(timeout_secs))
                .with_auth_config(auth_config)
                .build_with_device_auth()
                .await
        })
        .map_err(|e| PyRuntimeError::new_err(format!("Device authentication failed: {}", e)))?;

        Ok(Self {
            inner: Arc::new(client),
            runtime,
        })
    }

    /// Create a new client with machine-to-machine authentication
    ///
    /// Args:
    ///     base_url: The base URL of the Basilica API
    ///     client_id: Auth0 M2M application client ID
    ///     client_secret: Auth0 M2M application client secret
    ///     timeout_secs: Request timeout in seconds (default: 30)
    #[staticmethod]
    #[pyo3(signature = (base_url, client_id, client_secret, timeout_secs=30))]
    fn with_m2m_auth(base_url: String, client_id: String, client_secret: String, timeout_secs: u64) -> PyResult<Self> {
        let runtime = Runtime::new()
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create runtime: {}", e)))?;

        let client = runtime.block_on(async {
            ClientBuilder::default()
                .base_url(base_url)
                .timeout(Duration::from_secs(timeout_secs))
                .build_with_m2m_auth(client_id, client_secret)
                .await
        })
        .map_err(|e| PyRuntimeError::new_err(format!("M2M authentication failed: {}", e)))?;

        Ok(Self {
            inner: Arc::new(client),
            runtime,
        })
    }
    
    /// Check the health of the API
    fn health_check(&self, py: Python) -> PyResult<PyObject> {
        let client = self.inner.clone();
        
        py.allow_threads(|| {
            self.runtime.block_on(async move {
                client.health_check().await
            })
        })
        .map_err(|e| PyRuntimeError::new_err(format!("Health check failed: {}", e)))
        .and_then(|response| {
            pythonize(py, &response)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to convert response: {}", e)))
        })
    }
    
    /// List available executors
    ///
    /// Args:
    ///     query: Optional query parameters as a dictionary
    fn list_executors(&self, py: Python, query: Option<&PyDict>) -> PyResult<PyObject> {
        let client = self.inner.clone();
        
        // Convert Python dict to query struct if provided
        let query = if let Some(dict) = query {
            Some(depythonize::<basilica_sdk::types::ListAvailableExecutorsQuery>(dict)
                .map_err(|e| PyValueError::new_err(format!("Invalid query parameters: {}", e)))?)
        } else {
            None
        };
        
        py.allow_threads(|| {
            self.runtime.block_on(async move {
                client.list_available_executors(query).await
            })
        })
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to list executors: {}", e)))
        .and_then(|response| {
            pythonize(py, &response)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to convert response: {}", e)))
        })
    }
    
    /// Start a new rental
    ///
    /// Args:
    ///     request: Rental request parameters as a dictionary
    fn start_rental(&self, py: Python, request: &PyDict) -> PyResult<PyObject> {
        let client = self.inner.clone();
        
        // Convert Python dict to StartRentalRequest
        let request = depythonize::<basilica_validator::api::rental_routes::StartRentalRequest>(request)
            .map_err(|e| PyValueError::new_err(format!("Invalid rental request: {}", e)))?;
        
        py.allow_threads(|| {
            self.runtime.block_on(async move {
                client.start_rental(request).await
            })
        })
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to start rental: {}", e)))
        .and_then(|response| {
            pythonize(py, &response)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to convert response: {}", e)))
        })
    }
    
    /// Get rental status
    ///
    /// Args:
    ///     rental_id: The rental ID
    fn get_rental(&self, py: Python, rental_id: String) -> PyResult<PyObject> {
        let client = self.inner.clone();
        
        py.allow_threads(|| {
            self.runtime.block_on(async move {
                client.get_rental_status(&rental_id).await
            })
        })
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to get rental: {}", e)))
        .and_then(|response| {
            pythonize(py, &response)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to convert response: {}", e)))
        })
    }
    
    /// Stop a rental
    ///
    /// Args:
    ///     rental_id: The rental ID
    fn stop_rental(&self, _py: Python, rental_id: String) -> PyResult<()> {
        let client = self.inner.clone();
        
        self.runtime.block_on(async move {
            client.stop_rental(&rental_id).await
        })
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to stop rental: {}", e)))
    }
    
    /// List rentals
    ///
    /// Args:
    ///     query: Optional query parameters as a dictionary
    fn list_rentals(&self, py: Python, query: Option<&PyDict>) -> PyResult<PyObject> {
        let client = self.inner.clone();
        
        // Convert Python dict to query struct if provided
        let query = if let Some(dict) = query {
            Some(depythonize::<basilica_sdk::types::ListRentalsQuery>(dict)
                .map_err(|e| PyValueError::new_err(format!("Invalid query parameters: {}", e)))?)
        } else {
            None
        };
        
        py.allow_threads(|| {
            self.runtime.block_on(async move {
                client.list_rentals(query).await
            })
        })
        .map_err(|e| PyRuntimeError::new_err(format!("Failed to list rentals: {}", e)))
        .and_then(|response| {
            pythonize(py, &response)
                .map_err(|e| PyRuntimeError::new_err(format!("Failed to convert response: {}", e)))
        })
    }
}

/// Helper function to create rental request
#[pyfunction]
#[pyo3(signature = (executor_id=None, container_image=None, ssh_public_key=None, **kwargs))]
fn create_rental_request(
    py: Python,
    executor_id: Option<String>,
    container_image: Option<String>,
    ssh_public_key: Option<String>,
    kwargs: Option<&PyDict>,
) -> PyResult<PyObject> {
    let dict = PyDict::new(py);
    
    if let Some(id) = executor_id {
        dict.set_item("executor_id", id)?;
    }
    if let Some(image) = container_image {
        dict.set_item("container_image", image)?;
    } else {
        dict.set_item("container_image", "ubuntu:latest")?;
    }
    if let Some(key) = ssh_public_key {
        dict.set_item("ssh_public_key", key)?;
    }
    
    // Add any additional kwargs
    if let Some(kw) = kwargs {
        for (key, value) in kw {
            dict.set_item(key, value)?;
        }
    }
    
    Ok(dict.into())
}

/// Python module for Basilica SDK
#[pymodule]
fn _basilica(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<BasilicaClient>()?;
    m.add_function(wrap_pyfunction!(create_rental_request, m)?)?;
    Ok(())
}