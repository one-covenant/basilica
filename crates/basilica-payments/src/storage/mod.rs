use sqlx::{PgPool, Postgres, Transaction};

pub mod deposit_accounts;
pub mod observed_deposits;
pub mod outbox;

pub type PgTx<'a> = Transaction<'a, Postgres>;

#[async_trait::async_trait]
pub trait DepositAccountsRepo {
    async fn get_by_user(
        &self,
        user_id: &str,
    ) -> sqlx::Result<Option<(String, String, String, String)>>;
    async fn insert_tx(
        &self,
        tx: &mut PgTx<'_>,
        user_id: &str,
        addr: &str,
        acct_hex: &str,
        pub_hex: &str,
        mnemonic_ct: &str,
    ) -> sqlx::Result<()>;
    async fn list_account_hexes(&self) -> sqlx::Result<Vec<String>>;
}

pub struct ObservedRow {
    pub block_number: i64,
    pub event_index: i32,
    pub from_account_hex: String,
    pub to_account_hex: String,
    pub amount_plancks: String,
    pub status: String,
    pub observed_at_rfc3339: String,
    pub credited_at_rfc3339: String,
    pub billing_credit_id: String,
}

#[async_trait::async_trait]
pub trait ObservedDepositsRepo {
    async fn insert_finalized_tx(
        &self,
        tx: &mut PgTx<'_>,
        block: i64,
        idx: i32,
        to_hex: &str,
        from_hex: &str,
        amount: &str,
    ) -> sqlx::Result<()>;
    async fn mark_credited_tx(
        &self,
        tx: &mut PgTx<'_>,
        transaction_id: &str,
        credit_id: &str,
    ) -> sqlx::Result<()>;
    async fn list_by_user(
        &self,
        user_id: &str,
        limit: i64,
        offset: i64,
    ) -> sqlx::Result<Vec<ObservedRow>>;
}

pub struct OutboxRow {
    pub id: i64,
    pub user_id: String,
    pub amount_plancks: String,
    pub transaction_id: String,
    pub attempts: i32,
}

#[async_trait::async_trait]
pub trait OutboxRepo {
    async fn enqueue_tx(
        &self,
        tx: &mut PgTx<'_>,
        to_hex: &str,
        amount: &str,
        txid: &str,
    ) -> sqlx::Result<()>;
    async fn claim_batch(&self, limit: i64) -> sqlx::Result<Vec<OutboxRow>>;
    async fn mark_dispatched_tx(&self, tx: &mut PgTx<'_>, id: i64) -> sqlx::Result<()>;
    async fn backoff(&self, id: i64, secs: i64) -> sqlx::Result<()>;
    async fn get_pending_count(&self) -> sqlx::Result<usize>;
}

#[derive(Clone)]
pub struct PgRepos {
    pub pool: PgPool,
}

impl PgRepos {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn begin(&self) -> sqlx::Result<PgTx<'_>> {
        self.pool.begin().await
    }
}
