use crate::persistence::SimplePersistence;
use alloy_primitives::{Address, U256};
use chrono::Utc;
use collateral_contract::config::CONTRACT_DEPLOYED_BLOCK_NUMBER;
use collateral_contract::{Deposit, Reclaimed, Slashed};
use hex::ToHex;
use sqlx::Row;
use tracing::warn;

impl SimplePersistence {
    pub async fn create_collateral_scanned_blocks_table(&self) -> Result<(), anyhow::Error> {
        let now = Utc::now().to_rfc3339();
        let query = r#"
            CREATE TABLE IF NOT EXISTS collateral_status (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                hotkey TEXT NOT NULL,
                executor_id TEXT NOT NULL,
                miner TEXT NOT NULL,
                collateral TEXT NOT NULL,
                url TEXT,
                url_content_md5_checksum TEXT,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(hotkey, executor_id)
            );

            CREATE TABLE IF NOT EXISTS collateral_scan_status (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                last_scanned_block_number INTEGER NOT NULL,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP
            );
        "#;

        sqlx::query(&query).execute(self.pool()).await?;

        let index = r#"
            CREATE INDEX IF NOT EXISTS idx_collateral_status ON collateral_status(hotkey, executor_id);
            "#;

        sqlx::query(&index).execute(self.pool()).await?;

        let insert_initial_scan_row = r#"
            INSERT INTO collateral_scan_status (last_scanned_block_number, updated_at) VALUES (?, ?);
        "#;
        sqlx::query(&insert_initial_scan_row)
            .bind(CONTRACT_DEPLOYED_BLOCK_NUMBER as i64)
            .bind(now)
            .execute(self.pool())
            .await?;

        Ok(())
    }

    pub async fn get_last_scanned_block_number(&self) -> Result<u64, anyhow::Error> {
        let query = "SELECT last_scanned_block_number FROM collateral_scan_status WHERE id = 1";

        let row = sqlx::query(&query).fetch_one(self.pool()).await?;

        let block_number: i64 = row.get(0);
        Ok(block_number as u64)
    }

    pub async fn update_last_scanned_block_number(
        &self,
        last_scanned_block: u64,
    ) -> Result<(), anyhow::Error> {
        let now = Utc::now().to_rfc3339();
        let query =
            "UPDATE collateral_scan_status SET last_scanned_block_number = ?, updated_at = ? WHERE id = 1";

        sqlx::query(&query)
            .bind(last_scanned_block as i64)
            .bind(now)
            .execute(self.pool())
            .await?;

        Ok(())
    }

    pub async fn get_collateral_status_id(
        &self,
        hotkey: &str,
        executor_id: &str,
    ) -> Result<Option<(i64, U256)>, anyhow::Error> {
        let query =
            "SELECT id, collateral FROM collateral_status WHERE hotkey = ? AND executor_id = ?";

        let row = sqlx::query(&query)
            .bind(hotkey)
            .bind(executor_id)
            .fetch_optional(self.pool())
            .await?;

        if let Some(row) = row {
            let id: i64 = row.get(0);
            let collateral_str: String = row.get(1);
            let collateral = U256::from_str_radix(&collateral_str, 10)
                .map_err(|_| anyhow::anyhow!("Invalid collateral"))?;
            Ok(Some((id, collateral)))
        } else {
            Ok(None)
        }
    }

    pub async fn handle_deposit(&self, deposit: &Deposit) -> Result<(), anyhow::Error> {
        match self
            .get_collateral_status_id(
                deposit.hotkey.encode_hex::<String>().as_str(),
                deposit.executorId.encode_hex::<String>().as_str(),
            )
            .await?
        {
            Some((id, collateral)) => {
                let now = Utc::now().to_rfc3339();
                let query =
                    "UPDATE collateral_status SET collateral = ?, updated_at = ? WHERE id = ?";
                let new_collateral = collateral.saturating_add(deposit.amount);
                sqlx::query(&query)
                    .bind(new_collateral.to_string())
                    .bind(now)
                    .bind(id)
                    .execute(self.pool())
                    .await?;
            }
            None => {
                let query = "INSERT INTO collateral_status (hotkey, executor_id, miner, collateral) VALUES (?, ?, ?, ?)";
                sqlx::query(&query)
                    .bind(deposit.hotkey.encode_hex::<String>())
                    .bind(deposit.executorId.encode_hex::<String>())
                    .bind(format!(
                        "0x{}",
                        deposit.miner.as_slice().encode_hex::<String>()
                    ))
                    .bind(deposit.amount.to_string())
                    .execute(self.pool())
                    .await?;
            }
        }

        Ok(())
    }

    pub async fn handle_reclaimed(&self, reclaimed: &Reclaimed) -> Result<(), anyhow::Error> {
        match self
            .get_collateral_status_id(
                reclaimed.hotkey.encode_hex::<String>().as_str(),
                reclaimed.executorId.encode_hex::<String>().as_str(),
            )
            .await?
        {
            Some((id, collateral)) => {
                let now = Utc::now().to_rfc3339();
                let query =
                    "UPDATE collateral_status SET collateral = ?, updated_at = ? WHERE id = ?";
                let new_collateral = collateral.saturating_sub(reclaimed.amount);
                sqlx::query(&query)
                    .bind(new_collateral.to_string())
                    .bind(now)
                    .bind(id)
                    .execute(self.pool())
                    .await?;
                Ok(())
            }
            None => Err(anyhow::anyhow!("Collateral status not found")),
        }
    }

    pub async fn handle_slashed(&self, slashed: &Slashed) -> Result<(), anyhow::Error> {
        match self
            .get_collateral_status_id(
                slashed.hotkey.encode_hex::<String>().as_str(),
                slashed.executorId.encode_hex::<String>().as_str(),
            )
            .await?
        {
            Some((id, collateral)) => {
                let now = Utc::now().to_rfc3339();
                let query = "UPDATE collateral_status SET collateral = ?, miner = ? , url = ? , url_content_md5_checksum = ?, updated_at = ? WHERE id = ?";
                if slashed.amount != collateral {
                    warn!(
                        "Slashed amount {} does not match collateral {} in database",
                        slashed.amount, collateral
                    );
                }

                sqlx::query(&query)
                    .bind("0".to_string())
                    .bind(format!(
                        "0x{}",
                        Address::ZERO.as_slice().encode_hex::<String>()
                    ))
                    .bind(slashed.url.clone())
                    .bind(slashed.urlContentMd5Checksum.encode_hex::<String>())
                    .bind(now)
                    .bind(id)
                    .execute(self.pool())
                    .await?;
                Ok(())
            }
            None => Err(anyhow::anyhow!("Collateral status not found")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::FixedBytes;

    fn make_hotkey(byte: u8) -> [u8; 32] {
        [byte; 32]
    }
    fn make_executor_id(byte: u8) -> [u8; 16] {
        [byte; 16]
    }

    fn ev_deposit(hk: [u8; 32], ex: [u8; 16], amount: u64) -> Deposit {
        Deposit {
            hotkey: FixedBytes::from_slice(&hk),
            executorId: FixedBytes::from_slice(&ex),
            miner: Address::from_slice(&[0u8; 20]),
            amount: U256::from(amount),
        }
    }
    fn ev_reclaimed(hk: [u8; 32], ex: [u8; 16], amount: u64) -> Reclaimed {
        Reclaimed {
            reclaimRequestId: U256::from(1u64),
            hotkey: FixedBytes::from_slice(&hk),
            executorId: FixedBytes::from_slice(&ex),
            miner: Address::from_slice(&[0u8; 20]),
            amount: U256::from(amount),
        }
    }
    fn ev_slashed(hk: [u8; 32], ex: [u8; 16], amount: u64) -> Slashed {
        Slashed {
            hotkey: FixedBytes::from_slice(&hk),
            executorId: FixedBytes::from_slice(&ex),
            miner: Address::from_slice(&[0u8; 20]),
            amount: U256::from(amount),
            url: String::new(),
            urlContentMd5Checksum: FixedBytes::from_slice(&[0u8; 16]),
        }
    }

    #[tokio::test]
    async fn test_tables_and_index_creation() {
        let db_path = ":memory:";
        let persistence = SimplePersistence::new(db_path, "validator".to_string())
            .await
            .expect("persistence");

        persistence
            .create_collateral_scanned_blocks_table()
            .await
            .expect("create tables");

        // basic sanity: insert initial scan row
        sqlx::query("INSERT INTO collateral_scan_status (last_scanned_block_number) VALUES (0)")
            .execute(persistence.pool())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_scan_block_number_roundtrip() {
        let db_path = ":memory:";
        let persistence = SimplePersistence::new(db_path, "validator".to_string())
            .await
            .unwrap();
        persistence
            .create_collateral_scanned_blocks_table()
            .await
            .unwrap();

        // seed row
        sqlx::query("INSERT INTO collateral_scan_status (last_scanned_block_number) VALUES (1)")
            .execute(persistence.pool())
            .await
            .unwrap();

        let n = persistence.get_last_scanned_block_number().await.unwrap();
        assert_eq!(n, 1);

        persistence
            .update_last_scanned_block_number(42)
            .await
            .unwrap();

        let n2: i64 =
            sqlx::query_scalar("SELECT last_scanned_block_number FROM collateral_scan_status")
                .fetch_one(persistence.pool())
                .await
                .unwrap();
        assert_eq!(n2 as u64, 42);
    }

    #[tokio::test]
    async fn test_handle_deposit_insert_and_update() {
        let db_path = ":memory:";
        let persistence = SimplePersistence::new(db_path, "validator".to_string())
            .await
            .unwrap();
        persistence
            .create_collateral_scanned_blocks_table()
            .await
            .unwrap();

        let hk = make_hotkey(1);
        let ex = make_executor_id(2);

        // first deposit inserts
        let d1 = ev_deposit(hk, ex, 100);
        persistence.handle_deposit(&d1).await.unwrap();

        let coll1: String = sqlx::query_scalar(
            "SELECT collateral FROM collateral_status WHERE hotkey = ? AND executor_id = ?",
        )
        .bind(d1.hotkey.encode_hex::<String>())
        .bind(d1.executorId.encode_hex::<String>())
        .fetch_one(persistence.pool())
        .await
        .unwrap();
        assert_eq!(coll1, "100");

        // second deposit updates
        let d2 = ev_deposit(hk, ex, 50);
        persistence.handle_deposit(&d2).await.unwrap();
        let coll2: String = sqlx::query_scalar(
            "SELECT collateral FROM collateral_status WHERE hotkey = ? AND executor_id = ?",
        )
        .bind(d1.hotkey.encode_hex::<String>())
        .bind(d1.executorId.encode_hex::<String>())
        .fetch_one(persistence.pool())
        .await
        .unwrap();
        assert_eq!(coll2, "150");
    }

    #[tokio::test]
    async fn test_handle_reclaimed_and_slashed() {
        let db_path = ":memory:";
        let persistence = SimplePersistence::new(db_path, "validator".to_string())
            .await
            .unwrap();
        persistence
            .create_collateral_scanned_blocks_table()
            .await
            .unwrap();

        let hk = make_hotkey(9);
        let ex = make_executor_id(7);

        // seed with deposit 200
        let d = ev_deposit(hk, ex, 200);
        persistence.handle_deposit(&d).await.unwrap();

        // reclaim 80
        let r = ev_reclaimed(hk, ex, 80);
        persistence.handle_reclaimed(&r).await.unwrap();
        let coll_after_reclaim: String = sqlx::query_scalar(
            "SELECT collateral FROM collateral_status WHERE hotkey = ? AND executor_id = ?",
        )
        .bind(d.hotkey.encode_hex::<String>())
        .bind(d.executorId.encode_hex::<String>())
        .fetch_one(persistence.pool())
        .await
        .unwrap();
        assert_eq!(coll_after_reclaim, "120");

        // slash 20
        let s = ev_slashed(hk, ex, 20);
        persistence.handle_slashed(&s).await.unwrap();
        let coll_after_slash: String = sqlx::query_scalar(
            "SELECT collateral FROM collateral_status WHERE hotkey = ? AND executor_id = ?",
        )
        .bind(d.hotkey.encode_hex::<String>())
        .bind(d.executorId.encode_hex::<String>())
        .fetch_one(persistence.pool())
        .await
        .unwrap();
        assert_eq!(coll_after_slash, "100");
    }
}
