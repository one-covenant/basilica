variable "name_prefix" {
  description = "Name prefix for all resources"
  type        = string
  validation {
    condition     = length(var.name_prefix) > 0
    error_message = "Name prefix cannot be empty."
  }
}

variable "task_name" {
  description = "Specific name for this task (e.g., 'db-init', 'migration', 'cleanup')"
  type        = string
  validation {
    condition     = length(var.task_name) > 0 && can(regex("^[a-z0-9-]+$", var.task_name))
    error_message = "Task name must be lowercase alphanumeric with hyphens only."
  }
}

variable "container_image" {
  description = "Docker image to use for the task"
  type        = string
  validation {
    condition     = length(var.container_image) > 0
    error_message = "Container image cannot be empty."
  }
}

variable "container_command" {
  description = "Command to run in the container (optional, uses image default if not provided)"
  type        = list(string)
  default     = []
}

variable "container_entrypoint" {
  description = "Entrypoint to override in the container (optional)"
  type        = list(string)
  default     = []
}

variable "working_directory" {
  description = "Working directory for the container (optional)"
  type        = string
  default     = ""
}

variable "environment_variables" {
  description = "Environment variables to set in the container"
  type        = map(string)
  default     = {}
}

variable "secrets" {
  description = "List of secrets from AWS Secrets Manager to inject as environment variables"
  type = list(object({
    name      = string
    valueFrom = string
  }))
  default = []
}

variable "cpu" {
  description = "CPU units for the task (256, 512, 1024, 2048, 4096)"
  type        = number
  default     = 256
  validation {
    condition     = contains([256, 512, 1024, 2048, 4096], var.cpu)
    error_message = "CPU must be one of: 256, 512, 1024, 2048, 4096."
  }
}

variable "memory" {
  description = "Memory in MB for the task"
  type        = number
  default     = 512
  validation {
    condition     = var.memory >= 512 && var.memory <= 30720 && var.memory % 256 == 0
    error_message = "Memory must be between 512 and 30720 MB and be a multiple of 256."
  }
}

variable "execution_role_policies" {
  description = "List of IAM policy ARNs to attach to the execution role (for pulling images, logging, secrets)"
  type        = list(string)
  default     = ["arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"]
}

variable "task_role_policies" {
  description = "List of IAM policy ARNs to attach to the task role (for runtime permissions)"
  type        = list(string)
  default     = []
}

variable "custom_execution_policy" {
  description = "Custom IAM policy document for execution role (JSON string)"
  type        = string
  default     = ""
}

variable "custom_task_policy" {
  description = "Custom IAM policy document for task role (JSON string)"
  type        = string
  default     = ""
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

variable "enable_logging" {
  description = "Enable CloudWatch logging for the task"
  type        = bool
  default     = true
}

variable "log_group_name" {
  description = "Custom CloudWatch log group name (optional, auto-generated if not provided)"
  type        = string
  default     = ""
}

variable "tags" {
  description = "Common tags to apply to all resources"
  type        = map(string)
  default     = {}
}

variable "network_mode" {
  description = "Network mode for the task definition"
  type        = string
  default     = "awsvpc"
  validation {
    condition     = contains(["awsvpc", "bridge", "host", "none"], var.network_mode)
    error_message = "Network mode must be one of: awsvpc, bridge, host, none."
  }
}

variable "requires_compatibilities" {
  description = "List of launch types the task definition is compatible with"
  type        = list(string)
  default     = ["FARGATE"]
  validation {
    condition     = length(var.requires_compatibilities) > 0
    error_message = "At least one compatibility mode must be specified."
  }
}

variable "volumes" {
  description = "List of volumes to mount in the task"
  type = list(object({
    name = string
    host_path = optional(string)
    efs_volume_configuration = optional(object({
      file_system_id     = string
      root_directory     = optional(string)
      transit_encryption = optional(string)
    }))
  }))
  default = []
}

variable "mount_points" {
  description = "List of mount points for volumes in the container"
  type = list(object({
    source_volume  = string
    container_path = string
    read_only      = optional(bool, false)
  }))
  default = []
}

variable "ulimits" {
  description = "List of ulimits to set in the container"
  type = list(object({
    name      = string
    soft_limit = number
    hard_limit = number
  }))
  default = []
}

variable "essential" {
  description = "Whether the container is essential"
  type        = bool
  default     = true
}

variable "auto_run" {
  description = "Whether to automatically run the task after creation"
  type        = bool
  default     = false
}

variable "cluster_id" {
  description = "ECS cluster ID to run the task in (required if auto_run is true)"
  type        = string
  default     = ""
}

variable "subnet_ids" {
  description = "List of subnet IDs for task networking (required if auto_run is true)"
  type        = list(string)
  default     = []
}

variable "security_group_id" {
  description = "Security group ID for task networking (required if auto_run is true)"
  type        = string
  default     = ""
}