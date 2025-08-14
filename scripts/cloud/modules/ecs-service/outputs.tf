output "service_name" {
  description = "ECS service name"
  value       = aws_ecs_service.main.name
}

output "service_arn" {
  description = "ECS service ARN"
  value       = aws_ecs_service.main.id
}

output "task_definition_arn" {
  description = "Task definition ARN"
  value       = aws_ecs_task_definition.main.arn
}

output "task_execution_role_arn" {
  description = "Task execution role ARN"
  value       = aws_iam_role.task_execution.arn
}

output "task_role_arn" {
  description = "Task role ARN"
  value       = aws_iam_role.task.arn
}

output "security_group_id" {
  description = "ECS tasks security group ID"
  value       = var.ecs_tasks_security_group_id
}

output "service_discovery_service_arn" {
  description = "Service discovery service ARN"
  value       = aws_service_discovery_service.main.arn
}

output "log_group_name" {
  description = "CloudWatch log group name"
  value       = var.enable_logging ? aws_cloudwatch_log_group.main[0].name : null
}

output "auto_scaling_target_resource_id" {
  description = "Auto scaling target resource ID"
  value       = var.enable_auto_scaling ? aws_appautoscaling_target.main[0].resource_id : null
}