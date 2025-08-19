locals {
  name_prefix = "${var.project_name}-${terraform.workspace}-v3"

  common_tags = {
    Project     = var.project_name
    Environment = terraform.workspace
    ManagedBy   = "terraform"
  }

  env_config = {
    dev = {
      vpc_cidr                   = "10.0.0.0/16"
      availability_zones         = ["us-east-2a", "us-east-2b"]
      min_acu_capacity           = 0.5
      max_acu_capacity           = 4
      cluster_instance_count     = 1
      enable_deletion_protection = false
      backup_retention_period    = 1
      billing_cpu                = 256
      billing_memory             = 512
      payments_cpu               = 256
      payments_memory            = 512
      basilica_api_cpu           = 256
      basilica_api_memory        = 512
      min_capacity               = 1
      max_capacity               = 3
      target_capacity            = 1
    }
    prod = {
      vpc_cidr                   = "10.1.0.0/16"
      availability_zones         = ["us-east-2a", "us-east-2b", "us-east-2c"]
      min_acu_capacity           = 1
      max_acu_capacity           = 16
      cluster_instance_count     = 2
      enable_deletion_protection = true
      backup_retention_period    = 30
      billing_cpu                = 512
      billing_memory             = 1024
      payments_cpu               = 512
      payments_memory            = 1024
      basilica_api_cpu           = 512
      basilica_api_memory        = 1024
      min_capacity               = 2
      max_capacity               = 10
      target_capacity            = 3
    }
  }

  workspace_config = lookup(local.env_config, terraform.workspace, local.env_config["dev"])
}

# Data sources
data "aws_caller_identity" "current" {}
data "aws_region" "current" {}

# Networking module
module "networking" {
  source = "./modules/networking"

  name_prefix        = local.name_prefix
  vpc_cidr           = local.workspace_config.vpc_cidr
  availability_zones = local.workspace_config.availability_zones

  tags = local.common_tags
}

# Aurora Serverless v2 module
module "rds" {
  source = "./modules/rds"

  name_prefix        = local.name_prefix
  vpc_id             = module.networking.vpc_id
  subnet_ids         = module.networking.database_subnet_ids
  security_group_ids = [module.networking.rds_security_group_id]

  min_acu_capacity           = local.workspace_config.min_acu_capacity
  max_acu_capacity           = local.workspace_config.max_acu_capacity
  cluster_instance_count     = local.workspace_config.cluster_instance_count
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

  # gRPC target groups
  additional_target_groups = {
    "bill-grpc" = {
      port             = 50051
      protocol         = "HTTP"
      protocol_version = "GRPC"
      health_check = {
        healthy_threshold   = 2
        unhealthy_threshold = 3
        timeout             = 10
        interval            = 30
        protocol            = "HTTP"
        path                = "/grpc.health.v1.Health/Check"
        matcher             = "0,12"
      }
    }
    "pay-grpc" = {
      port             = 50061
      protocol         = "HTTP"
      protocol_version = "GRPC"
      health_check = {
        healthy_threshold   = 2
        unhealthy_threshold = 3
        timeout             = 10
        interval            = 30
        protocol            = "HTTP"
        path                = "/grpc.health.v1.Health/Check"
        matcher             = "0,12"
      }
    }
  }

  # gRPC listener rules
  additional_listener_rules = {
    "billing-grpc" = {
      priority          = 150
      target_group_key  = "bill-grpc"
      listener_protocol = "HTTPS"
      path_patterns     = ["/basilica.billing.v1.BillingService/*"]
    }
    "payments-grpc" = {
      priority          = 250
      target_group_key  = "pay-grpc"
      listener_protocol = "HTTPS"
      path_patterns     = ["/basilica.payments.v1.PaymentsService/*"]
    }
  }

  tags = local.common_tags
}

# External Application Load Balancer for Basilica API
module "basilica_api_alb" {
  source = "./modules/alb"

  name_prefix        = "${var.project_name}-${terraform.workspace}-api"
  vpc_id             = module.networking.vpc_id
  subnet_ids         = module.networking.public_subnet_ids
  security_group_ids = [module.networking.alb_security_group_id]
  certificate_arn    = var.certificate_arn

  # Don't create billing/payments target groups for API ALB
  create_billing_target_group  = false
  create_payments_target_group = false

  # Create API target group with proper health check
  additional_target_groups = {
    "api-http" = {
      port             = 8000
      protocol         = "HTTP"
      protocol_version = "HTTP1"
      health_check = {
        enabled             = true
        healthy_threshold   = 2
        unhealthy_threshold = 2
        timeout             = 5
        interval            = 30
        path                = "/health"
        matcher             = "200"
        protocol            = "HTTP"
      }
    }
  }

  # Forward all traffic to API service
  additional_listener_rules = {
    "api-all" = {
      target_group_key  = "api-http"
      priority          = 100
      listener_protocol = "HTTPS"
      path_patterns     = ["/*"]
    }
  }

  tags = local.common_tags
}
