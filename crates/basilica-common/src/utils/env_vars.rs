//! Environment variable parsing utilities
//!
//! This module provides utilities for parsing environment variable strings
//! in KEY=VALUE format, commonly used throughout Basilica for container configuration.

use anyhow::{anyhow, Result};
use std::collections::HashMap;

/// Parse environment variable strings into a HashMap
///
/// This function accepts environment variable specifications in KEY=VALUE format
/// and converts them into a HashMap for use with container configurations.
///
/// # Errors
///
/// This function will return an error if:
/// - Any string is not in KEY=VALUE format (missing '=' separator)
/// - A key is empty
///
/// # Examples
///
/// ```
/// use basilica_common::utils::parse_env_vars;
/// use std::collections::HashMap;
///
/// let vars = vec![
///     "DATABASE_URL=postgres://localhost".to_string(),
///     "PORT=8080".to_string(),
///     "DEBUG=true".to_string(),
/// ];
///
/// let result = parse_env_vars(&vars).unwrap();
/// assert_eq!(result.get("DATABASE_URL"), Some(&"postgres://localhost".to_string()));
/// assert_eq!(result.get("PORT"), Some(&"8080".to_string()));
/// assert_eq!(result.get("DEBUG"), Some(&"true".to_string()));
/// ```
pub fn parse_env_vars(env_vars: &[String]) -> Result<HashMap<String, String>> {
    let mut result = HashMap::new();

    for env_var in env_vars {
        // Use split_once to handle values that contain '=' characters
        if let Some((key, value)) = env_var.split_once('=') {
            // Validate that the key is not empty
            if key.is_empty() {
                return Err(anyhow!(
                    "Invalid environment variable format: '{}'. Key cannot be empty",
                    env_var
                ));
            }
            result.insert(key.to_string(), value.to_string());
        } else {
            return Err(anyhow!(
                "Invalid environment variable format: '{}'. Expected KEY=VALUE",
                env_var
            ));
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_env_vars() {
        let vars = vec![
            "KEY1=value1".to_string(),
            "KEY2=value2".to_string(),
            "KEY3=value3".to_string(),
        ];

        let result = parse_env_vars(&vars).unwrap();
        assert_eq!(result.len(), 3);
        assert_eq!(result.get("KEY1"), Some(&"value1".to_string()));
        assert_eq!(result.get("KEY2"), Some(&"value2".to_string()));
        assert_eq!(result.get("KEY3"), Some(&"value3".to_string()));
    }

    #[test]
    fn test_parse_env_var_with_equals_in_value() {
        let vars = vec![
            "DATABASE_URL=postgres://user:pass@host=localhost".to_string(),
            "EQUATION=x=y+2".to_string(),
        ];

        let result = parse_env_vars(&vars).unwrap();
        assert_eq!(
            result.get("DATABASE_URL"),
            Some(&"postgres://user:pass@host=localhost".to_string())
        );
        assert_eq!(result.get("EQUATION"), Some(&"x=y+2".to_string()));
    }

    #[test]
    fn test_parse_empty_value() {
        let vars = vec!["EMPTY_VAR=".to_string()];

        let result = parse_env_vars(&vars).unwrap();
        assert_eq!(result.get("EMPTY_VAR"), Some(&"".to_string()));
    }

    #[test]
    fn test_invalid_format_no_equals() {
        let vars = vec!["INVALID_VAR".to_string()];

        let result = parse_env_vars(&vars);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Expected KEY=VALUE"));
    }

    #[test]
    fn test_empty_key() {
        let vars = vec!["=value".to_string()];

        let result = parse_env_vars(&vars);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Key cannot be empty"));
    }

    #[test]
    fn test_empty_input() {
        let vars: Vec<String> = vec![];

        let result = parse_env_vars(&vars).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_whitespace_in_key_value() {
        let vars = vec![
            "KEY WITH SPACES=value with spaces".to_string(),
            "NORMAL_KEY=normal value".to_string(),
        ];

        let result = parse_env_vars(&vars).unwrap();
        assert_eq!(
            result.get("KEY WITH SPACES"),
            Some(&"value with spaces".to_string())
        );
        assert_eq!(result.get("NORMAL_KEY"), Some(&"normal value".to_string()));
    }
}
