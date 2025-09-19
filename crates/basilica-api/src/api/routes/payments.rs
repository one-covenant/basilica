use crate::api::middleware::AuthContext;
use crate::error::{ApiError, Result};
use crate::server::AppState;
use axum::{
    extract::{Query, State},
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info};

#[derive(Debug, Serialize)]
pub struct DepositAccountResponse {
    pub user_id: String,
    pub address: String,
    pub exists: bool,
}

#[derive(Debug, Serialize)]
pub struct CreateDepositAccountResponse {
    pub user_id: String,
    pub address: String,
}

#[derive(Debug, Serialize)]
pub struct DepositRecord {
    pub tx_hash: String,
    pub block_number: u64,
    pub event_index: u32,
    pub from_address: String,
    pub to_address: String,
    pub amount_tao: String,
    pub status: String,
    pub observed_at: String,
    pub finalized_at: Option<String>,
    pub credited_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ListDepositsResponse {
    pub deposits: Vec<DepositRecord>,
    pub total_count: usize,
}

#[derive(Debug, Deserialize)]
pub struct ListDepositsQuery {
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

fn default_limit() -> u32 {
    50
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/deposit-account", post(create_deposit_account))
        .route("/deposit-account", get(get_deposit_account))
        .route("/deposits", get(list_deposits))
}

async fn create_deposit_account(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthContext>,
) -> Result<Json<CreateDepositAccountResponse>> {
    info!("Creating deposit account for user: {}", auth.user_id);

    let payments_client = state
        .payments_client
        .as_ref()
        .ok_or_else(|| ApiError::ServiceUnavailable)?;

    let response = payments_client
        .create_deposit_account(auth.user_id.clone())
        .await
        .map_err(|e| {
            error!("Failed to create deposit account: {}", e);
            ApiError::Internal {
                message: format!("Failed to create deposit account: {}", e),
            }
        })?;

    debug!(
        "Created deposit account with address: {}",
        response.address_ss58
    );

    Ok(Json(CreateDepositAccountResponse {
        user_id: response.user_id,
        address: response.address_ss58,
    }))
}

async fn get_deposit_account(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthContext>,
) -> Result<Json<DepositAccountResponse>> {
    debug!("Getting deposit account for user: {}", auth.user_id);

    let payments_client = state
        .payments_client
        .as_ref()
        .ok_or_else(|| ApiError::ServiceUnavailable)?;

    let response = payments_client
        .get_deposit_account(auth.user_id.clone())
        .await
        .map_err(|e| {
            error!("Failed to get deposit account: {}", e);
            ApiError::Internal {
                message: format!("Failed to get deposit account: {}", e),
            }
        })?;

    Ok(Json(DepositAccountResponse {
        user_id: response.user_id,
        address: response.address_ss58,
        exists: response.exists,
    }))
}

async fn list_deposits(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthContext>,
    Query(params): Query<ListDepositsQuery>,
) -> Result<Json<ListDepositsResponse>> {
    debug!(
        "Listing deposits for user: {}, limit: {}, offset: {}",
        auth.user_id, params.limit, params.offset
    );

    if params.limit > 100 {
        return Err(ApiError::BadRequest {
            message: "Limit cannot exceed 100".into(),
        });
    }

    let payments_client = state
        .payments_client
        .as_ref()
        .ok_or_else(|| ApiError::ServiceUnavailable)?;

    let response = payments_client
        .list_deposits(auth.user_id, Some(params.limit), Some(params.offset))
        .await
        .map_err(|e| {
            error!("Failed to list deposits: {}", e);
            ApiError::Internal {
                message: format!("Failed to list deposits: {}", e),
            }
        })?;

    let deposits: Vec<DepositRecord> = response
        .items
        .into_iter()
        .map(|item| {
            let amount_plancks: u128 = item.amount_plancks.parse().unwrap_or(0);
            let amount_tao = format!("{:.9}", amount_plancks as f64 / 1_000_000_000f64);

            DepositRecord {
                tx_hash: item.tx_hash,
                block_number: item.block_number,
                event_index: item.event_index,
                from_address: item.from_address,
                to_address: item.to_address,
                amount_tao,
                status: item.status,
                observed_at: item.observed_at,
                finalized_at: if item.finalized_at.is_empty() {
                    None
                } else {
                    Some(item.finalized_at)
                },
                credited_at: if item.credited_at.is_empty() {
                    None
                } else {
                    Some(item.credited_at)
                },
            }
        })
        .collect();

    let total_count = deposits.len();

    Ok(Json(ListDepositsResponse {
        deposits,
        total_count,
    }))
}
