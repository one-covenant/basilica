# Data source for current region
data "aws_region" "current" {}

# Local variables for computed values
locals {
  # Generate log group name if not provided
  log_group_name = var.log_group_name != "" ? var.log_group_name : "/ecs/${var.name_prefix}-${var.task_name}"
  
  # Generate family name
  family_name = "${var.name_prefix}-${var.task_name}"
  
  # Prepare environment variables for container definition
  environment_vars = [
    for key, value in var.environment_variables : {
      name  = key
      value = tostring(value)
    }
  ]
  
  # Container definition base
  container_definition = {
    name      = var.task_name
    image     = var.container_image
    essential = var.essential
    
    # Optional command and entrypoint
    command    = length(var.container_command) > 0 ? var.container_command : null
    entryPoint = length(var.container_entrypoint) > 0 ? var.container_entrypoint : null
    
    # Optional working directory
    workingDirectory = var.working_directory != "" ? var.working_directory : null
    
    # Environment variables
    environment = length(local.environment_vars) > 0 ? local.environment_vars : null
    
    # Secrets
    secrets = length(var.secrets) > 0 ? var.secrets : null
    
    # Mount points
    mountPoints = length(var.mount_points) > 0 ? var.mount_points : null
    
    # Ulimits
    ulimits = length(var.ulimits) > 0 ? var.ulimits : null
    
    # Logging configuration
    logConfiguration = var.enable_logging ? {
      logDriver = "awslogs"
      options = {
        awslogs-group         = local.log_group_name
        awslogs-region        = data.aws_region.current.name
        awslogs-stream-prefix = "ecs"
      }
    } : null
  }
  
  # Remove null values from container definition
  container_definition_clean = {
    for k, v in local.container_definition : k => v if v != null
  }
}

# CloudWatch Log Group (optional)
resource "aws_cloudwatch_log_group" "task" {
  count             = var.enable_logging ? 1 : 0
  name              = local.log_group_name
  retention_in_days = var.log_retention_days

  tags = merge(var.tags, {
    Name = "${local.family_name}-logs"
  })
}

# ECS Task Definition
resource "aws_ecs_task_definition" "task" {
  family                   = local.family_name
  network_mode             = var.network_mode
  requires_compatibilities = var.requires_compatibilities
  cpu                      = tostring(var.cpu)
  memory                   = tostring(var.memory)
  execution_role_arn       = aws_iam_role.execution.arn
  task_role_arn            = aws_iam_role.task.arn

  # Volumes
  dynamic "volume" {
    for_each = var.volumes
    content {
      name      = volume.value.name
      host_path = volume.value.host_path
      
      dynamic "efs_volume_configuration" {
        for_each = volume.value.efs_volume_configuration != null ? [volume.value.efs_volume_configuration] : []
        content {
          file_system_id     = efs_volume_configuration.value.file_system_id
          root_directory     = efs_volume_configuration.value.root_directory
          transit_encryption = efs_volume_configuration.value.transit_encryption
        }
      }
    }
  }

  container_definitions = jsonencode([local.container_definition_clean])

  tags = merge(var.tags, {
    Name = "${local.family_name}-task"
  })

  depends_on = [aws_cloudwatch_log_group.task]
}


# IAM role for task execution (pulling images, logs, secrets)
resource "aws_iam_role" "execution" {
  name = "${local.family_name}-execution-role"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Action = "sts:AssumeRole"
        Effect = "Allow"
        Principal = {
          Service = "ecs-tasks.amazonaws.com"
        }
      }
    ]
  })

  tags = var.tags
}

# Attach managed policies to execution role
resource "aws_iam_role_policy_attachment" "execution_policies" {
  count      = length(var.execution_role_policies)
  role       = aws_iam_role.execution.name
  policy_arn = var.execution_role_policies[count.index]
}

# Custom execution role policy (if provided)
resource "aws_iam_role_policy" "execution_custom" {
  count  = var.custom_execution_policy != "" ? 1 : 0
  name   = "${local.family_name}-execution-custom-policy"
  role   = aws_iam_role.execution.id
  policy = var.custom_execution_policy
}

# IAM role for task runtime
resource "aws_iam_role" "task" {
  name = "${local.family_name}-task-role"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Action = "sts:AssumeRole"
        Effect = "Allow"
        Principal = {
          Service = "ecs-tasks.amazonaws.com"
        }
      }
    ]
  })

  tags = var.tags
}

# Attach managed policies to task role
resource "aws_iam_role_policy_attachment" "task_policies" {
  count      = length(var.task_role_policies)
  role       = aws_iam_role.task.name
  policy_arn = var.task_role_policies[count.index]
}

# Custom task role policy (if provided)
resource "aws_iam_role_policy" "task_custom" {
  count  = var.custom_task_policy != "" ? 1 : 0
  name   = "${local.family_name}-task-custom-policy"
  role   = aws_iam_role.task.id
  policy = var.custom_task_policy
}

