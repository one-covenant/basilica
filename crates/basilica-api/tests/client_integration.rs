//! Integration tests for the Basilica API client

use basilica_api::{api::types::*, BasilicaClient};

#[tokio::test]
#[ignore] // Run with --ignored flag for integration tests against a real server
async fn test_real_api_health() {
    let client = BasilicaClient::new("http://localhost:8080").unwrap();
    let health = client.health_check().await;
    assert!(health.is_ok());
}

#[tokio::test]
#[ignore]
async fn test_list_available_gpus() {
    let client = BasilicaClient::new("http://localhost:8080").unwrap();

    let query = ListExecutorsQuery {
        min_gpu_count: Some(1),
        gpu_type: None,
        page: None,
        page_size: None,
    };

    let response = client.list_available_gpus(query).await;
    assert!(response.is_ok());
}
