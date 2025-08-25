variable "name_prefix" {
  description = "Name prefix for resources"
  type        = string
}

variable "vpc_id" {
  description = "VPC ID"
  type        = string
}

variable "subnet_ids" {
  description = "List of subnet IDs for DB subnet group"
  type        = list(string)
}

variable "security_group_ids" {
  description = "List of security group IDs"
  type        = list(string)
}

variable "min_acu_capacity" {
  description = "Minimum Aurora Capacity Units (ACUs)"
  type        = number
  default     = 0.5
}

variable "max_acu_capacity" {
  description = "Maximum Aurora Capacity Units (ACUs)"
  type        = number
  default     = 16
}

variable "cluster_instance_count" {
  description = "Number of instances in the Aurora cluster"
  type        = number
  default     = 1
}

variable "db_username" {
  description = "Database username"
  type        = string
}

variable "db_password" {
  description = "Database password"
  type        = string
  sensitive   = true
}

variable "db_port" {
  description = "Database port"
  type        = number
  default     = 5432
}

variable "engine_version" {
  description = "Aurora PostgreSQL engine version"
  type        = string
  default     = "15.12"
}

variable "parameter_group_family" {
  description = "Aurora PostgreSQL parameter group family"
  type        = string
  default     = "aurora-postgresql15"
}

variable "backup_retention_period" {
  description = "Backup retention period in days"
  type        = number
  default     = 7
}

variable "backup_window" {
  description = "Backup window"
  type        = string
  default     = "03:00-04:00"
}

variable "maintenance_window" {
  description = "Maintenance window"
  type        = string
  default     = "sun:04:00-sun:05:00"
}

variable "enable_deletion_protection" {
  description = "Enable deletion protection"
  type        = bool
  default     = true
}

variable "enable_monitoring" {
  description = "Enable enhanced monitoring"
  type        = bool
  default     = true
}

variable "monitoring_interval" {
  description = "Enhanced monitoring interval in seconds"
  type        = number
  default     = 60
}

variable "enable_performance_insights" {
  description = "Enable Performance Insights"
  type        = bool
  default     = true
}

variable "performance_insights_retention_period" {
  description = "Performance Insights retention period in days"
  type        = number
  default     = 7
}

variable "tags" {
  description = "Tags to apply to resources"
  type        = map(string)
  default     = {}
}
