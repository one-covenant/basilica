use crate::domain::{
    credits::{CreditManager, CreditOperations},
    rentals::{RentalManager, RentalOperations},
    rules_engine::RulesEngine,
    types::{CreditBalance, PackageId, RentalId, RentalState, ReservationId, ResourceSpec, UserId},
};
use crate::error::BillingError;
use crate::storage::rds::RdsConnection;
use crate::storage::repositories::{
    CreditRepository, PackageRepository, RentalRepository, SqlCreditRepository,
    SqlPackageRepository, SqlRentalRepository,
};
use crate::telemetry::{TelemetryIngester, TelemetryProcessor};

use basilica_protocol::billing::{
    billing_service_server::BillingService, ActiveRental, ApplyCreditsRequest,
    ApplyCreditsResponse, FinalizeRentalRequest, FinalizeRentalResponse, GetActiveRentalsRequest,
    GetActiveRentalsResponse, GetBalanceRequest, GetBalanceResponse, GetBillingPackagesRequest,
    GetBillingPackagesResponse, IngestResponse, ReleaseReservationRequest,
    ReleaseReservationResponse, RentalStatus, ReserveCreditsRequest, ReserveCreditsResponse,
    SetUserPackageRequest, SetUserPackageResponse, TelemetryData, TrackRentalRequest,
    TrackRentalResponse, UpdateRentalStatusRequest, UpdateRentalStatusResponse, UsageDataPoint,
    UsageReportRequest, UsageReportResponse, UsageSummary,
};

use chrono::Duration;
use rust_decimal::prelude::*;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use tokio_stream::StreamExt;
use tonic::{Request, Response, Status};
use tracing::{error, info};

pub struct BillingServiceImpl {
    credit_manager: Arc<dyn CreditOperations + Send + Sync>,
    rental_manager: Arc<dyn RentalOperations + Send + Sync>,
    _rules_engine: Arc<RulesEngine>,
    #[allow(dead_code)] // Used in server's consumer loop
    telemetry_processor: Arc<TelemetryProcessor>,
    telemetry_ingester: Arc<TelemetryIngester>,
    credit_repository: Arc<dyn CreditRepository + Send + Sync>,
    rental_repository: Arc<dyn RentalRepository + Send + Sync>,
    package_repository: Arc<dyn PackageRepository + Send + Sync>,
}

impl BillingServiceImpl {
    pub fn new(
        rds_connection: Arc<RdsConnection>,
        telemetry_ingester: Arc<TelemetryIngester>,
        telemetry_processor: Arc<TelemetryProcessor>,
    ) -> Self {
        let credit_repository = Arc::new(SqlCreditRepository::new(rds_connection.clone()));
        let rental_repository = Arc::new(SqlRentalRepository::new(rds_connection.clone()));
        let package_repository = Arc::new(SqlPackageRepository::new(rds_connection.clone()));

        Self {
            credit_manager: Arc::new(CreditManager::new()),
            rental_manager: Arc::new(RentalManager::new()),
            _rules_engine: Arc::new(RulesEngine::new()),
            telemetry_processor,
            telemetry_ingester,
            credit_repository: credit_repository.clone(),
            rental_repository: rental_repository.clone(),
            package_repository: package_repository.clone(),
        }
    }

    fn parse_decimal(s: &str) -> crate::error::Result<Decimal> {
        Decimal::from_str(s).map_err(|e| BillingError::ValidationError {
            field: "amount".to_string(),
            message: format!("Invalid decimal value: {}", e),
        })
    }

    fn rental_status_to_domain(status: RentalStatus) -> RentalState {
        match status {
            RentalStatus::Pending => RentalState::Pending,
            RentalStatus::Active => RentalState::Active,
            RentalStatus::Stopping => RentalState::Terminating,
            RentalStatus::Stopped => RentalState::Completed,
            RentalStatus::Failed => RentalState::Failed,
            RentalStatus::Unspecified => RentalState::Pending,
        }
    }

    fn domain_status_to_proto(state: RentalState) -> RentalStatus {
        match state {
            RentalState::Pending => RentalStatus::Pending,
            RentalState::Active => RentalStatus::Active,
            RentalState::Suspended => RentalStatus::Stopping,
            RentalState::Terminating => RentalStatus::Stopping,
            RentalState::Completed => RentalStatus::Stopped,
            RentalState::Failed => RentalStatus::Failed,
        }
    }
}

#[tonic::async_trait]
impl BillingService for BillingServiceImpl {
    async fn apply_credits(
        &self,
        request: Request<ApplyCreditsRequest>,
    ) -> std::result::Result<Response<ApplyCreditsResponse>, Status> {
        let req = request.into_inner();
        let user_id = UserId::new(req.user_id);
        let amount = Self::parse_decimal(&req.amount)
            .map_err(|e| Status::invalid_argument(format!("Invalid amount: {}", e)))?;
        let credit_balance = CreditBalance::from_decimal(amount);

        info!("Applying {} credits to user {}", amount, user_id);

        let new_balance = self
            .credit_manager
            .apply_credits(&user_id, credit_balance)
            .await
            .map_err(|e| Status::internal(format!("Failed to apply credits: {}", e)))?;

        let account = self
            .credit_manager
            .get_account(&user_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to get account: {}", e)))?;

        self.credit_repository
            .update_balance(&user_id, account.balance)
            .await
            .map_err(|e| Status::internal(format!("Failed to persist balance: {}", e)))?;

        let response = ApplyCreditsResponse {
            success: true,
            new_balance: new_balance.to_string(),
            credit_id: req.transaction_id,
            applied_at: Some(prost_types::Timestamp::from(std::time::SystemTime::now())),
        };

        Ok(Response::new(response))
    }

    async fn get_balance(
        &self,
        request: Request<GetBalanceRequest>,
    ) -> std::result::Result<Response<GetBalanceResponse>, Status> {
        let req = request.into_inner();
        let user_id = UserId::new(req.user_id);

        let account = self
            .credit_manager
            .get_account(&user_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to get account: {}", e)))?;

        let response = GetBalanceResponse {
            available_balance: account.available_balance().to_string(),
            reserved_balance: account.reserved.to_string(),
            total_balance: account.balance.to_string(),
            last_updated: Some(prost_types::Timestamp::from(std::time::SystemTime::from(
                account.last_updated,
            ))),
        };

        Ok(Response::new(response))
    }

    async fn reserve_credits(
        &self,
        request: Request<ReserveCreditsRequest>,
    ) -> std::result::Result<Response<ReserveCreditsResponse>, Status> {
        let req = request.into_inner();
        let user_id = UserId::new(req.user_id);
        let rental_id = if req.rental_id.is_empty() {
            None
        } else {
            Some(
                RentalId::from_str(&req.rental_id)
                    .map_err(|e| Status::invalid_argument(format!("Invalid rental ID: {}", e)))?,
            )
        };
        let amount = Self::parse_decimal(&req.amount)
            .map_err(|e| Status::invalid_argument(format!("Invalid amount: {}", e)))?;
        let credit_balance = CreditBalance::from_decimal(amount);
        let duration = Duration::hours(req.duration_hours as i64);

        info!(
            "Reserving {} credits for user {} for {} hours",
            amount, user_id, req.duration_hours
        );

        let reservation_id = self
            .credit_manager
            .reserve_credits(&user_id, credit_balance, duration, rental_id)
            .await
            .map_err(|e| match e {
                BillingError::InsufficientBalance {
                    available,
                    required,
                } => Status::failed_precondition(format!(
                    "Insufficient balance: available={}, required={}",
                    available, required
                )),
                _ => Status::internal(format!("Failed to reserve credits: {}", e)),
            })?;

        let reservation = self
            .credit_manager
            .get_reservation(&reservation_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to get reservation: {}", e)))?;

        self.credit_repository
            .create_reservation(&reservation)
            .await
            .map_err(|e| Status::internal(format!("Failed to persist reservation: {}", e)))?;

        let response = ReserveCreditsResponse {
            success: true,
            reservation_id: reservation_id.to_string(),
            reserved_amount: req.amount,
            reserved_until: Some(prost_types::Timestamp::from(std::time::SystemTime::from(
                reservation.expires_at,
            ))),
            error_message: String::new(),
        };

        Ok(Response::new(response))
    }

    async fn release_reservation(
        &self,
        request: Request<ReleaseReservationRequest>,
    ) -> std::result::Result<Response<ReleaseReservationResponse>, Status> {
        let req = request.into_inner();
        let reservation_id = ReservationId::from_uuid(
            uuid::Uuid::parse_str(&req.reservation_id)
                .map_err(|e| Status::invalid_argument(format!("Invalid reservation ID: {}", e)))?,
        );
        let final_amount = Self::parse_decimal(&req.final_amount)
            .map_err(|e| Status::invalid_argument(format!("Invalid amount: {}", e)))?;
        let final_balance = CreditBalance::from_decimal(final_amount);

        info!(
            "Releasing reservation {} with final amount {}",
            reservation_id, final_amount
        );

        let released_amount = self
            .credit_manager
            .charge_from_reservation(&reservation_id, final_balance)
            .await
            .map_err(|e| match e {
                BillingError::ReservationNotFound { .. } => {
                    Status::not_found(format!("Reservation not found: {}", e))
                }
                BillingError::ReservationAlreadyReleased { .. } => {
                    Status::failed_precondition(format!("Reservation already released: {}", e))
                }
                _ => Status::internal(format!("Failed to release reservation: {}", e)),
            })?;

        self.credit_repository
            .release_reservation(&reservation_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to update reservation: {}", e)))?;

        let refunded = released_amount
            .subtract(final_balance)
            .unwrap_or(CreditBalance::zero());

        let response = ReleaseReservationResponse {
            success: true,
            charged_amount: final_amount.to_string(),
            refunded_amount: refunded.to_string(),
            new_balance: released_amount.to_string(),
        };

        Ok(Response::new(response))
    }

    async fn track_rental(
        &self,
        request: Request<TrackRentalRequest>,
    ) -> std::result::Result<Response<TrackRentalResponse>, Status> {
        let req = request.into_inner();
        let rental_id = RentalId::from_str(&req.rental_id)
            .map_err(|e| Status::invalid_argument(format!("Invalid rental ID: {}", e)))?;
        let user_id = UserId::new(req.user_id);
        let hourly_rate = Self::parse_decimal(&req.hourly_rate)
            .map_err(|e| Status::invalid_argument(format!("Invalid hourly rate: {}", e)))?;
        let credit_rate = CreditBalance::from_decimal(hourly_rate);

        info!(
            "Tracking rental {} for user {} at {} credits/hour",
            rental_id, user_id, hourly_rate
        );

        let package_id = PackageId::standard();
        let resource_spec = ResourceSpec {
            gpu_count: 1, // Default values, should come from request
            gpu_model: None,
            cpu_cores: 4,
            memory_gb: 16,
            storage_gb: 100,
        };

        let rental_id = self
            .rental_manager
            .create_rental(
                user_id.clone(),
                req.executor_id.clone(),
                package_id,
                resource_spec,
                None,
            )
            .await
            .map_err(|e| Status::internal(format!("Failed to create rental: {}", e)))?;

        let created = self
            .rental_manager
            .get_rental(&rental_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to get created rental: {}", e)))?;

        let max_duration = Duration::hours(req.max_duration_hours as i64);
        let estimated_cost = credit_rate.multiply(Decimal::from(req.max_duration_hours));

        let reservation_id = self
            .credit_manager
            .reserve_credits(&user_id, estimated_cost, max_duration, Some(rental_id))
            .await
            .map_err(|e| match e {
                BillingError::InsufficientBalance { .. } => {
                    Status::failed_precondition(format!("Insufficient balance: {}", e))
                }
                _ => Status::internal(format!("Failed to reserve credits: {}", e)),
            })?;

        self.rental_repository
            .create_rental(&created)
            .await
            .map_err(|e| Status::internal(format!("Failed to persist rental: {}", e)))?;

        let response = TrackRentalResponse {
            success: true,
            tracking_id: rental_id.to_string(),
            reservation_id: reservation_id.to_string(),
            estimated_cost: estimated_cost.to_string(),
        };

        Ok(Response::new(response))
    }

    async fn update_rental_status(
        &self,
        request: Request<UpdateRentalStatusRequest>,
    ) -> std::result::Result<Response<UpdateRentalStatusResponse>, Status> {
        let req = request.into_inner();
        let rental_id = RentalId::from_str(&req.rental_id)
            .map_err(|e| Status::invalid_argument(format!("Invalid rental ID: {}", e)))?;
        let new_status = Self::rental_status_to_domain(req.status());

        info!("Updating rental {} status to {}", rental_id, new_status);

        let _rental = self
            .rental_manager
            .update_status(&rental_id, new_status)
            .await
            .map_err(|e| match e {
                BillingError::RentalNotFound { .. } => {
                    Status::not_found(format!("Rental not found: {}", e))
                }
                BillingError::InvalidStateTransition { .. } => {
                    Status::failed_precondition(format!("Invalid state transition: {}", e))
                }
                _ => Status::internal(format!("Failed to update rental: {}", e)),
            })?;

        let rental = self
            .rental_manager
            .get_rental(&rental_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to get rental: {}", e)))?;
        self.rental_repository
            .update_rental(&rental)
            .await
            .map_err(|e| Status::internal(format!("Failed to persist status: {}", e)))?;

        let response = UpdateRentalStatusResponse {
            success: true,
            current_cost: rental.cost_breakdown.total_cost.to_string(),
            updated_at: Some(prost_types::Timestamp::from(std::time::SystemTime::now())),
        };

        Ok(Response::new(response))
    }

    async fn get_active_rentals(
        &self,
        request: Request<GetActiveRentalsRequest>,
    ) -> std::result::Result<Response<GetActiveRentalsResponse>, Status> {
        let req = request.into_inner();

        let rentals = if let Some(filter) = req.filter {
            match filter {
                basilica_protocol::billing::get_active_rentals_request::Filter::UserId(user_id) => {
                    let uid = UserId::new(user_id);
                    self.rental_repository
                        .get_active_rentals(Some(&uid))
                        .await
                        .map_err(|e| Status::internal(format!("Failed to list rentals: {}", e)))?
                }
                basilica_protocol::billing::get_active_rentals_request::Filter::ExecutorId(
                    _executor_id,
                ) => {
                    // Executor filter not supported in trait yet
                    self.rental_repository
                        .get_active_rentals(None)
                        .await
                        .map_err(|e| Status::internal(format!("Failed to list rentals: {}", e)))?
                }
                basilica_protocol::billing::get_active_rentals_request::Filter::ValidatorId(_) => {
                    // Validator filter not implemented yet
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };

        let active_rentals: Vec<ActiveRental> = rentals
            .into_iter()
            .filter(|r| r.state.is_active())
            .map(|r| ActiveRental {
                rental_id: r.id.to_string(),
                user_id: r.user_id.to_string(),
                executor_id: r.executor_id.clone(),
                validator_id: String::new(), // Not tracked yet
                status: Self::domain_status_to_proto(r.state).into(),
                resource_spec: None, // Would need to be added to domain
                hourly_rate: r.cost_breakdown.base_cost.to_string(),
                current_cost: r.cost_breakdown.total_cost.to_string(),
                start_time: Some(prost_types::Timestamp::from(std::time::SystemTime::from(
                    r.created_at,
                ))),
                last_updated: Some(prost_types::Timestamp::from(std::time::SystemTime::from(
                    r.last_updated,
                ))),
                metadata: HashMap::new(),
            })
            .collect();

        let response = GetActiveRentalsResponse {
            rentals: active_rentals.clone(),
            total_count: active_rentals.len() as u64,
        };

        Ok(Response::new(response))
    }

    async fn finalize_rental(
        &self,
        request: Request<FinalizeRentalRequest>,
    ) -> std::result::Result<Response<FinalizeRentalResponse>, Status> {
        let req = request.into_inner();
        let rental_id = RentalId::from_str(&req.rental_id)
            .map_err(|e| Status::invalid_argument(format!("Invalid rental ID: {}", e)))?;
        let final_cost = Self::parse_decimal(&req.final_cost)
            .map_err(|e| Status::invalid_argument(format!("Invalid final cost: {}", e)))?;
        let final_balance = CreditBalance::from_decimal(final_cost);

        info!("Finalizing rental {} with cost {}", rental_id, final_cost);

        let rental = self
            .rental_manager
            .update_status(&rental_id, RentalState::Completed)
            .await
            .map_err(|e| Status::internal(format!("Failed to finalize rental: {}", e)))?;

        let duration = rental.last_updated - rental.created_at;
        let duration_hours =
            duration.num_hours() as f64 + (duration.num_minutes() % 60) as f64 / 60.0;

        let reservations = self
            .credit_manager
            .get_active_reservations(&rental.user_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to get reservations: {}", e)))?;

        let rental_reservation = reservations.iter().find(|r| r.rental_id == Some(rental_id));

        let (charged_amount, refunded_amount) = if let Some(reservation) = rental_reservation {
            let reserved = reservation.amount;
            let refunded = reserved
                .subtract(final_balance)
                .unwrap_or(CreditBalance::zero());

            self.credit_manager
                .charge_from_reservation(&reservation.id, final_balance)
                .await
                .map_err(|e| Status::internal(format!("Failed to charge reservation: {}", e)))?;

            (final_balance, refunded)
        } else {
            self.credit_manager
                .charge_credits(&rental.user_id, final_balance)
                .await
                .map_err(|e| Status::internal(format!("Failed to charge credits: {}", e)))?;

            (final_balance, CreditBalance::zero())
        };

        let final_rental = self
            .rental_manager
            .get_rental(&rental_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to get rental: {}", e)))?;
        self.rental_repository
            .update_rental(&final_rental)
            .await
            .map_err(|e| Status::internal(format!("Failed to persist rental: {}", e)))?;

        let response = FinalizeRentalResponse {
            success: true,
            total_cost: final_cost.to_string(),
            duration_hours: duration_hours.to_string(),
            charged_amount: charged_amount.to_string(),
            refunded_amount: refunded_amount.to_string(),
        };

        Ok(Response::new(response))
    }

    async fn ingest_telemetry(
        &self,
        request: Request<tonic::Streaming<TelemetryData>>,
    ) -> std::result::Result<Response<IngestResponse>, Status> {
        let mut stream = request.into_inner();
        let ingester = self.telemetry_ingester.clone();

        let mut events_received = 0u64;
        let mut events_processed = 0u64;
        let mut events_failed = 0u64;
        let mut last_processed = chrono::Utc::now();

        while let Some(result) = stream.next().await {
            match result {
                Ok(telemetry_data) => {
                    events_received += 1;

                    match ingester.ingest(telemetry_data).await {
                        Ok(_) => {
                            events_processed += 1;
                            last_processed = chrono::Utc::now();
                        }
                        Err(e) => {
                            error!("Failed to ingest telemetry: {}", e);
                            events_failed += 1;
                        }
                    }
                }
                Err(e) => {
                    error!("Error receiving telemetry: {}", e);
                    events_failed += 1;
                }
            }
        }

        let response = IngestResponse {
            events_received,
            events_processed,
            events_failed,
            last_processed: Some(prost_types::Timestamp::from(std::time::SystemTime::from(
                last_processed,
            ))),
        };

        Ok(Response::new(response))
    }

    async fn get_usage_report(
        &self,
        request: Request<UsageReportRequest>,
    ) -> std::result::Result<Response<UsageReportResponse>, Status> {
        let req = request.into_inner();
        let rental_id = RentalId::from_str(&req.rental_id)
            .map_err(|e| Status::invalid_argument(format!("Invalid rental ID: {}", e)))?;

        let rental = self
            .rental_repository
            .get_rental(&rental_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to get rental: {}", e)))?
            .ok_or_else(|| Status::not_found("Rental not found"))?;

        let duration = rental.last_updated - rental.created_at;
        let duration_hours =
            duration.num_hours() as f64 + (duration.num_minutes() % 60) as f64 / 60.0;

        let summary = UsageSummary {
            avg_cpu_percent: rental.usage_metrics.cpu_hours.to_f64().unwrap_or(0.0)
                / duration_hours.max(1.0),
            avg_memory_mb: (rental.usage_metrics.memory_gb_hours.to_f64().unwrap_or(0.0) * 1024.0
                / duration_hours.max(1.0)) as u64,
            total_network_bytes: (rental.usage_metrics.network_gb.to_f64().unwrap_or(0.0)
                * 1_073_741_824.0) as u64,
            total_disk_bytes: 0, // Not tracked yet
            avg_gpu_utilization: rental.usage_metrics.gpu_hours.to_f64().unwrap_or(0.0)
                / duration_hours.max(1.0),
            duration_hours: duration_hours.to_string(),
        };

        // Generate mock data points (should come from telemetry data)
        let data_points = vec![UsageDataPoint {
            timestamp: Some(prost_types::Timestamp::from(std::time::SystemTime::from(
                rental.created_at,
            ))),
            usage: None,
            cost: rental.cost_breakdown.base_cost.to_string(),
        }];

        let response = UsageReportResponse {
            rental_id: rental_id.to_string(),
            data_points,
            summary: Some(summary),
            total_cost: rental.cost_breakdown.total_cost.to_string(),
        };

        Ok(Response::new(response))
    }

    async fn get_billing_packages(
        &self,
        request: Request<GetBillingPackagesRequest>,
    ) -> std::result::Result<Response<GetBillingPackagesResponse>, Status> {
        let req = request.into_inner();
        let _user_id = UserId::new(req.user_id);

        let packages = self
            .package_repository
            .list_packages(true)
            .await
            .map_err(|e| Status::internal(format!("Failed to list packages: {}", e)))?;

        let current_package_id = PackageId::standard();

        let billing_packages = packages
            .into_iter()
            .map(|pkg| basilica_protocol::billing::BillingPackage {
                package_id: pkg.id.to_string(),
                name: pkg.id.to_string(),
                description: format!("{} billing package", pkg.id),
                rates: Some(basilica_protocol::billing::PackageRates {
                    cpu_rate_per_hour: "0.01".to_string(),
                    memory_rate_per_gb_hour: "0.005".to_string(),
                    gpu_rates: HashMap::from([
                        ("RTX_4090".to_string(), "1.5".to_string()),
                        ("A100".to_string(), "2.5".to_string()),
                    ]),
                    network_rate_per_gb: "0.1".to_string(),
                    disk_iops_rate: "0.0001".to_string(),
                    base_rate_per_hour: pkg.base_rate.to_string(),
                }),
                included_resources: Some(basilica_protocol::billing::IncludedResources {
                    cpu_core_hours: pkg.included_resources.cpu_hours.to_u32().unwrap_or(0),
                    memory_gb_hours: pkg.included_resources.memory_gb_hours.to_u64().unwrap_or(0),
                    gpu_hours: HashMap::from([(
                        "RTX_4090".to_string(),
                        pkg.included_resources.gpu_hours.to_u32().unwrap_or(0),
                    )]),
                    network_gb: pkg.included_resources.network_gb.to_u64().unwrap_or(0),
                    disk_iops: 0,
                }),
                overage_rates: None,
                priority: 1,
                is_active: true,
            })
            .collect();

        let response = GetBillingPackagesResponse {
            packages: billing_packages,
            current_package_id: current_package_id.to_string(),
        };

        Ok(Response::new(response))
    }

    async fn set_user_package(
        &self,
        request: Request<SetUserPackageRequest>,
    ) -> std::result::Result<Response<SetUserPackageResponse>, Status> {
        let req = request.into_inner();
        let user_id = UserId::new(req.user_id);
        let new_package_id = PackageId::new(req.package_id.clone());

        info!("Setting package {} for user {}", new_package_id, user_id);

        let _package = self
            .package_repository
            .get_package(&new_package_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to get package: {}", e)))?
            .ok_or_else(|| Status::not_found("Package not found"))?;

        // Update user's package (would need user preferences table)
        // For now, just return success

        let response = SetUserPackageResponse {
            success: true,
            previous_package_id: PackageId::standard().to_string(),
            new_package_id: new_package_id.to_string(),
            effective_from: req.effective_from,
        };

        Ok(Response::new(response))
    }
}
