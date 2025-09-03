//! Common helper functions for GPU rental operations

use crate::error::{CliError, Result};
use crate::progress::{complete_spinner_and_clear, complete_spinner_error, create_spinner};
use basilica_api::api::types::ListRentalsQuery;
use basilica_api::client::BasilicaClient;
use basilica_validator::rental::types::RentalState;

/// Resolve target rental ID - if not provided, fetch active rentals and prompt for selection
///
/// # Arguments
/// * `target` - Optional rental ID provided by user
/// * `api_client` - Authenticated API client
/// * `require_ssh` - If true, only show rentals with SSH access
pub async fn resolve_target_rental(
    target: Option<String>,
    api_client: &BasilicaClient,
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

    // Fetch active rentals
    let query = Some(ListRentalsQuery {
        status: Some(RentalState::Active),
        gpu_type: None,
        min_gpu_count: None,
    });

    let rentals_list = api_client.list_rentals(query).await.map_err(|e| {
        complete_spinner_error(spinner.clone(), "Failed to load rentals");
        CliError::api_request_failed("list rentals", e.to_string())
    })?;

    complete_spinner_and_clear(spinner);

    // Filter for SSH-enabled rentals if required
    let eligible_rentals = if require_ssh {
        rentals_list
            .rentals
            .into_iter()
            .filter(|r| r.has_ssh)
            .collect()
    } else {
        rentals_list.rentals
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
