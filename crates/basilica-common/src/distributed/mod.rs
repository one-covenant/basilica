#[cfg(feature = "postgres")]
pub mod postgres_lock;

#[cfg(feature = "postgres")]
pub use postgres_lock::{AdvisoryLock, AdvisoryLockGuard, LeaderElection, LockError, LockKey};
