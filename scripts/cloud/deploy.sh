#!/bin/bash

set -euo pipefail

WORKSPACE="${1:-}"
ACTION="${2:-plan}"

if [[ ! "$WORKSPACE" =~ ^(dev|prod)$ ]]; then
    echo "Usage: $0 <dev|prod> [plan|apply|destroy]"
    exit 1
fi

if [[ ! "$ACTION" =~ ^(plan|apply|destroy)$ ]]; then
    echo "Usage: $0 <dev|prod> [plan|apply|destroy]"
    exit 1
fi

if ! command -v terraform &> /dev/null; then
    echo "Terraform not found"
    exit 1
fi

if ! aws sts get-caller-identity &> /dev/null; then
    echo "AWS credentials not configured"
    exit 1
fi

BUCKET_NAME="basilica-terraform-state"
TABLE_NAME="basilica-terraform-locks"

if ! aws s3 ls "s3://$BUCKET_NAME" &> /dev/null; then
    echo "S3 bucket $BUCKET_NAME not found. Run: ./setup-backend.sh"
    exit 1
fi

if ! aws dynamodb describe-table --table-name "$TABLE_NAME" &> /dev/null; then
    echo "DynamoDB table $TABLE_NAME not found. Run: ./setup-backend.sh"
    exit 1
fi

cd "$(dirname "$0")"

terraform init

# Create workspace if it doesn't exist
if ! terraform workspace list | grep -q "^  $WORKSPACE$"; then
    terraform workspace new "$WORKSPACE"
fi

# Select workspace
terraform workspace select "$WORKSPACE"

case "$ACTION" in
    plan)
        terraform plan
        ;;
    apply)
        terraform apply
        ;;
    destroy)
        terraform destroy
        ;;
esac