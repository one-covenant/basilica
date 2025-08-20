//! Route handlers for the validator API

pub mod capacity;
pub mod config;
pub mod gpu;
pub mod health;
pub mod miners;
pub mod verification;

pub use capacity::*;
pub use config::*;
pub use gpu::*;
pub use health::*;
pub use miners::*;
pub use verification::*;
