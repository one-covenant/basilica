use super::{OutboxRepo, OutboxRow, PgRepos, PgTx};
use sqlx::types::BigDecimal;
use sqlx::{Result, Row};

#[async_trait::async_trait]
impl OutboxRepo for PgRepos {
    async fn enqueue_tx(
        &self,
        tx: &mut PgTx<'_>,
        to_hex: &str,
        amount: &str,
        txid: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO billing_outbox (user_id, amount_plancks, transaction_id)
               SELECT user_id, $2, $3 FROM deposit_accounts WHERE account_id_hex = $1
               ON CONFLICT (transaction_id) DO NOTHING"#,
        )
        .bind(to_hex)
        .bind(amount)
        .bind(txid)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    async fn claim_batch(&self, limit: i64) -> Result<Vec<OutboxRow>> {
        let rows = sqlx::query(
            r#"
            WITH cte AS (
              SELECT id
              FROM billing_outbox
              WHERE dispatched_at IS NULL
                AND next_attempt_at <= now()
                AND (claimed_at IS NULL OR claimed_at < now() - interval '5 minutes')
              ORDER BY id
              LIMIT $1
              FOR UPDATE SKIP LOCKED
            )
            UPDATE billing_outbox b
               SET claimed_at = now(), attempts = b.attempts + 1
            FROM cte
            WHERE b.id = cte.id
            RETURNING b.id, b.user_id, b.amount_plancks, b.transaction_id, b.attempts
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let amount: Option<BigDecimal> = r.get("amount_plancks");
                OutboxRow {
                    id: r.get("id"),
                    user_id: r.get("user_id"),
                    amount_plancks: amount.map(|a| a.to_string()).unwrap_or_default(),
                    transaction_id: r.get("transaction_id"),
                    attempts: r.get("attempts"),
                }
            })
            .collect())
    }

    async fn mark_dispatched_tx(&self, tx: &mut PgTx<'_>, id: i64) -> Result<()> {
        sqlx::query(r#"UPDATE billing_outbox SET dispatched_at = now() WHERE id = $1"#)
            .bind(id)
            .execute(&mut **tx)
            .await?;
        Ok(())
    }

    async fn backoff(&self, id: i64, secs: i64) -> Result<()> {
        sqlx::query(
            r#"UPDATE billing_outbox SET next_attempt_at = now() + make_interval(secs => $2) WHERE id = $1"#
        )
        .bind(id)
        .bind(secs)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn get_pending_count(&self) -> Result<usize> {
        let row = sqlx::query(
            r#"
            SELECT COUNT(*) as count
            FROM billing_outbox
            WHERE dispatched_at IS NULL
              AND next_attempt_at <= now()
            "#,
        )
        .fetch_one(&self.pool)
        .await?;

        let count: i64 = row.get("count");
        Ok(count as usize)
    }
}
