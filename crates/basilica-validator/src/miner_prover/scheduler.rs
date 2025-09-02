//! # Verification Scheduler
//!
//! Manages the scheduling and lifecycle of verification tasks.
//! Implements Single Responsibility Principle by focusing only on task scheduling.

use super::discovery::MinerDiscovery;
use super::types::{MinerInfo, ValidationType};
use super::verification::VerificationEngine;
use crate::config::VerificationConfig;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio::time::interval;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

#[derive(Clone)]
struct SchedulerSharedState {
    verification_handles:
        Arc<RwLock<HashMap<Uuid, JoinHandle<Result<super::verification::VerificationResult>>>>>,
    active_full_tasks: Arc<RwLock<HashMap<Uuid, VerificationTask>>>,
    active_lightweight_tasks: Arc<RwLock<HashMap<Uuid, VerificationTask>>>,
    full_validation_semaphore: Arc<tokio::sync::Semaphore>,
}

impl SchedulerSharedState {
    fn new(max_concurrent_full_validations: usize) -> Self {
        Self {
            verification_handles: Arc::new(RwLock::new(HashMap::new())),
            active_full_tasks: Arc::new(RwLock::new(HashMap::new())),
            active_lightweight_tasks: Arc::new(RwLock::new(HashMap::new())),
            full_validation_semaphore: Arc::new(tokio::sync::Semaphore::new(
                max_concurrent_full_validations,
            )),
        }
    }
}

pub struct VerificationScheduler {
    config: VerificationConfig,
    shared_state: SchedulerSharedState,
}

impl VerificationScheduler {
    pub fn new(config: VerificationConfig) -> Self {
        let max_concurrent_full_validations = config.max_concurrent_full_validations;
        info!(
            "[EVAL_FLOW] Initializing VerificationScheduler with max_concurrent_full_validations: {}",
            max_concurrent_full_validations
        );
        Self {
            config,
            shared_state: SchedulerSharedState::new(max_concurrent_full_validations),
        }
    }

    /// Start the verification scheduling loop
    pub async fn start(
        self,
        discovery: MinerDiscovery,
        verification: VerificationEngine,
    ) -> Result<()> {
        let shared_state = self.shared_state.clone();
        let config = self.config.clone();
        let discovery = Arc::new(discovery);
        let verification = Arc::new(verification);

        info!("Starting verification scheduler");
        info!(
            "Verification interval: {}s, Cleanup interval: {}s",
            config.verification_interval.as_secs(),
            900
        );

        // Full validation loop
        let full_shared_state = shared_state.clone();
        let full_config = config.clone();
        let full_discovery = discovery.clone();
        let full_verification = verification.clone();
        let full_loop = tokio::spawn(async move {
            let mut full_interval = interval(full_config.verification_interval);
            loop {
                tokio::select! {
                    _ = full_interval.tick() => {
                        if let Err(e) = run_full_validation(
                            &full_shared_state,
                            &full_config,
                            &full_discovery,
                            &full_verification,
                        ).await {
                            error!("Full validation cycle failed: {}", e);
                        }
                    }
                }
            }
        });

        // Lightweight validation loop
        let lightweight_shared_state = shared_state.clone();
        let lightweight_config = config.clone();
        let lightweight_discovery = discovery.clone();
        let lightweight_verification = verification.clone();
        let lightweight_loop = tokio::spawn(async move {
            let mut lightweight_interval = interval(lightweight_config.verification_interval);
            loop {
                tokio::select! {
                    _ = lightweight_interval.tick() => {
                        if let Err(e) = run_lightweight_validation(
                            &lightweight_shared_state,
                            &lightweight_config,
                            &lightweight_discovery,
                            &lightweight_verification,
                        ).await {
                            error!("Lightweight validation cycle failed: {}", e);
                        }
                    }
                }
            }
        });

        // Cleanup loop
        let cleanup_shared_state = shared_state.clone();
        let cleanup_verification = verification.clone();
        let cleanup_loop = tokio::spawn(async move {
            let mut cleanup_interval = tokio::time::interval(Duration::from_secs(900));
            loop {
                tokio::select! {
                    _ = cleanup_interval.tick() => {
                        cleanup_completed_verification_handles(&cleanup_shared_state).await;

                        info!("Running scheduled executor cleanup for failed executors");
                        match cleanup_verification.cleanup_failed_executors_after_failures(2).await {
                            Ok(()) => info!("Executor cleanup completed successfully"),
                            Err(e) => error!("Failed executor cleanup failed: {}", e),
                        }
                    }
                }
            }
        });

        // Run all loops concurrently
        let (full_result, lightweight_result, cleanup_result) =
            tokio::join!(full_loop, lightweight_loop, cleanup_loop);

        if let Err(e) = full_result {
            error!("Full validation loop panicked: {}", e);
        }
        if let Err(e) = lightweight_result {
            error!("Lightweight validation loop panicked: {}", e);
        }
        if let Err(e) = cleanup_result {
            error!("Cleanup loop panicked: {}", e);
        }

        Ok(())
    }

    pub async fn cleanup_completed_verification_handles(&self) {
        let mut to_remove: Vec<Uuid> = Vec::new();
        {
            let handles = self.shared_state.verification_handles.read().await;
            for (id, handle) in handles.iter() {
                if handle.is_finished() {
                    to_remove.push(*id);
                }
            }
        }
        if !to_remove.is_empty() {
            let mut to_await = Vec::new();
            {
                let mut full_tasks = self.shared_state.active_full_tasks.write().await;
                let mut lightweight_tasks =
                    self.shared_state.active_lightweight_tasks.write().await;
                let mut handles = self.shared_state.verification_handles.write().await;
                for id in &to_remove {
                    if let Some(h) = handles.remove(id) {
                        to_await.push((*id, h));
                    }
                    full_tasks.remove(id);
                    lightweight_tasks.remove(id);
                }
            }

            for (id, h) in to_await {
                if let Err(e) = h.await {
                    tracing::error!("[EVAL_FLOW] Verification task {} panicked: {}", id, e);
                }
            }
            tracing::debug!(
                "[EVAL_FLOW] Cleaned up {} completed verification tasks",
                to_remove.len()
            );
        }
    }
}

async fn run_full_validation(
    shared_state: &SchedulerSharedState,
    config: &VerificationConfig,
    discovery: &MinerDiscovery,
    verification: &VerificationEngine,
) -> Result<()> {
    info!("[EVAL_FLOW] Starting full validation cycle");
    let cycle_start = std::time::Instant::now();

    let discovered_miners = discovery.get_miners_for_verification().await?;
    if discovered_miners.is_empty() {
        return Ok(());
    }

    verification
        .sync_miners_from_metagraph(&discovered_miners)
        .await?;

    let schedulable_miners: Vec<MinerInfo> = discovered_miners
        .into_iter()
        .filter(|miner| can_schedule(shared_state, miner, ValidationType::Full))
        .collect();

    if schedulable_miners.is_empty() {
        return Ok(());
    }

    let full_tasks = spawn_validation_pipeline(
        shared_state,
        config,
        schedulable_miners,
        verification,
        ValidationType::Full,
    )
    .await?;

    info!(
        "[EVAL_FLOW] Full validation cycle completed in {:?}: {} tasks",
        cycle_start.elapsed(),
        full_tasks
    );

    Ok(())
}

async fn run_lightweight_validation(
    shared_state: &SchedulerSharedState,
    config: &VerificationConfig,
    discovery: &MinerDiscovery,
    verification: &VerificationEngine,
) -> Result<()> {
    info!("[EVAL_FLOW] Starting lightweight validation cycle");
    let cycle_start = std::time::Instant::now();

    let discovered_miners = discovery.get_miners_for_verification().await?;
    if discovered_miners.is_empty() {
        return Ok(());
    }

    verification
        .sync_miners_from_metagraph(&discovered_miners)
        .await?;

    let schedulable_miners: Vec<MinerInfo> = discovered_miners
        .into_iter()
        .filter(|miner| can_schedule(shared_state, miner, ValidationType::Lightweight))
        .collect();

    if schedulable_miners.is_empty() {
        return Ok(());
    }

    let lightweight_tasks = spawn_validation_pipeline(
        shared_state,
        config,
        schedulable_miners,
        verification,
        ValidationType::Lightweight,
    )
    .await?;

    info!(
        "[EVAL_FLOW] Lightweight validation cycle completed in {:?}: {} tasks",
        cycle_start.elapsed(),
        lightweight_tasks
    );

    Ok(())
}

async fn spawn_validation_pipeline(
    shared_state: &SchedulerSharedState,
    config: &VerificationConfig,
    miners: Vec<MinerInfo>,
    verification: &VerificationEngine,
    intended_strategy: ValidationType,
) -> Result<usize> {
    let mut tasks_spawned = 0;

    info!(
        "[EVAL_FLOW] Starting {:?} validation pipeline for {} miners",
        intended_strategy,
        miners.len()
    );

    for miner in miners {
        let verification_task = VerificationTask {
            miner_uid: miner.uid.as_u16(),
            miner_hotkey: miner.hotkey.to_string(),
            miner_endpoint: miner.endpoint.clone(),
            stake_tao: miner.stake_tao,
            is_validator: miner.is_validator,
            verification_type: VerificationType::AutomatedWithSsh,
            intended_validation_strategy: intended_strategy.clone(),
            created_at: chrono::Utc::now(),
            timeout: config.challenge_timeout,
        };

        match spawn_verification_task(shared_state, verification_task, verification).await {
            Ok(_) => tasks_spawned += 1,
            Err(e) => warn!(
                miner_uid = miner.uid.as_u16(),
                intended_strategy = ?intended_strategy,
                error = %e,
                "[EVAL_FLOW] Failed to spawn {:?} validation task",
                intended_strategy
            ),
        }
    }

    info!(
        "[EVAL_FLOW] {:?} validation pipeline spawned {} tasks",
        intended_strategy, tasks_spawned
    );

    Ok(tasks_spawned)
}

async fn spawn_verification_task(
    shared_state: &SchedulerSharedState,
    task: VerificationTask,
    verification: &VerificationEngine,
) -> Result<()> {
    let verification_engine = verification.clone();
    let task_id = uuid::Uuid::new_v4();

    info!(
        "[EVAL_FLOW] Spawning verification task {} for miner UID: {}",
        task_id, task.miner_uid
    );
    debug!(
        "[EVAL_FLOW] Task details: type={:?}, timeout={:?}, endpoint={}",
        task.verification_type, task.timeout, task.miner_endpoint
    );

    let active_tasks = match task.intended_validation_strategy {
        ValidationType::Full => &shared_state.active_full_tasks,
        ValidationType::Lightweight => &shared_state.active_lightweight_tasks,
    };

    info!(
        "[EVAL_FLOW] Registering {:?} validation task {} in active tasks tracker",
        task.intended_validation_strategy, task_id
    );
    {
        let mut tasks_map = active_tasks.write().await;
        tasks_map.insert(task_id, task.clone());
        info!(
            "[EVAL_FLOW] Active {:?} validation tasks count: {}",
            task.intended_validation_strategy,
            tasks_map.len()
        );
    }

    info!("[EVAL_FLOW] Spawning tokio task for verification workflow execution");

    // Clone the semaphore reference for use in the spawned task
    let semaphore = shared_state.full_validation_semaphore.clone();
    let is_full_validation = matches!(task.intended_validation_strategy, ValidationType::Full);

    let verification_handle = tokio::spawn(async move {
        // Acquire semaphore permit for full validations only
        let _permit = if is_full_validation {
            info!(
                "[EVAL_FLOW] Acquiring full validation permit for miner {} (task: {}), permits before acquire: {}",
                task.miner_uid, task_id, semaphore.available_permits()
            );
            match semaphore.acquire().await {
                Ok(permit) => {
                    info!(
                        "[EVAL_FLOW] Full validation permit acquired for miner {} (task: {}), available permits: {}",
                        task.miner_uid, task_id, semaphore.available_permits()
                    );
                    Some(permit)
                }
                Err(e) => {
                    error!(
                        "[EVAL_FLOW] Failed to acquire full validation permit for miner {} (task: {}): {}",
                        task.miner_uid, task_id, e
                    );
                    return Err(anyhow::anyhow!(
                        "Failed to acquire full validation permit: {}",
                        e
                    ));
                }
            }
        } else {
            debug!(
                "[EVAL_FLOW] Lightweight validation - no permit required for miner {} (task: {})",
                task.miner_uid, task_id
            );
            None
        };

        info!(
            "[EVAL_FLOW] Starting automated verification workflow for miner {} in task {}",
            task.miner_uid, task_id
        );
        let workflow_start = std::time::Instant::now();

        let result = verification_engine
            .execute_verification_workflow(&task)
            .await;

        // Permit is automatically released when _permit goes out of scope
        if is_full_validation {
            info!(
                "[EVAL_FLOW] Full validation completing for miner {} (task: {}), about to release permit, current available: {}",
                task.miner_uid, task_id, semaphore.available_permits()
            );
        }

        match result {
            Ok(verification_result) => {
                info!(
                    "[EVAL_FLOW] Automated verification completed for miner {} in {:?}: score={:.2} (task: {})",
                    task.miner_uid, workflow_start.elapsed(), verification_result.overall_score, task_id
                );
                debug!(
                    "[EVAL_FLOW] Verification steps completed: {}",
                    verification_result.verification_steps.len()
                );
                for step in &verification_result.verification_steps {
                    debug!(
                        "[EVAL_FLOW]   Step: {} - {:?} - {}",
                        step.step_name, step.status, step.details
                    );
                }
                Ok(verification_result)
            }
            Err(e) => {
                error!(
                    "[EVAL_FLOW] Automated verification failed for miner {} after {:?} (task: {}): {}",
                    task.miner_uid, workflow_start.elapsed(), task_id, e
                );
                Err(e)
            }
        }
    });

    info!(
        "[EVAL_FLOW] Storing verification handle for task {} cleanup tracking",
        task_id
    );
    {
        let mut verification_handles = shared_state.verification_handles.write().await;
        verification_handles.insert(task_id, verification_handle);
        info!(
            "[EVAL_FLOW] Total verification handles tracked: {}",
            verification_handles.len()
        );
    }

    Ok(())
}

fn can_schedule(
    shared_state: &SchedulerSharedState,
    miner: &MinerInfo,
    strategy: ValidationType,
) -> bool {
    let miner_uid = miner.uid.as_u16();

    let active_tasks = match strategy {
        ValidationType::Full => &shared_state.active_full_tasks,
        ValidationType::Lightweight => &shared_state.active_lightweight_tasks,
    };

    if let Ok(tasks_map) = active_tasks.try_read() {
        if tasks_map.values().any(|t| t.miner_uid == miner_uid) {
            debug!(
                "Miner {} already has an active {:?} validation task",
                miner_uid, strategy
            );
            return false;
        }
    }

    true
}

async fn cleanup_completed_verification_handles(shared_state: &SchedulerSharedState) {
    let mut to_remove: Vec<Uuid> = Vec::new();
    {
        let handles = shared_state.verification_handles.read().await;
        for (id, handle) in handles.iter() {
            if handle.is_finished() {
                to_remove.push(*id);
            }
        }
    }
    if !to_remove.is_empty() {
        let mut to_await = Vec::new();
        {
            let mut full_tasks = shared_state.active_full_tasks.write().await;
            let mut lightweight_tasks = shared_state.active_lightweight_tasks.write().await;
            let mut handles = shared_state.verification_handles.write().await;
            for id in &to_remove {
                if let Some(h) = handles.remove(id) {
                    to_await.push((*id, h));
                }
                full_tasks.remove(id);
                lightweight_tasks.remove(id);
            }
        }

        for (id, h) in to_await {
            if let Err(e) = h.await {
                tracing::error!("[EVAL_FLOW] Verification task {} panicked: {}", id, e);
            }
        }
        tracing::debug!(
            "[EVAL_FLOW] Cleaned up {} completed verification tasks",
            to_remove.len()
        );
    }
}

/// Enhanced verification task structure
#[derive(Debug, Clone)]
pub struct VerificationTask {
    pub miner_uid: u16,
    pub miner_hotkey: String,
    pub miner_endpoint: String,
    pub stake_tao: f64,
    pub is_validator: bool,
    pub verification_type: VerificationType,
    pub intended_validation_strategy: ValidationType,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub timeout: std::time::Duration,
}

/// Verification type specification
#[derive(Debug, Clone)]
pub enum VerificationType {
    Manual,
    AutomatedWithSsh,
    ScheduledRoutine,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::timeout;

    fn create_test_config(max_concurrent_full_validations: usize) -> VerificationConfig {
        VerificationConfig {
            verification_interval: Duration::from_secs(60),
            max_concurrent_verifications: 50,
            max_concurrent_full_validations,
            challenge_timeout: Duration::from_secs(120),
            min_score_threshold: 0.1,
            max_miners_per_round: 20,
            min_verification_interval: Duration::from_secs(1800),
            netuid: 39,
            use_dynamic_discovery: true,
            discovery_timeout: Duration::from_secs(30),
            fallback_to_static: true,
            cache_miner_info_ttl: Duration::from_secs(300),
            grpc_port_offset: None,
            binary_validation: crate::config::BinaryValidationConfig::default(),
            collateral_event_scan_interval: Duration::from_secs(12),
            executor_validation_interval: Duration::from_secs(6 * 3600),
            gpu_assignment_cleanup_ttl: Some(Duration::from_secs(120 * 60)),
        }
    }

    fn create_test_task(miner_uid: u16, validation_strategy: ValidationType) -> VerificationTask {
        VerificationTask {
            miner_uid,
            miner_hotkey: format!("hotkey_{}", miner_uid),
            miner_endpoint: format!("http://miner-{}.example.com:8091", miner_uid),
            stake_tao: 100.0,
            is_validator: false,
            verification_type: VerificationType::AutomatedWithSsh,
            intended_validation_strategy: validation_strategy,
            created_at: chrono::Utc::now(),
            timeout: Duration::from_secs(300),
        }
    }

    #[tokio::test]
    async fn test_scheduler_shared_state_initialization() {
        let max_concurrent_full_validations = 3;
        let shared_state = SchedulerSharedState::new(max_concurrent_full_validations);

        // Verify semaphore is initialized with correct permit count
        assert_eq!(
            shared_state.full_validation_semaphore.available_permits(),
            max_concurrent_full_validations
        );

        // Verify other components are initialized
        assert_eq!(shared_state.verification_handles.read().await.len(), 0);
        assert_eq!(shared_state.active_full_tasks.read().await.len(), 0);
        assert_eq!(shared_state.active_lightweight_tasks.read().await.len(), 0);
    }

    #[tokio::test]
    async fn test_verification_scheduler_initialization() {
        let config = create_test_config(2);
        let scheduler = VerificationScheduler::new(config.clone());

        // Verify scheduler uses config value for semaphore initialization
        assert_eq!(
            scheduler
                .shared_state
                .full_validation_semaphore
                .available_permits(),
            config.max_concurrent_full_validations
        );
    }

    #[tokio::test]
    async fn test_full_validation_concurrency_limit() {
        let max_concurrent = 1;
        let shared_state = SchedulerSharedState::new(max_concurrent);

        // Create a mock verification engine that simulates work
        let execution_count = Arc::new(AtomicUsize::new(0));
        let max_concurrent_executions = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();

        // Spawn multiple full validation tasks
        for i in 1..=3 {
            let _task = create_test_task(i, ValidationType::Full);
            let semaphore = shared_state.full_validation_semaphore.clone();
            let exec_count = execution_count.clone();
            let max_concurrent_exec = max_concurrent_executions.clone();

            let handle = tokio::spawn(async move {
                let _permit = semaphore.acquire().await.unwrap();

                // Track concurrent executions
                let current = exec_count.fetch_add(1, Ordering::SeqCst) + 1;
                let max = max_concurrent_exec.load(Ordering::SeqCst);
                if current > max {
                    max_concurrent_exec.store(current, Ordering::SeqCst);
                }

                // Simulate work
                tokio::time::sleep(Duration::from_millis(100)).await;

                exec_count.fetch_sub(1, Ordering::SeqCst);
            });

            handles.push(handle);
        }

        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify that only 1 task ran concurrently at any time
        assert_eq!(
            max_concurrent_executions.load(Ordering::SeqCst),
            max_concurrent
        );

        // Verify semaphore is back to full capacity
        assert_eq!(
            shared_state.full_validation_semaphore.available_permits(),
            max_concurrent
        );
    }

    #[tokio::test]
    async fn test_lightweight_validation_not_limited() {
        let shared_state = SchedulerSharedState::new(1); // Only 1 full validation permit

        // Create multiple lightweight validation tasks
        let mut handles = Vec::new();
        let execution_count = Arc::new(AtomicUsize::new(0));
        let max_concurrent_executions = Arc::new(AtomicUsize::new(0));

        for i in 1..=5 {
            let _task = create_test_task(i, ValidationType::Lightweight);
            let exec_count = execution_count.clone();
            let max_concurrent_exec = max_concurrent_executions.clone();

            let handle = tokio::spawn(async move {
                // Lightweight validations don't use semaphore
                let current = exec_count.fetch_add(1, Ordering::SeqCst) + 1;
                let max = max_concurrent_exec.load(Ordering::SeqCst);
                if current > max {
                    max_concurrent_exec.store(current, Ordering::SeqCst);
                }

                tokio::time::sleep(Duration::from_millis(50)).await;
                exec_count.fetch_sub(1, Ordering::SeqCst);
            });

            handles.push(handle);
        }

        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify that multiple lightweight validations can run concurrently
        assert!(max_concurrent_executions.load(Ordering::SeqCst) > 1);

        // Verify semaphore is unaffected
        assert_eq!(
            shared_state.full_validation_semaphore.available_permits(),
            1
        );
    }

    #[tokio::test]
    async fn test_mixed_validation_types() {
        let shared_state = SchedulerSharedState::new(1); // Only 1 full validation permit

        let mut handles = Vec::new();
        let full_exec_count = Arc::new(AtomicUsize::new(0));
        let lightweight_exec_count = Arc::new(AtomicUsize::new(0));

        // Spawn full validation tasks (should be limited to 1)
        for i in 1..=2 {
            let _task = create_test_task(i, ValidationType::Full);
            let semaphore = shared_state.full_validation_semaphore.clone();
            let exec_count = full_exec_count.clone();

            let handle = tokio::spawn(async move {
                let _permit = semaphore.acquire().await.unwrap();
                exec_count.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(100)).await;
                exec_count.fetch_sub(1, Ordering::SeqCst);
            });

            handles.push(handle);
        }

        // Spawn lightweight validation tasks (should not be limited)
        for i in 3..=5 {
            let _task = create_test_task(i, ValidationType::Lightweight);
            let exec_count = lightweight_exec_count.clone();

            let handle = tokio::spawn(async move {
                exec_count.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(50)).await;
                exec_count.fetch_sub(1, Ordering::SeqCst);
            });

            handles.push(handle);
        }

        // Wait for all tasks to complete
        for handle in handles {
            handle.await.unwrap();
        }

        // Verify semaphore is back to full capacity
        assert_eq!(
            shared_state.full_validation_semaphore.available_permits(),
            1
        );
    }

    #[tokio::test]
    async fn test_semaphore_permit_timeout_behavior() {
        let shared_state = SchedulerSharedState::new(1);

        // Acquire the only permit
        let _permit1 = shared_state
            .full_validation_semaphore
            .acquire()
            .await
            .unwrap();

        // Try to acquire another permit with timeout - should fail
        let result = timeout(
            Duration::from_millis(100),
            shared_state.full_validation_semaphore.acquire(),
        )
        .await;

        assert!(result.is_err(), "Second permit acquisition should timeout");

        // Verify permit count is still 0
        assert_eq!(
            shared_state.full_validation_semaphore.available_permits(),
            0
        );

        // Drop the first permit
        drop(_permit1);

        // Now permit should be available again
        assert_eq!(
            shared_state.full_validation_semaphore.available_permits(),
            1
        );
    }

    #[tokio::test]
    async fn test_arc_semaphore_cloning_shares_permits() {
        let shared_state = SchedulerSharedState::new(1);

        // Clone the semaphore (like we do in spawn_verification_task)
        let cloned_semaphore1 = shared_state.full_validation_semaphore.clone();
        let cloned_semaphore2 = shared_state.full_validation_semaphore.clone();

        // All should show 1 available permit initially
        assert_eq!(
            shared_state.full_validation_semaphore.available_permits(),
            1
        );
        assert_eq!(cloned_semaphore1.available_permits(), 1);
        assert_eq!(cloned_semaphore2.available_permits(), 1);

        // Acquire permit from first clone
        let _permit1 = cloned_semaphore1.acquire().await.unwrap();

        // All should show 0 available permits (proving they share the same semaphore)
        assert_eq!(
            shared_state.full_validation_semaphore.available_permits(),
            0
        );
        assert_eq!(cloned_semaphore1.available_permits(), 0);
        assert_eq!(cloned_semaphore2.available_permits(), 0);

        // Try to acquire from second clone - should block
        let result = timeout(Duration::from_millis(50), cloned_semaphore2.acquire()).await;

        assert!(
            result.is_err(),
            "Second clone should not be able to acquire permit"
        );

        // Release permit
        drop(_permit1);

        // All should show 1 available permit again
        assert_eq!(
            shared_state.full_validation_semaphore.available_permits(),
            1
        );
        assert_eq!(cloned_semaphore1.available_permits(), 1);
        assert_eq!(cloned_semaphore2.available_permits(), 1);
    }
}
