output "alb_dns_name" {
  value = module.alb.alb_dns_name
}

output "ecs_cluster_name" {
  description = "Name of the ECS cluster"
  value       = aws_ecs_cluster.main.name
}

output "ecs_cluster_arn" {
  description = "ARN of the ECS cluster"
  value       = aws_ecs_cluster.main.arn
}

output "service_discovery_namespace_id" {
  description = "ID of the service discovery namespace"
  value       = aws_service_discovery_private_dns_namespace.main.id
}

output "service_discovery_namespace_name" {
  description = "Name of the service discovery namespace"
  value       = aws_service_discovery_private_dns_namespace.main.name
}

output "db_endpoint" {
  value     = module.rds.db_endpoint
  sensitive = true
}

output "billing_database_name" {
  description = "Name of the billing database"
  value       = "basilica_v3_billing"
}

output "payments_database_name" {
  description = "Name of the payments database"
  value       = "basilica_v3_payments"
}

output "database_connection_info" {
  description = "Database connection information for services"
  value = {
    endpoint    = module.rds.db_endpoint
    port        = module.rds.db_port
    username    = module.rds.db_username
    default_db  = module.rds.db_name
    billing_db  = "basilica_v3_billing"
    payments_db = "basilica_v3_payments"
  }
  sensitive = true
}

# Outputs needed for tasks command
output "cluster_name" {
  description = "ECS cluster name for running tasks"
  value       = aws_ecs_cluster.main.name
}

output "db_init_task_definition_arn" {
  description = "ARN of the db-init task definition"
  value       = module.db_init.task_definition_arn
}

output "private_subnet_ids" {
  description = "Private subnet IDs for ECS tasks"
  value       = module.networking.private_subnet_ids
}

output "ecs_tasks_security_group_id" {
  description = "Security group ID for ECS tasks"
  value       = module.networking.ecs_tasks_security_group_id
}