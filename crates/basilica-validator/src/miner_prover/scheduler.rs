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

/// Batch verification task handle
struct BatchVerificationHandle {
    miners: Vec<MinerInfo>,
    task_handle: tokio::task::JoinHandle<()>,
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
        let mut discovery_interval = tokio::time::interval(Duration::from_secs(300)); // 5-minute discovery cycle
        let mut cleanup_interval = tokio::time::interval(Duration::from_secs(900)); // 15-minute cleanup cycle

        info!("Starting enhanced verification scheduler with automatic SSH session management");
        info!(
            "Verification interval: {}s, Discovery interval: 300s, Cleanup interval: 900s (15 min)",
            self.config.verification_interval.as_secs()
        );

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if let Err(e) = self.schedule_verifications(&discovery, &verification).await {
                        error!("Scheduled verification cycle failed: {}", e);
                    }
                    self.cleanup_completed_verifications().await;
                }
                _ = discovery_interval.tick() => {
                    if let Err(e) = self.run_discovery_verification_cycle(&discovery, &verification).await {
                        error!("Discovery verification cycle failed: {}", e);
                    }
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

    /// Run automatic verification cycle for discovered miners
    async fn run_discovery_verification_cycle(
        &mut self,
        discovery: &MinerDiscovery,
        verification: &VerificationEngine,
    ) -> Result<()> {
        info!("[EVAL_FLOW] Starting discovery-based verification cycle");
        let cycle_start = std::time::Instant::now();

        // Get current miners from discovery
        info!("[EVAL_FLOW] Fetching miners from discovery service");
        let discovery_start = std::time::Instant::now();
        let discovered_miners = discovery.get_miners_for_verification().await?;

        info!(
            "[EVAL_FLOW] Discovery completed in {:?}: {} active miners for verification",
            discovery_start.elapsed(),
            discovered_miners.len()
        );

        // Sync miners from metagraph to database
        info!("[EVAL_FLOW] Syncing discovered miners to database");
        if let Err(e) = verification
            .sync_miners_from_metagraph(&discovered_miners)
            .await
        {
            warn!("[EVAL_FLOW] Failed to sync miners to database: {}", e);
        } else {
            info!("[EVAL_FLOW] Successfully synced miners to database");
        }

        // Log detailed miner information
        for (i, miner) in discovered_miners.iter().enumerate() {
            debug!(
                miner_uid = miner.uid.as_u16(),
                miner_endpoint = %miner.endpoint,
                stake_tao = miner.stake_tao,
                last_verified = ?miner.last_verified,
                index = i,
                "[EVAL_FLOW] Miner details"
            );
        }

        // Trigger verification for each discovered miner
        info!(
            "[EVAL_FLOW] Initiating verification tasks for {} miners",
            discovered_miners.len()
        );
        let mut verification_tasks = 0;
        let mut verification_failures = 0;

        for (i, miner_info) in discovered_miners.iter().enumerate() {
            info!(
                miner_uid = miner_info.uid.as_u16(),
                progress = format!("{}/{}", i + 1, discovered_miners.len()),
                "[EVAL_FLOW] Processing miner"
            );

            match self
                .initiate_miner_verification_with_ssh(miner_info, verification)
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
                        progress = format!("{}/{}", i + 1, discovered_miners.len()),
                        error = %e,
                        "[EVAL_FLOW] Failed to initiate verification"
                    );
                }
            }
        }

        info!("[EVAL_FLOW] Discovery verification cycle completed in {:?}: {} tasks initiated, {} failures",
              cycle_start.elapsed(), verification_tasks, verification_failures);

        Ok(())
    }

    /// Initiate verification with automatic SSH session management
    async fn initiate_miner_verification_with_ssh(
        &mut self,
        miner_info: &super::types::MinerInfo,
        verification: &VerificationEngine,
    ) -> Result<()> {
        let miner_uid = miner_info.uid.as_u16();
        info!(
            miner_uid = miner_uid,
            "[EVAL_FLOW] Initiating automated verification with SSH"
        );
        debug!(
            miner_uid = miner_uid,
            miner_endpoint = %miner_info.endpoint,
            stake_tao = miner_info.stake_tao,
            is_validator = miner_info.is_validator,
            "[EVAL_FLOW] Miner details"
        );

        // Log when miner was last verified
        if let Some(last_verified) = miner_info.last_verified {
            let time_since_verification = chrono::Utc::now().signed_duration_since(last_verified);
            info!(
                miner_uid = miner_uid,
                time_since_last_verification = ?time_since_verification,
                "[EVAL_FLOW] Miner was last verified ago, proceeding to verify all executors"
            );
        } else {
            info!(
                miner_uid = miner_uid,
                "[EVAL_FLOW] Miner has never been verified, proceeding with verification"
            );
        }

        // Create verification task with SSH automation
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

        // Spawn verification task with SSH automation
        info!(
            miner_uid = miner_uid,
            "[EVAL_FLOW] Spawning automated verification task"
        );
        let spawn_start = std::time::Instant::now();
        let result = self
            .spawn_automated_verification_task(verification_task, verification)
            .await;

        match &result {
            Ok(_) => info!(
                miner_uid = miner_uid,
                spawn_duration = ?spawn_start.elapsed(),
                "[EVAL_FLOW] Verification task spawned successfully"
            ),
            Err(e) => error!(
                miner_uid = miner_uid,
                spawn_duration = ?spawn_start.elapsed(),
                error = %e,
                "[EVAL_FLOW] Failed to spawn verification task"
            ),
        }

        result?;

        Ok(())
    }

    /// Enhanced verification task spawning with SSH automation
    async fn spawn_automated_verification_task(
        &mut self,
        task: VerificationTask,
        verification: &VerificationEngine,
    ) -> Result<()> {
        let verification_engine = verification.clone();
        let task_id = uuid::Uuid::new_v4();

        info!(
            "[EVAL_FLOW] Spawning automated verification task {} for miner UID: {}",
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
                .execute_automated_verification_workflow(&task)
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

    async fn schedule_verifications(
        &mut self,
        discovery: &MinerDiscovery,
        verification: &VerificationEngine,
    ) -> Result<()> {
        let miners = discovery.get_miners_for_verification().await?;
        info!("Selected {} miners for verification", miners.len());

        // Filter miners that can be scheduled
        let schedulable_miners: Vec<MinerInfo> = miners
            .into_iter()
            .filter(|miner| self.can_schedule_verification(miner))
            .collect();

        if schedulable_miners.is_empty() {
            debug!("No miners available for scheduling verification");
            return Ok(());
        }

        // Check if automated verification is available and batch process if possible
        if schedulable_miners.len() > 1 && verification.supports_batch_processing() {
            info!(
                "Using batch automated verification for {} miners",
                schedulable_miners.len()
            );
            let handle = self
                .spawn_batch_verification_task(schedulable_miners, verification.clone())
                .await?;

            // Track first miner with the handle (batch processing)
            if let Some(first_miner) = handle.miners.first() {
                self.active_verifications
                    .insert(first_miner.uid, handle.task_handle);
            }
        } else {
            // Process individual miners
            for miner in schedulable_miners {
                let miner_uid = miner.uid;
                let handle = self
                    .spawn_verification_task(miner, verification.clone())
                    .await?;
                self.active_verifications.insert(miner_uid, handle);
            }
        }

        Ok(())
    }

    fn can_schedule_verification(&self, miner: &MinerInfo) -> bool {
        if self.active_verifications.len() >= self.config.max_concurrent_verifications {
            warn!(
                "Maximum concurrent verifications reached, skipping miner {}",
                miner.uid.as_u16()
            );
            return false;
        }

        if self.active_verifications.contains_key(&miner.uid) {
            debug!("Miner {} already being verified", miner.uid.as_u16());
            return false;
        }

        true
    }

    async fn spawn_verification_task(
        &self,
        miner: MinerInfo,
        verification: VerificationEngine,
    ) -> Result<tokio::task::JoinHandle<()>> {
        let handle = tokio::spawn(async move {
            info!(
                "Starting automated verification workflow for miner {}",
                miner.uid.as_u16()
            );

            // Convert MinerInfo to VerificationTask
            let verification_task = VerificationTask {
                miner_uid: miner.uid.as_u16(),
                miner_hotkey: miner.hotkey.to_string(),
                miner_endpoint: miner.endpoint.clone(),
                stake_tao: miner.stake_tao,
                is_validator: miner.is_validator,
                verification_type: VerificationType::AutomatedWithSsh,
                created_at: chrono::Utc::now(),
                timeout: std::time::Duration::from_secs(300),
            };

            // Use automated verification workflow with SSH session management
            match verification
                .execute_automated_verification_workflow(&verification_task)
                .await
            {
                Ok(result) => {
                    info!(
                        "Miner {} automated verification completed with score: {:.4}",
                        miner.uid.as_u16(),
                        result.overall_score
                    );
                }
                Err(e) => {
                    error!(
                        "Miner {} automated verification failed: {}",
                        miner.uid.as_u16(),
                        e
                    );

                    // Fallback to basic verification if automated workflow fails
                    info!(
                        "Attempting fallback verification for miner {}",
                        miner.uid.as_u16()
                    );
                    match verification.verify_miner(miner.clone()).await {
                        Ok(score) => {
                            info!(
                                "Miner {} fallback verification completed with score: {:.4}",
                                miner.uid.as_u16(),
                                score
                            );
                        }
                        Err(fallback_err) => {
                            error!(
                                "Miner {} fallback verification also failed: {}",
                                miner.uid.as_u16(),
                                fallback_err
                            );
                        }
                    }
                }
            }
        });

        Ok(handle)
    }

    async fn spawn_batch_verification_task(
        &self,
        miners: Vec<MinerInfo>,
        verification: VerificationEngine,
    ) -> Result<BatchVerificationHandle> {
        let miners_clone = miners.clone();
        let handle = tokio::spawn(async move {
            info!(
                "Starting batch automated verification workflow for {} miners",
                miners.len()
            );

            // Convert first miner to VerificationTask for batch processing
            let verification_task = VerificationTask {
                miner_uid: miners[0].uid.as_u16(),
                miner_hotkey: miners[0].hotkey.to_string(),
                miner_endpoint: miners[0].endpoint.clone(),
                stake_tao: miners[0].stake_tao,
                is_validator: miners[0].is_validator,
                verification_type: VerificationType::AutomatedWithSsh,
                created_at: chrono::Utc::now(),
                timeout: std::time::Duration::from_secs(300),
            };

            match verification
                .execute_automated_verification_workflow(&verification_task)
                .await
            {
                Ok(result) => {
                    info!(
                        "Batch verification completed for {} miners with score: {:.4}",
                        miners.len(),
                        result.overall_score
                    );

                    if let Err(e) = verification
                        .cleanup_failed_executors_after_failures(2)
                        .await
                    {
                        warn!("Failed to cleanup executors after batch: {}", e);
                    }
                }
                Err(e) => {
                    error!(
                        "Batch automated verification failed for {} miners: {}",
                        miners.len(),
                        e
                    );

                    // Fallback to individual verification for failed batch
                    for miner in &miners {
                        info!(
                            "Attempting individual fallback verification for miner {}",
                            miner.uid.as_u16()
                        );
                        match verification.verify_miner(miner.clone()).await {
                            Ok(score) => {
                                info!(
                                    "Miner {} individual fallback completed with score: {:.4}",
                                    miner.uid.as_u16(),
                                    score
                                );
                            }
                            Err(fallback_err) => {
                                error!(
                                    "Miner {} individual fallback also failed: {}",
                                    miner.uid.as_u16(),
                                    fallback_err
                                );
                            }
                        }
                    }
                }
            }
        });

        Ok(BatchVerificationHandle {
            miners: miners_clone,
            task_handle: handle,
        })
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
