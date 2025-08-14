use super::{DepositAccountsRepo, PgRepos, PgTx};
use sqlx::{Result, Row};

#[async_trait::async_trait]
impl DepositAccountsRepo for PgRepos {
    async fn get_by_user(&self, user_id: &str) -> Result<Option<(String, String, String, String)>> {
        let row = sqlx::query(
            r#"SELECT address_ss58, account_id_hex, hotkey_public_hex, hotkey_mnemonic_ct
               FROM deposit_accounts WHERE user_id = $1"#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|r| {
            (
                r.get("address_ss58"),
                r.get("account_id_hex"),
                r.get("hotkey_public_hex"),
                r.get("hotkey_mnemonic_ct"),
            )
        }))
    }

    async fn insert_tx(
        &self,
        tx: &mut PgTx<'_>,
        user_id: &str,
        addr: &str,
        acct_hex: &str,
        pub_hex: &str,
        mnemonic_ct: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"INSERT INTO deposit_accounts (user_id, address_ss58, account_id_hex, hotkey_public_hex, hotkey_mnemonic_ct)
               VALUES ($1,$2,$3,$4,$5)"#
        )
        .bind(user_id)
        .bind(addr)
        .bind(acct_hex)
        .bind(pub_hex)
        .bind(mnemonic_ct)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    async fn list_account_hexes(&self) -> Result<Vec<String>> {
        let rows = sqlx::query(r#"SELECT account_id_hex FROM deposit_accounts"#)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|r| r.get("account_id_hex")).collect())
    }
}
