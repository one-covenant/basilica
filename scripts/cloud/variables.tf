variable "billing_image" {
  type = string
}

variable "payments_image" {
  type = string
}

variable "aws_region" {
  type    = string
  default = "us-east-2"
}

variable "project_name" {
  type    = string
  default = "basilica"
}

variable "certificate_arn" {
  type    = string
  default = null
}

variable "basilica_api_image" {
  type = string
}

variable "basilica_api_validator_hotkey" {
  type = string
}

variable "basilica_api_network" {
  type    = string
  default = "finney"
}

variable "basilica_api_netuid" {
  type    = number
  default = 39
}

variable "basilica_auth0_domain" {
  type    = string
  default = "your-auth0-domain"
}

variable "basilica_auth0_client_id" {
  type    = string
  default = "your-auth0-client-id"
}

variable "basilica_auth0_audience" {
  type    = string
  default = "your-auth0-audience"
}

variable "basilica_auth0_issuer" {
  type    = string
  default = "your-auth0-issuer"
}
