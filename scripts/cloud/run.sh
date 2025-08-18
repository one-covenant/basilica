#!/bin/bash

set -euo pipefail

PROFILE="${1:-}"
WORKSPACE="${2:-}"
ACTION="${3:-plan}"

if [[ -z "$PROFILE" ]]; then
    echo "Usage: $0 <profile> <workspace> [action]"
    exit 1
fi

if [[ ! "$WORKSPACE" =~ ^(dev|prod)$ ]]; then
    echo "Workspace must be 'dev' or 'prod'"
    exit 1
fi

if [[ ! "$ACTION" =~ ^(plan|apply|apply-f|destroy|force-unlock|tasks)$ ]]; then
    echo "Action must be 'plan', 'apply', 'apply-f', 'destroy', 'force-unlock', or 'tasks'"
    exit 1
fi

if ! command -v terraform &> /dev/null; then
    echo "Terraform not found"
    exit 1
fi

export AWS_PROFILE="$PROFILE"

if ! aws sts get-caller-identity --profile "$PROFILE" &> /dev/null; then
    echo "AWS credentials not configured for profile '$PROFILE'"
    exit 1
fi

BUCKET_NAME="basilica-terraform-state"
TABLE_NAME="basilica-terraform-locks"

if ! aws s3 ls "s3://$BUCKET_NAME" --profile "$PROFILE" &> /dev/null; then
    echo "S3 bucket $BUCKET_NAME not found"
    exit 1
fi

if ! aws dynamodb describe-table --table-name "$TABLE_NAME" --profile "$PROFILE" &> /dev/null; then
    echo "DynamoDB table $TABLE_NAME not found"
    exit 1
fi

cd "$(dirname "$0")"

if [[ "$ACTION" != "force-unlock" ]]; then
    terraform init
    terraform workspace select "$WORKSPACE" || terraform workspace new "$WORKSPACE"
fi

case "$ACTION" in
    plan)
        terraform plan
        ;;
    apply)
        if [[ "$WORKSPACE" == "prod" ]]; then
            echo "WARNING: Applying to PRODUCTION"
            read -p "Continue? (yes/no): " confirm
            if [[ "$confirm" != "yes" ]]; then
                exit 0
            fi
        fi
        terraform apply
        ;;
    apply-f)
        terraform apply -auto-approve
        ;;
    destroy)
        echo "WARNING: This will DESTROY infrastructure in '$WORKSPACE'"
        read -p "Type 'destroy-$WORKSPACE' to confirm: " confirm
        if [[ "$confirm" != "destroy-$WORKSPACE" ]]; then
            exit 0
        fi
        terraform destroy
        ;;
    force-unlock)
        echo "Checking for terraform state locks..."
        
        # Initialize terraform to ensure proper backend config
        terraform init -reconfigure >/dev/null 2>&1 || true
        terraform workspace select "$WORKSPACE" >/dev/null 2>&1 || terraform workspace new "$WORKSPACE" >/dev/null 2>&1 || true
        
        # Try to get lock info from DynamoDB
        LOCKS=$(aws dynamodb scan --table-name "$TABLE_NAME" --profile "$PROFILE" --query 'Items[].LockID.S' --output text 2>/dev/null || echo "")
        
        if [[ -n "$LOCKS" ]]; then
            echo "Found locks: $LOCKS"
            for LOCK_ID in $LOCKS; do
                echo "Unlocking: $LOCK_ID"
                # Use || true to continue even if unlock fails
                terraform force-unlock -force "$LOCK_ID" 2>/dev/null || {
                    echo "Failed to unlock $LOCK_ID (may not be current lock or already unlocked)"
                    continue
                }
                echo "Successfully unlocked: $LOCK_ID"
            done
        else
            echo "No locks found in DynamoDB table"
        fi
        
        # Also try to unlock any current state lock by checking terraform state
        echo "Checking for current terraform state lock..."
        CURRENT_LOCK=$(terraform plan -detailed-exitcode 2>&1 | grep -oP 'ID:\s+\K[a-f0-9-]+' | head -1 || echo "")
        if [[ -n "$CURRENT_LOCK" ]]; then
            echo "Found current lock: $CURRENT_LOCK"
            terraform force-unlock -force "$CURRENT_LOCK" 2>/dev/null || {
                echo "Current lock $CURRENT_LOCK could not be unlocked (may be stale)"
            }
        fi
        
        # Clean up any stale lock entries in DynamoDB
        echo "Cleaning up stale DynamoDB lock entries..."
        REMAINING_LOCKS=$(aws dynamodb scan --table-name "$TABLE_NAME" --profile "$PROFILE" --query 'Items[].LockID.S' --output text 2>/dev/null || echo "")
        if [[ -n "$REMAINING_LOCKS" ]]; then
            for LOCK_ID in $REMAINING_LOCKS; do
                # Try to delete stale entries
                aws dynamodb delete-item --table-name "$TABLE_NAME" --key "{\"LockID\":{\"S\":\"$LOCK_ID\"}}" --profile "$PROFILE" >/dev/null 2>&1 || true
            done
            echo "Cleaned up stale lock entries"
        fi
        
        echo "Force unlock completed. Checking final state..."
        # Final verification
        if terraform plan -detailed-exitcode >/dev/null 2>&1; then
            echo "✓ No state locks detected"
        else
            echo "⚠ State may still be locked or there are infrastructure changes pending"
        fi
        ;;
    tasks)
        echo "Running ECS tasks for workspace: $WORKSPACE"
        
        # Initialize terraform to get outputs
        terraform init >/dev/null 2>&1
        terraform workspace select "$WORKSPACE" >/dev/null 2>&1
        
        # Get infrastructure details from terraform outputs
        CLUSTER_NAME=$(terraform output -raw cluster_name 2>/dev/null || echo "")
        DB_INIT_TASK_DEF=$(terraform output -raw db_init_task_definition_arn 2>/dev/null || echo "")
        PRIVATE_SUBNETS=$(terraform output -json private_subnet_ids 2>/dev/null | jq -r '.[]' | tr '\n' ',' | sed 's/,$//') 
        ECS_SECURITY_GROUP=$(terraform output -raw ecs_tasks_security_group_id 2>/dev/null || echo "")
        
        if [[ -z "$CLUSTER_NAME" || -z "$DB_INIT_TASK_DEF" || -z "$PRIVATE_SUBNETS" || -z "$ECS_SECURITY_GROUP" ]]; then
            echo "Error: Could not get required infrastructure details from terraform outputs"
            echo "Make sure terraform has been applied successfully"
            exit 1
        fi
        
        echo "Infrastructure details:"
        echo "  Cluster: $CLUSTER_NAME"
        echo "  DB Init Task: $DB_INIT_TASK_DEF"
        echo "  Subnets: $PRIVATE_SUBNETS"
        echo "  Security Group: $ECS_SECURITY_GROUP"
        echo
        
        echo "Running database initialization task..."
        TASK_ARN=$(aws ecs run-task \
            --cluster "$CLUSTER_NAME" \
            --task-definition "$DB_INIT_TASK_DEF" \
            --launch-type FARGATE \
            --network-configuration "awsvpcConfiguration={subnets=[$PRIVATE_SUBNETS],securityGroups=[$ECS_SECURITY_GROUP],assignPublicIp=DISABLED}" \
            --profile "$PROFILE" \
            --query 'tasks[0].taskArn' \
            --output text)
        
        if [[ "$TASK_ARN" == "None" || -z "$TASK_ARN" ]]; then
            echo "Failed to start database initialization task"
            exit 1
        fi
        
        echo "Task started: $TASK_ARN"
        echo "Monitoring task status..."
        
        # Monitor task until completion
        while true; do
            TASK_STATUS=$(aws ecs describe-tasks \
                --cluster "$CLUSTER_NAME" \
                --tasks "$TASK_ARN" \
                --profile "$PROFILE" \
                --query 'tasks[0].lastStatus' \
                --output text)
            
            echo "Task status: $TASK_STATUS"
            
            if [[ "$TASK_STATUS" == "STOPPED" ]]; then
                # Get exit code
                EXIT_CODE=$(aws ecs describe-tasks \
                    --cluster "$CLUSTER_NAME" \
                    --tasks "$TASK_ARN" \
                    --profile "$PROFILE" \
                    --query 'tasks[0].containers[0].exitCode' \
                    --output text)
                
                if [[ "$EXIT_CODE" == "0" ]]; then
                    echo "✓ Database initialization completed successfully"
                else
                    echo "✗ Database initialization failed with exit code: $EXIT_CODE"
                fi
                
                # Show logs
                echo
                echo "Task logs:"
                LOG_GROUP="/ecs/basilica-$WORKSPACE-v3-db-init"
                LOG_STREAM=$(aws logs describe-log-streams \
                    --log-group-name "$LOG_GROUP" \
                    --order-by LastEventTime \
                    --descending \
                    --max-items 1 \
                    --profile "$PROFILE" \
                    --query 'logStreams[0].logStreamName' \
                    --output text 2>/dev/null || echo "")
                
                if [[ -n "$LOG_STREAM" && "$LOG_STREAM" != "None" ]]; then
                    aws logs get-log-events \
                        --log-group-name "$LOG_GROUP" \
                        --log-stream-name "$LOG_STREAM" \
                        --profile "$PROFILE" \
                        --query 'events[].message' \
                        --output text
                else
                    echo "No logs found (logs may not be available yet)"
                fi
                
                break
            fi
            
            sleep 10
        done
        ;;
esac
