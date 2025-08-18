# Data sources
data "aws_region" "current" {}
data "aws_caller_identity" "current" {}

# CloudWatch Log Group
resource "aws_cloudwatch_log_group" "main" {
  count             = var.enable_logging ? 1 : 0
  name              = "/ecs/${var.name_prefix}-${var.service_name}"
  retention_in_days = var.log_retention_days

  tags = merge(var.tags, {
    Name = "${var.name_prefix}-${var.service_name}-logs"
  })
}

# Task Execution Role
resource "aws_iam_role" "task_execution" {
  name = "${var.name_prefix}-${var.service_name}-execution-role"

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

resource "aws_iam_role_policy_attachment" "task_execution" {
  role       = aws_iam_role.task_execution.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"
}

# Additional policy for Secrets Manager access
resource "aws_iam_role_policy" "secrets_manager" {
  count = length(var.secrets) > 0 ? 1 : 0
  name  = "${var.name_prefix}-${var.service_name}-secrets-policy"
  role  = aws_iam_role.task_execution.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect = "Allow"
        Action = [
          "secretsmanager:GetSecretValue"
        ]
        Resource = [for secret in var.secrets : secret.valueFrom]
      }
    ]
  })
}

# Task Role (for application permissions)
resource "aws_iam_role" "task" {
  name = "${var.name_prefix}-${var.service_name}-task-role"

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

# Policy for ECS Exec (if enabled)
resource "aws_iam_role_policy" "ecs_exec" {
  count = var.enable_execute_command ? 1 : 0
  name  = "${var.name_prefix}-${var.service_name}-exec-policy"
  role  = aws_iam_role.task.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect = "Allow"
        Action = [
          "ssmmessages:CreateControlChannel",
          "ssmmessages:CreateDataChannel",
          "ssmmessages:OpenControlChannel",
          "ssmmessages:OpenDataChannel"
        ]
        Resource = "*"
      }
    ]
  })
}

# Service Discovery Service
resource "aws_service_discovery_service" "main" {
  name = "${var.service_name}-v3"

  dns_config {
    namespace_id = var.service_discovery_namespace_id

    dns_records {
      ttl  = 10
      type = "A"
    }

    routing_policy = "MULTIVALUE"
  }

  health_check_custom_config {
    failure_threshold = 1
  }

  tags = merge(var.tags, {
    Name = "${var.name_prefix}-${var.service_name}-discovery"
  })
}

# Task Definition
resource "aws_ecs_task_definition" "main" {
  family                   = "${var.name_prefix}-${var.service_name}"
  network_mode             = "awsvpc"
  requires_compatibilities = ["FARGATE"]
  cpu                      = var.cpu
  memory                   = var.memory
  execution_role_arn       = aws_iam_role.task_execution.arn
  task_role_arn            = aws_iam_role.task.arn

  container_definitions = jsonencode(concat(
    # Optional init container
    var.init_container.enabled ? [
      {
        name      = "db-init"
        image     = var.init_container.image
        essential = false
        
        command = length(var.init_container.command) > 0 ? var.init_container.command : null
        
        environment = var.init_container.environment
        secrets     = var.init_container.secrets
        
        logConfiguration = var.enable_logging ? {
          logDriver = "awslogs"
          options = {
            awslogs-group         = aws_cloudwatch_log_group.main[0].name
            awslogs-region        = data.aws_region.current.name
            awslogs-stream-prefix = "db-init"
          }
        } : null
      }
    ] : [],
    # Main service container
    [
      {
        name  = var.service_name
        image = var.container_image
        
        # Depend on init container if enabled
        dependsOn = var.init_container.enabled ? [
          {
            containerName = "db-init"
            condition     = "SUCCESS"
          }
        ] : null

        portMappings = [
          {
            containerPort = var.container_port
            protocol      = "tcp"
            name          = "http"
          },
          {
            containerPort = var.grpc_port
            protocol      = "tcp"
            name          = "grpc"
          },
          {
            containerPort = var.metrics_port
            protocol      = "tcp"
            name          = "metrics"
          }
        ]

        environment = [
          for key, value in var.environment_variables : {
            name  = key
            value = value
          }
        ]

        secrets = var.secrets

        logConfiguration = var.enable_logging ? {
          logDriver = "awslogs"
          options = {
            awslogs-group         = aws_cloudwatch_log_group.main[0].name
            awslogs-region        = data.aws_region.current.name
            awslogs-stream-prefix = "ecs"
          }
        } : null

        healthCheck = var.health_check_type == "grpc" ? {
          command = [
            "CMD-SHELL",
            "test -f /usr/local/bin/grpc_health_probe || (wget -q -O /usr/local/bin/grpc_health_probe https://github.com/grpc-ecosystem/grpc-health-probe/releases/download/v0.4.24/grpc_health_probe-linux-amd64 && chmod +x /usr/local/bin/grpc_health_probe); /usr/local/bin/grpc_health_probe -addr=localhost:${var.grpc_port}"
          ]
          interval    = 30
          timeout     = 10
          retries     = 3
          startPeriod = 90
        } : {
          command = [
            "CMD-SHELL",
            "wget --spider -q http://localhost:${var.container_port}${var.health_check_path} || exit 1"
          ]
          interval    = 30
          timeout     = 5
          retries     = 3
          startPeriod = 60
        }

        essential = true
      }
    ]
  ))

  tags = merge(var.tags, {
    Name = "${var.name_prefix}-${var.service_name}-task"
  })
}

# Additional security group rules for this specific service
resource "aws_security_group_rule" "service_grpc" {
  type              = "ingress"
  from_port         = var.grpc_port
  to_port           = var.grpc_port
  protocol          = "tcp"
  cidr_blocks       = ["10.0.0.0/8"]
  security_group_id = var.ecs_tasks_security_group_id
  description       = "gRPC for ${var.service_name}"
}

resource "aws_security_group_rule" "service_metrics" {
  type              = "ingress"
  from_port         = var.metrics_port
  to_port           = var.metrics_port
  protocol          = "tcp"
  cidr_blocks       = ["10.0.0.0/8"]
  security_group_id = var.ecs_tasks_security_group_id
  description       = "Metrics for ${var.service_name}"
}

# ECS Service
resource "aws_ecs_service" "main" {
  name            = "${var.name_prefix}-${var.service_name}"
  cluster         = var.cluster_id
  task_definition = aws_ecs_task_definition.main.arn
  desired_count   = var.target_capacity

  capacity_provider_strategy {
    capacity_provider = "FARGATE"
    weight            = 100
  }

  network_configuration {
    security_groups  = [var.ecs_tasks_security_group_id]
    subnets          = var.subnet_ids
    assign_public_ip = false
  }

  load_balancer {
    target_group_arn = var.alb_target_group_arn
    container_name   = var.service_name
    container_port   = var.load_balancer_port != null ? var.load_balancer_port : (var.health_check_type == "grpc" ? var.grpc_port : var.container_port)
  }

  service_registries {
    registry_arn = aws_service_discovery_service.main.arn
  }

  deployment_maximum_percent         = var.deployment_configuration.maximum_percent
  deployment_minimum_healthy_percent = var.deployment_configuration.minimum_healthy_percent

  enable_execute_command = var.enable_execute_command

  depends_on = [aws_iam_role_policy_attachment.task_execution]

  tags = merge(var.tags, {
    Name = "${var.name_prefix}-${var.service_name}-service"
  })

  lifecycle {
    ignore_changes = [desired_count]
  }
}

# Auto Scaling Target
resource "aws_appautoscaling_target" "main" {
  count              = var.enable_auto_scaling ? 1 : 0
  max_capacity       = var.max_capacity
  min_capacity       = var.min_capacity
  resource_id        = "service/${split("/", var.cluster_id)[1]}/${aws_ecs_service.main.name}"
  scalable_dimension = "ecs:service:DesiredCount"
  service_namespace  = "ecs"

  tags = var.tags
}

# Auto Scaling Policy - Scale Up
resource "aws_appautoscaling_policy" "scale_up" {
  count              = var.enable_auto_scaling ? 1 : 0
  name               = "${var.name_prefix}-${var.service_name}-scale-up"
  policy_type        = "TargetTrackingScaling"
  resource_id        = aws_appautoscaling_target.main[0].resource_id
  scalable_dimension = aws_appautoscaling_target.main[0].scalable_dimension
  service_namespace  = aws_appautoscaling_target.main[0].service_namespace

  target_tracking_scaling_policy_configuration {
    predefined_metric_specification {
      predefined_metric_type = "ECSServiceAverageCPUUtilization"
    }

    target_value       = var.scale_up_cpu_threshold
    scale_out_cooldown = 300
    scale_in_cooldown  = 300
  }
}

# CloudWatch Alarms
resource "aws_cloudwatch_metric_alarm" "high_cpu" {
  count               = var.enable_auto_scaling ? 1 : 0
  alarm_name          = "${var.name_prefix}-${var.service_name}-high-cpu"
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = "2"
  metric_name         = "CPUUtilization"
  namespace           = "AWS/ECS"
  period              = "300"
  statistic           = "Average"
  threshold           = "80"
  alarm_description   = "This metric monitors ecs cpu utilization"

  dimensions = {
    ServiceName = aws_ecs_service.main.name
    ClusterName = split("/", var.cluster_id)[1]
  }

  tags = var.tags
}

resource "aws_cloudwatch_metric_alarm" "high_memory" {
  count               = var.enable_auto_scaling ? 1 : 0
  alarm_name          = "${var.name_prefix}-${var.service_name}-high-memory"
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = "2"
  metric_name         = "MemoryUtilization"
  namespace           = "AWS/ECS"
  period              = "300"
  statistic           = "Average"
  threshold           = "80"
  alarm_description   = "This metric monitors ecs memory utilization"

  dimensions = {
    ServiceName = aws_ecs_service.main.name
    ClusterName = split("/", var.cluster_id)[1]
  }

  tags = var.tags
}
