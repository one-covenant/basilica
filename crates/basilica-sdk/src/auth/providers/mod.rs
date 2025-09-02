//! Authentication provider implementations

pub mod device;
pub mod existing_token;
pub mod oauth2;

pub use device::DeviceFlowProvider;
pub use existing_token::ExistingTokenProvider;
pub use oauth2::OAuth2Provider;
