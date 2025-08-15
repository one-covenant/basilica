use crate::{
    domain::{price::PriceConverter, types::BillingClient},
    storage::{ObservedDepositsRepo, OutboxRepo, PgRepos},
};
use anyhow::Result;
use tokio::time::{sleep, Duration};
use tracing::{error, info};

pub struct OutboxDispatcher<B: BillingClient> {
    repos: PgRepos,
    billing: B,
    price: PriceConverter,
}

impl<B: BillingClient> OutboxDispatcher<B> {
    pub fn new(repos: PgRepos, billing: B, price: PriceConverter) -> Self {
        Self {
            repos,
            billing,
            price,
        }
    }

    pub async fn run(&self) -> Result<()> {
        loop {
            let rows = self.repos.claim_batch(100).await?;
            if rows.is_empty() {
                sleep(Duration::from_millis(350)).await;
                continue;
            }

            for r in rows {
                let credits = match self.price.tao_to_credits(&r.amount_plancks).await {
                    Ok(c) => c,
                    Err(e) => {
                        let secs =
                            2_i64.pow(std::cmp::min(6, (r.attempts as u32).saturating_sub(1)));
                        error!(outbox_id = r.id, err = %e, backoff = secs, "price conversion failed");
                        // Schedule a retry instead of leaving the item claimed indefinitely.
                        let _ = self.repos.backoff(r.id, secs).await;
                        continue;
                    }
                };

                match self
                    .billing
                    .apply_credits(&r.user_id, &credits, &r.transaction_id)
                    .await
                {
                    Ok(credit_id) => {
                        // Persist state changes; failures here should not tear down the dispatcher.
                        match self.repos.begin().await {
                            Ok(mut tx) => {
                                if let Err(e) = self.repos.mark_dispatched_tx(&mut tx, r.id).await {
                                    error!(outbox_id = r.id, %credit_id, err=%e, "failed to mark dispatched; scheduling retry");
                                    let secs = 2_i64.pow(std::cmp::min(
                                        6,
                                        (r.attempts as u32).saturating_sub(1),
                                    ));
                                    let _ = self.repos.backoff(r.id, secs).await;
                                    continue;
                                }
                                if let Err(e) = self
                                    .repos
                                    .mark_credited_tx(&mut tx, &r.transaction_id, &credit_id)
                                    .await
                                {
                                    error!(outbox_id = r.id, %credit_id, err=%e, "failed to mark credited; scheduling retry");
                                    let secs = 2_i64.pow(std::cmp::min(
                                        6,
                                        (r.attempts as u32).saturating_sub(1),
                                    ));
                                    let _ = self.repos.backoff(r.id, secs).await;
                                    continue;
                                }
                                if let Err(e) = tx.commit().await {
                                    error!(outbox_id = r.id, %credit_id, err=%e, "failed to commit credited state; scheduling retry");
                                    let secs = 2_i64.pow(std::cmp::min(
                                        6,
                                        (r.attempts as u32).saturating_sub(1),
                                    ));
                                    let _ = self.repos.backoff(r.id, secs).await;
                                    continue;
                                }
                                info!(outbox_id = r.id, %credit_id, "credited");
                            }
                            Err(e) => {
                                error!(outbox_id = r.id, %credit_id, err=%e, "failed to open transaction; scheduling retry");
                                let secs = 2_i64
                                    .pow(std::cmp::min(6, (r.attempts as u32).saturating_sub(1)));
                                let _ = self.repos.backoff(r.id, secs).await;
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        let secs =
                            2_i64.pow(std::cmp::min(6, (r.attempts as u32).saturating_sub(1)));
                        error!(outbox_id = r.id, err = %e, backoff = secs, "apply_credits failed");
                        self.repos.backoff(r.id, secs).await?;
                    }
                }
            }
        }
    }
}
