use crate::{
    domain::{price::PriceConverter, types::BillingClient},
    metrics::PaymentsMetricsSystem,
    storage::{ObservedDepositsRepo, OutboxRepo, PgRepos},
};
use anyhow::Result;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info};

pub struct OutboxDispatcher<B: BillingClient> {
    repos: PgRepos,
    billing: B,
    price: PriceConverter,
    metrics: Option<Arc<PaymentsMetricsSystem>>,
}

impl<B: BillingClient> OutboxDispatcher<B> {
    pub fn new(
        repos: PgRepos,
        billing: B,
        price: PriceConverter,
        metrics: Option<Arc<PaymentsMetricsSystem>>,
    ) -> Self {
        Self {
            repos,
            billing,
            price,
            metrics,
        }
    }

    pub async fn run(&self) -> Result<()> {
        info!("Starting outbox dispatcher");

        loop {
            // Record queue size if metrics are enabled
            if let Some(ref metrics) = self.metrics {
                if let Ok(queue_size) = self.repos.get_pending_count().await {
                    metrics
                        .business_metrics()
                        .set_outbox_queue_size(queue_size)
                        .await;
                }
            }

            let batch_timer = self
                .metrics
                .as_ref()
                .map(|m| m.payment_metrics().start_payment_timer());

            let rows = self.repos.claim_batch(100).await?;

            if rows.is_empty() {
                if let Some(metrics) = &self.metrics {
                    if let Some(timer) = batch_timer {
                        metrics
                            .payment_metrics()
                            .record_payment_complete(timer, true, 0.0)
                            .await;
                    }
                }
                sleep(Duration::from_millis(350)).await;
                continue;
            }

            let batch_size = rows.len();
            debug!(batch_size = batch_size, "Processing batch");

            for r in rows {
                let message_timer = self
                    .metrics
                    .as_ref()
                    .map(|m| m.payment_metrics().start_payment_timer());

                let credits = match self.price.tao_to_credits(&r.amount_plancks).await {
                    Ok(c) => c,
                    Err(e) => {
                        let secs =
                            2_i64.pow(std::cmp::min(6, (r.attempts as u32).saturating_sub(1)));
                        error!(outbox_id = r.id, err = %e, backoff = secs, "price conversion failed");

                        if let Some(ref metrics) = self.metrics {
                            metrics
                                .business_metrics()
                                .record_payment_failed(&[("reason", "price_conversion")])
                                .await;
                            if let Some(timer) = message_timer {
                                metrics
                                    .payment_metrics()
                                    .record_payment_complete(timer, false, 0.0)
                                    .await;
                            }
                        }

                        // Schedule a retry instead of leaving the item claimed indefinitely.
                        let _ = self.repos.backoff(r.id, secs).await;
                        continue;
                    }
                };

                let billing_timer = self
                    .metrics
                    .as_ref()
                    .map(|m| m.payment_metrics().start_blockchain_timer());

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

                                // Record successful payment
                                if let Some(ref metrics) = self.metrics {
                                    let amount_tao =
                                        r.amount_plancks.parse::<f64>().unwrap_or(0.0) / 1e9;
                                    metrics
                                        .business_metrics()
                                        .record_payment_processed(amount_tao, &[("type", "outbox")])
                                        .await;
                                    metrics.business_metrics().record_outbox_message().await;

                                    if let Some(timer) = billing_timer {
                                        metrics
                                            .payment_metrics()
                                            .record_blockchain_transaction(
                                                timer,
                                                "apply_credits",
                                                true,
                                            )
                                            .await;
                                    }

                                    if let Some(timer) = message_timer {
                                        metrics
                                            .payment_metrics()
                                            .record_payment_complete(timer, true, amount_tao)
                                            .await;
                                    }
                                }
                            }
                            Err(e) => {
                                error!(outbox_id = r.id, %credit_id, err=%e, "failed to open transaction; scheduling retry");
                                let secs = 2_i64
                                    .pow(std::cmp::min(6, (r.attempts as u32).saturating_sub(1)));

                                if let Some(ref metrics) = self.metrics {
                                    metrics
                                        .business_metrics()
                                        .record_payment_failed(&[("reason", "transaction_begin")])
                                        .await;
                                    if let Some(timer) = message_timer {
                                        metrics
                                            .payment_metrics()
                                            .record_payment_complete(timer, false, 0.0)
                                            .await;
                                    }
                                }

                                let _ = self.repos.backoff(r.id, secs).await;
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        let secs =
                            2_i64.pow(std::cmp::min(6, (r.attempts as u32).saturating_sub(1)));
                        error!(outbox_id = r.id, err = %e, backoff = secs, "apply_credits failed");

                        if let Some(ref metrics) = self.metrics {
                            metrics
                                .business_metrics()
                                .record_payment_failed(&[("reason", "billing_service")])
                                .await;

                            if let Some(timer) = billing_timer {
                                metrics
                                    .payment_metrics()
                                    .record_blockchain_transaction(timer, "apply_credits", false)
                                    .await;
                            }

                            if let Some(timer) = message_timer {
                                metrics
                                    .payment_metrics()
                                    .record_payment_complete(timer, false, 0.0)
                                    .await;
                            }
                        }

                        self.repos.backoff(r.id, secs).await?;
                    }
                }
            }

            // Record batch completion
            if let Some(ref metrics) = self.metrics {
                if let Some(timer) = batch_timer {
                    metrics
                        .payment_metrics()
                        .record_payment_complete(timer, true, batch_size as f64)
                        .await;
                }
            }
        }
    }
}
