//! Output formatting utilities

pub mod table_output;

use crate::error::Result;
use console::style;
use serde::Serialize;

/// Output data as JSON
pub fn json_output<T: Serialize>(data: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(data)?;
    println!("{json}");
    Ok(())
}

/// Print a success message with green checkmark
pub fn print_success(message: &str) {
    println!("{} {}", style("✓").green().bold(), message);
}

/// Print an error message with red X
pub fn print_error(message: &str) {
    eprintln!("{} {}", style("✗").red().bold(), style(message).red());
}

/// Print an informational message with blue info icon
pub fn print_info(message: &str) {
    println!("{} {}", style("ℹ").blue(), message);
}

/// Print a link/URL with label
pub fn print_link(label: &str, url: &str) {
    println!("{} {}: {}", style("→").cyan(), label, style(url).dim());
}

/// Print a security/auth related message  
pub fn print_auth(message: &str) {
    println!("{} {}", style("🔐").cyan(), message);
}
