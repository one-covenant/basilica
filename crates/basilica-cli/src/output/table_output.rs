//! Table formatting for CLI output

use crate::error::Result;
use basilica_api::api::types::{ApiRentalListItem, ExecutorDetails, RentalStatusResponse};
use chrono::{DateTime, Local};
use std::collections::HashMap;
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

/// Extract GPU category from full GPU name
pub fn extract_gpu_category(gpu_name: &str) -> String {
    let name_upper = gpu_name.to_uppercase();

    // Common GPU categories to look for
    let categories = [
        "H100", "H200", "B200", "B100", "A100", "A6000", "A40", "A30", "A10", "RTX 4090",
        "RTX 4080", "RTX 3090", "RTX 3080", "V100", "T4", "L4", "L40", "L40S",
    ];

    for category in &categories {
        if name_upper.contains(category) {
            return category.to_string();
        }
    }

    // If no known category found, try to extract the model name
    // Remove common prefixes
    let cleaned = name_upper
        .replace("NVIDIA", "")
        .replace("GEFORCE", "")
        .replace("TESLA", "")
        .trim()
        .to_string();

    // Return first word/token as category
    cleaned
        .split_whitespace()
        .next()
        .unwrap_or("Unknown")
        .to_string()
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
                        extract_gpu_category(&first_gpu.name)
                    };

                    if rental.gpu_specs.len() > 1 {
                        format!(
                            "{}x {} ({}GB)",
                            rental.gpu_specs.len(),
                            gpu_display_name,
                            first_gpu.memory_gb
                        )
                    } else {
                        format!("{} ({}GB)", gpu_display_name, first_gpu.memory_gb)
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
                                extract_gpu_category(&g.name)
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
