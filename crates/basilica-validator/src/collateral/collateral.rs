use crate::persistence::SimplePersistence;
use anyhow::Result;
use collateral_contract::{Deposit, Reclaimed, Slashed};
use std::sync::Arc;
use tracing::{error, info};

pub struct Collateral {
    config: crate::config::VerificationConfig,
    persistence: Arc<SimplePersistence>,
}

impl Collateral {
    pub fn new(
        config: crate::config::VerificationConfig,
        persistence: Arc<SimplePersistence>,
    ) -> Self {
        Self {
            config,
            persistence,
        }
    }

    /// Start the collateral event scan loop
    pub async fn start(&mut self) -> Result<()> {
        info!("Starting collateral event scan loop");
        let mut interval = tokio::time::interval(self.config.collateral_event_scan_interval);

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = self.scan_handle_collateral_events().await {
                        error!("Collateral event scan failed: {}", e);
                    }
                }
            }
        }
    }

    pub async fn scan_handle_collateral_events(&self) -> Result<()> {
        let last_block = self.persistence.get_last_scanned_block_number().await?;
        let from_block = last_block + 1;
        let (to_block, events_map) = collateral_contract::scan_events(from_block).await?;

        let mut sorted_events_map = events_map.iter().collect::<Vec<_>>();
        sorted_events_map.sort_by(|a, b| a.0.cmp(b.0));
        for (block_number, events_vec) in sorted_events_map.iter() {
            for event in events_vec.0.iter() {
                self.persistence.handle_deposit(&event).await?;
            }

            for event in events_vec.1.iter() {
                self.persistence.handle_reclaimed(&event).await?;
            }

            for event in events_vec.2.iter() {
                self.persistence.handle_slashed(&event).await?;
            }

            self.persistence
                .update_last_scanned_block_number(**block_number)
                .await?;
        }

        self.persistence
            .update_last_scanned_block_number(to_block)
            .await?;

        Ok(())
    }
}
