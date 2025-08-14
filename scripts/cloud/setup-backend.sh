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

AWS_REGION=$(aws configure get region || echo "us-east-1")
BUCKET_NAME="basilica-terraform-state"
TABLE_NAME="basilica-terraform-locks"

if ! aws s3 ls "s3://$BUCKET_NAME" &> /dev/null; then
    if [[ "$AWS_REGION" == "us-east-1" ]]; then
        aws s3 mb "s3://$BUCKET_NAME"
    else
        aws s3 mb "s3://$BUCKET_NAME" --region "$AWS_REGION"
    fi
    
    aws s3api put-bucket-versioning \
        --bucket "$BUCKET_NAME" \
        --versioning-configuration Status=Enabled
        
    aws s3api put-bucket-encryption \
        --bucket "$BUCKET_NAME" \
        --server-side-encryption-configuration '{"Rules":[{"ApplyServerSideEncryptionByDefault":{"SSEAlgorithm":"AES256"}}]}'
        
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
echo "Create workspaces with: terraform workspace new dev/prod"