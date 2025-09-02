//! Re-export client functionality from basilica-sdk
//!
//! This module maintains backward compatibility by re-exporting the client
//! implementation from the basilica-sdk crate.

#[cfg(feature = "client")]
pub use basilica_sdk::{BasilicaClient, ClientBuilder};

#[cfg(test)]
mod tests {
    // Tests moved to basilica-sdk
}