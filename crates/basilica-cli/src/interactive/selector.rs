//! Interactive selection utilities

use crate::error::{CliError, Result};
use basilica_api::api::types::RentalStatusResponse;
use basilica_validator::api::types::{AvailableExecutor, RentalListItem};
use dialoguer::{theme::ColorfulTheme, MultiSelect, Select};

/// Interactive selector for CLI operations
pub struct InteractiveSelector {
    theme: ColorfulTheme,
}

impl InteractiveSelector {
    /// Create a new interactive selector
    pub fn new() -> Self {
        Self {
            theme: ColorfulTheme::default(),
        }
    }

    /// Let user select an executor from available options
    pub fn select_executor(&self, executors: &[AvailableExecutor]) -> Result<String> {
        if executors.is_empty() {
            return Err(CliError::not_found("No executors available"));
        }

        let items: Vec<String> = executors
            .iter()
            .map(|executor| {
                let gpu_info = if executor.executor.gpu_specs.is_empty() {
                    "No GPUs".to_string()
                } else {
                    let gpu = &executor.executor.gpu_specs[0];
                    if executor.executor.gpu_specs.len() > 1 {
                        format!(
                            "{}x {} ({}GB)",
                            executor.executor.gpu_specs.len(),
                            gpu.name,
                            gpu.memory_gb
                        )
                    } else {
                        format!("{} ({}GB)", gpu.name, gpu.memory_gb)
                    }
                };

                format!(
                    "{} - {} - {} cores, {}GB RAM",
                    gpu_info,
                    executor.executor.id,
                    executor.executor.cpu_specs.cores,
                    executor.executor.cpu_specs.memory_gb
                )
            })
            .collect();

        let selection = Select::with_theme(&self.theme)
            .with_prompt("Select an executor")
            .items(&items)
            .default(0)
            .interact()
            .map_err(|e| CliError::interactive(format!("Selection failed: {e}")))?;

        let executor_id = executors[selection].executor.id.clone();

        // Remove miner prefix from executor ID if present
        let executor_id = match executor_id.split_once("__") {
            Some((_, second)) => second.to_string(),
            None => executor_id,
        };

        Ok(executor_id)
    }

    /// Let user select a single rental from active rentals
    pub fn select_rental(&self, rentals: &[RentalListItem]) -> Result<String> {
        use crate::cache::RentalCache;

        if rentals.is_empty() {
            return Err(CliError::not_found("No active rentals"));
        }

        // Load cache to get GPU info
        let cache = futures::executor::block_on(RentalCache::load()).unwrap_or_default();

        let items: Vec<String> = rentals
            .iter()
            .map(|rental| {
                // Try to get GPU info from cache
                let gpu = cache
                    .get_rental(&rental.rental_id)
                    .and_then(|cached| cached.gpu_info.clone())
                    .unwrap_or_else(|| "Unknown".to_string());

                format!(
                    "{} - {} - {} - {}",
                    gpu, rental.rental_id, rental.state, rental.container_image
                )
            })
            .collect();

        let selection = Select::with_theme(&self.theme)
            .with_prompt("Select a rental")
            .items(&items)
            .default(0)
            .interact()
            .map_err(|e| CliError::interactive(format!("Selection failed: {e}")))?;

        Ok(rentals[selection].rental_id.clone())
    }

    /// Let user select rentals for termination (legacy - for RentalStatusResponse)
    pub fn select_rentals_for_termination_legacy(
        &self,
        rentals: &[RentalStatusResponse],
    ) -> Result<Vec<String>> {
        if rentals.is_empty() {
            return Err(CliError::not_found("No active rentals"));
        }

        let items: Vec<String> = rentals
            .iter()
            .map(|rental| {
                format!(
                    "{} - {:?} - {}",
                    rental.rental_id, rental.status, rental.executor.id
                )
            })
            .collect();

        let selections = MultiSelect::with_theme(&self.theme)
            .with_prompt("Select rentals to terminate (Space to select, Enter to confirm)")
            .items(&items)
            .interact()
            .map_err(|e| CliError::interactive(format!("Selection failed: {e}")))?;

        if selections.is_empty() {
            return Err(CliError::interactive("No rentals selected"));
        }

        let selected_ids: Vec<String> = selections
            .into_iter()
            .map(|i| rentals[i].rental_id.clone())
            .collect();

        Ok(selected_ids)
    }

    /// Let user select rental items for termination
    pub fn select_rental_items_for_termination(
        &self,
        rentals: &[RentalListItem],
    ) -> Result<Vec<String>> {
        use crate::cache::RentalCache;

        if rentals.is_empty() {
            return Err(CliError::not_found("No active rentals"));
        }

        // Load cache to get GPU info
        let cache = futures::executor::block_on(RentalCache::load()).unwrap_or_default();

        let items: Vec<String> = rentals
            .iter()
            .map(|rental| {
                // Try to get GPU info from cache
                let gpu = cache
                    .get_rental(&rental.rental_id)
                    .and_then(|cached| cached.gpu_info.clone())
                    .unwrap_or_else(|| "Unknown".to_string());

                format!(
                    "{} - {} - {} - {}",
                    gpu, rental.rental_id, rental.state, rental.container_image
                )
            })
            .collect();

        let selections = MultiSelect::with_theme(&self.theme)
            .with_prompt("Select rentals to terminate (Space to select, Enter to confirm)")
            .items(&items)
            .interact()
            .map_err(|e| CliError::interactive(format!("Selection failed: {e}")))?;

        if selections.is_empty() {
            return Err(CliError::interactive("No rentals selected"));
        }

        let selected_ids: Vec<String> = selections
            .into_iter()
            .map(|i| rentals[i].rental_id.clone())
            .collect();

        Ok(selected_ids)
    }

    /// Confirm an action with yes/no prompt
    pub fn confirm(&self, message: &str) -> Result<bool> {
        let confirmed = dialoguer::Confirm::with_theme(&self.theme)
            .with_prompt(message)
            .default(false)
            .interact()
            .map_err(|e| CliError::interactive(format!("Confirmation failed: {e}")))?;

        Ok(confirmed)
    }
}

impl Default for InteractiveSelector {
    fn default() -> Self {
        Self::new()
    }
}
