variable "name_prefix" {
  description = "Name prefix for resources"
  type        = string
}

variable "service_name" {
  description = "Name of the service"
  type        = string
}

variable "cluster_id" {
  description = "ECS cluster ID"
  type        = string
}

variable "vpc_id" {
  description = "VPC ID"
  type        = string
}

variable "ecs_tasks_security_group_id" {
  description = "Security group ID for ECS tasks"
  type        = string
}

variable "subnet_ids" {
  description = "List of subnet IDs"
  type        = list(string)
}

variable "container_image" {
  description = "Container image URL"
  type        = string
}

variable "container_port" {
  description = "Container port for HTTP traffic"
  type        = number
}

variable "grpc_port" {
  description = "Container port for gRPC traffic"
  type        = number
}

variable "metrics_port" {
  description = "Container port for metrics"
  type        = number
}

variable "cpu" {
  description = "CPU units for the task"
  type        = number
  default     = 256
}

variable "memory" {
  description = "Memory in MiB for the task"
  type        = number
  default     = 512
}

variable "alb_target_group_arn" {
  description = "ALB target group ARN"
  type        = string
}

variable "alb_listener_arn" {
  description = "ALB listener ARN (optional, for future use)"
  type        = string
  default     = ""
}

variable "health_check_path" {
  description = "Health check path"
  type        = string
  default     = "/health"
}

variable "health_check_type" {
  description = "Type of health check: http or grpc"
  type        = string
  default     = "http"
  validation {
    condition     = contains(["http", "grpc"], var.health_check_type)
    error_message = "Health check type must be either 'http' or 'grpc'."
  }
}

variable "load_balancer_port" {
  description = "Port for load balancer target (defaults to container_port for HTTP, grpc_port for gRPC)"
  type        = number
  default     = null
}

variable "min_capacity" {
  description = "Minimum number of tasks"
  type        = number
  default     = 1
}

variable "max_capacity" {
  description = "Maximum number of tasks"
  type        = number
  default     = 10
}

variable "target_capacity" {
  description = "Desired number of tasks"
  type        = number
  default     = 2
}

variable "service_discovery_namespace_id" {
  description = "Service discovery namespace ID"
  type        = string
}

variable "environment_variables" {
  description = "Environment variables for the container"
  type        = map(string)
  default     = {}
}

variable "secrets" {
  description = "Secrets for the container"
  type = list(object({
    name      = string
    valueFrom = string
  }))
  default = []
}

variable "enable_logging" {
  description = "Enable CloudWatch logging"
  type        = bool
  default     = true
}

variable "log_retention_days" {
  description = "CloudWatch log retention in days"
  type        = number
  default     = 7
}

variable "enable_auto_scaling" {
  description = "Enable auto scaling"
  type        = bool
  default     = true
}

variable "scale_up_cpu_threshold" {
  description = "CPU threshold for scaling up"
  type        = number
  default     = 70
}

variable "scale_down_cpu_threshold" {
  description = "CPU threshold for scaling down"
  type        = number
  default     = 30
}

variable "deployment_configuration" {
  description = "Deployment configuration"
  type = object({
    maximum_percent         = number
    minimum_healthy_percent = number
  })
  default = {
    maximum_percent         = 200
    minimum_healthy_percent = 50
  }
}

variable "enable_execute_command" {
  description = "Enable ECS Exec for debugging"
  type        = bool
  default     = false
}

variable "tags" {
  description = "Tags to apply to resources"
  type        = map(string)
  default     = {}
}

variable "init_container" {
  description = "Configuration for init container (optional)"
  type = object({
    enabled = bool
    image   = optional(string, "postgres:15-alpine")
    command = optional(list(string), [])
    environment = optional(list(object({
      name  = string
      value = string
    })), [])
    secrets = optional(list(object({
      name      = string
      valueFrom = string
    })), [])
  })
  default = {
    enabled = false
  }
}