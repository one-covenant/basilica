# ECS Task Module

A generalized Terraform module for creating one-off ECS tasks. This module follows SOLID principles and can be reused for any containerized task scenario.

## Features

- **Generic & Reusable**: Works with any Docker image and command
- **Flexible Configuration**: Supports environment variables, secrets, volumes, and more
- **IAM Management**: Configurable execution and task roles with custom policies
- **Logging**: Optional CloudWatch logging with configurable retention
- **SOLID Principles**: Single responsibility, open for extension, dependency injection

## Usage Examples

### Database Initialization

```hcl
module "db_init" {
  source = "./modules/ecs-task"
  
  name_prefix = "basilica-dev"
  task_name   = "db-init"
  
  container_image   = "postgres:15-alpine"
  container_command = [
    "/bin/sh", "-c", <<-EOT
      echo "Waiting for RDS to be ready..."
      until pg_isready -h $PGHOST -p $PGPORT -U $PGUSER; do
        echo "Waiting for database connection..."
        sleep 2
      done
      
      echo "Creating databases..."
      echo "CREATE DATABASE basilica_billing;" | psql || echo "Database may already exist"
      echo "CREATE DATABASE basilica_payments;" | psql || echo "Database may already exist"
      
      echo "Database initialization completed"
    EOT
  ]
  
  environment_variables = {
    PGHOST     = "my-rds-endpoint.amazonaws.com"
    PGPORT     = "5432"
    PGUSER     = "admin"
    PGDATABASE = "postgres"
    PGSSLMODE  = "require"
  }
  
  secrets = [
    {
      name      = "PGPASSWORD"
      valueFrom = "arn:aws:secretsmanager:us-east-1:123456789012:secret:rds-password"
    }
  ]
  
  custom_execution_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect = "Allow"
        Action = ["secretsmanager:GetSecretValue"]
        Resource = ["arn:aws:secretsmanager:us-east-1:123456789012:secret:rds-password"]
      }
    ]
  })
  
  cpu    = 256
  memory = 512
  
  tags = {
    Environment = "dev"
    Purpose     = "database-initialization"
  }
}
```

### Database Migration

```hcl
module "db_migration" {
  source = "./modules/ecs-task"
  
  name_prefix = "basilica-prod"
  task_name   = "db-migration"
  
  container_image   = "migrate/migrate:latest"
  container_command = [
    "migrate",
    "-path", "/migrations",
    "-database", "postgres://user:pass@host/db?sslmode=require",
    "up"
  ]
  
  volumes = [
    {
      name = "migrations"
      efs_volume_configuration = {
        file_system_id = "fs-1234567890abcdef0"
        root_directory = "/migrations"
      }
    }
  ]
  
  mount_points = [
    {
      source_volume  = "migrations"
      container_path = "/migrations"
      read_only      = true
    }
  ]
  
  task_role_policies = [
    "arn:aws:iam::aws:policy/AmazonElasticFileSystemClientWrite"
  ]
  
  tags = {
    Environment = "prod"
    Purpose     = "database-migration"
  }
}
```

### Data Processing Task

```hcl
module "data_processor" {
  source = "./modules/ecs-task"
  
  name_prefix = "analytics"
  task_name   = "daily-report"
  
  container_image = "my-org/data-processor:latest"
  
  environment_variables = {
    PROCESSING_DATE = "2024-01-15"
    OUTPUT_BUCKET   = "my-analytics-bucket"
    LOG_LEVEL       = "info"
  }
  
  task_role_policies = [
    "arn:aws:iam::aws:policy/AmazonS3FullAccess"
  ]
  
  cpu    = 1024
  memory = 2048
  
  log_retention_days = 30
  
  tags = {
    Environment = "prod"
    Purpose     = "data-processing"
    Schedule    = "daily"
  }
}
```

## Variables

| Name | Description | Type | Default | Required |
|------|-------------|------|---------|----------|
| name_prefix | Name prefix for all resources | string | - | yes |
| task_name | Specific name for this task | string | - | yes |
| container_image | Docker image to use | string | - | yes |
| container_command | Command to run | list(string) | [] | no |
| environment_variables | Environment variables | map(string) | {} | no |
| secrets | Secrets from AWS Secrets Manager | list(object) | [] | no |
| cpu | CPU units (256, 512, 1024, 2048, 4096) | number | 256 | no |
| memory | Memory in MB | number | 512 | no |
| execution_role_policies | IAM policies for execution role | list(string) | [default ECS policy] | no |
| task_role_policies | IAM policies for task role | list(string) | [] | no |
| custom_execution_policy | Custom execution policy JSON | string | "" | no |
| custom_task_policy | Custom task policy JSON | string | "" | no |
| enable_logging | Enable CloudWatch logging | bool | true | no |
| log_retention_days | Log retention period | number | 7 | no |
| tags | Resource tags | map(string) | {} | no |

## Outputs

| Name | Description |
|------|-------------|
| task_definition_arn | ARN of the ECS task definition |
| task_definition_family | Family name of the task definition |
| execution_role_arn | ARN of the execution role |
| task_role_arn | ARN of the task role |
| log_group_name | CloudWatch log group name |

## Design Principles

1. **Single Responsibility**: This module only manages ECS task definitions and related IAM roles
2. **Open/Closed**: Open for extension through variables, closed for modification
3. **Dependency Inversion**: Accepts any container image and configuration
4. **DRY**: Single module handles all one-off task scenarios
5. **Interface Segregation**: Clean, focused variable interface

## Migration from Specific Modules

To migrate from a specific module like `db-init`:

1. Replace module source: `./modules/db-init` → `./modules/ecs-task`
2. Add `task_name` parameter with descriptive name
3. Move hardcoded values to `environment_variables`
4. Customize IAM policies through `custom_*_policy` variables
5. Update module outputs if needed
