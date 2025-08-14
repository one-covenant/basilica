use sqlx::{pool::PoolConnection, PgPool, Postgres};
use std::fmt;
use thiserror::Error;
use tracing::{debug, info, warn};

#[derive(Debug, Error)]
pub enum LockError {
    #[error("Lock is already held by another instance")]
    AlreadyHeld,

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Lock timeout after {0} seconds")]
    Timeout(u64),
}

/// A unique key identifying a specific lock
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LockKey(i64);

impl LockKey {
    pub const fn new(key: i64) -> Self {
        Self(key)
    }

    pub const fn from_components(high: i32, low: i32) -> Self {
        Self(((high as i64) << 32) | (low as i64 & 0xFFFFFFFF))
    }

    pub fn value(&self) -> i64 {
        self.0
    }
}

impl fmt::Display for LockKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{:016X}", self.0)
    }
}

impl LockKey {
    pub const PAYMENTS_MONITOR: LockKey = LockKey::new(0x00B1_77A0_0001);
}

/// A guard that holds a PostgreSQL advisory lock
///
/// The lock is automatically released when this guard is dropped
pub struct AdvisoryLockGuard {
    conn: PoolConnection<Postgres>,
    key: LockKey,
}

impl AdvisoryLockGuard {
    pub fn key(&self) -> LockKey {
        self.key
    }

    /// Release the lock explicitly
    ///
    /// This is not usually necessary as the lock is released when dropped,
    /// but can be useful for testing.
    pub async fn release(mut self) -> Result<(), LockError> {
        let released: bool = sqlx::query_scalar::<_, bool>("SELECT pg_advisory_unlock($1)")
            .bind(self.key.value())
            .fetch_one(&mut *self.conn)
            .await?;

        if released {
            info!("Released advisory lock {}", self.key);
            Ok(())
        } else {
            warn!(
                "Failed to release advisory lock {} - was not held",
                self.key
            );
            Ok(())
        }
    }
}

impl Drop for AdvisoryLockGuard {
    fn drop(&mut self) {
        debug!("Dropping advisory lock guard for key {}", self.key);
        // The lock is automatically released when the connection is returned to the pool
    }
}

/// PostgreSQL Advisory Lock manager
///
/// Provides distributed locking capabilities using PostgreSQL advisory locks.
/// These locks are automatically released if the connection is closed (crash safety).
#[derive(Clone)]
pub struct AdvisoryLock {
    pool: PgPool,
}

impl AdvisoryLock {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Try to acquire a session-level advisory lock (non-blocking)
    ///
    /// Returns a guard if the lock was acquired, or an error if it's already held.
    /// The lock is automatically released when the guard is dropped.
    ///
    /// # Example
    /// ```no_run
    /// use basilica_common::distributed::{AdvisoryLock, LockKey};
    ///
    /// async fn become_leader(lock_manager: &AdvisoryLock) -> Result<(), Box<dyn std::error::Error>> {
    ///     let guard = lock_manager.try_acquire(LockKey::PAYMENTS_MONITOR).await?;
    ///     println!("I am the leader!");
    ///
    ///     // Do leader work...
    ///
    ///     // Lock is automatically released when guard is dropped
    ///     Ok(())
    /// }
    /// ```
    pub async fn try_acquire(&self, key: LockKey) -> Result<AdvisoryLockGuard, LockError> {
        let mut conn = self.pool.acquire().await?;

        let acquired: bool = sqlx::query_scalar::<_, bool>("SELECT pg_try_advisory_lock($1)")
            .bind(key.value())
            .fetch_one(&mut *conn)
            .await?;

        if acquired {
            info!("Acquired advisory lock {}", key);
            Ok(AdvisoryLockGuard { conn, key })
        } else {
            debug!("Failed to acquire advisory lock {} - already held", key);
            Err(LockError::AlreadyHeld)
        }
    }

    /// Acquire a session-level advisory lock (blocking with timeout)
    ///
    /// Waits up to `timeout_secs` seconds to acquire the lock.
    /// Returns a guard if the lock was acquired within the timeout.
    ///
    /// # Example
    /// ```no_run
    /// use basilica_common::distributed::{AdvisoryLock, LockKey};
    ///
    /// async fn wait_for_leadership(lock_manager: &AdvisoryLock) -> Result<(), Box<dyn std::error::Error>> {
    ///     // Wait up to 30 seconds to become leader
    ///     let guard = lock_manager.acquire_with_timeout(LockKey::PAYMENTS_MONITOR, 30).await?;
    ///     println!("Became leader after waiting");
    ///
    ///     // Do leader work...
    ///     Ok(())
    /// }
    /// ```
    pub async fn acquire_with_timeout(
        &self,
        key: LockKey,
        timeout_secs: u64,
    ) -> Result<AdvisoryLockGuard, LockError> {
        let mut conn = self.pool.acquire().await?;

        // Use PostgreSQL's lock_timeout to implement timeout behavior
        sqlx::query("SET LOCAL lock_timeout = $1")
            .bind(format!("{}s", timeout_secs))
            .execute(&mut *conn)
            .await?;

        let acquired: bool = match sqlx::query_scalar::<_, ()>("SELECT pg_advisory_lock($1)")
            .bind(key.value())
            .fetch_one(&mut *conn)
            .await
        {
            Ok(_) => true,
            Err(sqlx::Error::Database(e)) if e.message().contains("lock timeout") => {
                return Err(LockError::Timeout(timeout_secs));
            }
            Err(e) => return Err(e.into()),
        };

        if acquired {
            info!("Acquired advisory lock {} after waiting", key);
            Ok(AdvisoryLockGuard { conn, key })
        } else {
            Err(LockError::AlreadyHeld)
        }
    }

    /// Check if a lock is currently held (by any connection)
    ///
    /// This is useful for monitoring and debugging.
    pub async fn is_locked(&self, key: LockKey) -> Result<bool, LockError> {
        let locked: bool = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                SELECT 1 FROM pg_locks
                WHERE locktype = 'advisory'
                AND objid = $1
            )",
        )
        .bind(key.value())
        .fetch_one(&self.pool)
        .await?;

        Ok(locked)
    }

    /// Get information about who holds a specific lock
    ///
    /// Returns the process ID of the PostgreSQL backend holding the lock,
    /// or None if the lock is not held.
    pub async fn lock_holder(&self, key: LockKey) -> Result<Option<i32>, LockError> {
        let pid: Option<i32> = sqlx::query_scalar::<_, i32>(
            "SELECT pid FROM pg_locks
             WHERE locktype = 'advisory'
             AND objid = $1
             LIMIT 1",
        )
        .bind(key.value())
        .fetch_optional(&self.pool)
        .await?;

        Ok(pid)
    }
}

/// Leader election helper using advisory locks
///
/// This provides a higher-level interface for implementing leader election patterns.
pub struct LeaderElection {
    lock_manager: AdvisoryLock,
    key: LockKey,
    retry_interval_secs: u64,
}

impl LeaderElection {
    /// Create a new leader election instance
    pub fn new(pool: PgPool, key: LockKey) -> Self {
        Self {
            lock_manager: AdvisoryLock::new(pool),
            key,
            retry_interval_secs: 3,
        }
    }

    /// Set the retry interval for failed lock acquisition attempts
    pub fn with_retry_interval(mut self, secs: u64) -> Self {
        self.retry_interval_secs = secs;
        self
    }

    /// Run a function as leader, with automatic failover
    ///
    /// This will continuously attempt to become leader and run the provided
    /// async function. If leadership is lost (function returns or errors),
    /// it will retry after the configured interval.
    ///
    /// # Example
    /// ```no_run
    /// use basilica_common::distributed::{LeaderElection, LockKey};
    /// use sqlx::PgPool;
    ///
    /// async fn monitor_blockchain() -> Result<(), Box<dyn std::error::Error>> {
    ///     println!("Monitoring blockchain as leader...");
    ///     // Blockchain monitoring logic here
    ///     Ok(())
    /// }
    ///
    /// async fn run_with_leader_election(pool: PgPool) {
    ///     let election = LeaderElection::new(pool, LockKey::PAYMENTS_MONITOR);
    ///
    ///     election.run_as_leader(|| async {
    ///         monitor_blockchain().await
    ///     }).await;
    /// }
    /// ```
    pub async fn run_as_leader<F, Fut>(&self, f: F) -> !
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<(), Box<dyn std::error::Error>>>,
    {
        loop {
            match self.lock_manager.try_acquire(self.key).await {
                Ok(_guard) => {
                    info!("Became leader for {}", self.key);

                    // Run the leader function
                    if let Err(e) = f().await {
                        warn!("Leader function error: {}", e);
                    }

                    // Guard is dropped here, releasing the lock
                    info!("Lost leadership for {}", self.key);
                }
                Err(LockError::AlreadyHeld) => {
                    debug!("Waiting for leadership of {}", self.key);
                }
                Err(e) => {
                    warn!("Error acquiring leader lock: {}", e);
                }
            }

            // Wait before retrying
            tokio::time::sleep(std::time::Duration::from_secs(self.retry_interval_secs)).await;
        }
    }
}
