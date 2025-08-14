# Basilica AWS Infrastructure

Terraform configuration for deploying billing and payments services on AWS using workspaces.

## Services

- **Billing**: Port 8080 (HTTP), 50051 (gRPC) - Routes: `/billing/*`
- **Payments**: Port 8082 (HTTP), 50061 (gRPC) - Routes: `/payments/*`

## Prerequisites

1. AWS CLI configured
2. Terraform >= 1.5
3. Container images pushed to GHCR

## Setup

```bash
# Create unified backend resources
./setup-backend.sh

# Copy example variables and update with your container images
cp terraform.tfvars.example terraform.tfvars
# Edit terraform.tfvars with your actual container image URLs

# Initialize and create workspaces
terraform init
terraform workspace new dev
terraform workspace new prod

# Deploy to development
./deploy.sh dev plan
./deploy.sh dev apply

# Deploy to production
./deploy.sh prod plan
./deploy.sh prod apply
```

## Workspaces

- **dev** - Development environment (10.0.0.0/16, minimal resources)
- **prod** - Production environment (10.1.0.0/16, high availability)

Environment-specific settings are automatically configured based on the workspace.

## Architecture

- ECS Fargate services with auto-scaling
- Shared RDS PostgreSQL (separate schemas)
- Application Load Balancer with path routing
- Single NAT Gateway
- Service discovery for inter-service communication
- AWS Secrets Manager for encryption keys and database credentials

## Workspace Management

```bash
# List workspaces
terraform workspace list

# Switch workspace
terraform workspace select dev

# Show current workspace
terraform workspace show
```