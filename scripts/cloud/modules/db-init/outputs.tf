output "task_definition_arn" {
  description = "ARN of the database initialization task definition"
  value       = aws_ecs_task_definition.db_init.arn
}

output "task_definition_family" {
  description = "Family name of the database initialization task definition"
  value       = aws_ecs_task_definition.db_init.family
}

output "task_definition_revision" {
  description = "Revision number of the database initialization task definition"
  value       = aws_ecs_task_definition.db_init.revision
}

output "execution_role_arn" {
  description = "ARN of the task execution role"
  value       = aws_iam_role.db_init_execution.arn
}

output "task_role_arn" {
  description = "ARN of the task role"
  value       = aws_iam_role.db_init_task.arn
}

output "log_group_name" {
  description = "Name of the CloudWatch log group"
  value       = aws_cloudwatch_log_group.db_init.name
}

output "log_group_arn" {
  description = "ARN of the CloudWatch log group"
  value       = aws_cloudwatch_log_group.db_init.arn
}