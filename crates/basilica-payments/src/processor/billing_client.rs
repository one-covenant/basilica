use crate::domain::types::BillingClient;
use anyhow::Result;
use std::collections::HashMap;
use tonic::transport::Channel;

pub struct GrpcBillingClient {
    inner: basilica_protocol::billing::billing_service_client::BillingServiceClient<Channel>,
}

impl GrpcBillingClient {
    pub async fn connect(uri: &str) -> Result<Self> {
        use basilica_protocol::billing::billing_service_client::BillingServiceClient;
        Ok(Self {
            inner: BillingServiceClient::connect(uri.to_string()).await?,
        })
    }
}

#[async_trait::async_trait]
impl BillingClient for GrpcBillingClient {
    async fn apply_credits(
        &self,
        user_id: &str,
        credits_dec: &str,
        transaction_id: &str,
    ) -> Result<String> {
        use basilica_protocol::billing::ApplyCreditsRequest;

        let mut md = HashMap::new();
        md.insert("asset".into(), "USD_CREDIT".into());
        md.insert("unit".into(), "credit".into());

        let req = ApplyCreditsRequest {
            user_id: user_id.into(),
            amount: credits_dec.into(),
            transaction_id: transaction_id.into(),
            payment_method: "TAO_ONCHAIN_DEPOSIT".into(),
            metadata: md,
        };

        let resp = self.inner.clone().apply_credits(req).await?.into_inner();
        Ok(resp.credit_id)
    }
}
