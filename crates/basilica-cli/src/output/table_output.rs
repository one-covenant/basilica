//! Table formatting for CLI output

use crate::api::client::PricingResponse;
use crate::cli::commands::PricingFilters;
use crate::error::Result;
use basilica_api::api::types::{ExecutorDetails, RentalStatusResponse};
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

/// Display pricing information in table format
pub fn display_pricing(pricing: &PricingResponse, filters: &PricingFilters) -> Result<()> {
    #[derive(Tabled)]
    struct PricingRow {
        #[tabled(rename = "GPU Type")]
        gpu_type: String,
        #[tabled(rename = "Memory")]
        memory: String,
        #[tabled(rename = "Price/Hour")]
        price: String,
        #[tabled(rename = "Available")]
        available: u32,
        #[tabled(rename = "Compute")]
        compute: String,
    }

    let mut rows: Vec<PricingRow> = pricing
        .gpu_types
        .iter()
        .filter(|(gpu_type, _)| {
            if let Some(ref filter_type) = filters.gpu_type {
                gpu_type
                    .to_lowercase()
                    .contains(&filter_type.to_lowercase())
            } else {
                true
            }
        })
        .filter(|(_, pricing)| {
            if let Some(min_memory) = filters.min_memory {
                pricing.memory_gb >= min_memory
            } else {
                true
            }
        })
        .map(|(gpu_type, pricing)| PricingRow {
            gpu_type: gpu_type.clone(),
            memory: format!("{}GB", pricing.memory_gb),
            price: format!("${:.4}", pricing.base_price_per_hour),
            available: pricing.available_count,
            compute: pricing.compute_capability.clone(),
        })
        .collect();

    // Sort based on filter
    match filters.sort.as_str() {
        "price-asc" => rows.sort_by(|a, b| {
            let price_a: f64 = a.price[1..].parse().unwrap_or(0.0);
            let price_b: f64 = b.price[1..].parse().unwrap_or(0.0);
            price_a
                .partial_cmp(&price_b)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        "price-desc" => rows.sort_by(|a, b| {
            let price_a: f64 = a.price[1..].parse().unwrap_or(0.0);
            let price_b: f64 = b.price[1..].parse().unwrap_or(0.0);
            price_b
                .partial_cmp(&price_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        "memory-asc" => rows.sort_by(|a, b| {
            let mem_a: u32 = a.memory.trim_end_matches("GB").parse().unwrap_or(0);
            let mem_b: u32 = b.memory.trim_end_matches("GB").parse().unwrap_or(0);
            mem_a.cmp(&mem_b)
        }),
        "memory-desc" => rows.sort_by(|a, b| {
            let mem_a: u32 = a.memory.trim_end_matches("GB").parse().unwrap_or(0);
            let mem_b: u32 = b.memory.trim_end_matches("GB").parse().unwrap_or(0);
            mem_b.cmp(&mem_a)
        }),
        _ => {} // No sorting
    }

    let table = Table::new(rows);
    println!("{}", table);

    Ok(())
}

/// Display active rentals in table format
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
