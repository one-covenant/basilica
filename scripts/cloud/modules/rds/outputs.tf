output "db_cluster_id" {
  description = "Aurora cluster ID"
  value       = aws_rds_cluster.main.id
}

output "db_cluster_arn" {
  description = "Aurora cluster ARN"
  value       = aws_rds_cluster.main.arn
}

output "db_endpoint" {
  description = "Aurora cluster writer endpoint (hostname only)"
  value       = aws_rds_cluster.main.endpoint
}

output "db_reader_endpoint" {
  description = "Aurora cluster reader endpoint (hostname only)"
  value       = aws_rds_cluster.main.reader_endpoint
}

output "db_port" {
  description = "Aurora cluster port"
  value       = aws_rds_cluster.main.port
}

output "db_name" {
  description = "Database name"
  value       = aws_rds_cluster.main.database_name
}

output "db_username" {
  description = "Database username"
  value       = aws_rds_cluster.main.master_username
  sensitive   = true
}

output "db_password" {
  description = "Database password"
  value       = var.db_password
  sensitive   = true
}

output "secret_arn" {
  description = "Secrets Manager secret ARN"
  value       = aws_secretsmanager_secret.db_credentials.arn
}

output "secret_name" {
  description = "Secrets Manager secret name"
  value       = aws_secretsmanager_secret.db_credentials.name
}

output "db_subnet_group_name" {
  description = "Database subnet group name"
  value       = aws_db_subnet_group.main.name
}

output "cluster_parameter_group_name" {
  description = "Aurora cluster parameter group name"
  value       = aws_rds_cluster_parameter_group.main.name
}