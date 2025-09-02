//! Authentication provider implementations

pub mod oauth2;
pub mod device;

pub use oauth2::OAuth2Provider;
pub use device::DeviceFlowProvider;