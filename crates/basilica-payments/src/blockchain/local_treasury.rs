use crate::domain::types::Treasury;
use anyhow::Result;
use async_trait::async_trait;
use basilica_common::crypto::wallet::generate_sr25519_wallet;

pub struct LocalTreasury {
    ss58_prefix: u16,
}

impl LocalTreasury {
    pub fn new(ss58_prefix: u16) -> Self {
        Self { ss58_prefix }
    }
}

#[async_trait]
impl Treasury for LocalTreasury {
    async fn generate_hotkey(&self) -> Result<(String, String, String, String)> {
        let wallet = generate_sr25519_wallet(self.ss58_prefix)
            .map_err(|e| anyhow::anyhow!("Failed to generate wallet: {}", e))?;

        Ok((
            wallet.address,
            wallet.account_hex,
            wallet.public_hex,
            wallet.mnemonic,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_local_treasury_wallet_generation() {
        let treasury = LocalTreasury::new(42); // Generic Substrate prefix

        let (address, account_hex, public_hex, mnemonic) =
            treasury.generate_hotkey().await.unwrap();

        // Check all fields are populated
        assert!(!address.is_empty());
        assert!(!account_hex.is_empty());
        assert!(!public_hex.is_empty());
        assert!(!mnemonic.is_empty());

        // Check hex format
        assert_eq!(public_hex.len(), 64); // 32 bytes * 2
        assert!(public_hex.chars().all(|c| c.is_ascii_hexdigit()));

        // Check mnemonic has expected word count
        let word_count = mnemonic.split_whitespace().count();
        assert!(word_count >= 12);

        // Generate another wallet - should be different
        let (address2, _, public_hex2, mnemonic2) = treasury.generate_hotkey().await.unwrap();
        assert_ne!(address, address2);
        assert_ne!(public_hex, public_hex2);
        assert_ne!(mnemonic, mnemonic2);
    }

    #[tokio::test]
    async fn test_local_treasury_different_prefixes() {
        let treasury_substrate = LocalTreasury::new(42);
        let treasury_polkadot = LocalTreasury::new(0);

        // Generate wallets with different prefixes
        let (addr_sub, _, pub_sub, _) = treasury_substrate.generate_hotkey().await.unwrap();
        let (addr_pol, _, pub_pol, _) = treasury_polkadot.generate_hotkey().await.unwrap();

        // Different wallets should have different keys
        assert_ne!(pub_sub, pub_pol);
        // And different addresses
        assert_ne!(addr_sub, addr_pol);
    }
}
