use anyhow::Result;
use std::sync::Arc;
use tonic::{Request, Response, Status};

use crate::{
    domain::types::Treasury,
    storage::{DepositAccountsRepo, ObservedDepositsRepo, PgRepos},
};
use basilica_common::crypto::Aead;
use basilica_protocol::payments::{
    payments_service_server::{PaymentsService, PaymentsServiceServer},
    CreateDepositAccountRequest, CreateDepositAccountResponse, DepositRecord,
    GetDepositAccountRequest, GetDepositAccountResponse, ListDepositsRequest, ListDepositsResponse,
};

pub struct PaymentsServer<T: Treasury + 'static> {
    repos: PgRepos,
    treasury: Arc<T>,
    aead: Arc<Aead>,
}

impl<T: Treasury> PaymentsServer<T> {
    pub fn new(repos: PgRepos, treasury: Arc<T>, aead: Arc<Aead>) -> Self {
        Self {
            repos,
            treasury,
            aead,
        }
    }

    pub fn into_service(self) -> PaymentsServiceServer<Self> {
        PaymentsServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl<T: Treasury + Send + Sync> PaymentsService for PaymentsServer<T> {
    async fn create_deposit_account(
        &self,
        req: Request<CreateDepositAccountRequest>,
    ) -> Result<Response<CreateDepositAccountResponse>, Status> {
        let user_id = req.into_inner().user_id;

        if let Some((addr, _, pub_hex, _mn_ct)) =
            self.repos.get_by_user(&user_id).await.map_err(internal)?
        {
            return Ok(Response::new(CreateDepositAccountResponse {
                user_id,
                address_ss58: addr,
                hotkey_public: pub_hex,
            }));
        }

        let (addr, acct_hex, pub_hex, mnemonic) =
            self.treasury.generate_hotkey().await.map_err(internal)?;
        let mnemonic_ct = self.aead.encrypt(&mnemonic).map_err(internal)?;

        let mut tx = self.repos.begin().await.map_err(internal)?;
        self.repos
            .insert_tx(&mut tx, &user_id, &addr, &acct_hex, &pub_hex, &mnemonic_ct)
            .await
            .map_err(internal)?;
        tx.commit().await.map_err(internal)?;

        Ok(Response::new(CreateDepositAccountResponse {
            user_id,
            address_ss58: addr,
            hotkey_public: pub_hex,
        }))
    }

    async fn get_deposit_account(
        &self,
        req: Request<GetDepositAccountRequest>,
    ) -> Result<Response<GetDepositAccountResponse>, Status> {
        let user_id = req.into_inner().user_id;

        let resp = match self.repos.get_by_user(&user_id).await.map_err(internal)? {
            Some((addr, _, _, _)) => GetDepositAccountResponse {
                user_id,
                address_ss58: addr,
                exists: true,
            },
            None => GetDepositAccountResponse {
                user_id,
                address_ss58: "".into(),
                exists: false,
            },
        };

        Ok(Response::new(resp))
    }

    async fn list_deposits(
        &self,
        req: Request<ListDepositsRequest>,
    ) -> Result<Response<ListDepositsResponse>, Status> {
        let q = req.into_inner();
        let rows = self
            .repos
            .list_by_user(&q.user_id, (q.limit as i64).max(1), q.offset as i64)
            .await
            .map_err(internal)?;

        let items = rows
            .into_iter()
            .map(|r| DepositRecord {
                tx_hash: format!(
                    "b{}#e{}#{}",
                    r.block_number, r.event_index, r.to_account_hex
                ),
                block_number: r.block_number as u64,
                event_index: r.event_index as u32,
                from_address: r.from_account_hex,
                to_address: r.to_account_hex,
                amount_plancks: r.amount_plancks,
                credited_credit_id: r.billing_credit_id,
                status: r.status,
                observed_at: r.observed_at_rfc3339.clone(),
                finalized_at: r.observed_at_rfc3339,
                credited_at: r.credited_at_rfc3339,
            })
            .collect();

        Ok(Response::new(ListDepositsResponse { items }))
    }
}

fn internal<E: std::fmt::Display>(e: E) -> Status {
    Status::internal(e.to_string())
}
