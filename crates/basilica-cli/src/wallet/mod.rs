//! Wallet management module using bittensor_wallet

use crate::error::Result;
use bittensor_wallet::Wallet;
use std::path::Path;

/// Load a Bittensor wallet from the specified base path
pub fn load_wallet(base_wallet_path: &Path, wallet_name: &str) -> Result<Wallet> {
    // Validate wallet name to prevent path traversal
    if wallet_name.contains('/') || wallet_name.contains('\\') {
        return Err(crate::error::CliError::invalid_argument(
            "Wallet name cannot contain path separators".to_string(),
        ));
    }
    // Check if wallet exists before creating Wallet object
    if !wallet_exists(base_wallet_path, wallet_name) {
        return Err(crate::error::CliError::not_found(format!(
            "Wallet '{}' not found",
            wallet_name
        )));
    }

    let wallet = Wallet::new(
        Some(wallet_name.to_string()),
        None, // Use default hotkey name
        Some(base_wallet_path.to_string_lossy().to_string()),
        None, // Use default config
    );

    Ok(wallet)
}

/// Get wallet addresses (coldkey and hotkey)
pub struct WalletAddresses {
    pub coldkey: Option<String>,
    pub hotkey: Option<String>,
}

/// Load wallet and get addresses
pub fn get_wallet_addresses(
    base_wallet_path: &Path,
    wallet_name: &str,
    password: Option<&str>,
) -> Result<WalletAddresses> {
    let wallet = load_wallet(base_wallet_path, wallet_name)?;

    let password_string = password.map(|p| p.to_string());

    // Try to get coldkey address
    let coldkey = wallet
        .get_coldkeypub(password_string.clone())
        .ok()
        .and_then(|keypair| keypair.ss58_address());

    // Try to get hotkey address
    let hotkey = wallet
        .get_hotkeypub(password_string)
        .ok()
        .and_then(|keypair| keypair.ss58_address());

    Ok(WalletAddresses { coldkey, hotkey })
}

/// Check if wallet exists at the specified base path
pub fn wallet_exists(base_wallet_path: &Path, wallet_name: &str) -> bool {
    let coldkey_path = base_wallet_path.join(wallet_name).join("coldkey");
    let hotkey_path = base_wallet_path
        .join(wallet_name)
        .join("hotkeys")
        .join("default");

    coldkey_path.exists() || hotkey_path.exists()
}
