//! Common helper functions for GPU rental operations

use crate::cache::RentalCache;
use crate::error::{CliError, Result};
use crate::progress::{complete_spinner_and_clear, complete_spinner_error, create_spinner};
use basilica_api::models::rental::RentalFilters;
use basilica_api::services::ServiceClient;
use basilica_validator::api::types::RentalListItem;
use basilica_validator::rental::types::RentalState;


/// Get SSH credentials from cache for a rental
///
/// # Arguments
/// * `target` - Rental ID to look up
/// * `cache` - Rental cache instance
pub fn get_ssh_credentials_from_cache(target: &str, cache: &RentalCache) -> Result<String> {
    let cached_rental = cache.get_rental(target).ok_or_else(|| {
        CliError::rental_not_found(target)
            .with_context("SSH credentials are only available for rentals created in this session")
    })?;

    cached_rental.ssh_credentials.clone().ok_or_else(|| {
        CliError::not_supported(
            "This rental does not have SSH access. Container was created without SSH port mapping.",
        )
    })
}

/// Filter rentals to only include those with SSH credentials in cache
///
/// # Arguments
/// * `rentals` - List of rentals to filter
/// * `cache` - Rental cache instance
pub fn filter_rentals_with_ssh(
    rentals: Vec<RentalListItem>,
    cache: &RentalCache,
) -> Vec<RentalListItem> {
    // Get all cached rentals that have SSH credentials
    let ssh_rentals: Vec<String> = cache
        .list_rentals()
        .into_iter()
        .filter_map(|r| {
            if r.ssh_credentials.is_some() {
                Some(r.rental_id.clone())
            } else {
                None
            }
        })
        .collect();

    // Filter to only show rentals with SSH access
    rentals
        .into_iter()
        .filter(|r| ssh_rentals.contains(&r.rental_id))
        .collect()
}

/// Resolve target rental ID using ServiceClient - if not provided, fetch active rentals and prompt for selection
///
/// # Arguments
/// * `target` - Optional rental ID provided by user
/// * `service_client` - Service client instance
/// * `require_ssh` - If true, only show rentals with SSH access
pub async fn resolve_target_rental_with_service(
    target: Option<String>,
    service_client: &ServiceClient,
    require_ssh: bool,
) -> Result<String> {
    if let Some(t) = target {
        return Ok(t);
    }

    let spinner = if require_ssh {
        create_spinner("Fetching rentals with SSH access...")
    } else {
        create_spinner("Fetching active rentals...")
    };

    // Fetch active rentals using ServiceClient
    let rental_filters = RentalFilters {
        status: Some(basilica_api::models::rental::RentalStatus::Active),
        ..Default::default()
    };

    let rentals = service_client.list_rentals(rental_filters).await.map_err(|e| {
        complete_spinner_error(spinner.clone(), "Failed to load rentals");
        CliError::api_request_failed("list rentals", e.to_string())
    })?;

    complete_spinner_and_clear(spinner);

    // Convert to RentalListItem format for compatibility with selector
    let rental_items: Vec<RentalListItem> = rentals
        .into_iter()
        .map(|r| RentalListItem {
            rental_id: r.id,
            executor_id: r.executor_id,
            container_id: r.deployment_id.unwrap_or_else(|| format!("rental-{}", uuid::Uuid::new_v4())),
            state: match r.status {
                basilica_api::models::rental::RentalStatus::Active => RentalState::Active,
                basilica_api::models::rental::RentalStatus::Pending => RentalState::Provisioning,
                basilica_api::models::rental::RentalStatus::Terminated => RentalState::Stopped,
                _ => RentalState::Failed,
            },
            created_at: r.created_at.to_rfc3339(),
            miner_id: "0".to_string(), // Default miner ID
            container_image: "unknown".to_string(), // We don't have this info in the rental model
        })
        .collect();

    // Filter for SSH-enabled rentals if required
    let eligible_rentals = if require_ssh {
        let cache = RentalCache::load().await?;
        filter_rentals_with_ssh(rental_items, &cache)
    } else {
        rental_items
    };

    if eligible_rentals.is_empty() {
        return if require_ssh {
            Err(
                CliError::not_found("No rentals with SSH access found").with_context(
                    "SSH credentials are only available for rentals created in this session",
                ),
            )
        } else {
            Err(CliError::not_found("No active rentals found"))
        };
    }

    // Use interactive selector to choose a rental
    let selector = crate::interactive::InteractiveSelector::new();
    selector.select_rental(&eligible_rentals)
}
