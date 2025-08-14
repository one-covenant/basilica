locals {
  name_prefix = "${var.project_name}-${terraform.workspace}"

  common_tags = {
    Project     = var.project_name
    Environment = terraform.workspace
    ManagedBy   = "terraform"
  }

  # Workspace-specific defaults
  env_config = {
    dev = {
      vpc_cidr                   = "10.0.0.0/16"
      availability_zones         = ["us-east-1a", "us-east-1b"]
      db_instance_class          = "db.t3.micro"
      db_allocated_storage       = 20
      enable_deletion_protection = false
      backup_retention_period    = 1
      billing_cpu                = 256
      billing_memory             = 512
      payments_cpu               = 256
      payments_memory            = 512
      min_capacity               = 1
      max_capacity               = 3
      target_capacity            = 1
    }
    prod = {
      vpc_cidr                   = "10.1.0.0/16"
      availability_zones         = ["us-east-1a", "us-east-1b", "us-east-1c"]
      db_instance_class          = "db.t3.small"
      db_allocated_storage       = 100
      enable_deletion_protection = true
      backup_retention_period    = 30
      billing_cpu                = 512
      billing_memory             = 1024
      payments_cpu               = 512
      payments_memory            = 1024
      min_capacity               = 2
      max_capacity               = 10
      target_capacity            = 3
    }
  }

  workspace_config = lookup(local.env_config, terraform.workspace, local.env_config["dev"])
}

# Data sources
data "aws_caller_identity" "current" {}

# Generate random password for RDS
resource "random_password" "db_password" {
  length  = 32
  special = true
}

# Generate random AEAD key for payments encryption (32 bytes = 256 bits)
resource "random_bytes" "payments_aead_key" {
  length = 32
}

# Secrets Manager secret for payments AEAD encryption key
resource "aws_secretsmanager_secret" "payments_aead_key" {
  name                    = "${local.name_prefix}-payments-aead-key"
  description             = "AEAD encryption key for payments service"
  recovery_window_in_days = 7

  tags = merge(local.common_tags, {
    Name = "${local.name_prefix}-payments-aead-key"
  })
}

resource "aws_secretsmanager_secret_version" "payments_aead_key" {
  secret_id     = aws_secretsmanager_secret.payments_aead_key.id
  secret_string = random_bytes.payments_aead_key.hex
}

# Networking module
module "networking" {
  source = "./modules/networking"

  name_prefix        = local.name_prefix
  vpc_cidr           = local.workspace_config.vpc_cidr
  availability_zones = local.workspace_config.availability_zones

  tags = local.common_tags
}

# RDS module
module "rds" {
  source = "./modules/rds"

  name_prefix        = local.name_prefix
  vpc_id             = module.networking.vpc_id
  subnet_ids         = module.networking.private_subnet_ids
  security_group_ids = [module.networking.rds_security_group_id]

  db_instance_class          = local.workspace_config.db_instance_class
  db_allocated_storage       = local.workspace_config.db_allocated_storage
  db_username                = "basilica_admin"
  db_password                = random_password.db_password.result
  enable_deletion_protection = local.workspace_config.enable_deletion_protection
  backup_retention_period    = local.workspace_config.backup_retention_period

  tags = local.common_tags
}

# Application Load Balancer module
module "alb" {
  source = "./modules/alb"

  name_prefix        = local.name_prefix
  vpc_id             = module.networking.vpc_id
  subnet_ids         = module.networking.public_subnet_ids
  security_group_ids = [module.networking.alb_security_group_id]
  certificate_arn    = var.certificate_arn

  tags = local.common_tags
}

# ECS Cluster
resource "aws_ecs_cluster" "main" {
  name = "${local.name_prefix}-cluster"

  setting {
    name  = "containerInsights"
    value = "enabled"
  }

  tags = local.common_tags
}

resource "aws_ecs_cluster_capacity_providers" "main" {
  cluster_name = aws_ecs_cluster.main.name

  capacity_providers = ["FARGATE", "FARGATE_SPOT"]

  default_capacity_provider_strategy {
    base              = 1
    weight            = 100
    capacity_provider = "FARGATE"
  }
}

# Service Discovery Namespace
resource "aws_service_discovery_private_dns_namespace" "main" {
  name = "${local.name_prefix}.local"
  vpc  = module.networking.vpc_id

  tags = local.common_tags
}

# Billing service
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
  alb_target_group_arn = module.alb.billing_target_group_arn
  alb_listener_arn     = module.alb.listener_arn
  health_check_path    = "/health"

  # Scaling configuration
  min_capacity    = local.workspace_config.min_capacity
  max_capacity    = local.workspace_config.max_capacity
  target_capacity = local.workspace_config.target_capacity

  # Service discovery
  service_discovery_namespace_id = aws_service_discovery_private_dns_namespace.main.id

  # Environment variables
  environment_variables = {
    BILLING_SERVICE_ENVIRONMENT         = terraform.workspace
    BILLING_DATABASE_URL                = "postgresql://${module.rds.db_username}:${module.rds.db_password}@${module.rds.db_endpoint}/basilica_billing"
    BILLING_GRPC_LISTEN_ADDRESS         = "0.0.0.0"
    BILLING_GRPC_PORT                   = "50051"
    BILLING_HTTP_LISTEN_ADDRESS         = "0.0.0.0"
    BILLING_HTTP_PORT                   = "8080"
    BILLING_AWS_REGION                  = var.aws_region
    BILLING_AWS_USE_IAM_AUTH            = "true"
    BILLING_AWS_SECRETS_MANAGER_ENABLED = "true"
    BILLING_AWS_SECRET_NAME             = module.rds.secret_arn
  }

  tags = local.common_tags
}

# Payments service
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
  alb_target_group_arn = module.alb.payments_target_group_arn
  alb_listener_arn     = module.alb.listener_arn
  health_check_path    = "/health"

  # Scaling configuration
  min_capacity    = local.workspace_config.min_capacity
  max_capacity    = local.workspace_config.max_capacity
  target_capacity = local.workspace_config.target_capacity

  # Service discovery
  service_discovery_namespace_id = aws_service_discovery_private_dns_namespace.main.id

  # Environment variables
  environment_variables = {
    DATABASE_URL       = "postgresql://${module.rds.db_username}:${module.rds.db_password}@${module.rds.db_endpoint}/basilica_payments"
    SUBXT_WS           = "wss://entrypoint-finney.opentensor.ai:443"
    BILLING_GRPC       = "http://billing.${aws_service_discovery_private_dns_namespace.main.name}:50051"
    PAYMENTS_GRPC_BIND = "0.0.0.0:50061"
    SS58_PREFIX        = "42"
    TAO_DECIMALS       = "9"
    TAO_USD_RATE       = "100.0"
  }

  # Secrets from AWS Secrets Manager
  secrets = [
    {
      name      = "PAYMENTS_AEAD_KEY_HEX"
      valueFrom = aws_secretsmanager_secret.payments_aead_key.arn
    }
  ]

  tags = local.common_tags
}