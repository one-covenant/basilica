//! Integration tests for the Basilica SDK

use basilica_sdk::{ApiError, BasilicaClient, ClientBuilder};
use serde_json::json;
use std::time::Duration;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_client_creation() {
    let client = ClientBuilder::default()
        .base_url("https://api.basilica.ai")
        .timeout(Duration::from_secs(30))
        .build();
    
    assert!(client.is_ok());
}

#[tokio::test]
async fn test_client_with_auth() {
    let client = ClientBuilder::default()
        .base_url("https://api.basilica.ai")
        .with_bearer_token("test-token")
        .timeout(Duration::from_secs(30))
        .build();
    
    assert!(client.is_ok());
    let client = client.unwrap();
    assert_eq!(client.get_bearer_token(), Some("test-token"));
}

#[tokio::test]
async fn test_health_check_success() {
    let mock_server = MockServer::start().await;
    
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "status": "healthy",
            "version": "1.0.0",
            "timestamp": chrono::Utc::now(),
            "healthy_validators": 5,
            "total_validators": 5,
        })))
        .mount(&mock_server)
        .await;
    
    let client = BasilicaClient::new(mock_server.uri(), Duration::from_secs(30)).unwrap();
    let health = client.health_check().await.unwrap();
    
    assert_eq!(health.status, "healthy");
    assert_eq!(health.version, "1.0.0");
    assert_eq!(health.healthy_validators, 5);
}

#[tokio::test]
async fn test_authentication_required() {
    let mock_server = MockServer::start().await;
    
    Mock::given(method("GET"))
        .and(path("/rentals"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({
            "error": {
                "code": "BASILICA_API_AUTH_MISSING",
                "message": "Authentication token required",
                "timestamp": chrono::Utc::now(),
                "retryable": false,
            }
        })))
        .mount(&mock_server)
        .await;
    
    let client = BasilicaClient::new(mock_server.uri(), Duration::from_secs(30)).unwrap();
    let result = client.list_rentals(None).await;
    
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        ApiError::MissingAuthentication { .. }
    ));
}

#[tokio::test]
async fn test_rate_limit_exceeded() {
    let mock_server = MockServer::start().await;
    
    Mock::given(method("GET"))
        .and(path("/executors"))
        .respond_with(ResponseTemplate::new(429).set_body_json(json!({
            "error": {
                "code": "BASILICA_API_RATE_LIMIT",
                "message": "Rate limit exceeded",
                "timestamp": chrono::Utc::now(),
                "retryable": true,
            }
        })))
        .mount(&mock_server)
        .await;
    
    let client = ClientBuilder::default()
        .base_url(mock_server.uri())
        .with_bearer_token("test-token")
        .build()
        .unwrap();
    
    let result = client.list_available_executors(None).await;
    
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ApiError::RateLimitExceeded));
}

#[tokio::test]
async fn test_list_executors_with_auth() {
    let mock_server = MockServer::start().await;
    
    Mock::given(method("GET"))
        .and(path("/executors"))
        .and(header("Authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "available_executors": [],
            "total_count": 0,
        })))
        .mount(&mock_server)
        .await;
    
    let client = ClientBuilder::default()
        .base_url(mock_server.uri())
        .with_bearer_token("test-token")
        .build()
        .unwrap();
    
    let result = client.list_available_executors(None).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_not_found_error() {
    let mock_server = MockServer::start().await;
    
    Mock::given(method("GET"))
        .and(path("/rentals/nonexistent"))
        .respond_with(ResponseTemplate::new(404).set_body_json(json!({
            "error": {
                "code": "BASILICA_API_NOT_FOUND",
                "message": "Rental not found",
                "timestamp": chrono::Utc::now(),
                "retryable": false,
            }
        })))
        .mount(&mock_server)
        .await;
    
    let client = ClientBuilder::default()
        .base_url(mock_server.uri())
        .with_bearer_token("test-token")
        .build()
        .unwrap();
    
    let result = client.get_rental_status("nonexistent").await;
    
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ApiError::NotFound { .. }));
}

#[tokio::test]
async fn test_builder_configuration() {
    // Test that builder requires base_url
    let result = ClientBuilder::default().build();
    assert!(result.is_err());
    
    // Test with all options
    let client = ClientBuilder::default()
        .base_url("https://api.basilica.ai")
        .with_bearer_token("token")
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(50)
        .build();
    
    assert!(client.is_ok());
}

#[tokio::test]
async fn test_error_properties() {
    // Test retryable errors
    assert!(ApiError::Timeout.is_retryable());
    assert!(ApiError::ServiceUnavailable.is_retryable());
    assert!(!ApiError::Authentication {
        message: "Invalid token".to_string()
    }
    .is_retryable());
    
    // Test client errors
    assert!(ApiError::BadRequest {
        message: "Invalid input".to_string()
    }
    .is_client_error());
    assert!(ApiError::RateLimitExceeded.is_client_error());
    assert!(!ApiError::Internal {
        message: "Server error".to_string()
    }
    .is_client_error());
    
    // Test error codes
    assert_eq!(
        ApiError::RateLimitExceeded.error_code(),
        "BASILICA_API_RATE_LIMIT"
    );
    assert_eq!(
        ApiError::NotFound {
            resource: "rental".to_string()
        }
        .error_code(),
        "BASILICA_API_NOT_FOUND"
    );
}