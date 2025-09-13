//! Docker image reference validation utilities
//!
//! Provides security validation for Docker image references to prevent
//! command injection attacks when image names are used in shell commands.

use anyhow::{anyhow, Result};
use regex::Regex;

/// Validate Docker image reference to prevent command injection
///
/// Validates that a Docker image reference is safe to use in shell commands
/// by ensuring it matches the expected format and doesn't contain shell metacharacters.
///
/// # Valid formats
/// - `nginx` (simple name)
/// - `nginx:latest` (with tag)
/// - `nginx@sha256:abc123...` (with digest)
/// - `registry.com/namespace/image:tag` (full registry path)
/// - `registry.com:5000/namespace/image:tag@sha256:abc123` (with port and digest)
pub fn validate_docker_image_ref(image: &str) -> Result<()> {
    let docker_image_regex = Regex::new(
        r"^(?:[a-zA-Z0-9][a-zA-Z0-9\-_.]*(?:\.[a-zA-Z0-9][a-zA-Z0-9\-_.]*)*(:[0-9]+)?/)?(?:[a-z0-9]+(?:[._\-][a-z0-9]+)*/)*[a-z0-9]+(?:[._\-][a-z0-9]+)*(?::[a-zA-Z0-9_][a-zA-Z0-9._\-]{0,127})?(?:@sha256:[a-f0-9]{64})?$"
    ).unwrap();
    let dangerous_patterns = [
        ";", "|", "&", "`", "$", "(", ")", "{", "}", "[", "]", "<", ">", "\\", "'", "\"", "\n",
        "\r", "\t", "*", "?", "~",
    ];

    if image.is_empty() {
        return Err(anyhow!("Docker image reference cannot be empty"));
    }

    if image.len() > 500 {
        return Err(anyhow!(
            "Docker image reference is too long (max 500 characters)"
        ));
    }

    if !docker_image_regex.is_match(image) {
        return Err(anyhow!(
            "Invalid Docker image reference '{}'. Must match format: [registry[:port]/]namespace/name[:tag][@digest]",
            image
        ));
    }

    for pattern in &dangerous_patterns {
        if image.contains(pattern) {
            return Err(anyhow!(
                "Docker image reference contains forbidden character '{}' that could be used for command injection",
                pattern
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_docker_images() {
        // Test valid Docker image references
        assert!(validate_docker_image_ref("nginx").is_ok());
        assert!(validate_docker_image_ref("nginx:latest").is_ok());
        assert!(validate_docker_image_ref("nginx:1.21.3").is_ok());
        assert!(validate_docker_image_ref("library/nginx").is_ok());
        assert!(validate_docker_image_ref("docker.io/library/nginx").is_ok());
        assert!(validate_docker_image_ref("registry.example.com/namespace/image:tag").is_ok());
        assert!(validate_docker_image_ref("registry.example.com:5000/namespace/image:tag").is_ok());
        assert!(validate_docker_image_ref("nvidia/cuda:12.8.0-runtime-ubuntu22.04").is_ok());
        assert!(validate_docker_image_ref("gcr.io/project-id/image:tag").is_ok());
        assert!(validate_docker_image_ref(
            "image@sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
        )
        .is_ok());
    }

    #[test]
    fn test_invalid_docker_images() {
        // Test empty string
        assert!(validate_docker_image_ref("").is_err());

        // Test shell injection attempts
        assert!(validate_docker_image_ref("nginx; rm -rf /").is_err());
        assert!(validate_docker_image_ref("nginx | cat /etc/passwd").is_err());
        assert!(validate_docker_image_ref("nginx && echo hacked").is_err());
        assert!(validate_docker_image_ref("nginx`whoami`").is_err());
        assert!(validate_docker_image_ref("nginx$(whoami)").is_err());
        assert!(validate_docker_image_ref("nginx > /tmp/out").is_err());
        assert!(validate_docker_image_ref("nginx < /etc/passwd").is_err());
        assert!(validate_docker_image_ref("nginx\\nrm -rf /").is_err());
        assert!(validate_docker_image_ref("nginx\nrm -rf /").is_err());
        assert!(validate_docker_image_ref("nginx\trm -rf /").is_err());
        assert!(validate_docker_image_ref("nginx'test'").is_err());
        assert!(validate_docker_image_ref("nginx\"test\"").is_err());

        // Test excessively long strings
        let long_string = "a".repeat(501);
        assert!(validate_docker_image_ref(&long_string).is_err());
    }
}
