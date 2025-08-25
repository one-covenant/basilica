# Database Initialization Module

This Terraform module creates an ECS task definition for initializing PostgreSQL databases. It's designed to be run as a one-off task to create the required databases for the Basilica application.

## Features

- Creates ECS Fargate task definition for database initialization
- Configurable database names to create
- IAM roles with least privilege access
- CloudWatch logging
- Secrets Manager integration for database credentials
- Proper resource tagging

## Usage

```hcl
module "db_init" {
  source = "./modules/db-init"

  name_prefix     = "basilica-dev"
  db_endpoint     = module.rds.db_endpoint
  db_username     = module.rds.db_username
  rds_secret_arn  = module.rds.secret_arn
  database_names  = ["basilica_billing", "basilica_payments"]
  aws_region      = "us-east-2"

  tags = {
    Project     = "basilica"
    Environment = "dev"
    ManagedBy   = "terraform"
  }
}
```

## Requirements

| Name | Version |
|------|------|
| terraform | >= 1.0 |
| aws | >= 5.0 |

## Providers

| Name | Version |
|------|------|
| aws | >= 5.0 |

## Resources

| Name | Type |
|------|------|
| aws_ecs_task_definition.db_init | resource |
| aws_cloudwatch_log_group.db_init | resource |
| aws_iam_role.db_init_execution | resource |
| aws_iam_role.db_init_task | resource |
| aws_iam_role_policy_attachment.db_init_execution | resource |
| aws_iam_role_policy.db_init_secrets | resource |
| aws_region.current | data source |

## Inputs

| Name | Description | Type | Default | Required |
|------|-------------|------|---------|:--------:|
| name_prefix | Name prefix for all resources | `string` | n/a | yes |
| db_endpoint | RDS database endpoint | `string` | n/a | yes |
| db_username | RDS database username | `string` | n/a | yes |
| rds_secret_arn | ARN of the RDS secret containing database credentials | `string` | n/a | yes |
| aws_region | AWS region for resources | `string` | n/a | yes |
| database_names | List of database names to create | `list(string)` | `["basilica_billing", "basilica_payments"]` | no |
| cpu | CPU units for the database initialization task | `number` | `256` | no |
| memory | Memory in MB for the database initialization task | `number` | `512` | no |
| postgres_image | PostgreSQL Docker image to use for database initialization | `string` | `"postgres:15-alpine"` | no |
| log_retention_days | Number of days to retain CloudWatch logs | `number` | `7` | no |
| tags | Common tags to apply to all resources | `map(string)` | `{}` | no |

## Outputs

| Name | Description |
|------|-------------|
| task_definition_arn | ARN of the database initialization task definition |
| task_definition_family | Family name of the database initialization task definition |
| task_definition_revision | Revision number of the database initialization task definition |
| execution_role_arn | ARN of the task execution role |
| task_role_arn | ARN of the task role |
| log_group_name | Name of the CloudWatch log group |
| log_group_arn | ARN of the CloudWatch log group |

## Running the Task

To execute the database initialization task, use the AWS CLI or console:

```bash
aws ecs run-task \
    --cluster <cluster-name> \
    --task-definition <task-definition-arn> \
    --network-configuration "awsvpcConfiguration={subnets=[<subnet-ids>],securityGroups=[<security-group-ids>],assignPublicIp=DISABLED}" \
    --launch-type FARGATE
```

## Security Considerations

- The task uses IAM roles with minimal required permissions
- Database credentials are retrieved from AWS Secrets Manager
- SSL/TLS is enforced for database connections
- CloudWatch logs are encrypted at rest
- No sensitive information is exposed in environment variables