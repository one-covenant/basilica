//! Output formatting utilities

pub mod table_output;

use crate::error::Result;
use serde::Serialize;

/// Output data as JSON
pub fn json_output<T: Serialize>(data: &T) -> Result<()> {
    let json = serde_json::to_string_pretty(data)?;
    println!("{}", json);
    Ok(())
}
