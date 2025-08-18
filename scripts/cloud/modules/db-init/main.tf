# Data source for current region
data "aws_region" "current" {}

# Database initialization task - one-off ECS task to create databases
resource "aws_ecs_task_definition" "db_init" {
  family                   = "${var.name_prefix}-db-init"
  network_mode             = "awsvpc"
  requires_compatibilities = ["FARGATE"]
  cpu                      = tostring(var.cpu)
  memory                   = tostring(var.memory)
  execution_role_arn       = aws_iam_role.db_init_execution.arn
  task_role_arn            = aws_iam_role.db_init_task.arn

  container_definitions = jsonencode([
    {
      name      = "db-init"
      image     = var.postgres_image
      essential = true

      environment = [
        {
          name  = "PGHOST"
          value = var.db_endpoint
        },
        {
          name  = "PGPORT"
          value = "5432"
        },
        {
          name  = "PGUSER"
          value = var.db_username
        },
        {
          name  = "PGDATABASE"
          value = "basilica"
        },
        {
          name  = "PGSSLMODE"
          value = "require"
        }
      ]

      secrets = [
        {
          name      = "PGPASSWORD"
          valueFrom = "${var.rds_secret_arn}:password::"
        }
      ]

      command = [
        "/bin/sh",
        "-c",
        <<-EOT
          echo "Waiting for RDS to be ready..."
          until pg_isready -h $PGHOST -p $PGPORT -U $PGUSER; do
            echo "Waiting for database connection..."
            sleep 2
          done

          echo "Attempting to create databases using environment password..."
          %{for db_name in var.database_names}
          echo "CREATE DATABASE ${db_name};" | psql || echo "Database ${db_name} may already exist"
          %{endfor}

          echo "Verifying databases exist..."
          echo "\l" | psql | grep -E "(${join("|", var.database_names)})" || echo "Could not verify databases"

          echo "Database initialization task completed"
        EOT
      ]

      logConfiguration = {
        logDriver = "awslogs"
        options = {
          awslogs-group         = aws_cloudwatch_log_group.db_init.name
          awslogs-region        = data.aws_region.current.name
          awslogs-stream-prefix = "ecs"
        }
      }
    }
  ])

  tags = merge(var.tags, {
    Name = "${var.name_prefix}-db-init-task"
  })
}

# CloudWatch Log Group for database initialization
resource "aws_cloudwatch_log_group" "db_init" {
  name              = "/ecs/${var.name_prefix}-db-init"
  retention_in_days = var.log_retention_days

  tags = merge(var.tags, {
    Name = "${var.name_prefix}-db-init-logs"
  })
}

# IAM role for task execution (pulling images, logs)
resource "aws_iam_role" "db_init_execution" {
  name = "${var.name_prefix}-db-init-execution-role"

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

resource "aws_iam_role_policy_attachment" "db_init_execution" {
  role       = aws_iam_role.db_init_execution.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"
}

# Policy for Secrets Manager access
resource "aws_iam_role_policy" "db_init_secrets" {
  name = "${var.name_prefix}-db-init-secrets-policy"
  role = aws_iam_role.db_init_execution.id

  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Effect = "Allow"
        Action = [
          "secretsmanager:GetSecretValue"
        ]
        Resource = [var.rds_secret_arn]
      }
    ]
  })
}

# IAM role for task runtime
resource "aws_iam_role" "db_init_task" {
  name = "${var.name_prefix}-db-init-task-role"

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