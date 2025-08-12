// Container lifecycle management module
// Only handles lifecycle status updates, not monitoring

use basilica_protocol::billing::RentalStatus;
use bollard::container::ListContainersOptions;
use bollard::Docker;
use std::collections::HashMap;
use tracing::{info, warn};

// Container labels that indicate telemetry should be collected
const LBL_TELEMETRY_ENABLED: &str = "io.basilica.telemetry";
const LBL_ENTITY_ID: &str = "io.basilica.entity_id";

// Legacy support for rental labels
const LBL_RENTAL: &str = "io.basilica.rental";
const LBL_RENTAL_ID: &str = "io.basilica.rental_id";

/// Configuration for lifecycle management
#[derive(Clone)]
pub struct LifecycleConfig {
    pub docker_host: String,
    pub check_interval_secs: u64,
    pub enabled: bool,
}

/// Check if container should be tracked
fn should_track(labels: &Option<HashMap<String, String>>) -> bool {
    if let Some(labels) = labels {
        if let Some(v) = labels.get(LBL_TELEMETRY_ENABLED) {
            return v == "true" || v == "1";
        }
        if let Some(v) = labels.get(LBL_RENTAL) {
            return v == "true" || v == "1";
        }
    }
    false
}

/// Extract entity ID from container labels
fn get_entity_id(labels: &Option<HashMap<String, String>>) -> Option<String> {
    labels.as_ref().and_then(|l| {
        l.get(LBL_ENTITY_ID)
            .or_else(|| l.get(LBL_RENTAL_ID))
            .cloned()
    })
}

/// Manage container lifecycle status updates
pub async fn run(
    cfg: LifecycleConfig,
    stream_cfg: super::stream::StreamConfig,
) -> anyhow::Result<()> {
    if !cfg.enabled {
        info!("Container lifecycle management disabled");
        return Ok(());
    }

    let docker = connect_docker(&cfg.docker_host).await?;

    let mut previous_containers = std::collections::HashSet::new();

    let containers = docker
        .list_containers(Some(ListContainersOptions::<String> {
            all: false,
            ..Default::default()
        }))
        .await?;

    for container in containers {
        if should_track(&container.labels) {
            if let Some(entity_id) = get_entity_id(&container.labels) {
                previous_containers.insert(entity_id.clone());
                update_status(
                    &stream_cfg,
                    &entity_id,
                    RentalStatus::Active,
                    "initial_scan",
                )
                .await;
            }
        }
    }

    // Main lifecycle tracking loop
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(cfg.check_interval_secs)).await;

        let containers = docker
            .list_containers(Some(ListContainersOptions::<String> {
                all: false,
                ..Default::default()
            }))
            .await?;

        let mut current_containers = std::collections::HashSet::new();

        // Check for new containers
        for container in containers {
            if should_track(&container.labels) {
                if let Some(entity_id) = get_entity_id(&container.labels) {
                    current_containers.insert(entity_id.clone());

                    if !previous_containers.contains(&entity_id) {
                        info!("New container detected: {}", entity_id);
                        update_status(
                            &stream_cfg,
                            &entity_id,
                            RentalStatus::Active,
                            "container_started",
                        )
                        .await;
                    }
                }
            }
        }

        // Check for stopped containers
        for entity_id in &previous_containers {
            if !current_containers.contains(entity_id) {
                info!("Container stopped: {}", entity_id);

                update_status(
                    &stream_cfg,
                    entity_id,
                    RentalStatus::Stopped,
                    "container_stopped",
                )
                .await;
            }
        }

        previous_containers = current_containers;
    }
}

async fn connect_docker(docker_host: &str) -> anyhow::Result<Docker> {
    let docker = match docker_host {
        s if s.starts_with("unix://") => {
            Docker::connect_with_unix(s, 120, bollard::API_DEFAULT_VERSION)?
        }
        s if s.starts_with("tcp://") || s.starts_with("http://") || s.starts_with("https://") => {
            Docker::connect_with_http(s, 120, bollard::API_DEFAULT_VERSION)?
        }
        _ => Docker::connect_with_local_defaults()?,
    };
    Ok(docker)
}

async fn update_status(
    stream_cfg: &super::stream::StreamConfig,
    entity_id: &str,
    status: RentalStatus,
    reason: &str,
) {
    if let Err(e) =
        super::stream::update_lifecycle_status(stream_cfg, entity_id, status, reason).await
    {
        warn!("Failed to update lifecycle status for {}: {}", entity_id, e);
    }
}
