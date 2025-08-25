variable "name_prefix" {
  description = "Name prefix for all resources"
  type        = string
  validation {
    condition     = length(var.name_prefix) > 0
    error_message = "Name prefix cannot be empty."
  }
}

variable "db_endpoint" {
  description = "RDS database endpoint"
  type        = string
  validation {
    condition     = length(var.db_endpoint) > 0
    error_message = "Database endpoint cannot be empty."
  }
}

variable "db_username" {
  description = "RDS database username"
  type        = string
  validation {
    condition     = length(var.db_username) > 0
    error_message = "Database username cannot be empty."
  }
}

variable "rds_secret_arn" {
  description = "ARN of the RDS secret containing database credentials"
  type        = string
  validation {
    condition     = can(regex("^arn:aws:secretsmanager:", var.rds_secret_arn))
    error_message = "RDS secret ARN must be a valid AWS Secrets Manager ARN."
  }
}

variable "database_names" {
  description = "List of database names to create"
  type        = list(string)
  default     = ["basilica_billing", "basilica_payments"]
  validation {
    condition     = length(var.database_names) > 0
    error_message = "At least one database name must be provided."
  }
}

variable "cpu" {
  description = "CPU units for the database initialization task"
  type        = number
  default     = 256
  validation {
    condition     = contains([256, 512, 1024, 2048, 4096], var.cpu)
    error_message = "CPU must be one of: 256, 512, 1024, 2048, 4096."
  }
}

variable "memory" {
  description = "Memory in MB for the database initialization task"
  type        = number
  default     = 512
  validation {
    condition     = var.memory >= 512 && var.memory <= 30720 && var.memory % 256 == 0
    error_message = "Memory must be between 512 and 30720 MB and be a multiple of 256."
  }
}

variable "postgres_image" {
  description = "PostgreSQL Docker image to use for database initialization"
  type        = string
  default     = "postgres:15-alpine"
  validation {
    condition     = can(regex("^postgres:", var.postgres_image))
    error_message = "Postgres image must be a valid PostgreSQL Docker image."
  }
}

variable "log_retention_days" {
  description = "Number of days to retain CloudWatch logs"
  type        = number
  default     = 7
  validation {
    condition     = contains([1, 3, 5, 7, 14, 30, 60, 90, 120, 150, 180, 365, 400, 545, 731, 1827, 3653], var.log_retention_days)
    error_message = "Log retention days must be one of the allowed CloudWatch values."
  }
}

variable "aws_region" {
  description = "AWS region for resources"
  type        = string
  validation {
    condition     = length(var.aws_region) > 0
    error_message = "AWS region cannot be empty."
  }
}

variable "tags" {
  description = "Common tags to apply to all resources"
  type        = map(string)
  default     = {}
}