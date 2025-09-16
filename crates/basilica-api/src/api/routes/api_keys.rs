//! API Key management route handlers

use crate::{
    api::{
        auth::api_keys::{self},
        middleware::AuthContext,
    },
    error::{ApiError, Result},
    server::AppState,
};
use axum::{extract::State, Json};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, instrument, warn};
use utoipa::ToSchema;

/// Request to create a new API key
#[derive(Debug, Deserialize, ToSchema)]
pub struct CreateKeyRequest {
    /// Name for the API key
    pub name: String,

    /// Optional scopes for the API key
    pub scopes: Option<Vec<String>>,
}

/// Response after creating a new API key
#[derive(Debug, Serialize, ToSchema)]
pub struct CreateKeyResponse {
    /// Name of the key
    pub name: String,

    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// The full API key token (only returned once at creation)
    pub token: String,
}

/// List item for API keys
#[derive(Debug, Serialize, ToSchema)]
pub struct ListKeyItem {
    /// Key identifier (kid)
    pub kid: String,

    /// Name of the key
    pub name: String,

    /// Creation timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,

    /// Last usage timestamp
    pub last_used_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Create a new API key
///
/// This endpoint requires JWT authentication (human users only).
/// Only one active key per user is allowed.
#[utoipa::path(
    post,
    path = "/api-keys",
    request_body = CreateKeyRequest,
    responses(
        (status = 200, description = "API key created successfully", body = CreateKeyResponse),
        (status = 400, description = "Bad request"),
        (status = 401, description = "Unauthorized - JWT authentication required"),
        (status = 409, description = "Conflict - active key already exists"),
        (status = 500, description = "Internal server error")
    ),
    tag = "api-keys",
    security(
        ("bearer_auth" = ["keys:create"])
    )
)]
#[instrument(skip(state, auth_context, request), fields(user_id = %auth_context.user_id, key_name = %request.name))]
pub async fn create_key(
    State(state): State<AppState>,
    axum::Extension(auth_context): axum::Extension<AuthContext>,
    Json(request): Json<CreateKeyRequest>,
) -> Result<Json<CreateKeyResponse>> {
    // Require JWT authentication for key management
    if !auth_context.is_jwt() {
        warn!(
            "User {} attempted to create API key with non-JWT auth",
            auth_context.user_id
        );
        return Err(ApiError::Authentication {
            message: "API key management requires human authentication (JWT)".to_string(),
        });
    }

    info!("Creating API key");

    // Check if user already has a key (only one allowed per user)
    let existing_keys = api_keys::list_user_api_keys(&state.db, &auth_context.user_id)
        .await
        .map_err(|e| ApiError::Internal {
            message: format!("Failed to check existing keys: {}", e),
        })?;

    if !existing_keys.is_empty() {
        return Err(ApiError::Conflict {
            message: "User already has an API key. Please delete it first.".to_string(),
        });
    }

    // Generate new API key
    let generated = api_keys::gen_api_key().map_err(|e| ApiError::Internal {
        message: format!("Failed to generate API key: {}", e),
    })?;

    // Use provided scopes or default to user's current scopes
    let scopes = request
        .scopes
        .unwrap_or_else(|| auth_context.scopes.clone());

    // Store in database
    let key = api_keys::insert_api_key(
        &state.db,
        &auth_context.user_id,
        &request.name,
        &generated.kid,
        &generated.hash,
        &scopes,
    )
    .await
    .map_err(|e| ApiError::Internal {
        message: format!("Failed to store API key: {}", e),
    })?;

    debug!("Successfully created API key");

    Ok(Json(CreateKeyResponse {
        name: key.name,
        created_at: key.created_at,
        token: generated.display_token,
    }))
}

/// List all API keys for the authenticated user
///
/// This endpoint requires JWT authentication (human users only).
#[utoipa::path(
    get,
    path = "/api-keys",
    responses(
        (status = 200, description = "List of API keys", body = Vec<ListKeyItem>),
        (status = 401, description = "Unauthorized - JWT authentication required"),
        (status = 500, description = "Internal server error")
    ),
    tag = "api-keys",
    security(
        ("bearer_auth" = ["keys:list"])
    )
)]
#[instrument(skip(state, auth_context), fields(user_id = %auth_context.user_id))]
pub async fn list_keys(
    State(state): State<AppState>,
    axum::Extension(auth_context): axum::Extension<AuthContext>,
) -> Result<Json<Vec<ListKeyItem>>> {
    // Require JWT authentication for key management
    if !auth_context.is_jwt() {
        warn!(
            "User {} attempted to list API keys with non-JWT auth",
            auth_context.user_id
        );
        return Err(ApiError::Authentication {
            message: "API key management requires human authentication (JWT)".to_string(),
        });
    }

    debug!("Listing API keys");

    let keys = api_keys::list_user_api_keys(&state.db, &auth_context.user_id)
        .await
        .map_err(|e| ApiError::Internal {
            message: format!("Failed to list API keys: {}", e),
        })?;

    let items: Vec<ListKeyItem> = keys
        .into_iter()
        .map(|key| ListKeyItem {
            kid: key.kid.clone(),
            name: key.name,
            created_at: key.created_at,
            last_used_at: key.last_used_at,
        })
        .collect();

    debug!("Found {} API keys", items.len());

    Ok(Json(items))
}

/// Delete an API key
///
/// This endpoint requires JWT authentication (human users only).
#[utoipa::path(
    delete,
    path = "/api-keys",
    responses(
        (status = 204, description = "API key deleted successfully"),
        (status = 401, description = "Unauthorized - JWT authentication required"),
        (status = 404, description = "API key not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "api-keys",
    security(
        ("bearer_auth" = ["keys:revoke"])
    )
)]
#[instrument(skip(state, auth_context), fields(user_id = %auth_context.user_id))]
pub async fn revoke_key(
    State(state): State<AppState>,
    axum::Extension(auth_context): axum::Extension<AuthContext>,
) -> Result<()> {
    // Require JWT authentication for key management
    if !auth_context.is_jwt() {
        warn!(
            "User {} attempted to delete API key with non-JWT auth",
            auth_context.user_id
        );
        return Err(ApiError::Authentication {
            message: "API key management requires human authentication (JWT)".to_string(),
        });
    }

    info!("Deleting API key");

    let deleted = api_keys::delete_api_key_by_user_id(&state.db, &auth_context.user_id)
        .await
        .map_err(|e| ApiError::Internal {
            message: format!("Failed to delete API key: {}", e),
        })?;

    if !deleted {
        return Err(ApiError::NotFound {
            message: "API key not found".to_string(),
        });
    }

    debug!("Successfully deleted API key");

    Ok(())
}
