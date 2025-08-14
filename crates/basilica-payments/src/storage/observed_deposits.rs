use super::{ObservedDepositsRepo, ObservedRow, PgRepos, PgTx};
use sqlx::types::BigDecimal;
use sqlx::{Result, Row};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

#[async_trait::async_trait]
impl ObservedDepositsRepo for PgRepos {
    async fn insert_finalized_tx(
        &self,
        tx: &mut PgTx<'_>,
        block: i64,
        idx: i32,
        to_hex: &str,
        from_hex: &str,
        amount: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO observed_deposits
               (block_number, event_index, to_account_hex, from_account_hex, amount_plancks, status)
               VALUES ($1,$2,$3,$4,$5,'FINALIZED')
               ON CONFLICT (block_number, event_index) DO NOTHING"#,
        )
        .bind(block)
        .bind(idx)
        .bind(to_hex)
        .bind(from_hex)
        .bind(amount)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    async fn mark_credited_tx(
        &self,
        tx: &mut PgTx<'_>,
        transaction_id: &str,
        credit_id: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"UPDATE observed_deposits
               SET status='CREDITED', credited_at = now(), billing_credit_id = $2
               WHERE ( 'b' || block_number::text || '#e' || event_index::text || '#' || to_account_hex ) = $1"#
        )
        .bind(transaction_id)
        .bind(credit_id)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    async fn list_by_user(
        &self,
        user_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ObservedRow>> {
        let rows = sqlx::query(
            r#"SELECT block_number, event_index, from_account_hex, to_account_hex, amount_plancks,
                      status, observed_at, credited_at, billing_credit_id
               FROM observed_deposits
               WHERE to_account_hex IN (SELECT account_id_hex FROM deposit_accounts WHERE user_id = $1)
               ORDER BY block_number DESC, event_index DESC
               LIMIT $2 OFFSET $3"#
        )
        .bind(user_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let amount: Option<BigDecimal> = r.get("amount_plancks");
                let observed_at: Option<OffsetDateTime> = r.get("observed_at");
                let credited_at: Option<OffsetDateTime> = r.get("credited_at");
                let billing_credit_id: Option<String> = r.get("billing_credit_id");

                ObservedRow {
                    block_number: r.get("block_number"),
                    event_index: r.get("event_index"),
                    from_account_hex: r.get("from_account_hex"),
                    to_account_hex: r.get("to_account_hex"),
                    amount_plancks: amount.map(|a| a.to_string()).unwrap_or_default(),
                    status: r.get("status"),
                    observed_at_rfc3339: observed_at
                        .map(|t| t.format(&Rfc3339).unwrap())
                        .unwrap_or_default(),
                    credited_at_rfc3339: credited_at
                        .map(|t| t.format(&Rfc3339).unwrap())
                        .unwrap_or_default(),
                    billing_credit_id: billing_credit_id.unwrap_or_default(),
                }
            })
            .collect())
    }
}
