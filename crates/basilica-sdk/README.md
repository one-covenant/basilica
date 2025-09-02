# Basilica SDK

Official Rust SDK for interacting with the Basilica GPU rental network.

## Overview

This SDK provides a type-safe, async Rust client for the Basilica API. It was extracted from the `basilica-api` crate to enable code reuse across multiple consumers:

- **basilica-api**: Re-exports the SDK for backward compatibility
- **basilica-cli**: Uses the SDK directly for all API interactions
- **basilica-sdk-python**: Python bindings built on top of this SDK

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
basilica-sdk = "0.1"
```

## Usage

### Basic Example

```rust
use basilica_sdk::{BasilicaClient, ClientBuilder};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create client with authentication
    let client = ClientBuilder::default()
        .base_url("https://api.basilica.ai")
        .with_bearer_token("your-auth-token")
        .timeout(Duration::from_secs(30))
        .build()?;
    
    // Check API health
    let health = client.health_check().await?;
    println!("API Status: {}", health.status);
    
    // List available executors
    let executors = client.list_available_executors(None).await?;
    println!("Found {} executors", executors.total_count);
    
    Ok(())
}
```

### Starting a Rental

```rust
use basilica_validator::api::rental_routes::StartRentalRequest;

let request = StartRentalRequest {
    executor_id: None,  // Let the system choose
    container_image: "nvidia/cuda:12.2.0-base-ubuntu22.04".to_string(),
    ssh_public_key: Some("ssh-rsa AAAAA...".to_string()),
    environment: Default::default(),
    ports: vec![],
    resources: Some(ResourceRequirementsRequest {
        gpu_count: Some(1),
        gpu_type: Some("h100".to_string()),
        ..Default::default()
    }),
    command: vec![],
    volumes: vec![],
    no_ssh: false,
};

let rental = client.start_rental(request).await?;
println!("Started rental: {}", rental.rental_id);
```

## API Reference

### ClientBuilder

```rust
ClientBuilder::default()
    .base_url(url)                    // Required: API base URL
    .with_bearer_token(token)         // Optional: Auth token
    .timeout(duration)                // Optional: Request timeout
    .connect_timeout(duration)        // Optional: Connection timeout
    .pool_max_idle_per_host(count)    // Optional: Connection pool size
    .build()
```

### BasilicaClient Methods

- `health_check()` - Check API health
- `list_available_executors(query)` - List available GPU executors
- `start_rental(request)` - Start a new GPU rental
- `get_rental_status(rental_id)` - Get rental status
- `stop_rental(rental_id)` - Stop a rental
- `list_rentals(query)` - List your rentals
- `get_rental_logs(rental_id, follow, tail)` - Stream rental logs

## Error Handling

The SDK uses a custom `ApiError` type that provides:

- Error categorization (client errors, server errors, network errors)
- Retry hints via `is_retryable()`
- Detailed error messages and codes

```rust
match client.start_rental(request).await {
    Ok(rental) => println!("Success: {}", rental.rental_id),
    Err(e) if e.is_retryable() => {
        // Retry the request
    }
    Err(e) if e.is_client_error() => {
        // Handle client error (bad request, auth, etc.)
    }
    Err(e) => {
        // Handle other errors
    }
}
```

## Features

- **Async/await support** - Built on tokio for async operations
- **Type safety** - Strongly typed request/response models
- **Error handling** - Comprehensive error types with retry hints
- **Authentication** - JWT Bearer token authentication
- **Configurable** - Timeouts, connection pooling, etc.

## Testing

Run tests with:

```bash
cargo test -p basilica-sdk
```

## License

MIT OR Apache-2.0