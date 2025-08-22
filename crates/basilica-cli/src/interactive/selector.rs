//! Interactive selection utilities

use crate::error::{CliError, Result};
use basilica_api::api::types::RentalStatusResponse;
use basilica_validator::api::types::{AvailableExecutor, RentalListItem};
use console::Term;
use dialoguer::{theme::ColorfulTheme, Confirm, MultiSelect, Select};

/// Interactive selector for CLI operations
pub struct InteractiveSelector {
    theme: ColorfulTheme,
}

impl InteractiveSelector {
    /// Create a new interactive selector
    pub fn new() -> Self {
        // Create a customized theme for better display
        let theme = ColorfulTheme::default();
        // The theme already has good defaults, we can customize if needed
        Self { theme }
    }

    /// Get GPU use case description based on GPU model
    fn get_gpu_use_case(gpu_name: &str) -> &'static str {
        match gpu_name {
            name if name.contains("H100") => "High-end training & inference",
            name if name.contains("H200") => "High-end training & inference",
            name if name.contains("A100") => "Training & large model inference",
            name if name.contains("RTX 4090") => "Development & prototyping",
            name if name.contains("RTX 4080") => "Development & prototyping",
            _ => "General GPU compute",
        }
    }

    /// Let user select an executor from available options
    pub fn select_executor(&self, executors: &[AvailableExecutor]) -> Result<String> {
        if executors.is_empty() {
            return Err(CliError::not_found("No executors available"));
        }

        // First pass: collect GPU info strings to determine max width
        let gpu_infos: Vec<String> = executors
            .iter()
            .map(|executor| {
                if executor.executor.gpu_specs.is_empty() {
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
                }
            })
            .collect();

        // Calculate the maximum width needed for proper alignment
        let max_width = gpu_infos.iter().map(|s| s.len()).max().unwrap_or(30);
        let padding = max_width + 3; // Add some padding for better visual separation

        // Create items for the selector with GPU info and use cases
        let selector_items: Vec<String> = executors
            .iter()
            .zip(gpu_infos.iter())
            .map(|(executor, gpu_info)| {
                if executor.executor.gpu_specs.is_empty() {
                    format!("{:<width$} {}", gpu_info, "General GPU compute", width = padding)
                } else {
                    let gpu = &executor.executor.gpu_specs[0];
                    let use_case = Self::get_gpu_use_case(&gpu.name);
                    format!("{:<width$} {}", gpu_info, use_case, width = padding)
                }
            })
            .collect();

        let selection = Select::with_theme(&self.theme)
            .with_prompt("Select GPU configuration")
            .items(&selector_items)
            .default(0)
            .interact_opt()
            .map_err(|e| CliError::interactive(format!("Selection failed: {e}")))?;

        let selection = match selection {
            Some(s) => s,
            None => return Err(CliError::interactive("Selection cancelled")),
        };

        // Get the selected GPU info for confirmation
        let selected_gpu = if executors[selection].executor.gpu_specs.is_empty() {
            "No GPUs".to_string()
        } else {
            let gpu = &executors[selection].executor.gpu_specs[0];
            if executors[selection].executor.gpu_specs.len() > 1 {
                format!(
                    "{}x {} ({}GB)",
                    executors[selection].executor.gpu_specs.len(),
                    gpu.name,
                    gpu.memory_gb
                )
            } else {
                format!("{} ({}GB)", gpu.name, gpu.memory_gb)
            }
        };

        // Use console crate to clear the previous line properly
        let term = Term::stdout();
        let _ = term.clear_last_lines(1); // Clear the selection prompt line

        // Use dialoguer's Confirm with the same theme for consistency
        let confirmed = Confirm::with_theme(&self.theme)
            .with_prompt(&format!("Proceed with {}?", selected_gpu))
            .default(true) // Default to yes for better UX
            .interact()
            .map_err(|e| CliError::interactive(format!("Confirmation failed: {e}")))?;

        if !confirmed {
            return Err(CliError::interactive("Selection cancelled"));
        }

        let executor_id = executors[selection].executor.id.clone();

        // Remove miner prefix from executor ID if present
        let executor_id = match executor_id.split_once("__") {
            Some((_, second)) => second.to_string(),
            None => executor_id,
        };

        Ok(executor_id)
    }

    /// Let user select a single instance from active instances
    pub fn select_rental(&self, rentals: &[RentalListItem]) -> Result<String> {
        use crate::cache::RentalCache;

        if rentals.is_empty() {
            return Err(CliError::not_found("No active instances"));
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
                    .unwrap_or_else(|| "Unknown GPU".to_string());

                // Format: "GPU Type    Container Image"
                format!("{:<30} {}", gpu, rental.container_image)
            })
            .collect();

        let selection = Select::with_theme(&self.theme)
            .with_prompt("Select instance")
            .items(&items)
            .default(0)
            .interact_opt()
            .map_err(|e| CliError::interactive(format!("Selection failed: {e}")))?;

        let selection = match selection {
            Some(s) => s,
            None => return Err(CliError::interactive("Selection cancelled")),
        };

        // Clear the selection prompt line
        let term = Term::stdout();
        let _ = term.clear_last_lines(1);

        Ok(rentals[selection].rental_id.clone())
    }

    /// Let user select instances for termination (legacy - for RentalStatusResponse)
    pub fn select_rentals_for_termination_legacy(
        &self,
        rentals: &[RentalStatusResponse],
    ) -> Result<Vec<String>> {
        if rentals.is_empty() {
            return Err(CliError::not_found("No active instances"));
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
            .with_prompt("Select instances to terminate (Space to select, Enter to confirm)")
            .items(&items)
            .interact()
            .map_err(|e| CliError::interactive(format!("Selection failed: {e}")))?;

        if selections.is_empty() {
            return Err(CliError::interactive("No instances selected"));
        }

        let selected_ids: Vec<String> = selections
            .into_iter()
            .map(|i| rentals[i].rental_id.clone())
            .collect();

        Ok(selected_ids)
    }

    /// Let user select instance items for termination
    pub fn select_rental_items_for_termination(
        &self,
        rentals: &[RentalListItem],
    ) -> Result<Vec<String>> {
        use crate::cache::RentalCache;

        if rentals.is_empty() {
            return Err(CliError::not_found("No active instances"));
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
                    .unwrap_or_else(|| "Unknown GPU".to_string());

                // Format consistently with select_rental
                format!("{:<30} {}", gpu, rental.container_image)
            })
            .collect();

        let selections = MultiSelect::with_theme(&self.theme)
            .with_prompt("Select instances to terminate (Space to select, Enter to confirm)")
            .items(&items)
            .interact()
            .map_err(|e| CliError::interactive(format!("Selection failed: {e}")))?;

        if selections.is_empty() {
            return Err(CliError::interactive("No instances selected"));
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
