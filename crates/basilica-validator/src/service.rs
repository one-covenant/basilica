use crate::api::ApiHandler;
use crate::basilica_api::{BasilicaApiClient, ValidatorSigner};
use crate::bittensor_core::{ChainRegistration, WeightSetter};
use crate::config::ValidatorConfig;
use crate::gpu::GpuScoringEngine;
use crate::grpc::start_registration_server;
use crate::incentive::cu_generator::CuGenerator;
use crate::metrics::ValidatorMetrics;
use crate::miner_prover::{MinerProver, MinerProverParams};
use crate::persistence::cleanup_task::CleanupTask;
use crate::persistence::gpu_profile_repository::GpuProfileRepository;
use crate::persistence::SimplePersistence;

use anyhow::{Context, Result};
use basilica_common::MemoryStorage;
use bittensor::Service as BittensorService;
use reqwest::Client;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration as StdDuration, SystemTime};
use sysinfo::{Pid, ProcessesToUpdate, System};
use tokio::signal;
use tokio::task::JoinHandle;
use tracing::{debug, error, info};

/// Main validator service that manages all validator components and their lifecycle
pub struct ValidatorService {
    config: ValidatorConfig,
}

struct RuntimeHandles {
    scoring_task: JoinHandle<()>,
    weight_setter_task: JoinHandle<()>,
    miner_prover_task: JoinHandle<()>,
    api_handler_task: JoinHandle<()>,
    registration_server_task: JoinHandle<()>,
    cleanup_task: Option<JoinHandle<()>>,
    cu_generator_task: Option<JoinHandle<()>>,
}

struct TaskInputs {
    weight_setter: Arc<WeightSetter>,
    miner_prover: MinerProver,
    api_handler: ApiHandler,
    persistence: Arc<SimplePersistence>,
    gpu_profile_repo: Arc<GpuProfileRepository>,
    validator_ssh_public_key: String,
    api_client: Arc<BasilicaApiClient>,
}

impl ValidatorService {
    /// Create a new validator service instance
    pub fn new(config: ValidatorConfig) -> Self {
        Self { config }
    }

    /// Start the validator with all its components
    pub async fn start(&self) -> Result<()> {
        let storage = self.init_storage().await?;
        let persistence_arc = self.init_persistence().await?;
        let gpu_profile_repo = Arc::new(GpuProfileRepository::new(persistence_arc.pool().clone()));
        let validator_metrics = self.init_metrics(persistence_arc.clone()).await?;
        let bittensor_service = self.init_bittensor_service().await?;

        let signer: Arc<dyn ValidatorSigner> = bittensor_service.clone();
        let api_client = Arc::new(BasilicaApiClient::new(
            self.config.api_endpoint.clone(),
            signer,
            self.config.billing.timeout_secs,
            StdDuration::from_secs(self.config.bidding.price_cache_ttl_secs),
            self.config.pricing.cache_ttl(),
        )?);

        let chain_registration = self
            .init_chain_registration(bittensor_service.clone())
            .await?;
        self.log_chain_registration(&chain_registration).await;

        let weight_setter = self.build_weight_setter(
            bittensor_service.clone(),
            storage.clone(),
            persistence_arc.clone(),
            gpu_profile_repo.clone(),
            api_client.clone(),
            validator_metrics.as_ref(),
        )?;
        let validator_hotkey = self.build_validator_hotkey(&bittensor_service)?;

        let mut api_handler = self.build_api_handler(
            persistence_arc.clone(),
            gpu_profile_repo.clone(),
            storage.clone(),
            validator_hotkey.clone(),
        );

        let miner_prover = self.init_miner_prover(
            bittensor_service.clone(),
            persistence_arc.clone(),
            validator_metrics.as_ref(),
        )?;

        let rental_manager = self
            .init_rental_manager(persistence_arc.clone(), validator_metrics.as_ref())
            .await?;

        // Extract SSH public key from rental manager (required for miner registration)
        let validator_ssh_public_key = rental_manager
            .as_ref()
            .map(|rm| rm.get_validator_ssh_public_key())
            .ok_or_else(|| {
                anyhow::anyhow!("Rental manager required for SSH key - metrics must be enabled")
            })?;

        info!("Validator SSH public key loaded for miner registration");

        if let Some(rental_manager) = rental_manager {
            api_handler = api_handler.with_rental_manager(Arc::new(rental_manager));
        }

        info!("All components initialized successfully");

        let handles = self
            .spawn_tasks(TaskInputs {
                weight_setter,
                miner_prover,
                api_handler,
                persistence: persistence_arc.clone(),
                gpu_profile_repo: gpu_profile_repo.clone(),
                validator_ssh_public_key,
                api_client,
            })
            .await;

        info!("Validator started successfully - all services running");
        signal::ctrl_c().await?;
        info!("Shutdown signal received, stopping validator...");

        self.shutdown(handles);

        info!("Validator shutdown complete");
        Ok(())
    }

    async fn init_storage(&self) -> Result<MemoryStorage> {
        let storage_path =
            PathBuf::from(&self.config.storage.data_dir).join("validator_storage.json");
        MemoryStorage::with_file(storage_path).await
    }

    fn resolve_db_path(&self) -> Result<String> {
        let db_url = &self.config.database.url;
        let db_path = if let Some(stripped) = db_url.strip_prefix("sqlite:") {
            stripped
        } else {
            db_url
        };
        debug!("Database URL: {}", db_url);
        debug!("Database path: {}", db_path);
        if let Some(parent) = std::path::Path::new(db_path).parent() {
            debug!("Creating directory: {:?}", parent);
            std::fs::create_dir_all(parent)?;
        }
        Ok(db_path.to_string())
    }

    async fn init_persistence(&self) -> Result<Arc<SimplePersistence>> {
        let db_path = self.resolve_db_path()?;
        let persistence =
            SimplePersistence::new(&db_path, self.config.bittensor.common.hotkey_name.clone())
                .await?;
        persistence.run_migrations().await?;
        Ok(Arc::new(persistence))
    }

    async fn init_metrics(
        &self,
        persistence: Arc<SimplePersistence>,
    ) -> Result<Option<ValidatorMetrics>> {
        if !self.config.metrics.enabled {
            return Ok(None);
        }
        let metrics = ValidatorMetrics::new(self.config.metrics.clone(), persistence)?;
        metrics.start_server().await?;
        info!("Validator metrics server started with GPU metrics collection");
        Ok(Some(metrics))
    }

    async fn init_bittensor_service(&self) -> Result<Arc<BittensorService>> {
        Ok(Arc::new(
            BittensorService::new(self.config.bittensor.common.clone()).await?,
        ))
    }

    async fn init_chain_registration(
        &self,
        bittensor_service: Arc<BittensorService>,
    ) -> Result<ChainRegistration> {
        let chain_registration = ChainRegistration::new(&self.config, bittensor_service).await?;
        chain_registration.register_startup().await?;
        Ok(chain_registration)
    }

    async fn log_chain_registration(&self, chain_registration: &ChainRegistration) {
        info!("Validator registered on chain with axon endpoint");
        if let Some(uid) = chain_registration.get_discovered_uid().await {
            info!("Validator registered with discovered UID: {uid}");
        } else {
            info!("No UID discovered - validator may not be registered on chain");
        }
    }

    fn build_weight_setter(
        &self,
        bittensor_service: Arc<BittensorService>,
        storage: MemoryStorage,
        persistence: Arc<SimplePersistence>,
        gpu_profile_repo: Arc<GpuProfileRepository>,
        api_client: Arc<BasilicaApiClient>,
        validator_metrics: Option<&ValidatorMetrics>,
    ) -> Result<Arc<WeightSetter>> {
        let gpu_scoring_engine = if let Some(metrics) = validator_metrics {
            Arc::new(GpuScoringEngine::with_metrics(
                gpu_profile_repo.clone(),
                persistence.clone(),
                Arc::new(metrics.clone()),
            ))
        } else {
            Arc::new(GpuScoringEngine::new(
                gpu_profile_repo.clone(),
                persistence.clone(),
            ))
        };

        let weight_setter = WeightSetter::new(
            self.config.bittensor.common.clone(),
            bittensor_service,
            storage,
            persistence,
            self.config.emission.weight_set_interval_blocks,
            gpu_scoring_engine,
            self.config.emission.clone(),
            api_client,
            gpu_profile_repo,
            validator_metrics.map(|m| Arc::new(m.clone())),
        )?;
        Ok(Arc::new(weight_setter))
    }

    fn build_validator_hotkey(
        &self,
        bittensor_service: &BittensorService,
    ) -> Result<basilica_common::identity::Hotkey> {
        let account_id = bittensor_service.get_account_id();
        let ss58_address = format!("{account_id}");
        basilica_common::identity::Hotkey::new(ss58_address)
            .map_err(|e| anyhow::anyhow!("Failed to create hotkey: {}", e))
    }

    fn build_api_handler(
        &self,
        persistence: Arc<SimplePersistence>,
        gpu_profile_repo: Arc<GpuProfileRepository>,
        storage: MemoryStorage,
        validator_hotkey: basilica_common::identity::Hotkey,
    ) -> ApiHandler {
        ApiHandler::new(
            self.config.api.clone(),
            persistence,
            gpu_profile_repo,
            storage,
            self.config.clone(),
            validator_hotkey,
        )
    }

    fn init_miner_prover(
        &self,
        bittensor_service: Arc<BittensorService>,
        persistence: Arc<SimplePersistence>,
        validator_metrics: Option<&ValidatorMetrics>,
    ) -> Result<MinerProver> {
        MinerProver::new(MinerProverParams {
            config: self.config.verification.clone(),
            automatic_config: self.config.automatic_verification.clone(),
            ssh_session_config: self.config.ssh_session.clone(),
            bittensor_service,
            persistence,
            metrics: validator_metrics.map(|m| Arc::new(m.clone())),
            netuid: self.config.bittensor.common.netuid,
        })
    }

    async fn init_rental_manager(
        &self,
        persistence: Arc<SimplePersistence>,
        validator_metrics: Option<&ValidatorMetrics>,
    ) -> Result<Option<crate::rental::RentalManager>> {
        let Some(metrics) = validator_metrics else {
            tracing::warn!("Rental manager disabled: metrics must be enabled for rentals");
            return Ok(None);
        };
        let manager =
            crate::rental::RentalManager::create(&self.config, persistence, metrics.prometheus())
                .await?;

        manager.start();
        manager
            .initialize_rental_metrics()
            .await
            .context("Failed to initialize rental metrics")?;
        manager
            .initialize_node_metrics()
            .await
            .context("Failed to initialize node metrics")?;
        Ok(Some(manager))
    }

    async fn spawn_tasks(&self, inputs: TaskInputs) -> RuntimeHandles {
        let weight_setter_clone = inputs.weight_setter.clone();
        let scoring_task = tokio::spawn(async move {
            let mut interval = tokio::time::interval(StdDuration::from_secs(300));
            loop {
                interval.tick().await;
                if let Err(e) = weight_setter_clone.update_all_miner_scores().await {
                    error!("Failed to update miner scores: {}", e);
                }
            }
        });

        let weight_setter_task = tokio::spawn(async move {
            if let Err(e) = inputs.weight_setter.start().await {
                error!("Weight setter task failed: {}", e);
            }
        });

        let miner_prover_task = tokio::spawn(async move {
            if let Err(e) = inputs.miner_prover.start().await {
                error!("Miner prover task failed: {}", e);
            }
        });

        let api_handler_task = tokio::spawn(async move {
            if let Err(e) = inputs.api_handler.start().await {
                error!("API handler task failed: {}", e);
            }
        });

        let registration_grpc_config = self.config.bid_grpc.clone();
        let registration_persistence = inputs.persistence.clone();
        let registration_bidding_config = self.config.bidding.clone();
        let validator_ssh_public_key = inputs.validator_ssh_public_key.clone();
        let registration_api_client = inputs.api_client.clone();
        let registration_server_task = tokio::spawn(async move {
            if let Err(e) = start_registration_server(
                registration_grpc_config,
                registration_persistence,
                registration_bidding_config,
                validator_ssh_public_key,
                Some(registration_api_client),
            )
            .await
            {
                error!("Registration gRPC server failed: {}", e);
            }
        });

        let cleanup_task = if self.config.cleanup.enabled {
            let cleanup_config = self.config.cleanup.clone();
            Some(tokio::spawn(async move {
                let cleanup_task = CleanupTask::new(cleanup_config, inputs.gpu_profile_repo);
                if let Err(e) = cleanup_task.start().await {
                    error!("Database cleanup task failed: {}", e);
                }
            }))
        } else {
            info!("Database cleanup task is disabled");
            None
        };

        let cu_generator_task = spawn_primary_validator_task(self.config.cu_generator_enabled, {
            let pool = inputs.persistence.pool().clone();
            let api_client = inputs.api_client.clone();
            let slash_mode = self.config.slash_mode;
            async move {
                info!(slash_mode = ?slash_mode, "CU generator starting");
                let generator = CuGenerator::new(pool, api_client, slash_mode);
                let mut interval = tokio::time::interval(StdDuration::from_secs(3600));
                loop {
                    interval.tick().await;
                    if let Err(error) = generator.run_once_at(chrono::Utc::now()).await {
                        error!(error = %error, "CU generator task failed");
                    }
                }
            }
        });

        RuntimeHandles {
            scoring_task,
            weight_setter_task,
            miner_prover_task,
            api_handler_task,
            registration_server_task,
            cleanup_task,
            cu_generator_task,
        }
    }

    fn shutdown(&self, handles: RuntimeHandles) {
        handles.scoring_task.abort();
        handles.weight_setter_task.abort();
        handles.miner_prover_task.abort();
        if let Some(handle) = handles.cu_generator_task {
            handle.abort();
        }
        if let Some(handle) = handles.cleanup_task {
            handle.abort();
        }
        handles.api_handler_task.abort();
        handles.registration_server_task.abort();
    }

    /// Stop all running validator processes
    pub async fn stop() -> Result<()> {
        ProcessUtils::stop_all_processes().await
    }

    /// Check the status of the validator and its components
    pub async fn status(&self) -> Result<ServiceStatus> {
        let status = ServiceStatus {
            process: ProcessUtils::check_validator_process()?,
            database_healthy: self.test_database_connectivity().await.is_ok(),
            api_response_time: self.test_api_health().await.ok(),
            bittensor_block: self.test_bittensor_connectivity().await.ok(),
        };

        Ok(status)
    }

    /// Test database connectivity
    async fn test_database_connectivity(&self) -> Result<()> {
        let pool = sqlx::SqlitePool::connect(&self.config.database.url).await?;
        sqlx::query("SELECT 1").fetch_one(&pool).await?;
        pool.close().await;
        Ok(())
    }

    /// Test API server health
    async fn test_api_health(&self) -> Result<u64> {
        let client = Client::new();
        let start_time = SystemTime::now();

        let api_url = format!(
            "http://{}:{}/health",
            self.config.server.host, self.config.server.port
        );
        let response = client
            .get(&api_url)
            .timeout(StdDuration::from_secs(10))
            .send()
            .await?;

        let elapsed = start_time.elapsed().unwrap_or(StdDuration::from_secs(0));

        if response.status().is_success() {
            Ok(elapsed.as_millis() as u64)
        } else {
            Err(anyhow::anyhow!(
                "API server returned status: {}",
                response.status()
            ))
        }
    }

    /// Test Bittensor network connectivity
    async fn test_bittensor_connectivity(&self) -> Result<u64> {
        let service = BittensorService::new(self.config.bittensor.common.clone())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create Bittensor service: {}", e))?;

        let block_number = service
            .get_block_number()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get block number: {}", e))?;

        Ok(block_number)
    }
}

fn spawn_primary_validator_task<F>(enabled: bool, future: F) -> Option<JoinHandle<()>>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    if enabled {
        Some(tokio::spawn(future))
    } else {
        None
    }
}

/// Status information for the validator service
#[derive(Default)]
pub struct ServiceStatus {
    pub process: Option<(u32, u64, f32)>, // pid, memory_mb, cpu_percent
    pub database_healthy: bool,
    pub api_response_time: Option<u64>,
    pub bittensor_block: Option<u64>,
}

impl ServiceStatus {
    pub fn is_healthy(&self) -> bool {
        self.process.is_some()
            && self.database_healthy
            && self.api_response_time.is_some()
            && self.bittensor_block.is_some()
    }
}

/// Process management utilities
struct ProcessUtils;

impl ProcessUtils {
    /// Check if validator process is currently running
    fn check_validator_process() -> Result<Option<(u32, u64, f32)>> {
        let mut system = System::new_all();
        system.refresh_all();

        for (pid, process) in system.processes() {
            let name = process.name();
            let cmd = process.cmd();

            if name == "validator"
                || cmd.iter().any(|arg| {
                    let arg = arg.to_string_lossy();
                    arg.contains("validator") && !arg.contains("cargo")
                })
            {
                let memory_mb = process.memory() / 1024 / 1024;
                let cpu_percent = process.cpu_usage();
                return Ok(Some((pid.as_u32(), memory_mb, cpu_percent)));
            }
        }

        Ok(None)
    }

    /// Find all running validator processes
    fn find_validator_processes() -> Result<Vec<u32>> {
        let mut system = System::new_all();
        system.refresh_all();

        let mut processes = Vec::new();

        for (pid, process) in system.processes() {
            let name = process.name();
            let cmd = process.cmd();

            if name == "validator"
                || cmd.iter().any(|arg| {
                    let arg = arg.to_string_lossy();
                    arg.contains("validator") && !arg.contains("cargo")
                })
            {
                processes.push(pid.as_u32());
            }
        }

        Ok(processes)
    }

    /// Send signal to process
    fn send_signal_to_process(pid: u32, signal: Signal) -> Result<()> {
        use std::process::Command;

        let signal_str = match signal {
            Signal::Term => "TERM",
            Signal::Kill => "KILL",
        };

        #[cfg(unix)]
        {
            let output = Command::new("kill")
                .arg(format!("-{signal_str}"))
                .arg(pid.to_string())
                .output()?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(anyhow::anyhow!(
                    "Failed to send {} to PID {}: {}",
                    signal_str,
                    pid,
                    stderr
                ));
            }
        }

        #[cfg(windows)]
        {
            match signal {
                Signal::Term => {
                    let output = Command::new("taskkill")
                        .args(["/PID", &pid.to_string()])
                        .output()?;

                    if !output.status.success() {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        return Err(anyhow::anyhow!(
                            "Failed to terminate PID {}: {}",
                            pid,
                            stderr
                        ));
                    }
                }
                Signal::Kill => {
                    let output = Command::new("taskkill")
                        .args(["/F", "/PID", &pid.to_string()])
                        .output()?;

                    if !output.status.success() {
                        let stderr = String::from_utf8_lossy(&output.stderr);
                        return Err(anyhow::anyhow!(
                            "Failed to force kill PID {}: {}",
                            pid,
                            stderr
                        ));
                    }
                }
            }
        }

        Ok(())
    }

    /// Check if process is still running
    fn is_process_running(pid: u32) -> Result<bool> {
        let mut system = System::new();
        let pid_obj = Pid::from_u32(pid);
        system.refresh_processes(ProcessesToUpdate::Some(&[pid_obj]), true);

        Ok(system.process(pid_obj).is_some())
    }

    /// Stop all validator processes with graceful shutdown and force kill if needed
    async fn stop_all_processes() -> Result<()> {
        let _start_time = SystemTime::now();

        let processes = Self::find_validator_processes()?;

        if processes.is_empty() {
            return Ok(());
        }

        // Send graceful shutdown signal (SIGTERM)
        for &pid in &processes {
            let _ = Self::send_signal_to_process(pid, Signal::Term);
        }

        // Wait for clean shutdown with timeout
        let shutdown_timeout = StdDuration::from_secs(30);
        let shutdown_start = SystemTime::now();

        let mut remaining_processes = processes.clone();

        while !remaining_processes.is_empty()
            && shutdown_start
                .elapsed()
                .unwrap_or(StdDuration::from_secs(0))
                < shutdown_timeout
        {
            tokio::time::sleep(StdDuration::from_millis(1000)).await;

            remaining_processes.retain(|&pid| matches!(Self::is_process_running(pid), Ok(true)));
        }

        // Force kill remaining processes if necessary
        if !remaining_processes.is_empty() {
            for &pid in &remaining_processes {
                let _ = Self::send_signal_to_process(pid, Signal::Kill);
                tokio::time::sleep(StdDuration::from_millis(500)).await;
            }
        }

        // Final verification
        let final_processes = Self::find_validator_processes()?;

        if !final_processes.is_empty() {
            return Err(anyhow::anyhow!("Some processes could not be terminated"));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::spawn_primary_validator_task;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn primary_validator_gating_spawns_task_only_when_enabled() {
        let ran = Arc::new(AtomicBool::new(false));
        let ran_clone = ran.clone();
        let handle = spawn_primary_validator_task(true, async move {
            ran_clone.store(true, Ordering::SeqCst);
        });
        handle.expect("task should be spawned").await.unwrap();
        assert!(ran.load(Ordering::SeqCst));

        let skipped = Arc::new(AtomicBool::new(false));
        let skipped_clone = skipped.clone();
        let handle = spawn_primary_validator_task(false, async move {
            skipped_clone.store(true, Ordering::SeqCst);
        });
        assert!(handle.is_none());
        assert!(!skipped.load(Ordering::SeqCst));
    }
}

#[derive(Debug, Clone, Copy)]
enum Signal {
    Term,
    Kill,
}
