#!/bin/bash

set -euo pipefail

if ! command -v aws &> /dev/null; then
    echo "AWS CLI not found"
    exit 1
fi

if ! aws sts get-caller-identity &> /dev/null; then
    echo "AWS credentials not configured"
    exit 1
fi

AWS_REGION=$(aws configure get region || echo "us-east-2")
BUCKET_NAME="basilica-terraform-state"
TABLE_NAME="basilica-terraform-locks"
KMS_KEY_ALIAS="alias/terraform-bucket-key"

# Create KMS key for S3 bucket encryption if it doesn't exist
if ! aws kms describe-key --key-id "$KMS_KEY_ALIAS" &> /dev/null; then
    echo "Creating KMS key for Terraform state encryption..."
    KMS_KEY_ID=$(aws kms create-key \
        --description "Terraform state bucket encryption key" \
        --key-usage ENCRYPT_DECRYPT \
        --key-spec SYMMETRIC_DEFAULT \
        --query 'KeyMetadata.KeyId' \
        --output text)

    aws kms create-alias \
        --alias-name "$KMS_KEY_ALIAS" \
        --target-key-id "$KMS_KEY_ID"

    echo "Created KMS key: $KMS_KEY_ID"
fi

if ! aws s3 ls "s3://$BUCKET_NAME" &> /dev/null; then
    if [[ "$AWS_REGION" == "us-east-2" ]]; then
        aws s3 mb "s3://$BUCKET_NAME"
    else
        aws s3 mb "s3://$BUCKET_NAME" --region "$AWS_REGION"
    fi

    aws s3api put-bucket-versioning \
        --bucket "$BUCKET_NAME" \
        --versioning-configuration Status=Enabled

    aws s3api put-bucket-encryption \
        --bucket "$BUCKET_NAME" \
        --server-side-encryption-configuration "{\"Rules\":[{\"ApplyServerSideEncryptionByDefault\":{\"SSEAlgorithm\":\"aws:kms\",\"KMSMasterKeyID\":\"$KMS_KEY_ALIAS\"}}]}"

    aws s3api put-public-access-block \
        --bucket "$BUCKET_NAME" \
        --public-access-block-configuration BlockPublicAcls=true,IgnorePublicAcls=true,BlockPublicPolicy=true,RestrictPublicBuckets=true
fi

if ! aws dynamodb describe-table --table-name "$TABLE_NAME" &> /dev/null; then
    aws dynamodb create-table \
        --table-name "$TABLE_NAME" \
        --attribute-definitions AttributeName=LockID,AttributeType=S \
        --key-schema AttributeName=LockID,KeyType=HASH \
        --provisioned-throughput ReadCapacityUnits=1,WriteCapacityUnits=1

    aws dynamodb wait table-exists --table-name "$TABLE_NAME"
fi

echo "Backend ready: $BUCKET_NAME, $TABLE_NAME"
echo "KMS key: $KMS_KEY_ALIAS"
echo ""
echo "Usage:"
echo "  terraform workspace new dev"
echo "  terraform workspace new staging"
echo "  terraform workspace new prod"
echo ""
echo "Current workspace: \$(terraform workspace show)"
