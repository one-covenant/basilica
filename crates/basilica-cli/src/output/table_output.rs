//! Table formatting for CLI output

use crate::error::Result;
use basilica_api::api::types::{ExecutorDetails, RentalStatusResponse};
use basilica_validator::api::types::RentalListItem;
use chrono::{DateTime, Local};
use std::collections::HashMap;
use tabled::{settings::Style, Table, Tabled};

/// Truncate container ID to first N characters for display (similar to Docker)
fn truncate_container_id(container_id: &str) -> String {
    const CONTAINER_ID_DISPLAY_LENGTH: usize = 12;

    if container_id.len() <= CONTAINER_ID_DISPLAY_LENGTH {
        container_id.to_string()
    } else {
        container_id
            .chars()
            .take(CONTAINER_ID_DISPLAY_LENGTH)
            .collect()
    }
}

/// Format RFC3339 timestamp to YY-MM-DD HH:MM:SS format
fn format_timestamp(timestamp: &str) -> String {
    DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .and_then(|dt| {
            let local_dt = dt.with_timezone(&Local);
            Some(local_dt.format("%y-%m-%d %H:%M:%S").to_string())
        })
        .unwrap_or_else(|| timestamp.to_string())
}

/// Display executors in table format
pub fn display_executors(executors: &[ExecutorDetails]) -> Result<()> {
    #[derive(Tabled)]
    struct ExecutorRow {
        #[tabled(rename = "ID")]
        id: String,
        #[tabled(rename = "GPUs")]
        gpus: String,
        #[tabled(rename = "CPU")]
        cpu: String,
        #[tabled(rename = "Memory")]
        memory: String,
        #[tabled(rename = "Location")]
        location: String,
    }

    let rows: Vec<ExecutorRow> = executors
        .iter()
        .map(|executor| {
            let gpu_info = if executor.gpu_specs.is_empty() {
                "None".to_string()
            } else {
                format!(
                    "{} x {} ({}GB)",
                    executor.gpu_specs.len(),
                    executor.gpu_specs[0].name,
                    executor.gpu_specs[0].memory_gb
                )
            };

            ExecutorRow {
                id: executor.id.clone(),
                gpus: gpu_info,
                cpu: format!("{} cores", executor.cpu_specs.cores),
                memory: format!("{}GB", executor.cpu_specs.memory_gb),
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

/* Commented out - AvailableExecutor type removed
/// Display available executors in table format with pricing information
pub fn display_available_executors(executors: &[AvailableExecutor]) -> Result<()> {
    #[derive(Tabled)]
    struct AvailableExecutorRow {
        #[tabled(rename = "ID")]
        id: String,
        #[tabled(rename = "GPUs")]
        gpus: String,
        #[tabled(rename = "CPU")]
        cpu: String,
        #[tabled(rename = "Memory")]
        memory: String,
        #[tabled(rename = "Location")]
        location: String,
        #[tabled(rename = "Price/Hour")]
        price: String,
        #[tabled(rename = "Available")]
        available: String,
    }

    let rows: Vec<AvailableExecutorRow> = executors
        .iter()
        .map(|executor| {
            let gpu_info = if executor.gpu_specs.is_empty() {
                "None".to_string()
            } else {
                format!(
                    "{} x {} ({}GB)",
                    executor.gpu_specs.len(),
                    executor.gpu_specs[0].name,
                    executor.gpu_specs[0].memory_gb
                )
            };

            AvailableExecutorRow {
                id: executor.executor_id.clone(),
                gpus: gpu_info,
                cpu: format!("{} cores", executor.cpu_specs.cores),
                memory: format!("{}GB", executor.cpu_specs.memory_gb),
                location: executor
                    .location
                    .clone()
                    .unwrap_or_else(|| "Unknown".to_string()),
                price: format!("${:.2}", executor.price_per_hour),
                available: if executor.available { "Yes" } else { "No" }.to_string(),
            }
        })
        .collect();

    let mut table = Table::new(rows);
    table.with(Style::modern());
    println!("{table}");

    Ok(())
}
*/

/* Commented out - RentalInfo type removed
/// Display active rentals in table format (for RentalInfo)
pub fn display_rental_list(rentals: &[RentalInfo]) -> Result<()> {
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
        #[tabled(rename = "Cost/hr")]
        cost_per_hour: String,
        #[tabled(rename = "Total Cost")]
        total_cost: String,
    }

    let rows: Vec<RentalRow> = rentals
        .iter()
        .map(|rental| RentalRow {
            rental_id: rental.rental_id.clone(),
            status: format!("{:?}", rental.status),
            executor: rental.executor_id.clone(),
            created: rental.created_at.format("%Y-%m-%d %H:%M").to_string(),
            cost_per_hour: format!("${:.4}", rental.cost_per_hour),
            total_cost: format!("${:.4}", rental.total_cost),
        })
        .collect();

    let mut table = Table::new(rows);
    table.with(Style::modern());
    println!("{table}");

    Ok(())
}
*/

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
pub fn display_rental_items(rentals: &[RentalListItem]) -> Result<()> {
    #[derive(Tabled)]
    struct RentalRow {
        #[tabled(rename = "Rental ID")]
        rental_id: String,
        #[tabled(rename = "State")]
        state: String,
        #[tabled(rename = "Executor")]
        executor: String,
        #[tabled(rename = "Container")]
        container: String,
        #[tabled(rename = "Image")]
        image: String,
        #[tabled(rename = "Created")]
        created: String,
    }

    let rows: Vec<RentalRow> = rentals
        .iter()
        .map(|rental| RentalRow {
            rental_id: rental.rental_id.clone(),
            state: rental.state.to_string(),
            executor: rental.executor_id.clone(),
            container: truncate_container_id(&rental.container_id),
            image: rental.container_image.clone(),
            created: format_timestamp(&rental.created_at),
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
