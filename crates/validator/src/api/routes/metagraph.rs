use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde_json::Value;
use tracing::{error, info};

use crate::api::{types::ApiError, ApiState};

// Bittensor Integration
pub async fn get_metagraph(State(state): State<ApiState>) -> Result<Json<Value>, ApiError> {
    let bittensor_service = match &state.bittensor_service {
        Some(service) => service,
        None => {
            error!("Bittensor service not available");
            return Err(ApiError::InternalError(
                "Bittensor service not available".to_string(),
            ));
        }
    };

    let netuid = state.validator_config.bittensor.common.netuid;
    match bittensor_service.get_metagraph(netuid).await {
        Ok(metagraph) => {
            info!(
                "Successfully retrieved metagraph with {} neurons",
                metagraph.hotkeys.len()
            );

            // Convert metagraph to JSON
            let metagraph_json = serde_json::json!({
                "netuid": netuid,
                "total_neurons": metagraph.hotkeys.len(),
                "neurons": metagraph.hotkeys.iter().enumerate().map(|(uid, hotkey)| {
                    let is_validator = metagraph.validator_permit.get(uid).copied().unwrap_or(false);
                    let total_stake = metagraph.total_stake.get(uid).map(|s| s.0).unwrap_or(0);
                    let stake_tao = bittensor::rao_to_tao(total_stake);

                    let axon_info = metagraph.axons.get(uid);
                    let endpoint = if let Some(axon) = axon_info {
                        if axon.ip != 0 && axon.port != 0 {
                            let ip_str = if axon.ip_type == 4 {
                                let ipv4_bits = axon.ip as u32;
                                format!("{}.{}.{}.{}",
                                    (ipv4_bits >> 24) & 0xFF,
                                    (ipv4_bits >> 16) & 0xFF,
                                    (ipv4_bits >> 8) & 0xFF,
                                    ipv4_bits & 0xFF
                                )
                            } else {
                                format!("{}", axon.ip)
                            };
                            Some(format!("{}:{}", ip_str, axon.port))
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    serde_json::json!({
                        "uid": uid,
                        "hotkey": hotkey.to_string(),
                        "is_validator": is_validator,
                        "stake_tao": stake_tao,
                        "endpoint": endpoint,
                        "axon_info": axon_info.map(|axon| serde_json::json!({
                            "ip": axon.ip,
                            "port": axon.port,
                            "ip_type": axon.ip_type,
                            "version": axon.version
                        }))
                    })
                }).collect::<Vec<_>>()
            });

            Ok(Json(metagraph_json))
        }
        Err(e) => {
            error!("Failed to fetch metagraph: {}", e);
            Err(ApiError::InternalError(e.to_string()))
        }
    }
}

pub async fn get_metagraph_miner(
    State(state): State<ApiState>,
    Path(uid): Path<u16>,
) -> Result<Json<Value>, StatusCode> {
    let bittensor_service = match &state.bittensor_service {
        Some(service) => service,
        None => {
            error!("Bittensor service not available");
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
    };

    let netuid = state.validator_config.bittensor.common.netuid;

    match bittensor_service.get_metagraph(netuid).await {
        Ok(metagraph) => {
            if uid as usize >= metagraph.hotkeys.len() {
                return Err(StatusCode::NOT_FOUND);
            }

            let hotkey = &metagraph.hotkeys[uid as usize];
            let is_validator = metagraph
                .validator_permit
                .get(uid as usize)
                .copied()
                .unwrap_or(false);
            let total_stake = metagraph
                .total_stake
                .get(uid as usize)
                .map(|s| s.0)
                .unwrap_or(0);
            let stake_tao = bittensor::rao_to_tao(total_stake);

            let axon_info = metagraph.axons.get(uid as usize);
            let endpoint = if let Some(axon) = axon_info {
                if axon.ip != 0 && axon.port != 0 {
                    let ip_str = if axon.ip_type == 4 {
                        let ipv4_bits = axon.ip as u32;
                        format!(
                            "{}.{}.{}.{}",
                            (ipv4_bits >> 24) & 0xFF,
                            (ipv4_bits >> 16) & 0xFF,
                            (ipv4_bits >> 8) & 0xFF,
                            ipv4_bits & 0xFF
                        )
                    } else {
                        format!("{}", axon.ip)
                    };
                    Some(format!("{}:{}", ip_str, axon.port))
                } else {
                    None
                }
            } else {
                None
            };

            let miner_json = serde_json::json!({
                "uid": uid,
                "hotkey": hotkey.to_string(),
                "is_validator": is_validator,
                "stake_tao": stake_tao,
                "endpoint": endpoint,
                "axon_info": axon_info.map(|axon| serde_json::json!({
                    "ip": axon.ip,
                    "port": axon.port,
                    "ip_type": axon.ip_type,
                    "version": axon.version
                }))
            });

            Ok(Json(miner_json))
        }
        Err(e) => {
            error!("Failed to fetch metagraph for miner {}: {}", uid, e);
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
