//! Interactive selection utilities

use crate::error::{CliError, Result};
use basilica_api::api::types::{ExecutorDetails, RentalStatusResponse};
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
    pub fn select_executor(&self, executors: &[ExecutorDetails]) -> Result<String> {
        if executors.is_empty() {
            return Err(CliError::not_found("No executors available"));
        }

        let items: Vec<String> = executors
            .iter()
            .map(|executor| {
                let gpu_info = if executor.gpu_specs.is_empty() {
                    "No GPUs".to_string()
                } else {
                    format!(
                        "{} x {} ({}GB each)",
                        executor.gpu_specs.len(),
                        executor.gpu_specs[0].name,
                        executor.gpu_specs[0].memory_gb
                    )
                };

                format!(
                    "{} - {} - CPU: {}c/{}GB{}",
                    executor.id,
                    gpu_info,
                    executor.cpu_specs.cores,
                    executor.cpu_specs.memory_gb,
                    executor
                        .location
                        .as_ref()
                        .map(|l| format!(" - {}", l))
                        .unwrap_or_default()
                )
            })
            .collect();

        let selection = Select::with_theme(&self.theme)
            .with_prompt("Select an executor")
            .items(&items)
            .default(0)
            .interact()
            .map_err(|e| CliError::interactive(format!("Selection failed: {}", e)))?;

        Ok(executors[selection].id.clone())
    }

    /// Let user select rentals for termination
    pub fn select_rentals_for_termination(
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
                    "{} - {:?} - {} - ${:.4}",
                    rental.rental_id, rental.status, rental.executor.id, rental.cost_incurred
                )
            })
            .collect();

        let selections = MultiSelect::with_theme(&self.theme)
            .with_prompt("Select rentals to terminate (Space to select, Enter to confirm)")
            .items(&items)
            .interact()
            .map_err(|e| CliError::interactive(format!("Selection failed: {}", e)))?;

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
            .map_err(|e| CliError::interactive(format!("Confirmation failed: {}", e)))?;

        Ok(confirmed)
    }
}

impl Default for InteractiveSelector {
    fn default() -> Self {
        Self::new()
    }
}
