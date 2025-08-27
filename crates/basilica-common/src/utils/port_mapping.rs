//! Port mapping utilities for container networking
//!
//! This module provides utilities for parsing and validating port mapping configurations
//! used throughout Basilica for GPU rental and container management.

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

/// A port mapping configuration between host and container
///
/// This struct represents a single port mapping that forwards traffic from a host port
/// to a container port using a specific protocol (TCP or UDP).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortMapping {
    /// The port on the host machine that will receive incoming connections
    pub host_port: u32,

    /// The port inside the container that traffic will be forwarded to
    pub container_port: u32,

    /// The network protocol to use ("tcp" or "udp")
    pub protocol: String,
}

/// Parse port mapping strings into structured PortMapping objects
///
/// This function accepts port mapping specifications in various formats and converts
/// them into validated PortMapping structs. It performs comprehensive validation
/// to ensure ports are within valid ranges and protocols are supported.
///
/// # Supported Formats
///
/// - `"host:container"` - Maps host port to container port using TCP (default)
/// - `"host:container:protocol"` - Explicitly specifies the protocol (tcp or udp)
///
/// # Errors
///
/// This function will return an error if:
/// - Port format is invalid (not enough or too many colons)
/// - Port numbers cannot be parsed as integers
/// - Host port is outside the valid range (0-65535, where 0 means random assignment)
/// - Container port is outside the valid range (1-65535, port 0 not supported by Docker)
/// - Protocol is not "tcp" or "udp" (case-insensitive)
///
/// # Examples
///
/// ```
/// use basilica_common::utils::parse_port_mappings;
///
/// // Simple TCP mapping (default protocol)
/// let mappings = parse_port_mappings(&["8080:80".to_string()])?;
/// assert_eq!(mappings[0].host_port, 8080);
/// assert_eq!(mappings[0].container_port, 80);
/// assert_eq!(mappings[0].protocol, "tcp");
///
/// // Explicit TCP mapping
/// let mappings = parse_port_mappings(&["3000:3000:tcp".to_string()])?;
/// assert_eq!(mappings[0].protocol, "tcp");
///
/// // UDP mapping
/// let mappings = parse_port_mappings(&["53:53:udp".to_string()])?;
/// assert_eq!(mappings[0].protocol, "udp");
///
/// // Multiple mappings
/// let mappings = parse_port_mappings(&[
///     "80:80".to_string(),
///     "443:443".to_string(),
///     "8080:8080:tcp".to_string(),
/// ])?;
/// assert_eq!(mappings.len(), 3);
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn parse_port_mappings(ports: &[String]) -> Result<Vec<PortMapping>> {
    let mut mappings = Vec::new();

    for port_str in ports {
        let parts: Vec<&str> = port_str.split(':').collect();

        // Validate format: must have 2 or 3 parts
        if parts.len() < 2 || parts.len() > 3 {
            return Err(anyhow!(
                "Invalid port mapping format: '{}'. Use 'host:container' or 'host:container:protocol'",
                port_str
            ));
        }

        // Parse host port
        let host_port = parts[0].parse::<u32>().with_context(|| {
            format!(
                "Invalid host port number '{}' in mapping '{}'",
                parts[0], port_str
            )
        })?;

        // Parse container port
        let container_port = parts[1].parse::<u32>().with_context(|| {
            format!(
                "Invalid container port number '{}' in mapping '{}'",
                parts[1], port_str
            )
        })?;

        // Validate port ranges
        // Host port: 0-65535 (port 0 lets the OS assign a random available port)
        // Container port: 1-65535 (port 0 is not supported by Docker for container ports)
        if host_port > 65535 {
            return Err(anyhow!(
                "Host port {} is out of valid range (0-65535) in mapping '{}'",
                host_port,
                port_str
            ));
        }
        if container_port == 0 || container_port > 65535 {
            return Err(anyhow!(
                "Container port {} is out of valid range (1-65535) in mapping '{}'",
                container_port,
                port_str
            ));
        }

        // Parse protocol (optional, defaults to TCP)
        let protocol = match parts.get(2) {
            Some(p) if p.to_lowercase() == "tcp" => "tcp".to_string(),
            Some(p) if p.to_lowercase() == "udp" => "udp".to_string(),
            Some(p) => {
                return Err(anyhow!(
                    "Invalid protocol '{}' in mapping '{}'. Only 'tcp' and 'udp' are supported",
                    p,
                    port_str
                ));
            }
            None => "tcp".to_string(), // Default to TCP
        };

        mappings.push(PortMapping {
            host_port,
            container_port,
            protocol,
        });
    }

    Ok(mappings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_tcp_mapping() {
        let result = parse_port_mappings(&["8080:80".to_string()]).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].host_port, 8080);
        assert_eq!(result[0].container_port, 80);
        assert_eq!(result[0].protocol, "tcp");
    }

    #[test]
    fn test_parse_explicit_tcp_mapping() {
        let result = parse_port_mappings(&["3000:3000:tcp".to_string()]).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].host_port, 3000);
        assert_eq!(result[0].container_port, 3000);
        assert_eq!(result[0].protocol, "tcp");
    }

    #[test]
    fn test_parse_udp_mapping() {
        let result = parse_port_mappings(&["53:53:udp".to_string()]).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].host_port, 53);
        assert_eq!(result[0].container_port, 53);
        assert_eq!(result[0].protocol, "udp");
    }

    #[test]
    fn test_parse_multiple_mappings() {
        let result = parse_port_mappings(&[
            "80:80".to_string(),
            "443:443:tcp".to_string(),
            "53:53:udp".to_string(),
        ])
        .unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].protocol, "tcp");
        assert_eq!(result[1].protocol, "tcp");
        assert_eq!(result[2].protocol, "udp");
    }

    #[test]
    fn test_invalid_format_too_few_parts() {
        let result = parse_port_mappings(&["8080".to_string()]);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid port mapping format"));
    }

    #[test]
    fn test_invalid_format_too_many_parts() {
        let result = parse_port_mappings(&["8080:80:tcp:extra".to_string()]);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid port mapping format"));
    }

    #[test]
    fn test_invalid_host_port() {
        let result = parse_port_mappings(&["not_a_number:80".to_string()]);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid host port number"));
    }

    #[test]
    fn test_invalid_container_port() {
        let result = parse_port_mappings(&["8080:not_a_number".to_string()]);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Invalid container port number"));
    }

    #[test]
    fn test_host_port_out_of_range() {
        let result = parse_port_mappings(&["70000:80".to_string()]);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("out of valid range"));
    }

    #[test]
    fn test_host_port_zero_is_valid() {
        // Host port 0 is valid - OS assigns random available port
        let result = parse_port_mappings(&["0:80".to_string()]);
        assert!(result.is_ok());
        let mappings = result.unwrap();
        assert_eq!(mappings[0].host_port, 0);
        assert_eq!(mappings[0].container_port, 80);
    }

    #[test]
    fn test_container_port_out_of_range() {
        let result = parse_port_mappings(&["80:70000".to_string()]);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("out of valid range"));
    }

    #[test]
    fn test_container_port_zero_is_invalid() {
        // Container port 0 is not supported by Docker
        let result = parse_port_mappings(&["80:0".to_string()]);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("out of valid range"));
    }

    #[test]
    fn test_invalid_protocol() {
        let result = parse_port_mappings(&["8080:80:sctp".to_string()]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid protocol"));
    }

    #[test]
    fn test_case_insensitive_protocol() {
        let result = parse_port_mappings(&["8080:80:TCP".to_string()]).unwrap();
        assert_eq!(result[0].protocol, "tcp");

        let result = parse_port_mappings(&["8080:80:UDP".to_string()]).unwrap();
        assert_eq!(result[0].protocol, "udp");
    }
}
