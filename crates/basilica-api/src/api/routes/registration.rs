//! Registration route handlers

use crate::{
    api::types::{CreditWalletResponse, RegisterRequest, RegisterResponse},
    error::{Error, Result},
    server::AppState,
};
use axum::{extract::State, Json};
use tracing::info;
use uuid::Uuid;

/// Register user and create account for credits
#[utoipa::path(
    post,
    path = "/api/v1/register",
    request_body = RegisterRequest,
    responses(
        (status = 201, description = "User registered successfully", body = RegisterResponse),
        (status = 400, description = "Invalid request", body = crate::error::ErrorResponse),
        (status = 409, description = "User already registered", body = crate::error::ErrorResponse),
    ),
    tag = "registration",
)]
pub async fn register_user(
    State(state): State<AppState>,
    Json(request): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>> {
    info!(
        "Processing registration for user: {}",
        request.user_identifier
    );

    // Check if user is already registered
    // In a real implementation, this would check a database
    if is_user_registered(&state, &request.user_identifier).await? {
        return Err(Error::BadRequest {
            message: "User already registered".to_string(),
        });
    }

    // Generate a new credit wallet address for this user
    // In production, this would create an actual wallet for holding credits
    let credit_wallet_address = generate_credit_wallet(&request.user_identifier).await?;

    // Store the registration in database
    store_user_registration(&state, &request.user_identifier, &credit_wallet_address).await?;

    info!(
        "Successfully registered user {} with credit wallet {}",
        request.user_identifier, credit_wallet_address
    );

    let response = RegisterResponse {
        success: true,
        credit_wallet_address,
        message: "User registered successfully".to_string(),
    };

    Ok(Json(response))
}

/// Get wallet address for registered user
#[utoipa::path(
    get,
    path = "/api/v1/register/wallet/{user_id}",
    params(
        ("user_id" = String, Path, description = "User identifier")
    ),
    responses(
        (status = 200, description = "Credit wallet address", body = CreditWalletResponse),
        (status = 404, description = "User not registered", body = crate::error::ErrorResponse),
    ),
    tag = "registration",
)]
pub async fn get_credit_wallet(
    State(state): State<AppState>,
    axum::extract::Path(user_id): axum::extract::Path<String>,
) -> Result<Json<CreditWalletResponse>> {
    info!("Getting credit wallet for user: {}", user_id);

    // Look up the user's credit wallet
    let credit_wallet_address =
        get_user_credit_wallet(&state, &user_id)
            .await?
            .ok_or_else(|| Error::NotFound {
                resource: format!("Registration for user {}", user_id),
            })?;

    let response = CreditWalletResponse {
        credit_wallet_address,
    };

    Ok(Json(response))
}

/// Check if user is already registered
async fn is_user_registered(_state: &AppState, user_identifier: &str) -> Result<bool> {
    // TODO: Implement database lookup
    // For now, simulate some users as already registered
    let registered_users = ["test-user@example.com", "demo-user@example.com"];

    Ok(registered_users.contains(&user_identifier))
}

/// Generate a new credit wallet for the user
async fn generate_credit_wallet(user_identifier: &str) -> Result<String> {
    // In production, this would create an actual wallet for holding credits
    // For now, generate a deterministic wallet address based on user identifier
    let uuid = Uuid::new_v4();
    let hash_suffix = format!("{:x}", user_identifier.len() * 31 + 42);
    let credit_wallet = format!("5Credit{}{}", &hash_suffix[0..8], &uuid.to_string()[0..8]);

    Ok(credit_wallet)
}

/// Store user registration in database
async fn store_user_registration(
    _state: &AppState,
    user_identifier: &str,
    credit_wallet: &str,
) -> Result<()> {
    // TODO: Implement database storage
    info!(
        "Storing registration: user={}, credit={}",
        user_identifier, credit_wallet
    );
    Ok(())
}

/// Get user's credit wallet from database
async fn get_user_credit_wallet(
    _state: &AppState,
    user_identifier: &str,
) -> Result<Option<String>> {
    // TODO: Implement database lookup
    // For now, simulate some existing registrations
    let existing_registrations = [
        ("test-user@example.com", "5Credit26Fz9rcQ12345678"),
        ("demo-user@example.com", "5CreditW46xGXgs87654321"),
    ];

    for (user, credit) in existing_registrations.iter() {
        if *user == user_identifier {
            return Ok(Some(credit.to_string()));
        }
    }

    Ok(None)
}
