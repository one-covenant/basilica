use crate::persistence::SimplePersistence;
use collateral_contract::{Deposit, Reclaimed, Slashed};

impl SimplePersistence {
    pub async fn get_last_scanned_block_number(&self) -> Result<u64, anyhow::Error> {
        // let last_scanned_block = self.pool.get_last_scanned_block().await?;
        // Ok(last_scanned_block)

        let query = "SELECT last_scanned_block FROM collateral_scanned_blocks ORDER BY last_scanned_block DESC LIMIT 1";

        // let rows = sqlx::query(&query).fetch_all(&self.pool()).await?;

        Ok(1_000_000u64)
    }

    pub async fn update_last_scanned_block_number(
        &self,
        last_scanned_block: u64,
    ) -> Result<(), anyhow::Error> {
        // let last_scanned_block = self.pool.get_last_scanned_block().await?;
        // Ok(last_scanned_block)

        let query = "SELECT last_scanned_block FROM collateral_scanned_blocks ORDER BY last_scanned_block DESC LIMIT 1";

        // let rows = sqlx::query(&query).fetch_all(&self.pool()).await?;

        Ok(())
    }

    pub async fn handle_deposit(&self, deposit: &Deposit) -> Result<(), anyhow::Error> {
        let query =
            "INSERT INTO collateral_deposits (hotkey, executor_id, amount) VALUES (?, ?, ?)";

        // let rows = sqlx::query(&query)
        //     .bind(deposit.hotkey)
        //     .bind(deposit.executor_id)
        //     .bind(deposit.amount)
        //     .execute(&self.pool())
        //     .await?;

        Ok(())
    }

    pub async fn handle_reclaimed(&self, reclaimed: &Reclaimed) -> Result<(), anyhow::Error> {
        let query =
            "INSERT INTO collateral_deposits (hotkey, executor_id, amount) VALUES (?, ?, ?)";

        // let rows = sqlx::query(&query)
        //     .bind(deposit.hotkey)
        //     .bind(deposit.executor_id)
        //     .bind(deposit.amount)
        //     .execute(&self.pool())
        //     .await?;

        Ok(())
    }

    pub async fn handle_slashed(&self, slashed: &Slashed) -> Result<(), anyhow::Error> {
        let query =
            "INSERT INTO collateral_deposits (hotkey, executor_id, amount) VALUES (?, ?, ?)";

        // let rows = sqlx::query(&query)
        //     .bind(deposit.hotkey)
        //     .bind(deposit.executor_id)
        //     .bind(deposit.amount)
        //     .execute(&self.pool())
        //     .await?;

        Ok(())
    }
}
