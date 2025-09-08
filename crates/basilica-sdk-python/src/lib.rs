//! Python bindings for the Basilica SDK
#![allow(clippy::useless_conversion)]

mod types;

use basilica_sdk::StartRentalApiRequest;
use basilica_sdk::{BasilicaClient as RustClient, ClientBuilder};
use pyo3::exceptions::{
    PyConnectionError, PyKeyError, PyPermissionError, PyRuntimeError, PyValueError,
};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use pythonize::{depythonize, pythonize};
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;

use crate::types::{AvailableExecutor, HealthCheckResponse, RentalResponse, RentalStatusResponse};

/// Python wrapper for BasilicaClient
#[pyclass]
struct BasilicaClient {
    inner: Arc<RustClient>,
    runtime: Runtime,
}

// Small helper to convert serializable Rust values into PyObject without
// re-wrapping PyErr (avoids clippy::useless_conversion).
fn to_pyobject<T: serde::Serialize>(py: Python<'_>, value: &T) -> PyResult<PyObject> {
    // `pythonize` already returns `PyResult<_>` so just propagate as-is.
    Ok(pythonize(py, value)?.unbind())
}

#[pymethods]
impl BasilicaClient {
    /// Create a new BasilicaClient
    ///
    /// Args:
    ///     base_url: The base URL of the Basilica API
    ///     token: Optional authentication token. If not provided, will try to use CLI tokens
    ///     timeout_secs: Request timeout in seconds (default: 30)
    ///     auto_auth: Automatically use CLI tokens if available (default: True)
    #[new]
    #[pyo3(signature = (base_url, token=None, timeout_secs=30, auto_auth=true))]
    fn new(
        base_url: String,
        token: Option<String>,
        timeout_secs: u64,
        auto_auth: bool,
    ) -> PyResult<Self> {
        let runtime = Runtime::new()
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create runtime: {}", e)))?;

        let client = runtime
            .block_on(async {
                let mut builder = ClientBuilder::default()
                    .base_url(base_url)
                    .timeout(Duration::from_secs(timeout_secs));

                if let Some(t) = token {
                    builder = builder.with_bearer_token(t);
                    builder.build()
                } else if auto_auth {
                    // Try automatic authentication from CLI tokens
                    builder.build_auto().await
                } else {
                    builder.build()
                }
            })
            .map_err(|e| PyRuntimeError::new_err(format!("Failed to create client: {}", e)))?;

        Ok(Self {
            inner: Arc::new(client),
            runtime,
        })
    }

    // Interactive authentication methods removed - Python SDK should use tokens
    // Users should authenticate via CLI first: `basilica login`
    // Then use the Python SDK with auto-detected tokens or provide token directly

    /// Check the health of the API
    fn health_check(&self, py: Python) -> PyResult<HealthCheckResponse> {
        let client = Arc::clone(&self.inner);

        let response = py
            .allow_threads(|| {
                self.runtime
                    .block_on(async move { client.health_check().await })
            })
            .map_err(|e| self.map_error_to_python(e))?;

        Ok(response.into())
    }

    /// List available executors
    ///
    /// Args:
    ///     query: Optional query parameters as a dictionary
    #[pyo3(signature = (query=None))]
    fn list_executors(
        &self,
        py: Python,
        query: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Vec<AvailableExecutor>> {
        let client = Arc::clone(&self.inner);

        // Convert Python dict to query struct if provided
        let query = if let Some(dict) = query {
            Some(
                depythonize::<basilica_sdk::types::ListAvailableExecutorsQuery>(dict.as_any())
                    .map_err(|e| {
                        PyValueError::new_err(format!("Invalid query parameters: {}", e))
                    })?,
            )
        } else {
            None
        };

        let response = py
            .allow_threads(|| {
                self.runtime
                    .block_on(async move { client.list_available_executors(query).await })
            })
            .map_err(|e| self.map_error_to_python(e))?;

        Ok(response
            .available_executors
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Start a new rental
    ///
    /// Args:
    ///     request: Rental request parameters as a dictionary
    fn start_rental(&self, py: Python, request: &Bound<'_, PyDict>) -> PyResult<RentalResponse> {
        let client = Arc::clone(&self.inner);

        // Convert Python dict to StartRentalRequest
        let request = depythonize::<StartRentalApiRequest>(request.as_any())
            .map_err(|e| PyValueError::new_err(format!("Invalid rental request: {}", e)))?;

        let response = py
            .allow_threads(|| {
                self.runtime
                    .block_on(async move { client.start_rental(request).await })
            })
            .map_err(|e| self.map_error_to_python(e))?;

        Ok(response.into())
    }

    /// Get rental status
    ///
    /// Args:
    ///     rental_id: The rental ID
    fn get_rental(&self, py: Python, rental_id: String) -> PyResult<RentalStatusResponse> {
        let client = Arc::clone(&self.inner);

        let response = py
            .allow_threads(|| {
                self.runtime
                    .block_on(async move { client.get_rental_status(&rental_id).await })
            })
            .map_err(|e| self.map_error_to_python(e))?;

        Ok(response.into())
    }

    /// Stop a rental
    ///
    /// Args:
    ///     rental_id: The rental ID
    fn stop_rental(&self, _py: Python, rental_id: String) -> PyResult<()> {
        let client = Arc::clone(&self.inner);

        self.runtime
            .block_on(async move { client.stop_rental(&rental_id).await })
            .map_err(|e| self.map_error_to_python(e))
    }

    /// List rentals
    ///
    /// Args:
    ///     query: Optional query parameters as a dictionary
    #[pyo3(signature = (query=None))]
    fn list_rentals(&self, py: Python, query: Option<&Bound<'_, PyDict>>) -> PyResult<PyObject> {
        let client = Arc::clone(&self.inner);

        // Convert Python dict to query struct if provided
        let query = if let Some(dict) = query {
            Some(
                depythonize::<basilica_sdk::types::ListRentalsQuery>(dict.as_any()).map_err(
                    |e| PyValueError::new_err(format!("Invalid query parameters: {}", e)),
                )?,
            )
        } else {
            None
        };

        let response = py
            .allow_threads(|| {
                self.runtime
                    .block_on(async move { client.list_rentals(query).await })
            })
            .map_err(|e| self.map_error_to_python(e))?;

        // Keep list_rentals as PyObject for now since it returns a complex structure
        to_pyobject(py, &response)
    }
}

impl BasilicaClient {
    /// Map Rust errors to appropriate Python exception types
    fn map_error_to_python(&self, error: basilica_sdk::ApiError) -> PyErr {
        use basilica_sdk::ApiError;

        match error {
            ApiError::InvalidRequest { message } => PyValueError::new_err(message),
            ApiError::NotFound { resource } => {
                PyKeyError::new_err(format!("Not found: {}", resource))
            }
            ApiError::Authentication { message } | ApiError::MissingAuthentication { message } => {
                PyPermissionError::new_err(message)
            }
            ApiError::Authorization { message } => PyPermissionError::new_err(message),
            ApiError::HttpClient(e) => PyConnectionError::new_err(e.to_string()),
            ApiError::BadRequest { message } => PyValueError::new_err(message),
            ApiError::Internal { message } => PyRuntimeError::new_err(message),
            _ => PyRuntimeError::new_err(error.to_string()),
        }
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
    kwargs: Option<&Bound<'_, PyDict>>,
) -> PyResult<PyObject> {
    let dict = PyDict::new_bound(py);

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
        for item in kw.items() {
            let (key, value): (String, PyObject) = item.extract()?;
            dict.set_item(key, value)?;
        }
    }

    Ok(dict.into_any().unbind())
}

/// Python module for Basilica SDK
#[pymodule]
fn _basilica(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Core client
    m.add_class::<BasilicaClient>()?;

    // Response types
    m.add_class::<types::HealthCheckResponse>()?;
    m.add_class::<types::RentalResponse>()?;
    m.add_class::<types::RentalStatusResponse>()?;
    m.add_class::<types::RentalStatus>()?;
    m.add_class::<types::SshAccess>()?;
    m.add_class::<types::ExecutorDetails>()?;
    m.add_class::<types::GpuSpec>()?;
    m.add_class::<types::CpuSpec>()?;
    m.add_class::<types::AvailableExecutor>()?;
    m.add_class::<types::AvailabilityInfo>()?;

    // Helper functions
    m.add_function(wrap_pyfunction!(create_rental_request, m)?)?;

    Ok(())
}
