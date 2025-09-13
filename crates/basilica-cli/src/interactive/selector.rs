//! Interactive selection utilities

use crate::error::Result;
use basilica_sdk::types::{ApiRentalListItem, ExecutorSelection};
use basilica_sdk::GpuRequirements;
use basilica_validator::api::types::AvailableExecutor;
use basilica_validator::gpu::GpuCategory;
use color_eyre::eyre::eyre;
use console::Term;
use dialoguer::{theme::ColorfulTheme, Confirm, MultiSelect, Select};
use std::collections::HashMap;
use std::str::FromStr;

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

    /// Let user select an executor from available options
    pub fn select_executor(
        &self,
        executors: &[AvailableExecutor],
        detailed: bool,
    ) -> Result<ExecutorSelection> {
        if executors.is_empty() {
            return Err(eyre!("No executors available").into());
        }

        if detailed {
            // Detailed mode: Show all executors individually
            self.select_executor_detailed(executors)
        } else {
            // Grouped mode: Group by GPU configuration
            self.select_executor_grouped(executors)
        }
    }

    /// Select executor in detailed mode (show all executors)
    fn select_executor_detailed(
        &self,
        executors: &[AvailableExecutor],
    ) -> Result<ExecutorSelection> {
        // First pass: collect GPU info strings to determine max width
        let gpu_infos: Vec<String> = executors
            .iter()
            .map(|executor| {
                if executor.executor.gpu_specs.is_empty() {
                    "No GPUs".to_string()
                } else {
                    let gpu = &executor.executor.gpu_specs[0];
                    let gpu_display_name = gpu.name.clone(); // Full name in detailed mode
                    if executor.executor.gpu_specs.len() > 1 {
                        format!(
                            "{}x {}",
                            executor.executor.gpu_specs.len(),
                            gpu_display_name
                        )
                    } else {
                        format!("1x {}", gpu_display_name)
                    }
                }
            })
            .collect();

        // Calculate the maximum width needed for proper alignment
        let max_width = gpu_infos.iter().map(|s| s.len()).max().unwrap_or(30);
        let padding = max_width + 3;

        // Create items for the selector with GPU info and use cases
        let selector_items: Vec<String> = executors
            .iter()
            .zip(gpu_infos.iter())
            .map(|(executor, gpu_info)| {
                if executor.executor.gpu_specs.is_empty() {
                    format!(
                        "{:<width$} {}",
                        gpu_info,
                        "General GPU compute",
                        width = padding
                    )
                } else {
                    let gpu = &executor.executor.gpu_specs[0];
                    let use_case = GpuCategory::from_str(&gpu.name).unwrap().description();
                    format!("{:<width$} {}", gpu_info, use_case, width = padding)
                }
            })
            .collect();

        let selection = Select::with_theme(&self.theme)
            .with_prompt("Select executor")
            .items(&selector_items)
            .default(0)
            .interact_opt()
            .map_err(|e| eyre!("Selection failed: {}", e))?;

        let selection = match selection {
            Some(s) => s,
            None => return Err(eyre!("Selection cancelled").into()),
        };

        // Get the selected executor ID
        let executor_id = executors[selection].executor.id.clone();
        let executor_id = match executor_id.split_once("__") {
            Some((_, second)) => second.to_string(),
            None => executor_id,
        };

        Ok(ExecutorSelection::ExecutorId { executor_id })
    }

    /// Select executor in grouped mode (group by GPU configuration)
    fn select_executor_grouped(
        &self,
        executors: &[AvailableExecutor],
    ) -> Result<ExecutorSelection> {
        // Group executors by GPU configuration
        let mut gpu_groups: HashMap<String, (String, u32, u32)> = HashMap::new();

        for executor in executors {
            let key = if executor.executor.gpu_specs.is_empty() {
                "no_gpu".to_string()
            } else {
                let gpu = &executor.executor.gpu_specs[0];
                let category = GpuCategory::from_str(&gpu.name)
                    .unwrap_or(GpuCategory::Other(gpu.name.clone()));
                let gpu_count = executor.executor.gpu_specs.len() as u32;
                format!("{}_{}_{}", gpu_count, category, gpu.memory_gb)
            };

            gpu_groups.entry(key).or_insert_with(|| {
                if executor.executor.gpu_specs.is_empty() {
                    ("".to_string(), 0, 0)
                } else {
                    let gpu = &executor.executor.gpu_specs[0];
                    let category = GpuCategory::from_str(&gpu.name)
                        .unwrap_or(GpuCategory::Other(gpu.name.clone()));
                    let gpu_count = executor.executor.gpu_specs.len() as u32;
                    (category.to_string(), gpu_count, gpu.memory_gb)
                }
            });
        }

        // Create sorted list of unique GPU configurations
        let mut gpu_configs: Vec<(String, String, u32, u32)> = gpu_groups
            .into_iter()
            .map(|(key, (gpu_type, count, memory))| (key, gpu_type, count, memory))
            .collect();
        gpu_configs.sort_by(|a, b| {
            // Sort by GPU type, then count, then memory
            a.1.cmp(&b.1).then(a.2.cmp(&b.2)).then(a.3.cmp(&b.3))
        });

        // Create display items with GPU use case descriptions
        let selector_items: Vec<String> = gpu_configs
            .iter()
            .map(|(_, gpu_type, count, _memory)| {
                if gpu_type.is_empty() {
                    format!("{:<30} {}", "No GPUs", "General compute")
                } else {
                    let gpu_info = if *count > 1 {
                        format!("{}x {}", count, gpu_type)
                    } else {
                        format!("1x {}", gpu_type)
                    };
                    // Parse the category string directly to get the enum and its description
                    let category = GpuCategory::from_str(gpu_type)
                        .unwrap_or(GpuCategory::Other(gpu_type.to_string()));
                    let use_case = category.description();
                    format!("{:<30} {}", gpu_info, use_case)
                }
            })
            .collect();

        let selection = Select::with_theme(&self.theme)
            .with_prompt("Select GPU configuration")
            .items(&selector_items)
            .default(0)
            .interact_opt()
            .map_err(|e| eyre!("Selection failed: {}", e))?;

        let selection = match selection {
            Some(s) => s,
            None => return Err(eyre!("Selection cancelled").into()),
        };

        let selected_config = &gpu_configs[selection];

        // Use console crate to clear the previous line properly
        let term = Term::stdout();
        let _ = term.clear_last_lines(1);

        // Confirm selection
        let display_name = &selector_items[selection];
        let confirmed = Confirm::with_theme(&self.theme)
            .with_prompt(format!("Proceed with {}?", display_name))
            .default(true)
            .interact()
            .map_err(|e| eyre!("Confirmation failed: {}", e))?;

        if !confirmed {
            return Err(eyre!("Selection cancelled").into());
        }

        // Return GPU requirements for automatic selection
        if selected_config.1.is_empty() {
            // No GPU case - just pick the first available executor
            let executor_id = executors[0].executor.id.clone();
            let executor_id = match executor_id.split_once("__") {
                Some((_, second)) => second.to_string(),
                None => executor_id,
            };
            Ok(ExecutorSelection::ExecutorId { executor_id })
        } else {
            Ok(ExecutorSelection::GpuRequirements {
                gpu_requirements: GpuRequirements {
                    gpu_type: Some(selected_config.1.clone()),
                    gpu_count: selected_config.2,
                    min_memory_gb: 0, // We match exact memory from the selection
                },
            })
        }
    }

    /// Let user select a single instance from active instances
    pub fn select_rental(&self, rentals: &[ApiRentalListItem], detailed: bool) -> Result<String> {
        if rentals.is_empty() {
            return Err(eyre!("No active instances").into());
        }

        let items: Vec<String> = rentals
            .iter()
            .map(|rental| {
                // Format GPU info from specs
                let gpu = if rental.gpu_specs.is_empty() {
                    "Unknown GPU".to_string()
                } else {
                    let first_gpu = &rental.gpu_specs[0];
                    let all_same = rental
                        .gpu_specs
                        .iter()
                        .all(|g| g.name == first_gpu.name && g.memory_gb == first_gpu.memory_gb);

                    if all_same {
                        let gpu_display_name = if detailed {
                            first_gpu.name.clone()
                        } else {
                            let category = GpuCategory::from_str(&first_gpu.name)
                                .unwrap_or(GpuCategory::Other(first_gpu.name.clone()));
                            category.to_string()
                        };
                        if detailed {
                            // Detailed mode: show memory
                            if rental.gpu_specs.len() > 1 {
                                format!(
                                    "{}x {}",
                                    rental.gpu_specs.len(),
                                    gpu_display_name
                                )
                            } else {
                                format!("1x {}", gpu_display_name)
                            }
                        } else {
                            // Non-detailed mode: no memory
                            if rental.gpu_specs.len() > 1 {
                                format!("{}x {}", rental.gpu_specs.len(), gpu_display_name)
                            } else {
                                format!("1x {}", gpu_display_name)
                            }
                        }
                    } else {
                        rental
                            .gpu_specs
                            .iter()
                            .map(|g| {
                                let display_name = if detailed {
                                    g.name.clone()
                                } else {
                                    let category = GpuCategory::from_str(&g.name)
                                        .unwrap_or(GpuCategory::Other(g.name.clone()));
                                    category.to_string()
                                };
                                if detailed {
                                    display_name
                                } else {
                                    display_name
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                };

                // Format: "GPU Type    Container Image"
                format!("{:<30} {:<30}", gpu, rental.container_image)
            })
            .collect();

        let selection = Select::with_theme(&self.theme)
            .with_prompt("Select instance")
            .items(&items)
            .default(0)
            .interact_opt()
            .map_err(|e| eyre!("Selection failed: {}", e))?;

        let selection = match selection {
            Some(s) => s,
            None => return Err(eyre!("Selection cancelled").into()),
        };

        // Clear the selection prompt line
        let term = Term::stdout();
        let _ = term.clear_last_lines(1);

        Ok(rentals[selection].rental_id.clone())
    }

    /// Let user select instance items for termination
    pub fn select_rental_items_for_termination(
        &self,
        rentals: &[ApiRentalListItem],
    ) -> Result<Vec<String>> {
        if rentals.is_empty() {
            return Err(eyre!("No active instances").into());
        }

        let items: Vec<String> = rentals
            .iter()
            .map(|rental| {
                // Format GPU info from specs
                let gpu = if rental.gpu_specs.is_empty() {
                    "Unknown GPU".to_string()
                } else {
                    let first_gpu = &rental.gpu_specs[0];
                    let all_same = rental
                        .gpu_specs
                        .iter()
                        .all(|g| g.name == first_gpu.name && g.memory_gb == first_gpu.memory_gb);

                    if all_same {
                        format!(
                            "{}x {}",
                            rental.gpu_specs.len(),
                            first_gpu.name
                        )
                    } else {
                        rental
                            .gpu_specs
                            .iter()
                            .map(|g| g.name.clone())
                            .collect::<Vec<_>>()
                            .join(", ")
                    }
                };

                // Format consistently with select_rental
                format!("{:<30} {:<30}", gpu, rental.container_image)
            })
            .collect();

        let selections = MultiSelect::with_theme(&self.theme)
            .with_prompt("Select instances to terminate (Space to select, Enter to confirm)")
            .items(&items)
            .interact()
            .map_err(|e| eyre!("Selection failed: {}", e))?;

        if selections.is_empty() {
            return Err(eyre!("No instances selected").into());
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
            .map_err(|e| eyre!("Confirmation failed: {}", e))?;

        Ok(confirmed)
    }
}

impl Default for InteractiveSelector {
    fn default() -> Self {
        Self::new()
    }
}
