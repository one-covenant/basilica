output "task_definition_arn" {
  description = "ARN of the ECS task definition"
  value       = aws_ecs_task_definition.task.arn
}

output "task_definition_family" {
  description = "Family name of the ECS task definition"
  value       = aws_ecs_task_definition.task.family
}

output "task_definition_revision" {
  description = "Revision number of the ECS task definition"
  value       = aws_ecs_task_definition.task.revision
}

output "execution_role_arn" {
  description = "ARN of the task execution role"
  value       = aws_iam_role.execution.arn
}

output "execution_role_name" {
  description = "Name of the task execution role"
  value       = aws_iam_role.execution.name
}

output "task_role_arn" {
  description = "ARN of the task runtime role"
  value       = aws_iam_role.task.arn
}

output "task_role_name" {
  description = "Name of the task runtime role"
  value       = aws_iam_role.task.name
}

output "log_group_name" {
  description = "Name of the CloudWatch log group (if logging enabled)"
  value       = var.enable_logging ? aws_cloudwatch_log_group.task[0].name : null
}

output "log_group_arn" {
  description = "ARN of the CloudWatch log group (if logging enabled)"
  value       = var.enable_logging ? aws_cloudwatch_log_group.task[0].arn : null
}

output "container_name" {
  description = "Name of the container in the task definition"
  value       = var.task_name
}

output "task_name" {
  description = "The task name used in this module"
  value       = var.task_name
}