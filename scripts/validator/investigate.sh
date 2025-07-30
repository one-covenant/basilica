#!/bin/bash

DB_PATH=""
SSH_CONN=""
MINER_UID=""
EXECUTOR_ID=""
GPU_PROFILE=""
SHOW_GPU_UUIDS=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --db)
            DB_PATH="$2"
            shift 2
            ;;
        -c)
            SSH_CONN="$2"
            shift 2
            ;;
        --miner-uid)
            MINER_UID="$2"
            shift 2
            ;;
        --executor-id)
            EXECUTOR_ID="$2"
            shift 2
            ;;
        --gpu-profile)
            GPU_PROFILE="$2"
            shift 2
            ;;
        --gpu-uuids)
            SHOW_GPU_UUIDS=true
            shift
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

if [ -z "$DB_PATH" ]; then
    echo "Usage: $0 --db <path> [-c <ssh_connection>] [--miner-uid <uid>] [--executor-id <id>] [--gpu-profile <h100|h200>] [--gpu-uuids]"
    exit 1
fi

run_query() {
    local query="$1"
    if [ -n "$SSH_CONN" ]; then
        ssh "$SSH_CONN" "sqlite3 -header -column '$DB_PATH' \"$query\""
    else
        sqlite3 -header -column "$DB_PATH" "$query"
    fi
}

if [ -n "$MINER_UID" ]; then
    echo "=== MINER UID $MINER_UID BREAKDOWN ==="
    echo

    echo "Miner Info:"
    run_query "SELECT hotkey, endpoint FROM miners WHERE id = 'miner_$MINER_UID';"
    echo

    echo "Profile:"
    run_query "SELECT miner_uid, primary_gpu_model, total_score, last_successful_validation FROM miner_gpu_profiles WHERE miner_uid = $MINER_UID;"
    echo

    echo "Executors:"
    run_query "SELECT
        executor_id,
        grpc_address,
        gpu_count
    FROM miner_executors WHERE miner_id = 'miner_$MINER_UID';"
    echo

    echo "GPU Assignments:"
    run_query "SELECT executor_id, COUNT(DISTINCT gpu_uuid) as verified_gpus, gpu_name FROM gpu_uuid_assignments WHERE miner_id = 'miner_$MINER_UID' GROUP BY executor_id, gpu_name;"
    echo

    if [ "$SHOW_GPU_UUIDS" = true ]; then
        echo "GPU UUIDs:"
        run_query "SELECT gpu_uuid, gpu_name, executor_id FROM gpu_uuid_assignments WHERE miner_id = 'miner_$MINER_UID' ORDER BY executor_id;"
        echo
    fi

    echo "Recent Weights:"
    run_query "SELECT weight_set_block, gpu_category, allocated_weight, timestamp FROM weight_allocation_history WHERE miner_uid = $MINER_UID ORDER BY timestamp DESC LIMIT 5;"
    echo

    echo "Validation Statistics (since last epoch):"
    if [ -n "$SSH_CONN" ]; then
        LAST_EPOCH=$(ssh "$SSH_CONN" "sqlite3 '$DB_PATH' \"SELECT MAX(timestamp) FROM weight_allocation_history WHERE weight_set_block < (SELECT MAX(weight_set_block) FROM weight_allocation_history);\"")
    else
        LAST_EPOCH=$(sqlite3 "$DB_PATH" "SELECT MAX(timestamp) FROM weight_allocation_history WHERE weight_set_block < (SELECT MAX(weight_set_block) FROM weight_allocation_history);")
    fi
    run_query "SELECT
        COUNT(*) as total_validations,
        SUM(CASE WHEN success = 1 THEN 1 ELSE 0 END) as successful,
        SUM(CASE WHEN success = 0 THEN 1 ELSE 0 END) as failed
    FROM verification_logs
    WHERE executor_id IN (SELECT executor_id FROM miner_executors WHERE miner_id = 'miner_$MINER_UID')
    AND timestamp > '$LAST_EPOCH';"
    echo

    echo "Successful Validations (last 5):"
    run_query "SELECT executor_id, timestamp, score FROM verification_logs WHERE executor_id IN (SELECT executor_id FROM miner_executors WHERE miner_id = 'miner_$MINER_UID') AND success = 1 ORDER BY timestamp DESC LIMIT 5;"
    echo
    
    echo "Failed Validations (last 5):"
    run_query "SELECT executor_id, timestamp, error_message FROM verification_logs WHERE executor_id IN (SELECT executor_id FROM miner_executors WHERE miner_id = 'miner_$MINER_UID') AND success = 0 ORDER BY timestamp DESC LIMIT 5;"

elif [ -n "$EXECUTOR_ID" ]; then
    echo "=== EXECUTOR $EXECUTOR_ID BREAKDOWN ==="
    echo

    echo "Executor Info:"
    run_query "SELECT
        me.miner_id,
        me.executor_id,
        me.grpc_address,
        me.gpu_count
    FROM miner_executors me WHERE me.executor_id = '$EXECUTOR_ID';"
    echo

    echo "Miner Hotkey:"
    run_query "SELECT m.hotkey
    FROM miners m
    INNER JOIN miner_executors me ON m.id = me.miner_id
    WHERE me.executor_id = '$EXECUTOR_ID';"
    echo

    echo "GPU Assignments:"
    run_query "SELECT COUNT(DISTINCT gpu_uuid) as verified_gpus, gpu_name FROM gpu_uuid_assignments WHERE executor_id = '$EXECUTOR_ID' GROUP BY gpu_name;"
    echo

    if [ "$SHOW_GPU_UUIDS" = true ]; then
        echo "GPU UUIDs:"
        run_query "SELECT gpu_uuid, gpu_name FROM gpu_uuid_assignments WHERE executor_id = '$EXECUTOR_ID';"
        echo
    fi

    echo "Recent Verifications (last 10):"
    run_query "SELECT timestamp, success, CASE WHEN success = 1 THEN score ELSE error_message END as result FROM verification_logs WHERE executor_id = '$EXECUTOR_ID' ORDER BY timestamp DESC LIMIT 10;"
    echo

    echo "Validation Statistics (since last epoch):"
    if [ -n "$SSH_CONN" ]; then
        LAST_EPOCH=$(ssh "$SSH_CONN" "sqlite3 '$DB_PATH' \"SELECT MAX(timestamp) FROM weight_allocation_history WHERE weight_set_block < (SELECT MAX(weight_set_block) FROM weight_allocation_history);\"")
    else
        LAST_EPOCH=$(sqlite3 "$DB_PATH" "SELECT MAX(timestamp) FROM weight_allocation_history WHERE weight_set_block < (SELECT MAX(weight_set_block) FROM weight_allocation_history);")
    fi
    run_query "SELECT
        COUNT(*) as total_validations,
        SUM(CASE WHEN success = 1 THEN 1 ELSE 0 END) as successful,
        SUM(CASE WHEN success = 0 THEN 1 ELSE 0 END) as failed
    FROM verification_logs
    WHERE executor_id = '$EXECUTOR_ID'
    AND timestamp > '$LAST_EPOCH';"

elif [ -n "$GPU_PROFILE" ]; then
    GPU_MODEL=$(echo "$GPU_PROFILE" | tr '[:lower:]' '[:upper:]')
    echo "=== GPU PROFILE $GPU_MODEL BREAKDOWN ==="
    echo

    echo "Miners with $GPU_MODEL:"
    run_query "SELECT miner_uid, total_score, last_successful_validation FROM miner_gpu_profiles WHERE primary_gpu_model = '$GPU_MODEL' AND total_score >= 0.1 ORDER BY total_score DESC;"
    echo

    echo "Total $GPU_MODEL GPUs:"
    run_query "SELECT COUNT(DISTINCT gpu_uuid) as total_gpus, COUNT(DISTINCT miner_id) as total_miners FROM gpu_uuid_assignments WHERE gpu_name LIKE '%$GPU_MODEL%';"
    echo

    echo "Recent Weight Allocations for $GPU_MODEL:"
    run_query "SELECT miner_uid, allocated_weight, weight_set_block FROM weight_allocation_history WHERE gpu_category = '$GPU_MODEL' AND weight_set_block = (SELECT MAX(weight_set_block) FROM weight_allocation_history) ORDER BY allocated_weight DESC;"

else
    echo "=== OVERALL DATABASE REPORT ==="
    echo

    echo "Summary:"
    run_query "SELECT
        (SELECT COUNT(DISTINCT miner_uid) FROM miner_gpu_profiles) as total_miners,
        (SELECT COUNT(DISTINCT miner_id) FROM miner_executors) as miners_with_executors,
        (SELECT COUNT(*) FROM miner_executors) as total_executors,
        (SELECT COUNT(DISTINCT gpu_uuid) FROM gpu_uuid_assignments) as verified_gpus;"
    echo

    echo "GPU Distribution:"
    run_query "SELECT gpu_name, COUNT(DISTINCT gpu_uuid) as gpu_count, COUNT(DISTINCT miner_id) as miner_count FROM gpu_uuid_assignments GROUP BY gpu_name ORDER BY gpu_count DESC;"
    echo

    echo "Miner Profiles by GPU Model:"
    run_query "SELECT primary_gpu_model, COUNT(*) as miner_count, AVG(total_score) as avg_score FROM miner_gpu_profiles WHERE total_score >= 0.1 GROUP BY primary_gpu_model ORDER BY miner_count DESC;"
    echo

    echo "Latest Weight Distribution:"
    run_query "SELECT gpu_category, COUNT(*) as miners_rewarded, SUM(allocated_weight) as total_weight FROM weight_allocation_history WHERE weight_set_block = (SELECT MAX(weight_set_block) FROM weight_allocation_history) GROUP BY gpu_category ORDER BY total_weight DESC;"
    echo

    echo "Top 10 Miners by Score:"
    run_query "SELECT p.miner_uid,
        SUBSTR(m.hotkey, 1, 10) || '...' as hotkey_prefix,
        p.primary_gpu_model,
        p.total_score,
        (SELECT COUNT(DISTINCT gpu_uuid) FROM gpu_uuid_assignments WHERE miner_id = 'miner_' || p.miner_uid) as verified_gpus,
        (SELECT COUNT(*) FROM miner_executors WHERE miner_id = 'miner_' || p.miner_uid) as executor_count
        FROM miner_gpu_profiles p
        LEFT JOIN miners m ON m.id = 'miner_' || p.miner_uid
        WHERE p.total_score >= 0.1 ORDER BY p.total_score DESC LIMIT 10;"
    echo

    echo "Miner gRPC Endpoints (top 10):"
    run_query "SELECT DISTINCT m.miner_id, m.grpc_address FROM miner_executors m
        INNER JOIN miner_gpu_profiles p ON p.miner_uid = CAST(SUBSTR(m.miner_id, 7) AS INTEGER)
        WHERE p.total_score >= 0.1
        ORDER BY p.total_score DESC LIMIT 10;"
fi
