variable "name_prefix" {
  type = string
}

variable "vpc_id" {
  type = string
}

variable "subnet_ids" {
  type = list(string)
}

variable "security_group_ids" {
  type = list(string)
}

variable "certificate_arn" {
  type    = string
  default = null
}

variable "tags" {
  type    = map(string)
  default = {}
}

variable "additional_target_groups" {
  description = "Additional target groups to create"
  type = map(object({
    port             = number
    protocol         = string
    protocol_version = optional(string, "HTTP1")
    health_check = object({
      healthy_threshold   = number
      unhealthy_threshold = number
      timeout             = number
      interval            = number
      protocol            = string
      path                = string
      matcher             = string
    })
  }))
  default = {}
}

variable "additional_listener_rules" {
  description = "Additional listener rules to create"
  type = map(object({
    priority           = number
    target_group_key   = string
    listener_protocol  = string
    path_patterns      = list(string)
  }))
  default = {}
}

variable "create_billing_target_group" {
  description = "Whether to create billing target group"
  type        = bool
  default     = true
}

variable "create_payments_target_group" {
  description = "Whether to create payments target group"
  type        = bool
  default     = true
}