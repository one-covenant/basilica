//! Table formatting for CLI output

use crate::error::Result;
use basilica_api::api::types::{AvailableExecutor, ExecutorDetails, RentalInfo, RentalStatusResponse};
use std::collections::HashMap;
use tabled::{Table, Tabled};

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

    let table = Table::new(rows);
    println!("{}", table);

    Ok(())
}

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

    let table = Table::new(rows);
    println!("{}", table);

    Ok(())
}

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

    let table = Table::new(rows);
    println!("{}", table);

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
        #[tabled(rename = "Cost")]
        cost: String,
    }

    let rows: Vec<RentalRow> = rentals
        .iter()
        .map(|rental| RentalRow {
            rental_id: rental.rental_id.clone(),
            status: format!("{:?}", rental.status),
            executor: rental.executor.id.clone(),
            created: rental.created_at.format("%Y-%m-%d %H:%M").to_string(),
            cost: format!("${:.4}", rental.cost_incurred),
        })
        .collect();

    let table = Table::new(rows);
    println!("{}", table);

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

    let table = Table::new(rows);
    println!("{}", table);

    Ok(())
}
