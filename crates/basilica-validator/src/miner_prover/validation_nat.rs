use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::persistence::SimplePersistence;
use crate::ssh::ValidatorSshClient;
use basilica_common::ssh::SshConnectionDetails;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatProfile {
    pub is_accessible: bool,
    pub test_port: u16,
    pub test_path: String,
    pub container_id: Option<String>,
    pub response_content: Option<String>,
    pub test_timestamp: DateTime<Utc>,
    pub full_json: String,
    pub error_message: Option<String>,
}

pub struct NatCollector {
    ssh_client: Arc<ValidatorSshClient>,
    persistence: Arc<SimplePersistence>,
    http_timeout_secs: u64,
    container_startup_delay_secs: u64,
}

impl NatCollector {
    pub fn new(ssh_client: Arc<ValidatorSshClient>, persistence: Arc<SimplePersistence>) -> Self {
        Self {
            ssh_client,
            persistence,
            http_timeout_secs: 10,
            container_startup_delay_secs: 3,
        }
    }

    pub async fn collect(
        &self,
        executor_id: &str,
        ssh_details: &SshConnectionDetails,
    ) -> Result<NatProfile> {
        info!(
            executor_id = executor_id,
            "[NAT] Starting NAT validation for executor"
        );

        let test_port = self.generate_random_port();
        let test_path = Uuid::new_v4().to_string().replace("-", "");
        let test_content = format!("NAT_TEST_{}", Uuid::new_v4().to_string().replace("-", ""));
        let test_id = Uuid::new_v4().to_string().to_string().replace("-", "");

        let mut container_id: Option<String> = None;
        let mut cleanup_needed = false;

        let result = async {
            let temp_dir = format!("/tmp/nat_test_{}", test_id);
            self.ssh_client
                .execute_command(
                    ssh_details,
                    &format!("mkdir -p {}", temp_dir),
                    false,
                )
                .await
                .context("Failed to create temp directory")?;
            cleanup_needed = true;

            let index_content = format!(
                r#"<!DOCTYPE html>
<html>
<head><title>NAT Test</title></head>
<body>
<h1>NAT Validation Test</h1>
<p>Test ID: {}</p>
<p>Test Content: {}</p>
<p>Port: {}</p>
<p>Path: {}</p>
</body>
</html>"#,
                test_id, test_content, test_port, test_path
            );

            self.ssh_client
                .execute_command(
                    ssh_details,
                    &format!(
                        "echo '{}' > {}/index.html",
                        index_content.replace('\'', "'\\''"),
                        temp_dir
                    ),
                    false,
                )
                .await
                .context("Failed to create index.html")?;

            let nginx_config = format!(
                r#"server {{
    listen {};
    listen [::]:{};
    server_name _;

    location /{} {{
        root /usr/share/nginx/html;
        index index.html;
    }}

    location / {{
        return 404;
    }}
}}"#,
                test_port, test_port, test_path
            );

            self.ssh_client
                .execute_command(
                    ssh_details,
                    &format!(
                        "echo '{}' > {}/default.conf",
                        nginx_config.replace('\'', "'\\''"),
                        temp_dir
                    ),
                    false,
                )
                .await
                .context("Failed to create nginx config")?;

            let docker_run_cmd = format!(
                "docker run -d --name nat_test_{} -p {}:{} -v {}:/usr/share/nginx/html:ro -v {}/default.conf:/etc/nginx/conf.d/default.conf:ro nginx:alpine",
                test_id, test_port, test_port, temp_dir, temp_dir
            );

            let output = self.ssh_client
                .execute_command(ssh_details, &docker_run_cmd, true)
                .await
                .context("Failed to start nginx container")?;

            let cid = output.trim().to_string();
            if cid.is_empty() {
                return Err(anyhow::anyhow!("Failed to get container ID"));
            }
            container_id = Some(cid.clone());

            debug!(
                executor_id = executor_id,
                container_id = cid,
                port = test_port,
                "[NAT] Container started, waiting for initialization"
            );

            tokio::time::sleep(tokio::time::Duration::from_secs(
                self.container_startup_delay_secs,
            ))
            .await;

            let http_client = reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(self.http_timeout_secs))
                .danger_accept_invalid_certs(true)
                .build()
                .context("Failed to create HTTP client")?;

            let test_url = format!(
                "http://{}:{}/{}",
                ssh_details.host, test_port, test_path
            );

            debug!(
                executor_id = executor_id,
                url = test_url,
                "[NAT] Testing HTTP connectivity"
            );

            let response = http_client
                .get(&test_url)
                .send()
                .await
                .context("Failed to connect to executor through NAT")?;

            let response_text = response
                .text()
                .await
                .context("Failed to read response body")?;

            let is_accessible = response_text.contains(&test_id) && response_text.contains(&test_content);

            if !is_accessible {
                return Err(anyhow::anyhow!(
                    "Response validation failed: expected test_id '{}' and content '{}' not found in response",
                    test_id, test_content
                ));
            }

            info!(
                executor_id = executor_id,
                port = test_port,
                "[NAT] NAT validation successful - executor is accessible"
            );

            Ok(NatProfile {
                is_accessible,
                test_port,
                test_path: format!("/{}", test_path),
                container_id: container_id.clone(),
                response_content: Some(response_text),
                test_timestamp: Utc::now(),
                full_json: serde_json::json!({
                    "test_id": test_id,
                    "test_port": test_port,
                    "test_path": test_path,
                    "test_content": test_content,
                    "is_accessible": is_accessible,
                    "container_id": container_id,
                })
                .to_string(),
                error_message: None,
            })
        }
        .await;

        if cleanup_needed {
            if let Some(ref cid) = container_id {
                if let Err(e) = self
                    .ssh_client
                    .execute_command(
                        ssh_details,
                        &format!("docker stop {} && docker rm {}", cid, cid),
                        false,
                    )
                    .await
                {
                    warn!(
                        executor_id = executor_id,
                        error = %e,
                        "[NAT] Failed to cleanup container"
                    );
                }
            }

            if let Err(e) = self
                .ssh_client
                .execute_command(
                    ssh_details,
                    &format!("rm -rf /tmp/nat_test_{}", test_id),
                    false,
                )
                .await
            {
                warn!(
                    executor_id = executor_id,
                    error = %e,
                    "[NAT] Failed to cleanup temp directory"
                );
            }
        }

        match result {
            Ok(profile) => Ok(profile),
            Err(e) => {
                error!(
                    executor_id = executor_id,
                    error = %e,
                    "[NAT] NAT validation failed: {}",
                    e
                );

                Err(anyhow::anyhow!(
                    "NAT validation failed: executor not accessible from internet - {}",
                    e
                ))
            }
        }
    }

    pub async fn store(
        &self,
        miner_uid: u16,
        executor_id: &str,
        profile: &NatProfile,
    ) -> Result<()> {
        debug!(
            miner_uid = miner_uid,
            executor_id = executor_id,
            "[NAT] Storing NAT profile"
        );

        self.persistence
            .store_executor_nat_profile(miner_uid, executor_id, profile)
            .await
            .context("Failed to store NAT profile")?;

        Ok(())
    }

    pub async fn collect_and_store(
        &self,
        executor_id: &str,
        miner_uid: u16,
        ssh_details: &SshConnectionDetails,
    ) -> Result<NatProfile> {
        let profile = self.collect(executor_id, ssh_details).await?;
        self.store(miner_uid, executor_id, &profile).await?;
        Ok(profile)
    }

    pub async fn collect_with_fallback(
        &self,
        executor_id: &str,
        miner_uid: u16,
        ssh_details: &SshConnectionDetails,
    ) -> Option<NatProfile> {
        match self
            .collect_and_store(executor_id, miner_uid, ssh_details)
            .await
        {
            Ok(profile) => {
                info!(
                    executor_id = executor_id,
                    is_accessible = profile.is_accessible,
                    "[NAT] NAT validation completed successfully"
                );
                Some(profile)
            }
            Err(e) => {
                error!(
                    executor_id = executor_id,
                    error = %e,
                    "[NAT] NAT validation failed: {}",
                    e
                );
                None
            }
        }
    }

    pub async fn retrieve(&self, miner_uid: u16, executor_id: &str) -> Result<Option<NatProfile>> {
        self.persistence
            .get_executor_nat_profile(miner_uid, executor_id)
            .await
            .context("Failed to retrieve NAT profile")
    }

    fn generate_random_port(&self) -> u16 {
        let mut rng = rand::thread_rng();
        rng.gen_range(30000..40000)
    }
}
