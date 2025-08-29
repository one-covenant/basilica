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
use tokio::sync::{Mutex, RwLock};
use tokio::task::JoinHandle;
use tokio::time::interval;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

pub struct VerificationScheduler {
    config: VerificationConfig,
    /// For tracking verification tasks by UUID
    verification_handles:
        Arc<RwLock<HashMap<Uuid, JoinHandle<Result<super::verification::VerificationResult>>>>>,
    /// For tracking active full validation tasks by UUID
    active_full_tasks: Arc<RwLock<HashMap<Uuid, VerificationTask>>>,
    /// For tracking active lightweight validation tasks by UUID
    active_lightweight_tasks: Arc<RwLock<HashMap<Uuid, VerificationTask>>>,
}

impl VerificationScheduler {
    pub fn new(config: VerificationConfig) -> Self {
        Self {
            config,
            verification_handles: Arc::new(RwLock::new(HashMap::new())),
            active_full_tasks: Arc::new(RwLock::new(HashMap::new())),
            active_lightweight_tasks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Start the verification scheduling loop
    pub async fn start(
        self,
        discovery: MinerDiscovery,
        verification: VerificationEngine,
    ) -> Result<()> {
        let scheduler = Arc::new(Mutex::new(self));
        let discovery = Arc::new(discovery);
        let verification = Arc::new(verification);

        info!("Starting verification scheduler");
        info!(
            "Verification interval: {}s, Cleanup interval: {}s",
            scheduler
                .lock()
                .await
                .config
                .verification_interval
                .as_secs(),
            900
        );

        // Full validation loop
        let full_scheduler = scheduler.clone();
        let full_discovery = discovery.clone();
        let full_verification = verification.clone();
        let full_loop = tokio::spawn(async move {
            let mut full_interval =
                interval(full_scheduler.lock().await.config.verification_interval);
            loop {
                tokio::select! {
                    _ = full_interval.tick() => {
                        if let Err(e) = full_scheduler.lock().await
                            .run_full_validation(&full_discovery, &full_verification).await {
                            error!("Full validation cycle failed: {}", e);
                        }
                    }
                }
            }
        });

        // Lightweight validation loop
        let lightweight_scheduler = scheduler.clone();
        let lightweight_discovery = discovery.clone();
        let lightweight_verification = verification.clone();
        let lightweight_loop = tokio::spawn(async move {
            let mut lightweight_interval = interval(
                lightweight_scheduler
                    .lock()
                    .await
                    .config
                    .verification_interval,
            );
            loop {
                tokio::select! {
                    _ = lightweight_interval.tick() => {
                        if let Err(e) = lightweight_scheduler.lock().await
                            .run_lightweight_validation(&lightweight_discovery, &lightweight_verification).await {
                            error!("Lightweight validation cycle failed: {}", e);
                        }
                    }
                }
            }
        });

        // Cleanup loop
        let cleanup_scheduler = scheduler.clone();
        let cleanup_verification = verification.clone();
        let cleanup_loop = tokio::spawn(async move {
            let mut cleanup_interval = tokio::time::interval(Duration::from_secs(900));
            loop {
                tokio::select! {
                    _ = cleanup_interval.tick() => {
                        cleanup_scheduler.lock().await.cleanup_completed_verification_handles().await;

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

    /// Verification task spawning
    async fn spawn_verification_task(
        &mut self,
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
            ValidationType::Full => &self.active_full_tasks,
            ValidationType::Lightweight => &self.active_lightweight_tasks,
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

        // Spawn verification task
        info!("[EVAL_FLOW] Spawning tokio task for verification workflow execution");
        let verification_handle = tokio::spawn(async move {
            info!(
                "[EVAL_FLOW] Starting automated verification workflow for miner {} in task {}",
                task.miner_uid, task_id
            );
            let workflow_start = std::time::Instant::now();

            let result = verification_engine
                .execute_verification_workflow(&task)
                .await;

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

        // Store verification handle for cleanup
        info!(
            "[EVAL_FLOW] Storing verification handle for task {} cleanup tracking",
            task_id
        );
        {
            let mut verification_handles = self.verification_handles.write().await;
            verification_handles.insert(task_id, verification_handle);
            info!(
                "[EVAL_FLOW] Total verification handles tracked: {}",
                verification_handles.len()
            );
        }

        Ok(())
    }

    /// Full validation workflow
    async fn run_full_validation(
        &mut self,
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
            .filter(|miner| {
                self.can_schedule_verification_for_strategy(miner, ValidationType::Full)
            })
            .collect();

        if schedulable_miners.is_empty() {
            return Ok(());
        }

        let full_tasks = self
            .spawn_validation_pipeline(schedulable_miners, verification, ValidationType::Full)
            .await?;

        info!(
            "[EVAL_FLOW] Full validation cycle completed in {:?}: {} tasks",
            cycle_start.elapsed(),
            full_tasks
        );

        Ok(())
    }

    /// Lightweight validation workflow
    async fn run_lightweight_validation(
        &mut self,
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
            .filter(|miner| {
                self.can_schedule_verification_for_strategy(miner, ValidationType::Lightweight)
            })
            .collect();

        if schedulable_miners.is_empty() {
            return Ok(());
        }

        let lightweight_tasks = self
            .spawn_validation_pipeline(
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

    /// Pipeline for all validation strategies
    async fn spawn_validation_pipeline(
        &mut self,
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
                timeout: self.config.challenge_timeout,
            };

            match self
                .spawn_verification_task(verification_task, verification)
                .await
            {
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

    fn can_schedule_verification_for_strategy(
        &self,
        miner: &MinerInfo,
        strategy: ValidationType,
    ) -> bool {
        let miner_uid = miner.uid.as_u16();

        let active_tasks = match strategy {
            ValidationType::Full => &self.active_full_tasks,
            ValidationType::Lightweight => &self.active_lightweight_tasks,
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

    pub async fn cleanup_completed_verification_handles(&mut self) {
        let mut to_remove: Vec<Uuid> = Vec::new();
        {
            let handles = self.verification_handles.read().await;
            for (id, handle) in handles.iter() {
                if handle.is_finished() {
                    to_remove.push(*id);
                }
            }
        }
        if !to_remove.is_empty() {
            let mut to_await = Vec::new();
            {
                let mut full_tasks = self.active_full_tasks.write().await;
                let mut lightweight_tasks = self.active_lightweight_tasks.write().await;
                let mut handles = self.verification_handles.write().await;
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
