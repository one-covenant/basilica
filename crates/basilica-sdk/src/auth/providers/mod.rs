//! Authentication provider implementations

pub mod oauth2;
pub mod device;
pub mod api_key;

pub use oauth2::OAuth2Provider;
pub use device::DeviceFlowProvider;
pub use api_key::ApiKeyProvider;