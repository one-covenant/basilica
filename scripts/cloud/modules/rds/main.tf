
# DB Subnet Group for Aurora Cluster
resource "aws_db_subnet_group" "main" {
  name       = "${var.name_prefix}-aurora-subnet-group"
  subnet_ids = var.subnet_ids

  tags = merge(var.tags, {
    Name = "${var.name_prefix}-aurora-subnet-group"
  })
}

# DB Cluster Parameter Group for Aurora PostgreSQL
resource "aws_rds_cluster_parameter_group" "main" {
  family = var.parameter_group_family
  name   = "${var.name_prefix}-aurora-cluster-params"

  parameter {
    name         = "shared_preload_libraries"
    value        = "pg_stat_statements"
    apply_method = "immediate"
  }

  parameter {
    name  = "log_statement"
    value = "all"
  }

  parameter {
    name  = "log_min_duration_statement"
    value = "1000"
  }

  tags = merge(var.tags, {
    Name = "${var.name_prefix}-aurora-cluster-params"
  })
}

# Enhanced monitoring role
resource "aws_iam_role" "rds_enhanced_monitoring" {
  count = var.enable_monitoring ? 1 : 0
  name  = "${var.name_prefix}-aurora-monitoring-role"

  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Action = "sts:AssumeRole"
        Effect = "Allow"
        Principal = {
          Service = "monitoring.rds.amazonaws.com"
        }
      }
    ]
  })

  tags = var.tags
}

resource "aws_iam_role_policy_attachment" "rds_enhanced_monitoring" {
  count      = var.enable_monitoring ? 1 : 0
  role       = aws_iam_role.rds_enhanced_monitoring[0].name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonRDSEnhancedMonitoringRole"
}

# Aurora Serverless v2 Cluster
resource "aws_rds_cluster" "main" {
  cluster_identifier = "${var.name_prefix}-aurora-postgres"

  # Engine configuration
  engine         = "aurora-postgresql"
  engine_version = var.engine_version
  engine_mode    = "provisioned"

  # Database configuration
  database_name   = "basilica_v3"
  master_username = var.db_username
  master_password = var.db_password
  port            = var.db_port

  # Network configuration
  db_subnet_group_name   = aws_db_subnet_group.main.name
  vpc_security_group_ids = var.security_group_ids

  # Parameter group
  db_cluster_parameter_group_name = aws_rds_cluster_parameter_group.main.name

  # Backup configuration
  backup_retention_period = var.backup_retention_period
  preferred_backup_window = var.backup_window
  copy_tags_to_snapshot   = true

  # Maintenance configuration
  preferred_maintenance_window = var.maintenance_window

  # Serverless v2 scaling configuration
  serverlessv2_scaling_configuration {
    max_capacity = var.max_acu_capacity
    min_capacity = var.min_acu_capacity
  }

  # Security
  storage_encrypted   = true
  deletion_protection = var.enable_deletion_protection
  skip_final_snapshot = false
  final_snapshot_identifier = "${var.name_prefix}-aurora-final-snapshot-${formatdate("YYYY-MM-DD-hhmm", timestamp())}"
  
  # Data API
  enable_http_endpoint = true

  tags = merge(var.tags, {
    Name = "${var.name_prefix}-aurora-postgres"
  })

  lifecycle {
    ignore_changes = [
      master_password,
      final_snapshot_identifier
    ]
  }
}

# Aurora Serverless v2 Instance
resource "aws_rds_cluster_instance" "main" {
  count              = var.cluster_instance_count
  identifier         = "${var.name_prefix}-aurora-instance-${count.index + 1}"
  cluster_identifier = aws_rds_cluster.main.id
  instance_class     = "db.serverless"
  engine             = aws_rds_cluster.main.engine
  engine_version     = aws_rds_cluster.main.engine_version

  # Monitoring configuration
  monitoring_interval = var.enable_monitoring ? var.monitoring_interval : 0
  monitoring_role_arn = var.enable_monitoring ? aws_iam_role.rds_enhanced_monitoring[0].arn : null

  # Performance Insights
  performance_insights_enabled          = var.enable_performance_insights
  performance_insights_retention_period = var.enable_performance_insights ? var.performance_insights_retention_period : null

  tags = merge(var.tags, {
    Name = "${var.name_prefix}-aurora-instance-${count.index + 1}"
  })
}

# Secrets Manager secret for database credentials
resource "aws_secretsmanager_secret" "db_credentials" {
  name                    = "${var.name_prefix}-aurora-credentials"
  description             = "Aurora database credentials for ${var.name_prefix}"
  recovery_window_in_days = 7

  tags = merge(var.tags, {
    Name = "${var.name_prefix}-aurora-credentials"
  })
}

resource "aws_secretsmanager_secret_version" "db_credentials" {
  secret_id = aws_secretsmanager_secret.db_credentials.id
  secret_string = jsonencode({
    username = var.db_username
    password = var.db_password
    engine   = "aurora-postgresql"
    host     = aws_rds_cluster.main.endpoint
    reader_host = aws_rds_cluster.main.reader_endpoint
    port     = aws_rds_cluster.main.port
    dbname   = aws_rds_cluster.main.database_name
  })
}

# CloudWatch Log Groups for Aurora
resource "aws_cloudwatch_log_group" "aurora_postgres" {
  name              = "/aws/rds/cluster/${aws_rds_cluster.main.cluster_identifier}/postgresql"
  retention_in_days = 7

  tags = merge(var.tags, {
    Name = "${var.name_prefix}-aurora-postgres-logs"
  })
}
