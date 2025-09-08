//! Python type exposures for Basilica SDK responses
//!
//! This module provides PyO3 bindings for response types, enabling
//! direct attribute access with full IDE autocomplete support.

use basilica_sdk::types::{
    AvailabilityInfo as SdkAvailabilityInfo, AvailableExecutor as SdkAvailableExecutor,
    CpuSpec as SdkCpuSpec, ExecutorDetails as SdkExecutorDetails, GpuSpec as SdkGpuSpec,
    RentalStatus as SdkRentalStatus, RentalStatusWithSshResponse, SshAccess as SdkSshAccess,
    ValidatorRentalStatusResponse,
};
use basilica_validator::rental::RentalResponse as SdkRentalResponse;
use pyo3::prelude::*;

/// SSH access information for a rental
#[pyclass]
#[derive(Clone)]
pub struct SshAccess {
    #[pyo3(get)]
    pub host: String,
    #[pyo3(get)]
    pub port: u16,
    #[pyo3(get)]
    pub user: String,
}

impl From<SdkSshAccess> for SshAccess {
    fn from(ssh: SdkSshAccess) -> Self {
        Self {
            host: ssh.host,
            port: ssh.port,
            user: ssh.username,
        }
    }
}

/// GPU specification details
#[pyclass]
#[derive(Clone)]
pub struct GpuSpec {
    #[pyo3(get)]
    pub name: String,
    #[pyo3(get)]
    pub memory_gb: u32,
    #[pyo3(get)]
    pub compute_capability: String,
}

impl From<SdkGpuSpec> for GpuSpec {
    fn from(spec: SdkGpuSpec) -> Self {
        Self {
            name: spec.name,
            memory_gb: spec.memory_gb,
            compute_capability: spec.compute_capability,
        }
    }
}

/// CPU specification details
#[pyclass]
#[derive(Clone)]
pub struct CpuSpec {
    #[pyo3(get)]
    pub cores: u32,
    #[pyo3(get)]
    pub model: String,
    #[pyo3(get)]
    pub memory_gb: u32,
}

impl From<SdkCpuSpec> for CpuSpec {
    fn from(spec: SdkCpuSpec) -> Self {
        Self {
            cores: spec.cores,
            model: spec.model,
            memory_gb: spec.memory_gb,
        }
    }
}

/// Executor details including GPU and CPU specifications
#[pyclass]
#[derive(Clone)]
pub struct ExecutorDetails {
    #[pyo3(get)]
    pub id: String,
    #[pyo3(get)]
    pub gpu_specs: Vec<GpuSpec>,
    #[pyo3(get)]
    pub cpu_specs: CpuSpec,
    #[pyo3(get)]
    pub location: Option<String>,
}

impl From<SdkExecutorDetails> for ExecutorDetails {
    fn from(details: SdkExecutorDetails) -> Self {
        Self {
            id: details.id,
            gpu_specs: details.gpu_specs.into_iter().map(Into::into).collect(),
            cpu_specs: details.cpu_specs.into(),
            location: details.location,
        }
    }
}

/// Rental status enumeration
#[pyclass]
#[derive(Clone)]
pub struct RentalStatus {
    #[pyo3(get)]
    pub state: String,
    #[pyo3(get)]
    pub message: Option<String>,
}

impl From<SdkRentalStatus> for RentalStatus {
    fn from(status: SdkRentalStatus) -> Self {
        let state = match status {
            SdkRentalStatus::Pending => "Pending",
            SdkRentalStatus::Active => "Active",
            SdkRentalStatus::Terminated => "Terminated",
            SdkRentalStatus::Failed => "Failed",
        };

        Self {
            state: state.to_string(),
            message: None,
        }
    }
}

/// Response from starting a rental
#[pyclass]
#[derive(Clone)]
pub struct RentalResponse {
    #[pyo3(get)]
    pub rental_id: String,
    #[pyo3(get)]
    pub ssh_credentials: Option<String>,
    #[pyo3(get)]
    pub container_id: String,
    #[pyo3(get)]
    pub container_name: String,
    #[pyo3(get)]
    pub status: String,
}

impl From<SdkRentalResponse> for RentalResponse {
    fn from(response: SdkRentalResponse) -> Self {
        Self {
            rental_id: response.rental_id,
            ssh_credentials: response.ssh_credentials,
            container_id: response.container_info.container_id,
            container_name: response.container_info.container_name,
            status: response.container_info.status,
        }
    }
}

/// Full rental status response with SSH access
#[pyclass]
#[derive(Clone)]
pub struct RentalStatusResponse {
    #[pyo3(get)]
    pub rental_id: String,
    #[pyo3(get)]
    pub status: RentalStatus,
    #[pyo3(get)]
    pub executor: ExecutorDetails,
    #[pyo3(get)]
    pub ssh_credentials: Option<String>,
    #[pyo3(get)]
    pub ssh_access: Option<SshAccess>,
    #[pyo3(get)]
    pub created_at: String,
    #[pyo3(get)]
    pub updated_at: String,
}

impl From<RentalStatusWithSshResponse> for RentalStatusResponse {
    fn from(response: RentalStatusWithSshResponse) -> Self {
        // Parse SSH credentials to create SshAccess if available
        let ssh_access = response.ssh_credentials.as_ref().and_then(|creds| {
            // SSH credentials format: "ssh -p <port> <user>@<host>"
            let parts: Vec<&str> = creds.split_whitespace().collect();
            if parts.len() >= 5 && parts[0] == "ssh" && parts[1] == "-p" {
                let port = parts[2].parse().ok()?;
                let user_host = parts[3];
                if let Some((user, host)) = user_host.split_once('@') {
                    return Some(SshAccess {
                        host: host.to_string(),
                        port,
                        user: user.to_string(),
                    });
                }
            }
            None
        });

        Self {
            rental_id: response.rental_id,
            status: response.status.into(),
            executor: response.executor.into(),
            ssh_credentials: response.ssh_credentials,
            ssh_access,
            created_at: response.created_at.to_rfc3339(),
            updated_at: response.updated_at.to_rfc3339(),
        }
    }
}

impl From<ValidatorRentalStatusResponse> for RentalStatusResponse {
    fn from(response: ValidatorRentalStatusResponse) -> Self {
        Self {
            rental_id: response.rental_id,
            status: response.status.into(),
            executor: response.executor.into(),
            ssh_credentials: None,
            ssh_access: None,
            created_at: response.created_at.to_rfc3339(),
            updated_at: response.updated_at.to_rfc3339(),
        }
    }
}

/// Availability information for an executor
#[pyclass]
#[derive(Clone)]
pub struct AvailabilityInfo {
    #[pyo3(get)]
    pub available_until: Option<String>,
    #[pyo3(get)]
    pub verification_score: f64,
    #[pyo3(get)]
    pub uptime_percentage: f64,
}

impl From<SdkAvailabilityInfo> for AvailabilityInfo {
    fn from(info: SdkAvailabilityInfo) -> Self {
        Self {
            available_until: info.available_until.map(|dt| dt.to_rfc3339()),
            verification_score: info.verification_score,
            uptime_percentage: info.uptime_percentage,
        }
    }
}

/// Available executor with details and availability info
#[pyclass]
#[derive(Clone)]
pub struct AvailableExecutor {
    #[pyo3(get)]
    pub executor: ExecutorDetails,
    #[pyo3(get)]
    pub availability: AvailabilityInfo,
}

impl From<SdkAvailableExecutor> for AvailableExecutor {
    fn from(executor: SdkAvailableExecutor) -> Self {
        Self {
            executor: executor.executor.into(),
            availability: executor.availability.into(),
        }
    }
}

/// Health check response
#[pyclass]
#[derive(Clone)]
pub struct HealthCheckResponse {
    #[pyo3(get)]
    pub status: String,
    #[pyo3(get)]
    pub version: String,
    #[pyo3(get)]
    pub timestamp: String,
    #[pyo3(get)]
    pub healthy_validators: usize,
    #[pyo3(get)]
    pub total_validators: usize,
}

impl From<basilica_sdk::types::HealthCheckResponse> for HealthCheckResponse {
    fn from(response: basilica_sdk::types::HealthCheckResponse) -> Self {
        Self {
            status: response.status,
            version: response.version,
            timestamp: response.timestamp.to_rfc3339(),
            healthy_validators: response.healthy_validators,
            total_validators: response.total_validators,
        }
    }
}
