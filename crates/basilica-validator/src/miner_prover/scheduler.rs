//! # Verification Scheduler
//!
//! Manages the scheduling and lifecycle of verification tasks.
//! Implements Single Responsibility Principle by focusing only on task scheduling.

use super::discovery::MinerDiscovery;
use super::types::MinerInfo;
use super::verification::VerificationEngine;
use crate::config::VerificationConfig;
use anyhow::Result;
use basilica_common::identity::MinerUid;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio::time::interval;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

pub struct VerificationScheduler {
    config: VerificationConfig,
    active_verifications: HashMap<MinerUid, tokio::task::JoinHandle<()>>,
    /// For tracking verification tasks by UUID
    verification_handles:
        Arc<RwLock<HashMap<Uuid, JoinHandle<Result<super::verification::VerificationResult>>>>>,
    /// For tracking active verifications by UUID
    active_verification_tasks: Arc<RwLock<HashMap<Uuid, VerificationTask>>>,
}

impl VerificationScheduler {
    pub fn new(config: VerificationConfig) -> Self {
        Self {
            config,
            active_verifications: HashMap::new(),
            verification_handles: Arc::new(RwLock::new(HashMap::new())),
            active_verification_tasks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Start the verification scheduling loop
    pub async fn start(
        &mut self,
        discovery: MinerDiscovery,
        verification: VerificationEngine,
    ) -> Result<()> {
        let mut interval = interval(self.config.verification_interval);
        let mut cleanup_interval = tokio::time::interval(Duration::from_secs(900)); // 15-minute cleanup cycle

        info!("Starting verification scheduler");
        info!(
            "Verification interval: {}s, Cleanup interval: {}s",
            interval.period().as_secs(),
            cleanup_interval.period().as_secs()
        );

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = self.run_verification(&discovery, &verification).await {
                        error!("Verification cycle failed: {}", e);
                    }
                    self.cleanup_completed_verifications().await;
                }
                _ = cleanup_interval.tick() => {
                    info!("Running scheduled executor cleanup for failed executors");
                    match verification.cleanup_failed_executors_after_failures(2).await {
                        Ok(()) => info!("Executor cleanup completed successfully"),
                        Err(e) => error!("Failed executor cleanup failed: {}", e),
                    }
                }
            }
        }
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

        // Track active verification
        info!(
            "[EVAL_FLOW] Registering verification task {} in active tasks tracker",
            task_id
        );
        {
            let mut active_verifications = self.active_verification_tasks.write().await;
            active_verifications.insert(task_id, task.clone());
            info!(
                "[EVAL_FLOW] Active verification tasks count: {}",
                active_verifications.len()
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

    /// Unified verification workflow combining discovery and scheduling
    async fn run_verification(
        &mut self,
        discovery: &MinerDiscovery,
        verification: &VerificationEngine,
    ) -> Result<()> {
        info!("[EVAL_FLOW] Starting verification cycle");
        let cycle_start = std::time::Instant::now();

        // Step 1: Discover miners from metagraph
        info!("[EVAL_FLOW] Fetching miners from discovery service");
        let discovery_start = std::time::Instant::now();
        let discovered_miners = discovery.get_miners_for_verification().await?;

        info!(
            "[EVAL_FLOW] Discovery completed in {:?}: {} miners discovered",
            discovery_start.elapsed(),
            discovered_miners.len()
        );

        if discovered_miners.is_empty() {
            info!("[EVAL_FLOW] No miners discovered, skipping cycle");
            return Ok(());
        }

        // Step 2: Sync miners from metagraph to database (fail if sync fails)
        info!("[EVAL_FLOW] Syncing discovered miners to database");
        verification
            .sync_miners_from_metagraph(&discovered_miners)
            .await
            .map_err(|e| {
                error!("[EVAL_FLOW] Failed to sync miners to database: {}", e);
                e
            })?;
        info!("[EVAL_FLOW] Successfully synced miners to database");

        // Step 3: Filter miners that can be scheduled for verification
        let schedulable_miners: Vec<MinerInfo> = discovered_miners
            .into_iter()
            .filter(|miner| self.can_schedule_verification(miner))
            .collect();

        info!(
            "[EVAL_FLOW] {} miners eligible for verification after filtering",
            schedulable_miners.len()
        );

        if schedulable_miners.is_empty() {
            info!("[EVAL_FLOW] No miners available for verification");
            return Ok(());
        }

        // Step 4: Execute individual verification tasks
        let mut verification_tasks = 0;
        let mut verification_failures = 0;

        info!(
            "[EVAL_FLOW] Processing {} miners individually",
            schedulable_miners.len()
        );

        for (i, miner_info) in schedulable_miners.iter().enumerate() {
            info!(
                miner_uid = miner_info.uid.as_u16(),
                progress = format!("{}/{}", i + 1, schedulable_miners.len()),
                "[EVAL_FLOW] Processing miner with SSH automation"
            );

            // Create verification task
            let verification_task = VerificationTask {
                miner_uid: miner_info.uid.as_u16(),
                miner_hotkey: miner_info.hotkey.to_string(),
                miner_endpoint: miner_info.endpoint.clone(),
                stake_tao: miner_info.stake_tao,
                is_validator: miner_info.is_validator,
                verification_type: VerificationType::AutomatedWithSsh,
                created_at: chrono::Utc::now(),
                timeout: self.config.challenge_timeout,
            };

            match self
                .spawn_verification_task(verification_task, verification)
                .await
            {
                Ok(_) => {
                    verification_tasks += 1;
                    info!(
                        miner_uid = miner_info.uid.as_u16(),
                        "[EVAL_FLOW] Successfully initiated verification task"
                    );
                }
                Err(e) => {
                    verification_failures += 1;
                    warn!(
                        miner_uid = miner_info.uid.as_u16(),
                        error = %e,
                        "[EVAL_FLOW] Failed to initiate verification"
                    );
                }
            }
        }

        info!(
            "[EVAL_FLOW] Cycle completed in {:?}: {} tasks initiated, {} failures",
            cycle_start.elapsed(),
            verification_tasks,
            verification_failures
        );

        Ok(())
    }

    fn can_schedule_verification(&self, miner: &MinerInfo) -> bool {
        if self.active_verifications.contains_key(&miner.uid) {
            debug!("Miner {} already being verified", miner.uid.as_u16());
            return false;
        }

        true
    }

    async fn cleanup_completed_verifications(&mut self) {
        let completed: Vec<MinerUid> = self
            .active_verifications
            .iter()
            .filter_map(|(uid, handle)| {
                if handle.is_finished() {
                    Some(*uid)
                } else {
                    None
                }
            })
            .collect();

        let num_completed = completed.len();

        for uid in completed {
            if let Some(handle) = self.active_verifications.remove(&uid) {
                if let Err(e) = handle.await {
                    error!(
                        "Verification task for miner {} panicked: {}",
                        uid.as_u16(),
                        e
                    );
                }
            }
        }

        if num_completed > 0 {
            debug!("Cleaned up {} completed verification tasks", num_completed);
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
