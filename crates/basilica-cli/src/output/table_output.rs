//! Table formatting for CLI output

use crate::error::Result;
use basilica_sdk::{
    types::{ApiRentalListItem, ExecutorDetails, RentalStatusResponse},
    AvailableExecutor,
};
use basilica_validator::gpu::GpuCategory;
use chrono::{DateTime, Local};
use std::{collections::HashMap, str::FromStr};
use tabled::{settings::Style, Table, Tabled};

/// Format RFC3339 timestamp to YY-MM-DD HH:MM:SS format
fn format_timestamp(timestamp: &str) -> String {
    DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|dt| {
            let local_dt = dt.with_timezone(&Local);
            local_dt.format("%y-%m-%d %H:%M:%S").to_string()
        })
        .unwrap_or_else(|| timestamp.to_string())
}

/// Display executors in table format
pub fn display_executors(executors: &[ExecutorDetails]) -> Result<()> {
    #[derive(Tabled)]
    struct ExecutorRow {
        #[tabled(rename = "ID")]
        id: String,
        // #[tabled(rename = "GPUs")]
        // gpus: String,
        // #[tabled(rename = "CPU")]
        // cpu: String,
        // #[tabled(rename = "Memory")]
        // memory: String,
        #[tabled(rename = "Location")]
        location: String,
    }

    let rows: Vec<ExecutorRow> = executors
        .iter()
        .map(|executor| {
            // let gpu_info = if executor.gpu_specs.is_empty() {
            //     "None".to_string()
            // } else {
            //     format!(
            //         "{} x {} ({}GB)",
            //         executor.gpu_specs.len(),
            //         executor.gpu_specs[0].name,
            //         executor.gpu_specs[0].memory_gb
            //     )
            // };

            ExecutorRow {
                id: executor.id.clone(),
                // gpus: gpu_info,
                // cpu: format!("{} cores", executor.cpu_specs.cores),
                // memory: format!("{}GB", executor.cpu_specs.memory_gb),
                location: executor
                    .location
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
            }
        })
        .collect();

    let mut table = Table::new(rows);
    table.with(Style::modern());
    println!("{table}");

    Ok(())
}

/// Display active rentals in table format (for RentalStatusResponse - legacy)
pub fn display_rentals(rentals: &[RentalStatusResponse]) -> Result<()> {
    #[derive(Tabled)]
    struct RentalRow {
        #[tabled(rename = "Rental ID")]
        rental_id: String,
        #[tabled(rename = "Status")]
        status: String,
        #[tabled(rename = "Executor")]
        executor: String,
        #[tabled(rename = "Created")]
        created: String,
    }

    let rows: Vec<RentalRow> = rentals
        .iter()
        .map(|rental| RentalRow {
            rental_id: rental.rental_id.clone(),
            status: format!("{:?}", rental.status),
            executor: rental.executor.id.clone(),
            created: rental.created_at.format("%y-%m-%d %H:%M:%S").to_string(),
        })
        .collect();

    let mut table = Table::new(rows);
    table.with(Style::modern());
    println!("{table}");

    Ok(())
}

/// Display rental items in table format
pub fn display_rental_items(rentals: &[ApiRentalListItem], detailed: bool) -> Result<()> {
    #[derive(Tabled)]
    struct RentalRow {
        #[tabled(rename = "GPU")]
        gpu: String,
        #[tabled(rename = "Rental ID")]
        rental_id: String,
        #[tabled(rename = "State")]
        state: String,
        #[tabled(rename = "SSH")]
        ssh: String,
        #[tabled(rename = "Image")]
        image: String,
        #[tabled(rename = "Created")]
        created: String,
    }

    let rows: Vec<RentalRow> = rentals
        .iter()
        .map(|rental| {
            // Format GPU info from specs
            let gpu = if rental.gpu_specs.is_empty() {
                "Unknown".to_string()
            } else {
                // Format like "2x H100 (80GB)" if all GPUs are the same,
                // otherwise list them separately
                let first_gpu = &rental.gpu_specs[0];
                let all_same = rental
                    .gpu_specs
                    .iter()
                    .all(|g| g.name == first_gpu.name && g.memory_gb == first_gpu.memory_gb);

                if all_same {
                    let gpu_display_name = if detailed {
                        // Detailed mode: show full GPU name
                        first_gpu.name.clone()
                    } else {
                        // Compact mode: show categorized name
                        GpuCategory::from_str(&first_gpu.name).unwrap().to_string()
                    };

                    if rental.gpu_specs.len() > 1 {
                        format!(
                            "{}x {} ({}GB)",
                            rental.gpu_specs.len(),
                            gpu_display_name,
                            first_gpu.memory_gb
                        )
                    } else {
                        format!("1x {} ({}GB)", gpu_display_name, first_gpu.memory_gb)
                    }
                } else {
                    // List each GPU
                    rental
                        .gpu_specs
                        .iter()
                        .map(|g| {
                            let display_name = if detailed {
                                g.name.clone()
                            } else {
                                GpuCategory::from_str(&g.name).unwrap().to_string()
                            };
                            format!("{} ({}GB)", display_name, g.memory_gb)
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            };

            // Format SSH availability
            let ssh = if rental.has_ssh { "✓" } else { "✗" };

            RentalRow {
                gpu,
                rental_id: rental.rental_id.clone(),
                state: rental.state.to_string(),
                ssh: ssh.to_string(),
                image: rental.container_image.clone(),
                created: format_timestamp(&rental.created_at),
            }
        })
        .collect();

    let mut table = Table::new(rows);
    table.with(Style::modern());
    println!("{table}");

    Ok(())
}

/// Display configuration in table format
pub fn display_config(config: &HashMap<String, String>) -> Result<()> {
    #[derive(Tabled)]
    struct ConfigRow {
        #[tabled(rename = "Key")]
        key: String,
        #[tabled(rename = "Value")]
        value: String,
    }

    let mut rows: Vec<ConfigRow> = config
        .iter()
        .map(|(key, value)| ConfigRow {
            key: key.clone(),
            value: value.clone(),
        })
        .collect();

    rows.sort_by(|a, b| a.key.cmp(&b.key));

    let mut table = Table::new(rows);
    table.with(Style::modern());
    println!("{table}");

    Ok(())
}

/// Display available executors in compact format (grouped by location and GPU type)
pub fn display_available_executors_compact(executors: &[AvailableExecutor]) -> Result<()> {
    if executors.is_empty() {
        println!("No available executors found matching the specified criteria.");
        return Ok(());
    }

    // Group executors by location and GPU configuration
    let mut location_groups: HashMap<String, HashMap<String, Vec<&AvailableExecutor>>> =
        HashMap::new();

    for executor in executors {
        let location = executor
            .executor
            .location
            .clone()
            .unwrap_or_else(|| "Unknown Region".to_string());

        let gpu_key = if executor.executor.gpu_specs.is_empty() {
            "No GPU".to_string()
        } else {
            let gpu = &executor.executor.gpu_specs[0];
            let category = GpuCategory::from_str(&gpu.name).unwrap();
            let gpu_count = executor.executor.gpu_specs.len();
            format!("{}x {} ({}GB)", gpu_count, category, gpu.memory_gb)
        };

        location_groups
            .entry(location)
            .or_default()
            .entry(gpu_key)
            .or_default()
            .push(executor);
    }

    // Sort locations for consistent display
    let mut sorted_locations: Vec<_> = location_groups.keys().cloned().collect();
    sorted_locations.sort();

    println!("Available GPU Instances by Region\n");

    for location in sorted_locations {
        // Print location header
        println!("{}", location);

        #[derive(Tabled)]
        struct CompactRow {
            #[tabled(rename = "GPU TYPE")]
            gpu_type: String,
            #[tabled(rename = "AVAILABLE")]
            available: String,
        }

        let gpu_groups = location_groups.get(&location).unwrap();
        let mut rows: Vec<CompactRow> = Vec::new();

        // Sort GPU configurations for consistent display
        let mut sorted_gpu_configs: Vec<_> = gpu_groups.keys().cloned().collect();
        sorted_gpu_configs.sort();

        for gpu_config in sorted_gpu_configs {
            let executors_in_group = gpu_groups.get(&gpu_config).unwrap();
            let count = executors_in_group.len();

            rows.push(CompactRow {
                gpu_type: gpu_config.clone(),
                available: count.to_string(),
            });
        }

        let mut table = Table::new(rows);
        table.with(Style::modern());
        println!("{}", table);
        println!();
    }

    let total_count = executors.len();
    println!("Total available executors: {}", total_count);

    Ok(())
}

/// Display available executors in detailed format (individual executors)
pub fn display_available_executors_detailed(
    executors: &[AvailableExecutor],
    show_full_gpu_names: bool,
) -> Result<()> {
    if executors.is_empty() {
        println!("No available executors found matching the specified criteria.");
        return Ok(());
    }

    #[derive(Tabled)]
    struct DetailedExecutorRow {
        #[tabled(rename = "GPU")]
        gpu_info: String,
        #[tabled(rename = "Executor ID")]
        id: String,
        #[tabled(rename = "CPU")]
        cpu: String,
        #[tabled(rename = "RAM")]
        ram: String,
        #[tabled(rename = "Score")]
        score: String,
        #[tabled(rename = "Uptime")]
        uptime: String,
    }

    let rows: Vec<DetailedExecutorRow> = executors
        .iter()
        .map(|executor| {
            let gpu_info = if executor.executor.gpu_specs.is_empty() {
                "No GPU".to_string()
            } else if executor.executor.gpu_specs.len() == 1 {
                // Single GPU
                let gpu = &executor.executor.gpu_specs[0];
                let gpu_display_name = if show_full_gpu_names {
                    gpu.name.clone()
                } else {
                    GpuCategory::from_str(&gpu.name).unwrap().to_string()
                };
                format!("1x {} ({}GB)", gpu_display_name, gpu.memory_gb)
            } else {
                // Multiple GPUs - check if they're all the same model
                let first_gpu = &executor.executor.gpu_specs[0];
                let all_same = executor
                    .executor
                    .gpu_specs
                    .iter()
                    .all(|g| g.name == first_gpu.name && g.memory_gb == first_gpu.memory_gb);

                if all_same {
                    // All GPUs are identical - use count prefix format
                    let gpu_display_name = if show_full_gpu_names {
                        first_gpu.name.clone()
                    } else {
                        GpuCategory::from_str(&first_gpu.name).unwrap().to_string()
                    };
                    format!(
                        "{}x {} ({}GB)",
                        executor.executor.gpu_specs.len(),
                        gpu_display_name,
                        first_gpu.memory_gb
                    )
                } else {
                    // Different GPU models - list them individually
                    let gpu_names: Vec<String> = executor
                        .executor
                        .gpu_specs
                        .iter()
                        .map(|g| {
                            let display_name = if show_full_gpu_names {
                                g.name.clone()
                            } else {
                                GpuCategory::from_str(&g.name).unwrap().to_string()
                            };
                            format!("{} ({}GB)", display_name, g.memory_gb)
                        })
                        .collect();
                    gpu_names.join(", ")
                }
            };

            // Remove miner prefix from executor ID if present
            let executor_id = match executor.executor.id.split_once("__") {
                Some((_, second)) => second.to_string(),
                None => executor.executor.id.clone(),
            };

            DetailedExecutorRow {
                gpu_info,
                id: executor_id,
                cpu: format!("{} cores", executor.executor.cpu_specs.cores),
                ram: format!("{}GB", executor.executor.cpu_specs.memory_gb),
                score: format!("{:.2}", executor.availability.verification_score),
                uptime: format!("{:.1}%", executor.availability.uptime_percentage),
            }
        })
        .collect();

    let mut table = Table::new(rows);
    table.with(Style::modern());
    println!("{}", table);
    println!("\nTotal available executors: {}", executors.len());

    Ok(())
}
