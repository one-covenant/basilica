# =============================================================================
# DATA SOURCES
# =============================================================================

# Get RDS credentials from secrets manager to construct database URLs
data "aws_secretsmanager_secret_version" "db_credentials" {
  secret_id = module.rds.secret_arn
}

locals {
  # Parse the RDS credentials JSON
  db_creds = jsondecode(data.aws_secretsmanager_secret_version.db_credentials.secret_string)

  # Construct database URLs for each service
  billing_database_url  = "postgres://${local.db_creds.username}:${urlencode(local.db_creds.password)}@${local.db_creds.host}:${local.db_creds.port}/basilica_v3_billing?sslmode=disable"
  payments_database_url = "postgres://${local.db_creds.username}:${urlencode(local.db_creds.password)}@${local.db_creds.host}:${local.db_creds.port}/basilica_v3_payments?sslmode=disable"
}

# =============================================================================
# ECS CLUSTER CONFIGURATION
# =============================================================================

# Main ECS cluster for running containerized services
resource "aws_ecs_cluster" "main" {
  name = "${local.name_prefix}-cluster"

  setting {
    name  = "containerInsights"
    value = "enabled"
  }

  tags = local.common_tags
}

# Configure capacity providers for the ECS cluster
resource "aws_ecs_cluster_capacity_providers" "main" {
  cluster_name = aws_ecs_cluster.main.name

  capacity_providers = ["FARGATE", "FARGATE_SPOT"]

  default_capacity_provider_strategy {
    base              = 1
    weight            = 100
    capacity_provider = "FARGATE"
  }
}

# =============================================================================
# SERVICE DISCOVERY
# =============================================================================

# Private DNS namespace for service-to-service communication
resource "aws_service_discovery_private_dns_namespace" "main" {
  name = "${local.name_prefix}.local"
  vpc  = module.networking.vpc_id

  tags = local.common_tags
}

# =============================================================================
# DATABASE SECRET DATA SOURCE
# =============================================================================

# Data source to retrieve RDS credentials from Secrets Manager
# =============================================================================
# BILLING SERVICE WITH DB INIT
# =============================================================================

# ECS service for handling billing operations with init container
module "billing_service" {
  source = "./modules/ecs-service"

  name_prefix                 = local.name_prefix
  service_name                = "billing"
  cluster_id                  = aws_ecs_cluster.main.id
  vpc_id                      = module.networking.vpc_id
  subnet_ids                  = module.networking.private_subnet_ids
  ecs_tasks_security_group_id = module.networking.ecs_tasks_security_group_id

  # Container configuration
  container_image = var.billing_image
  container_port  = 8080
  grpc_port       = 50051
  metrics_port    = 9090
  cpu             = local.workspace_config.billing_cpu
  memory          = local.workspace_config.billing_memory

  # Load balancer configuration
  alb_target_group_arn      = module.alb.billing_target_group_arn
  alb_grpc_target_group_arn = module.alb.additional_target_group_arns["bill-grpc"]
  alb_listener_arn          = module.alb.listener_arn
  health_check_path         = "/health"
  health_check_type         = "http"

  # Scaling configuration
  min_capacity    = local.workspace_config.min_capacity
  max_capacity    = local.workspace_config.max_capacity
  target_capacity = local.workspace_config.target_capacity

  # Service discovery
  service_discovery_namespace_id = aws_service_discovery_private_dns_namespace.main.id

  # No init container - using separate db init task
  init_container = {
    enabled = false
  }

  # Environment variables
  environment_variables = {
    # Service Configuration
    BILLING_SERVICE__NAME                   = "basilica-billing"
    BILLING_SERVICE__ENVIRONMENT            = terraform.workspace
    BILLING_SERVICE__LOG_LEVEL              = "info"
    BILLING_SERVICE__METRICS_ENABLED        = "true"
    BILLING_SERVICE__OPENTELEMETRY_ENDPOINT = ""
    BILLING_SERVICE__SERVICE_ID             = "billing-${terraform.workspace}"

    # Database Configuration - Complete URL
    BILLING_DATABASE__URL                     = local.billing_database_url
    BILLING_DATABASE__MAX_CONNECTIONS         = "32"
    BILLING_DATABASE__MIN_CONNECTIONS         = "5"
    BILLING_DATABASE__CONNECT_TIMEOUT_SECONDS = "30"
    BILLING_DATABASE__ACQUIRE_TIMEOUT_SECONDS = "30"
    BILLING_DATABASE__IDLE_TIMEOUT_SECONDS    = "600"
    BILLING_DATABASE__MAX_LIFETIME_SECONDS    = "1800"

    # gRPC Configuration
    BILLING_GRPC__LISTEN_ADDRESS             = "0.0.0.0"
    BILLING_GRPC__PORT                       = "50051"
    BILLING_GRPC__MAX_MESSAGE_SIZE           = "4194304"
    BILLING_GRPC__KEEPALIVE_INTERVAL_SECONDS = "300"
    BILLING_GRPC__KEEPALIVE_TIMEOUT_SECONDS  = "20"
    BILLING_GRPC__TLS_ENABLED                = "false"
    BILLING_GRPC__TLS_CERT_PATH              = ""
    BILLING_GRPC__TLS_KEY_PATH               = ""
    BILLING_GRPC__MAX_CONCURRENT_REQUESTS    = "1000"
    BILLING_GRPC__MAX_CONCURRENT_STREAMS     = "100"
    BILLING_GRPC__REQUEST_TIMEOUT_SECONDS    = "60"

    # HTTP Configuration
    BILLING_HTTP__LISTEN_ADDRESS       = "0.0.0.0"
    BILLING_HTTP__PORT                 = "8080"
    BILLING_HTTP__CORS_ENABLED         = "true"
    BILLING_HTTP__CORS_ALLOWED_ORIGINS = "[\"*\"]"

    # Aggregator Configuration
    BILLING_AGGREGATOR__BATCH_SIZE                  = "1000"
    BILLING_AGGREGATOR__BATCH_TIMEOUT_SECONDS       = "60"
    BILLING_AGGREGATOR__PROCESSING_INTERVAL_SECONDS = "30"
    BILLING_AGGREGATOR__RETENTION_DAYS              = "90"
    BILLING_AGGREGATOR__MAX_EVENTS_PER_SECOND       = "10000"

    # Telemetry Configuration
    BILLING_TELEMETRY__INGEST_BUFFER_SIZE     = "10000"
    BILLING_TELEMETRY__FLUSH_INTERVAL_SECONDS = "10"
    BILLING_TELEMETRY__MAX_BATCH_SIZE         = "500"
    BILLING_TELEMETRY__COMPRESSION_ENABLED    = "true"

    # Rules Engine Configuration
    BILLING_RULES_ENGINE__EVALUATION_INTERVAL_SECONDS = "60"
    BILLING_RULES_ENGINE__CACHE_TTL_SECONDS           = "300"
    BILLING_RULES_ENGINE__MAX_RULES_PER_PACKAGE       = "100"
    BILLING_RULES_ENGINE__DEFAULT_PACKAGE_ID          = "standard"

    # AWS Configuration
    BILLING_AWS__REGION                  = var.aws_region
    BILLING_AWS__USE_IAM_AUTH            = "true"
    BILLING_AWS__SECRETS_MANAGER_ENABLED = "false"
    BILLING_AWS__SECRET_NAME             = ""
    BILLING_AWS__ENDPOINT_URL            = ""

    # Logging
    RUST_LOG = "basilica_billing=info,basilica_protocol=info"
  }

  # No secrets needed - using environment variables
  secrets = []

  tags = local.common_tags

  depends_on = [module.rds]
}

# =============================================================================
# PAYMENTS SERVICE WITH DB INIT
# =============================================================================

# ECS service for handling payment operations with init container
module "payments_service" {
  source = "./modules/ecs-service"

  name_prefix                 = local.name_prefix
  service_name                = "payments"
  cluster_id                  = aws_ecs_cluster.main.id
  vpc_id                      = module.networking.vpc_id
  subnet_ids                  = module.networking.private_subnet_ids
  ecs_tasks_security_group_id = module.networking.ecs_tasks_security_group_id

  # Container configuration
  container_image = var.payments_image
  container_port  = 8082
  grpc_port       = 50061
  metrics_port    = 9092
  cpu             = local.workspace_config.payments_cpu
  memory          = local.workspace_config.payments_memory

  # Load balancer configuration
  alb_target_group_arn      = module.alb.payments_target_group_arn
  alb_grpc_target_group_arn = module.alb.additional_target_group_arns["pay-grpc"]
  alb_listener_arn          = module.alb.listener_arn
  health_check_path         = "/health"
  health_check_type         = "http"

  # Scaling configuration
  min_capacity    = local.workspace_config.min_capacity
  max_capacity    = local.workspace_config.max_capacity
  target_capacity = local.workspace_config.target_capacity

  # Service discovery
  service_discovery_namespace_id = aws_service_discovery_private_dns_namespace.main.id

  # No init container - using separate db init task
  init_container = {
    enabled = false
  }

  # Environment variables
  environment_variables = {
    # AWS Configuration
    PAYMENTS_AWS_REGION                  = var.aws_region
    PAYMENTS_AWS_SECRETS_MANAGER_ENABLED = "false"

    # Service Configuration
    PAYMENTS_SERVICE__ENVIRONMENT = terraform.workspace
    PAYMENTS_SERVICE__LOG_LEVEL   = "info"

    # Database Configuration - Complete URL
    PAYMENTS_DATABASE__URL = local.payments_database_url

    # Service Communication
    PAYMENTS_BILLING__GRPC_ENDPOINT = "http://billing-v3.${aws_service_discovery_private_dns_namespace.main.name}:50051"
    PAYMENTS_GRPC__LISTEN_ADDRESS   = "0.0.0.0"
    PAYMENTS_GRPC__PORT             = "50061"

    # Blockchain Configuration
    PAYMENTS_BLOCKCHAIN__WEBSOCKET_URL = "wss://entrypoint-finney.opentensor.ai:443"
    PAYMENTS_BLOCKCHAIN__SS58_PREFIX   = "42"

    # Treasury Configuration
    PAYMENTS_TREASURY__TAO_DECIMALS = "9"

    # Price Oracle Configuration
    PAYMENTS_PRICE_ORACLE__UPDATE_INTERVAL_SECONDS = "300"
    PAYMENTS_PRICE_ORACLE__MAX_PRICE_AGE_SECONDS   = "600"
    PAYMENTS_PRICE_ORACLE__REQUEST_TIMEOUT_SECONDS = "30"

    # Logging
    RUST_LOG = "basilica_payments=info,basilica_protocol=info"
  }

  # Secrets from AWS Secrets Manager
  secrets = [
    {
      name      = "PAYMENTS_AEAD_KEY_HEX"
      valueFrom = aws_secretsmanager_secret.payments_aead_key.arn
    }
  ]

  tags = local.common_tags

  depends_on = [module.rds]
}

# =============================================================================
# DATABASE INITIALIZATION
# =============================================================================

# One-time database initialization task
# Run manually with: aws ecs run-task --cluster <cluster> --task-definition <task-def> --launch-type FARGATE --network-configuration "awsvpcConfiguration={subnets=[subnet-ids],securityGroups=[sg-id],assignPublicIp=DISABLED}"
module "db_init" {
  source = "./modules/ecs-task"

  name_prefix = local.name_prefix
  task_name   = "db-init"

  # Container configuration
  container_image = "postgres:15-alpine"
  container_command = [
    "/bin/sh",
    "-c",
    <<-EOT
      echo "Waiting for RDS to be ready..."
      until pg_isready -h "$PGHOST" -p "$PGPORT" -U "$PGUSER"; do
        echo "Waiting for database connection..."
        sleep 2
      done

      echo "Extracting password from RDS credentials..."
      apk add --no-cache jq > /dev/null 2>&1
      export PGPASSWORD=$(echo "$RDS_CREDENTIALS" | jq -r '.password')

      echo "Attempting to create databases using extracted password..."
      echo "CREATE DATABASE basilica_v3_billing;" | psql || echo "Database basilica_v3_billing may already exist"
      echo "CREATE DATABASE basilica_v3_payments;" | psql || echo "Database basilica_v3_payments may already exist"

      echo "Verifying databases exist..."
      echo "\l" | psql | grep -E "(basilica_v3_billing|basilica_v3_payments)" || echo "Could not verify databases"

      echo "Database initialization task completed"
    EOT
  ]

  # Environment variables
  environment_variables = {
    PGHOST     = module.rds.db_endpoint
    PGPORT     = "5432"
    PGUSER     = module.rds.db_username
    PGDATABASE = "basilica_v3"
    PGSSLMODE  = "disable"
  }

  # Secrets from AWS Secrets Manager
  secrets = [
    {
      name      = "RDS_CREDENTIALS"
      valueFrom = module.rds.secret_arn
    }
  ]

  # Resource allocation
  cpu    = 256
  memory = 512

  # Custom IAM policy for Secrets Manager access
  custom_execution_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect = "Allow"
        Action = [
          "secretsmanager:GetSecretValue"
        ]
        Resource = [module.rds.secret_arn]
      }
    ]
  })

  tags = local.common_tags

  depends_on = [module.rds]
}
