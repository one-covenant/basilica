variable "billing_image" {
  type = string
}

variable "payments_image" {
  type = string
}

variable "aws_region" {
  type    = string
  default = "us-east-1"
}

variable "project_name" {
  type    = string
  default = "basilica"
}

variable "certificate_arn" {
  type    = string
  default = null
}