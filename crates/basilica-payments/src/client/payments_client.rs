use anyhow::{Context, Result};
use basilica_protocol::payments::{
    payments_service_client::PaymentsServiceClient, CreateDepositAccountRequest,
    CreateDepositAccountResponse, GetDepositAccountRequest, GetDepositAccountResponse,
    ListDepositsRequest, ListDepositsResponse,
};
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::transport::Channel;
use tracing::{debug, error, info};

pub struct PaymentsClient {
    client: Arc<Mutex<PaymentsServiceClient<Channel>>>,
    endpoint: String,
}

impl PaymentsClient {
    pub async fn new(endpoint: impl Into<String>) -> Result<Self> {
        let endpoint = endpoint.into();
        info!("Connecting to payments service at {}", endpoint);

        let channel = Channel::from_shared(endpoint.clone())
            .context("Invalid payments service endpoint")?
            .connect()
            .await
            .context("Failed to connect to payments service")?;

        let client = PaymentsServiceClient::new(channel);

        Ok(Self {
            client: Arc::new(Mutex::new(client)),
            endpoint,
        })
    }

    pub async fn create_deposit_account(
        &self,
        user_id: String,
    ) -> Result<CreateDepositAccountResponse> {
        debug!("Creating deposit account for user: {}", user_id);

        let request = CreateDepositAccountRequest { user_id };

        let response = self
            .client
            .lock()
            .await
            .create_deposit_account(request)
            .await
            .map_err(|e| {
                error!("Failed to create deposit account: {}", e);
                anyhow::anyhow!("Payment service error: {}", e.message())
            })?;

        Ok(response.into_inner())
    }

    pub async fn get_deposit_account(&self, user_id: String) -> Result<GetDepositAccountResponse> {
        debug!("Getting deposit account for user: {}", user_id);

        let request = GetDepositAccountRequest { user_id };

        let response = self
            .client
            .lock()
            .await
            .get_deposit_account(request)
            .await
            .map_err(|e| {
                error!("Failed to get deposit account: {}", e);
                anyhow::anyhow!("Payment service error: {}", e.message())
            })?;

        Ok(response.into_inner())
    }

    pub async fn list_deposits(
        &self,
        user_id: String,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<ListDepositsResponse> {
        debug!(
            "Listing deposits for user: {}, limit: {:?}, offset: {:?}",
            user_id, limit, offset
        );

        let request = ListDepositsRequest {
            user_id,
            limit: limit.unwrap_or(50),
            offset: offset.unwrap_or(0),
        };

        let response = self
            .client
            .lock()
            .await
            .list_deposits(request)
            .await
            .map_err(|e| {
                error!("Failed to list deposits: {}", e);
                anyhow::anyhow!("Payment service error: {}", e.message())
            })?;

        Ok(response.into_inner())
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }
}

impl Clone for PaymentsClient {
    fn clone(&self) -> Self {
        Self {
            client: Arc::clone(&self.client),
            endpoint: self.endpoint.clone(),
        }
    }
}
