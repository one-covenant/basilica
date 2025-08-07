#!/bin/bash

DB_PATH=""
SSH_CONN=""
MINER_UID=""
EXECUTOR_ID=""
EXECUTOR_ENDPOINT=""
GPU_UUID=""
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
        --executor-endpoint)
            EXECUTOR_ENDPOINT="$2"
            shift 2
            ;;
        --gpu-uuid)
            GPU_UUID="$2"
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
    echo "Usage: $0 --db <path> [-c <ssh_connection>] [--miner-uid <uid>] [--executor-id <id>] [--executor-endpoint <endpoint>] [--gpu-uuid <uuid>] [--gpu-profile <h100|h200>] [--gpu-uuids]"
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
    run_query "SELECT hotkey, endpoint, last_seen, registered_at FROM miners WHERE id = 'miner_$MINER_UID';"
    echo

    echo "Profile & Performance:"
    run_query "SELECT
        miner_uid,
        total_score,
        verification_count,
        gpu_counts_json,
        last_successful_validation,
        CASE
            WHEN total_score >= 0.9 THEN 'Excellent'
            WHEN total_score >= 0.7 THEN 'Good'
            WHEN total_score >= 0.5 THEN 'Fair'
            ELSE 'Poor'
        END as performance
    FROM miner_gpu_profiles WHERE miner_uid = $MINER_UID;"
    echo

    echo "Executors:"
    run_query "SELECT
        executor_id,
        grpc_address,
        gpu_count,
        status,
        last_health_check
    FROM miner_executors WHERE miner_id = 'miner_$MINER_UID';"
    echo

    echo "GPU Assignments:"
    run_query "SELECT executor_id, COUNT(DISTINCT gpu_uuid) as verified_gpus, gpu_name FROM gpu_uuid_assignments WHERE miner_id = 'miner_$MINER_UID' GROUP BY executor_id, gpu_name;"
    echo

    echo "=== FRAUD DETECTION ANALYSIS ==="
    echo

    echo "1. GPU UUID Conflicts (potential GPU hijacking attempt):"
    run_query "SELECT
        ga.gpu_uuid,
        ga.gpu_name,
        CASE
            WHEN other.miner_id IS NOT NULL THEN 'CONFLICT with ' || other.miner_id
            ELSE 'No conflicts'
        END as status
    FROM gpu_uuid_assignments ga
    LEFT JOIN gpu_uuid_assignments other ON
        ga.gpu_uuid = other.gpu_uuid
        AND other.miner_id != 'miner_$MINER_UID'
    WHERE ga.miner_id = 'miner_$MINER_UID'
    GROUP BY ga.gpu_uuid, ga.gpu_name, other.miner_id;"
    echo

    echo "2. GPU Count Consistency Check:"
    run_query "SELECT
        'Registered in executors' as source,
        SUM(gpu_count) as total_gpus
    FROM miner_executors
    WHERE miner_id = 'miner_$MINER_UID'
    UNION ALL
    SELECT
        'Verified in assignments (ALL)' as source,
        COUNT(DISTINCT gpu_uuid) as total_gpus
    FROM gpu_uuid_assignments
    WHERE miner_id = 'miner_$MINER_UID'
    UNION ALL
    SELECT
        'Verified with ACTIVE executors' as source,
        COUNT(DISTINCT ga.gpu_uuid) as total_gpus
    FROM gpu_uuid_assignments ga
    INNER JOIN miner_executors me ON ga.miner_id = me.miner_id AND ga.executor_id = me.executor_id
    WHERE ga.miner_id = 'miner_$MINER_UID'
    AND me.status IN ('online', 'verified')
    UNION ALL
    SELECT
        'Profile JSON (CACHED)' as source,
        CAST(
            (SELECT CASE
                WHEN gpu_counts_json IS NULL OR gpu_counts_json = '' THEN 0
                WHEN json_valid(gpu_counts_json) = 0 THEN 0
                ELSE (SELECT SUM(CAST(json_each.value AS INTEGER))
                      FROM json_each(gpu_counts_json))
            END
             FROM miner_gpu_profiles
             WHERE miner_uid = $MINER_UID)
        AS INTEGER) as total_gpus;"
    echo

    echo "3. Suspicious Patterns:"
    run_query "SELECT
        CASE
            WHEN EXISTS (
                SELECT 1 FROM gpu_uuid_assignments
                WHERE miner_id = 'miner_$MINER_UID'
                AND gpu_uuid IN ('Unknown UUID', '', 'GPU-0')
            ) THEN 'Invalid GPU UUIDs detected'
            ELSE 'Valid GPU UUIDs'
        END as uuid_check,
        CASE
            WHEN EXISTS (
                SELECT 1 FROM miner_executors
                WHERE miner_id = 'miner_$MINER_UID'
                AND status IN ('offline', 'failed', 'stale')
            ) THEN 'Executors offline/failed'
            ELSE 'Executors healthy'
        END as executor_health,
        CASE
            WHEN (
                SELECT COUNT(DISTINCT gpu_name)
                FROM gpu_uuid_assignments
                WHERE miner_id = 'miner_$MINER_UID'
            ) > 2 THEN 'Multiple GPU models (unusual)'
            ELSE 'Consistent GPU models'
        END as gpu_model_consistency,
        CASE
            WHEN (
                SELECT gpu_counts_json FROM miner_gpu_profiles WHERE miner_uid = $MINER_UID
            ) != '{}' AND NOT EXISTS (
                SELECT 1 FROM miner_executors
                WHERE miner_id = 'miner_$MINER_UID'
                AND status IN ('online', 'verified')
            ) THEN 'PHANTOM GPU PROFILE DETECTED!'
            ELSE 'No phantom GPUs'
        END as phantom_gpu_check;"
    echo

    echo "4. Historical GPU Changes (last 7 days):"
    run_query "SELECT
        DATE(last_verified) as date,
        COUNT(DISTINCT gpu_uuid) as gpu_count,
        COUNT(DISTINCT gpu_name) as gpu_models
    FROM gpu_uuid_assignments
    WHERE miner_id = 'miner_$MINER_UID'
    AND last_verified > datetime('now', '-7 days')
    GROUP BY DATE(last_verified)
    ORDER BY date DESC;"
    echo

    if [ "$SHOW_GPU_UUIDS" = true ]; then
        echo "GPU UUIDs (Detailed):"
        run_query "SELECT
            gpu_uuid,
            gpu_name,
            executor_id,
            last_verified,
            CASE
                WHEN last_verified < datetime('now', '-24 hours') THEN 'Stale'
                ELSE 'Fresh'
            END as freshness
        FROM gpu_uuid_assignments
        WHERE miner_id = 'miner_$MINER_UID'
        ORDER BY executor_id, gpu_uuid;"
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
        SUM(CASE WHEN success = 0 THEN 1 ELSE 0 END) as failed,
        ROUND(100.0 * SUM(CASE WHEN success = 1 THEN 1 ELSE 0 END) / COUNT(*), 2) as success_rate,
        AVG(CASE WHEN success = 1 THEN score ELSE NULL END) as avg_score
    FROM verification_logs
    WHERE executor_id IN (SELECT executor_id FROM miner_executors WHERE miner_id = 'miner_$MINER_UID')
    AND timestamp > '$LAST_EPOCH';"
    echo

    echo "Validation Patterns (last 24 hours):"
    run_query "SELECT
        strftime('%H', timestamp) as hour,
        COUNT(*) as validations,
        SUM(CASE WHEN success = 1 THEN 1 ELSE 0 END) as successful,
        ROUND(100.0 * SUM(CASE WHEN success = 1 THEN 1 ELSE 0 END) / COUNT(*), 1) as success_rate
    FROM verification_logs
    WHERE executor_id IN (SELECT executor_id FROM miner_executors WHERE miner_id = 'miner_$MINER_UID')
    AND timestamp > datetime('now', '-24 hours')
    GROUP BY hour
    ORDER BY hour DESC
    LIMIT 6;"
    echo

    echo "Successful Validations (last 5):"
    run_query "SELECT executor_id, timestamp, score FROM verification_logs WHERE executor_id IN (SELECT executor_id FROM miner_executors WHERE miner_id = 'miner_$MINER_UID') AND success = 1 ORDER BY timestamp DESC LIMIT 5;"
    echo

    echo "Failed Validations (last 5):"
    run_query "SELECT executor_id, timestamp, error_message FROM verification_logs WHERE executor_id IN (SELECT executor_id FROM miner_executors WHERE miner_id = 'miner_$MINER_UID') AND success = 0 ORDER BY timestamp DESC LIMIT 5;"
    echo

    echo "=== HEALTH CHECK ANALYSIS ==="
    echo
    echo "Executor Connectivity Status:"
    if [ -n "$SSH_CONN" ]; then
        EXECUTORS=$(ssh "$SSH_CONN" "sqlite3 '$DB_PATH' \"SELECT executor_id || '|' || grpc_address FROM miner_executors WHERE miner_id = 'miner_$MINER_UID';\"")

        for exec_info in $EXECUTORS; do
            executor_id=$(echo "$exec_info" | cut -d'|' -f1)
            grpc_address=$(echo "$exec_info" | cut -d'|' -f2)
            if [[ -z "$grpc_address" ]]; then
                echo "  $executor_id: GRPC address not set"
                continue
            fi
            if [[ "$grpc_address" =~ ^https?://([^:]+):([0-9]+) ]]; then
                host="${BASH_REMATCH[1]}"
                port="${BASH_REMATCH[2]}"
            elif [[ "$grpc_address" =~ ^([^:]+):([0-9]+)$ ]]; then
                host="${BASH_REMATCH[1]}"
                port="${BASH_REMATCH[2]}"
            else
                echo "  $executor_id: Invalid address format: $grpc_address"
                continue
            fi

            echo -n "  $executor_id ($host:$port): "

            if ssh "$SSH_CONN" "timeout 2 bash -c 'echo > /dev/tcp/$host/$port' 2>/dev/null"; then
                RECENT_SUCCESS=$(ssh "$SSH_CONN" "sqlite3 '$DB_PATH' \"SELECT COUNT(*) FROM verification_logs WHERE executor_id = '$executor_id' AND success = 1 AND timestamp > datetime('now', '-1 hour');\"")
                if [ "$RECENT_SUCCESS" -gt 0 ]; then
                    echo "TCP OK (verified recently)"
                else
                    echo "TCP OK (no recent verifications)"
                fi
            else
                echo "TCP UNREACHABLE"
            fi
        done
    fi
    echo

    echo "Executor Registration vs Activity:"
    run_query "SELECT
        me.executor_id,
        me.gpu_count as registered_gpus,
        COALESCE(ga.verified_gpus, 0) as verified_gpus,
        me.status,
        CASE
            WHEN vl.last_verification IS NULL THEN 'Never'
            ELSE datetime(vl.last_verification)
        END as last_verification,
        CASE
            WHEN datetime(vl.last_verification) > datetime('now', '-1 hour') THEN 'Active'
            WHEN datetime(vl.last_verification) > datetime('now', '-1 day') THEN 'Stale'
            ELSE 'Inactive'
        END as health_status
    FROM miner_executors me
    LEFT JOIN (
        SELECT executor_id, COUNT(DISTINCT gpu_uuid) as verified_gpus
        FROM gpu_uuid_assignments
        WHERE miner_id = 'miner_$MINER_UID'
        GROUP BY executor_id
    ) ga ON me.executor_id = ga.executor_id
    LEFT JOIN (
        SELECT executor_id, MAX(timestamp) as last_verification
        FROM verification_logs
        WHERE success = 1
        GROUP BY executor_id
    ) vl ON me.executor_id = vl.executor_id
    WHERE me.miner_id = 'miner_$MINER_UID'
    ORDER BY health_status;"
    echo

    echo "Discovery Failures (last 24h):"
    run_query "SELECT
        COUNT(*) as discovery_failures,
        MAX(timestamp) as last_failure
    FROM verification_logs
    WHERE executor_id = 'miner_$MINER_UID'
    AND error_message LIKE '%Failed to discover executors%'
    AND timestamp > datetime('now', '-1 day');"
    echo

    echo "Miner Endpoint Status:"
    if [ -n "$SSH_CONN" ]; then
        MINER_ENDPOINT=$(ssh "$SSH_CONN" "sqlite3 '$DB_PATH' \"SELECT endpoint FROM miners WHERE id = 'miner_$MINER_UID';\"")

        # Get actual executors from database for comparison
        ACTUAL_EXECUTORS=$(ssh "$SSH_CONN" "sqlite3 '$DB_PATH' \"
            SELECT COUNT(*)
            FROM miner_executors
            WHERE miner_id = 'miner_$MINER_UID'
            AND status IN ('online', 'verified');
        \"")

        if [ -n "$MINER_ENDPOINT" ]; then
            # Ensure endpoint has protocol
            if [[ ! "$MINER_ENDPOINT" =~ ^https?:// ]]; then
                MINER_ENDPOINT="http://$MINER_ENDPOINT"
            fi

            echo "  Configured Endpoint: $MINER_ENDPOINT"

            # Test connectivity with better error handling
            RESPONSE=$(ssh "$SSH_CONN" "timeout 5 curl -s -f -X GET '$MINER_ENDPOINT/executors' 2>&1" || echo "FAILED")

            if [[ "$RESPONSE" == "FAILED" ]] || [[ "$RESPONSE" == *"Connection refused"* ]] || [[ "$RESPONSE" == *"timed out"* ]] || [[ "$RESPONSE" == *"curl"* ]]; then
                echo "  Endpoint Status: UNREACHABLE"

                # If we have active executors but endpoint is unreachable, this is a configuration issue
                if [ "$ACTUAL_EXECUTORS" -gt 0 ]; then
                    echo "  WARNING: Endpoint unreachable but $ACTUAL_EXECUTORS active executor(s) in database"
                    echo "  This suggests the endpoint configuration may be outdated"

                    # Show actual executor addresses
                    echo "  Actual executor addresses in database:"
                    ssh "$SSH_CONN" "sqlite3 '$DB_PATH' \"
                        SELECT '    - ' || executor_id || ' at ' || grpc_address
                        FROM miner_executors
                        WHERE miner_id = 'miner_$MINER_UID'
                        AND status IN ('online', 'verified')
                        LIMIT 5;
                    \""
                else
                    echo "  Note: No active executors in database (consistent with unreachable endpoint)"
                fi
            else
                echo "  Endpoint Status: REACHABLE"

                # Try to parse executors if we got valid JSON
                if echo "$RESPONSE" | jq . >/dev/null 2>&1; then
                    EXEC_COUNT=$(echo "$RESPONSE" | jq '. | length' 2>/dev/null || echo "0")
                    echo "  Executors reported by endpoint: $EXEC_COUNT"
                    echo "  Executors in database: $ACTUAL_EXECUTORS"

                    if [ "$EXEC_COUNT" != "$ACTUAL_EXECUTORS" ]; then
                        echo "  WARNING: Mismatch between endpoint ($EXEC_COUNT) and database ($ACTUAL_EXECUTORS)"
                    fi

                    if [ "$EXEC_COUNT" != "0" ] && [ "$EXEC_COUNT" -le "5" ]; then
                        echo "  Executor details from endpoint:"
                        echo "$RESPONSE" | jq -r '.[] | "    - \(.id) at \(.grpc_address)"' 2>/dev/null || echo "    Unable to parse executor details"
                    elif [ "$EXEC_COUNT" -gt "5" ]; then
                        echo "  Showing first 5 executors from endpoint:"
                        echo "$RESPONSE" | jq -r '.[:5][] | "    - \(.id) at \(.grpc_address)"' 2>/dev/null || echo "    Unable to parse executor details"
                    fi
                else
                    echo "  Response received but not valid JSON"
                    echo "  Database shows $ACTUAL_EXECUTORS active executor(s)"
                fi
            fi
        else
            echo "  No endpoint configured for this miner"
            if [ "$ACTUAL_EXECUTORS" -gt 0 ]; then
                echo "  However, $ACTUAL_EXECUTORS active executor(s) found in database:"
                ssh "$SSH_CONN" "sqlite3 '$DB_PATH' \"
                    SELECT '    - ' || executor_id || ' at ' || grpc_address
                    FROM miner_executors
                    WHERE miner_id = 'miner_$MINER_UID'
                    AND status IN ('online', 'verified')
                    LIMIT 5;
                \""
            fi
        fi
    fi
    echo

    echo "Verification Pattern Analysis:"
    run_query "SELECT
        verification_type,
        COUNT(*) as count,
        SUM(CASE WHEN success = 1 THEN 1 ELSE 0 END) as successful,
        ROUND(100.0 * SUM(CASE WHEN success = 1 THEN 1 ELSE 0 END) / COUNT(*), 2) as success_rate
    FROM verification_logs
    WHERE executor_id IN (SELECT executor_id FROM miner_executors WHERE miner_id = 'miner_$MINER_UID')
    AND timestamp > datetime('now', '-1 day')
    GROUP BY verification_type;"
    echo

    echo "=== HEALTH & FRAUD SUMMARY ==="
    if [ -n "$SSH_CONN" ]; then
        TOTAL_EXECUTORS=$(ssh "$SSH_CONN" "sqlite3 '$DB_PATH' \"SELECT COUNT(*) FROM miner_executors WHERE miner_id = 'miner_$MINER_UID';\"")
        ACTIVE_EXECUTORS=$(ssh "$SSH_CONN" "sqlite3 '$DB_PATH' \"SELECT COUNT(DISTINCT executor_id) FROM verification_logs WHERE executor_id IN (SELECT executor_id FROM miner_executors WHERE miner_id = 'miner_$MINER_UID') AND success = 1 AND timestamp > datetime('now', '-1 hour');\"")
        EXECUTORS_WITH_GPUS=$(ssh "$SSH_CONN" "sqlite3 '$DB_PATH' \"SELECT COUNT(DISTINCT executor_id) FROM gpu_uuid_assignments WHERE miner_id = 'miner_$MINER_UID';\"")
        CONFLICTING_GPUS=$(ssh "$SSH_CONN" "sqlite3 '$DB_PATH' \"SELECT COUNT(*) FROM gpu_uuid_assignments ga WHERE ga.miner_id = 'miner_$MINER_UID' AND EXISTS (SELECT 1 FROM gpu_uuid_assignments other WHERE other.gpu_uuid = ga.gpu_uuid AND other.miner_id != ga.miner_id);\"")
        INVALID_UUIDS=$(ssh "$SSH_CONN" "sqlite3 '$DB_PATH' \"SELECT COUNT(*) FROM gpu_uuid_assignments WHERE miner_id = 'miner_$MINER_UID' AND gpu_uuid IN ('Unknown UUID', '', 'GPU-0');\"")
        TOTAL_SCORE=$(ssh "$SSH_CONN" "sqlite3 '$DB_PATH' \"SELECT total_score FROM miner_gpu_profiles WHERE miner_uid = $MINER_UID;\"")

        echo "  === Infrastructure ==="
        echo "  Total Executors: $TOTAL_EXECUTORS"
        echo "  Active Executors (last hour): $ACTIVE_EXECUTORS"
        echo "  Executors with verified GPUs: $EXECUTORS_WITH_GPUS"
        echo

        echo "  === Fraud Indicators ==="
        if [ "$CONFLICTING_GPUS" -gt 0 ]; then
            echo " CRITICAL: $CONFLICTING_GPUS GPUs conflict with other miners!"
        else
            echo " No GPU conflicts detected"
        fi

        if [ "$INVALID_UUIDS" -gt 0 ]; then
            echo " WARNING: $INVALID_UUIDS invalid GPU UUIDs found"
        else
            echo " All GPU UUIDs valid"
        fi

        REGISTERED_GPUS=$(ssh "$SSH_CONN" "sqlite3 '$DB_PATH' \"SELECT COALESCE(SUM(gpu_count), 0) FROM miner_executors WHERE miner_id = 'miner_$MINER_UID';\"")
        VERIFIED_GPUS=$(ssh "$SSH_CONN" "sqlite3 '$DB_PATH' \"SELECT COUNT(DISTINCT gpu_uuid) FROM gpu_uuid_assignments WHERE miner_id = 'miner_$MINER_UID';\"")

        if [ -z "$REGISTERED_GPUS" ]; then REGISTERED_GPUS=0; fi
        if [ -z "$VERIFIED_GPUS" ]; then VERIFIED_GPUS=0; fi

        if [ "$REGISTERED_GPUS" != "$VERIFIED_GPUS" ]; then
            echo "   GPU count mismatch: Registered=$REGISTERED_GPUS, Verified=$VERIFIED_GPUS"
        else
            echo "   GPU counts consistent: $REGISTERED_GPUS"
        fi

        echo
        echo "  === Overall Status ==="
        if [ "$ACTIVE_EXECUTORS" -eq 0 ]; then
            echo "  CRITICAL: No active executors!"
        elif [ "$ACTIVE_EXECUTORS" -lt "$TOTAL_EXECUTORS" ]; then
            echo "  WARNING: Only $ACTIVE_EXECUTORS/$TOTAL_EXECUTORS executors active"
        else
            echo "  All executors healthy"
        fi

        if [ -n "$TOTAL_SCORE" ]; then
            # Use awk for floating point comparison
            SCORE_STATUS=$(echo "$TOTAL_SCORE" | awk '{
                if ($1 >= 0.9) print "Excellent"
                else if ($1 >= 0.7) print "Good"
                else if ($1 >= 0.5) print "Fair"
                else print "Poor"
            }')

            if [ "$SCORE_STATUS" = "Excellent" ]; then
                echo "  Performance Score: ${TOTAL_SCORE} ($SCORE_STATUS)"
            elif [ "$SCORE_STATUS" = "Good" ] || [ "$SCORE_STATUS" = "Fair" ]; then
                echo "  Performance Score: ${TOTAL_SCORE} ($SCORE_STATUS)"
            else
                echo "  Performance Score: ${TOTAL_SCORE} ($SCORE_STATUS)"
            fi
        fi

        # Check for phantom GPU profile
        PROFILE_JSON=$(ssh "$SSH_CONN" "sqlite3 '$DB_PATH' \"SELECT gpu_counts_json FROM miner_gpu_profiles WHERE miner_uid = $MINER_UID;\"")
        ACTIVE_EXECUTOR_COUNT=$(ssh "$SSH_CONN" "sqlite3 '$DB_PATH' \"SELECT COUNT(*) FROM miner_executors WHERE miner_id = 'miner_$MINER_UID' AND status IN ('online', 'verified');\"")

        # Final verdict
        echo
        # Check if PROFILE_JSON is non-empty and has keys using jq
        if [ -n "$PROFILE_JSON" ]; then
            HAS_GPUS=$(echo "$PROFILE_JSON" | jq -r 'if . == null then "false" elif . == {} then "false" else (. | length > 0) end' 2>/dev/null || echo "false")
            if [ "$HAS_GPUS" = "true" ] && [ "$ACTIVE_EXECUTOR_COUNT" -eq 0 ]; then
                echo "  PHANTOM GPU PROFILE DETECTED!"
                echo "  This miner has GPU profile but NO ACTIVE executors"
                echo "  Profile shows: $PROFILE_JSON"
                echo "  This miner is receiving UNDESERVED weights!"
            fi
        fi

        if [ "$CONFLICTING_GPUS" -gt 0 ] || [ "$INVALID_UUIDS" -gt 0 ]; then
            echo "  POTENTIAL FRAUD DETECTED - Review GPU assignments immediately"
        elif [ "$ACTIVE_EXECUTORS" -eq 0 ]; then
            echo "  MINER INACTIVE - No recent validations"
        elif [ "$REGISTERED_GPUS" != "$VERIFIED_GPUS" ]; then
            echo "  INCONSISTENCIES DETECTED - Monitor closely"
        else
            echo "  MINER APPEARS LEGITIMATE"
        fi
    fi

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

elif [ -n "$EXECUTOR_ENDPOINT" ]; then
    echo "=== EXECUTOR ENDPOINT $EXECUTOR_ENDPOINT BREAKDOWN ==="
    echo

    echo "Executors with this endpoint:"
    run_query "SELECT
        me.executor_id,
        me.miner_id,
        me.gpu_count,
        me.status,
        me.last_health_check
    FROM miner_executors me
    WHERE me.grpc_address = '$EXECUTOR_ENDPOINT'
    OR me.grpc_address = 'http://$EXECUTOR_ENDPOINT'
    OR me.grpc_address = 'https://$EXECUTOR_ENDPOINT';"
    echo

    echo "Associated Miners:"
    run_query "SELECT DISTINCT
        m.id as miner_id,
        SUBSTR(m.hotkey, 1, 10) || '...' as hotkey_prefix,
        m.endpoint,
        p.total_score
    FROM miner_executors me
    INNER JOIN miners m ON me.miner_id = m.id
    LEFT JOIN miner_gpu_profiles p ON p.miner_uid = CAST(SUBSTR(m.id, 7) AS INTEGER)
    WHERE me.grpc_address = '$EXECUTOR_ENDPOINT'
    OR me.grpc_address = 'http://$EXECUTOR_ENDPOINT'
    OR me.grpc_address = 'https://$EXECUTOR_ENDPOINT';"
    echo

    echo "GPU Assignments for this endpoint:"
    run_query "SELECT
        ga.executor_id,
        ga.miner_id,
        COUNT(DISTINCT ga.gpu_uuid) as gpu_count,
        ga.gpu_name,
        MIN(ga.last_verified) as first_seen,
        MAX(ga.last_verified) as last_seen
    FROM gpu_uuid_assignments ga
    INNER JOIN miner_executors me ON ga.executor_id = me.executor_id
    WHERE me.grpc_address = '$EXECUTOR_ENDPOINT'
    OR me.grpc_address = 'http://$EXECUTOR_ENDPOINT'
    OR me.grpc_address = 'https://$EXECUTOR_ENDPOINT'
    GROUP BY ga.executor_id, ga.miner_id, ga.gpu_name;"
    echo

    if [ "$SHOW_GPU_UUIDS" = true ]; then
        echo "Detailed GPU UUIDs for this endpoint:"
        run_query "SELECT
            ga.gpu_uuid,
            ga.gpu_name,
            ga.executor_id,
            ga.miner_id,
            ga.last_verified
        FROM gpu_uuid_assignments ga
        INNER JOIN miner_executors me ON ga.executor_id = me.executor_id
        WHERE me.grpc_address = '$EXECUTOR_ENDPOINT'
        OR me.grpc_address = 'http://$EXECUTOR_ENDPOINT'
        OR me.grpc_address = 'https://$EXECUTOR_ENDPOINT'
        ORDER BY ga.executor_id, ga.gpu_uuid;"
        echo
    fi

    echo "Recent Validations (last 20):"
    run_query "SELECT
        vl.executor_id,
        vl.timestamp,
        vl.success,
        CASE WHEN vl.success = 1 THEN vl.score ELSE vl.error_message END as result
    FROM verification_logs vl
    INNER JOIN miner_executors me ON vl.executor_id = me.executor_id
    WHERE me.grpc_address = '$EXECUTOR_ENDPOINT'
    OR me.grpc_address = 'http://$EXECUTOR_ENDPOINT'
    OR me.grpc_address = 'https://$EXECUTOR_ENDPOINT'
    ORDER BY vl.timestamp DESC
    LIMIT 20;"
    echo

    echo "Validation Statistics (last 24 hours):"
    run_query "SELECT
        vl.executor_id,
        COUNT(*) as total_validations,
        SUM(CASE WHEN vl.success = 1 THEN 1 ELSE 0 END) as successful,
        SUM(CASE WHEN vl.success = 0 THEN 1 ELSE 0 END) as failed,
        ROUND(100.0 * SUM(CASE WHEN vl.success = 1 THEN 1 ELSE 0 END) / COUNT(*), 2) as success_rate
    FROM verification_logs vl
    INNER JOIN miner_executors me ON vl.executor_id = me.executor_id
    WHERE me.grpc_address = '$EXECUTOR_ENDPOINT'
    OR me.grpc_address = 'http://$EXECUTOR_ENDPOINT'
    OR me.grpc_address = 'https://$EXECUTOR_ENDPOINT'
    AND vl.timestamp > datetime('now', '-1 day')
    GROUP BY vl.executor_id;"
    echo

    echo "Health Status:"
    run_query "SELECT
        me.executor_id,
        me.status,
        me.last_health_check,
        CASE
            WHEN me.last_health_check > datetime('now', '-1 hour') THEN 'Active'
            WHEN me.last_health_check > datetime('now', '-1 day') THEN 'Stale'
            ELSE 'Inactive'
        END as health
    FROM miner_executors me
    WHERE me.grpc_address = '$EXECUTOR_ENDPOINT'
    OR me.grpc_address = 'http://$EXECUTOR_ENDPOINT'
    OR me.grpc_address = 'https://$EXECUTOR_ENDPOINT';"

elif [ -n "$GPU_UUID" ]; then
    echo "=== GPU UUID $GPU_UUID BREAKDOWN ==="
    echo

    echo "GPU Information:"
    run_query "SELECT
        gpu_uuid,
        gpu_name,
        gpu_index,
        executor_id,
        miner_id,
        last_verified,
        created_at
    FROM gpu_uuid_assignments
    WHERE gpu_uuid = '$GPU_UUID';"
    echo

    echo "Associated Miner:"
    run_query "SELECT
        m.id as miner_id,
        m.hotkey,
        m.endpoint,
        p.total_score,
        p.verification_count
    FROM gpu_uuid_assignments ga
    INNER JOIN miners m ON ga.miner_id = m.id
    LEFT JOIN miner_gpu_profiles p ON p.miner_uid = CAST(SUBSTR(m.id, 7) AS INTEGER)
    WHERE ga.gpu_uuid = '$GPU_UUID';"
    echo

    echo "Associated Executor:"
    run_query "SELECT
        me.executor_id,
        me.grpc_address,
        me.gpu_count,
        me.status,
        me.last_health_check
    FROM gpu_uuid_assignments ga
    INNER JOIN miner_executors me ON ga.executor_id = me.executor_id
    WHERE ga.gpu_uuid = '$GPU_UUID';"
    echo

    echo "Other GPUs on same executor:"
    run_query "SELECT
        other.gpu_uuid,
        other.gpu_name,
        other.last_verified
    FROM gpu_uuid_assignments ga
    INNER JOIN gpu_uuid_assignments other ON ga.executor_id = other.executor_id
    WHERE ga.gpu_uuid = '$GPU_UUID'
    AND other.gpu_uuid != '$GPU_UUID'
    ORDER BY other.gpu_uuid;"
    echo

    echo "GPU Conflict Check:"
    run_query "SELECT
        COUNT(*) as total_occurrences,
        COUNT(DISTINCT miner_id) as unique_miners,
        COUNT(DISTINCT executor_id) as unique_executors,
        CASE
            WHEN COUNT(DISTINCT miner_id) > 1 THEN 'CONFLICT: Used by multiple miners!'
            ELSE 'No conflicts'
        END as status
    FROM gpu_uuid_assignments
    WHERE gpu_uuid = '$GPU_UUID';"
    echo

    echo "Miners using this GPU UUID:"
    run_query "SELECT
        miner_id,
        executor_id,
        last_verified
    FROM gpu_uuid_assignments
    WHERE gpu_uuid = '$GPU_UUID'
    ORDER BY last_verified DESC;"
    echo

    echo "Prover Results for this GPU:"
    run_query "SELECT
        verification_timestamp,
        attestation_valid,
        gpu_model,
        gpu_memory_gb
    FROM miner_prover_results
    WHERE gpu_uuid = '$GPU_UUID'
    ORDER BY verification_timestamp DESC
    LIMIT 10;"
    echo

    echo "Recent Validations for associated executor (last 10):"
    run_query "SELECT
        vl.timestamp,
        vl.verification_type,
        vl.success,
        CASE WHEN vl.success = 1 THEN vl.score ELSE vl.error_message END as result
    FROM verification_logs vl
    WHERE vl.executor_id IN (
        SELECT executor_id FROM gpu_uuid_assignments WHERE gpu_uuid = '$GPU_UUID'
    )
    ORDER BY vl.timestamp DESC
    LIMIT 10;"
    echo

    echo "GPU History (changes over time):"
    run_query "SELECT
        DATE(last_verified) as date,
        executor_id,
        miner_id,
        COUNT(*) as verification_count
    FROM gpu_uuid_assignments
    WHERE gpu_uuid = '$GPU_UUID'
    GROUP BY DATE(last_verified), executor_id, miner_id
    ORDER BY date DESC
    LIMIT 7;"

elif [ -n "$GPU_PROFILE" ]; then
    GPU_MODEL=$(echo "$GPU_PROFILE" | tr '[:lower:]' '[:upper:]')
    echo "=== GPU PROFILE $GPU_MODEL BREAKDOWN ==="
    echo

    echo "Miners with $GPU_MODEL:"
    run_query "SELECT miner_uid, total_score, last_successful_validation FROM miner_gpu_profiles WHERE json_extract(gpu_counts_json, '\$.\"$GPU_MODEL\"') IS NOT NULL AND total_score >= 0.1 ORDER BY total_score DESC;"
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

    echo "=== CRITICAL FRAUD DETECTION ==="
    echo

    echo "PHANTOM GPU PROFILES (miners with scores but no active GPUs):"
    run_query "SELECT
        mgp.miner_uid,
        mgp.total_score,
        mgp.gpu_counts_json,
        CASE
            WHEN EXISTS (SELECT 1 FROM miner_executors WHERE miner_id = 'miner_' || mgp.miner_uid AND status IN ('online', 'verified'))
            THEN 'Has executors'
            ELSE 'NO EXECUTORS'
        END as executor_status
    FROM miner_gpu_profiles mgp
    WHERE mgp.total_score > 0
    AND mgp.miner_uid NOT IN (
        SELECT DISTINCT CAST(SUBSTR(ga.miner_id, 7) AS INTEGER)
        FROM gpu_uuid_assignments ga
        INNER JOIN miner_executors me ON ga.miner_id = me.miner_id AND ga.executor_id = me.executor_id
        WHERE me.status IN ('online', 'verified')
        AND me.gpu_count > 0
    )
    ORDER BY mgp.total_score DESC;"
    echo

    echo "VERIFIED EXECUTORS WITH 0 GPUs (should not be verified):"
    run_query "SELECT
        me.miner_id,
        me.executor_id,
        me.status,
        me.gpu_count,
        COALESCE(ga.verified_gpus, 0) as actual_gpus
    FROM miner_executors me
    LEFT JOIN (
        SELECT executor_id, COUNT(DISTINCT gpu_uuid) as verified_gpus
        FROM gpu_uuid_assignments
        GROUP BY executor_id
    ) ga ON me.executor_id = ga.executor_id
    WHERE me.status = 'verified'
    AND me.gpu_count = 0;"
    echo

    echo "ACTIVE MINERS RECEIVING WEIGHTS:"
    run_query "SELECT
        wah.miner_uid,
        wah.gpu_category,
        wah.allocated_weight,
        mgp.total_score,
        mgp.gpu_counts_json,
        CASE
            WHEN EXISTS (
                SELECT 1 FROM gpu_uuid_assignments ga
                INNER JOIN miner_executors me ON ga.miner_id = me.miner_id AND ga.executor_id = me.executor_id
                WHERE ga.miner_id = 'miner_' || wah.miner_uid
                AND me.status IN ('online', 'verified')
            ) THEN 'LEGITIMATE'
            ELSE 'PHANTOM'
        END as status
    FROM weight_allocation_history wah
    LEFT JOIN miner_gpu_profiles mgp ON wah.miner_uid = mgp.miner_uid
    WHERE wah.weight_set_block = (SELECT MAX(weight_set_block) FROM weight_allocation_history)
    AND wah.allocated_weight > 0
    ORDER BY status DESC, wah.allocated_weight DESC;"
    echo

    echo "ANOMALY CHECKS:"
    echo

    echo "Duplicate GPU UUIDs (GPUs claimed by multiple miners):"
    run_query "SELECT
        gpu_uuid,
        COUNT(DISTINCT miner_id) as miner_count,
        GROUP_CONCAT(DISTINCT miner_id) as miners
    FROM gpu_uuid_assignments
    GROUP BY gpu_uuid
    HAVING COUNT(DISTINCT miner_id) > 1;"
    echo

    echo "Executors with mismatched GPU counts:"
    run_query "SELECT
        me.miner_id,
        me.executor_id,
        me.gpu_count as registered_gpus,
        COALESCE(ga.actual_gpus, 0) as verified_gpus,
        me.status
    FROM miner_executors me
    LEFT JOIN (
        SELECT executor_id, miner_id, COUNT(DISTINCT gpu_uuid) as actual_gpus
        FROM gpu_uuid_assignments
        GROUP BY executor_id, miner_id
    ) ga ON me.executor_id = ga.executor_id AND me.miner_id = ga.miner_id
    WHERE me.gpu_count != COALESCE(ga.actual_gpus, 0)
    ORDER BY me.miner_id;"
    echo

    echo "Miners with 0 GPUs but non-zero score:"
    run_query "SELECT
        mgp.miner_uid,
        mgp.total_score,
        mgp.gpu_counts_json
    FROM miner_gpu_profiles mgp
    WHERE mgp.total_score > 0
    AND NOT EXISTS (
        SELECT 1 FROM gpu_uuid_assignments ga
        WHERE ga.miner_id = 'miner_' || mgp.miner_uid
    );"
    echo

    echo "Stale/Failed Executors (should be cleaned up):"
    run_query "SELECT
        miner_id,
        executor_id,
        status,
        last_health_check
    FROM miner_executors
    WHERE status IN ('offline', 'failed', 'stale')
    ORDER BY last_health_check DESC
    LIMIT 10;"
    echo

    echo "GPU Distribution:"
    run_query "SELECT gpu_name, COUNT(DISTINCT gpu_uuid) as gpu_count, COUNT(DISTINCT miner_id) as miner_count FROM gpu_uuid_assignments GROUP BY gpu_name ORDER BY gpu_count DESC;"
    echo

    echo "Miner Profiles by GPU Model:"
    echo "Note: GPU model distribution query removed - primary_gpu_model column no longer exists"
    echo "Use gpu_counts_json field for detailed GPU distribution analysis"
    echo

    echo "Latest Weight Distribution:"
    run_query "SELECT gpu_category, COUNT(*) as miners_rewarded, SUM(allocated_weight) as total_weight FROM weight_allocation_history WHERE weight_set_block = (SELECT MAX(weight_set_block) FROM weight_allocation_history) GROUP BY gpu_category ORDER BY total_weight DESC;"
    echo

    echo "Top 10 Miners by Score:"
    run_query "SELECT p.miner_uid,
        SUBSTR(m.hotkey, 1, 10) || '...' as hotkey_prefix,
        -- primary_gpu_model column removed
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
    echo

    echo "=== FRAUD DETECTION SUMMARY ==="
    echo

    echo "Statistics:"
    run_query "SELECT
        'Total active miners with real GPUs' as metric,
        COUNT(DISTINCT CAST(SUBSTR(ga.miner_id, 7) AS INTEGER)) as count
    FROM gpu_uuid_assignments ga
    INNER JOIN miner_executors me ON ga.miner_id = me.miner_id AND ga.executor_id = me.executor_id
    WHERE me.status IN ('online', 'verified')
    AND me.gpu_count > 0
    UNION ALL
    SELECT
        'Miners with scores > 0' as metric,
        COUNT(*) as count
    FROM miner_gpu_profiles
    WHERE total_score > 0
    UNION ALL
    SELECT
        'Phantom profiles (score > 0, no GPUs)' as metric,
        COUNT(*) as count
    FROM miner_gpu_profiles mgp
    WHERE mgp.total_score > 0
    AND mgp.miner_uid NOT IN (
        SELECT DISTINCT CAST(SUBSTR(ga.miner_id, 7) AS INTEGER)
        FROM gpu_uuid_assignments ga
        INNER JOIN miner_executors me ON ga.miner_id = me.miner_id AND ga.executor_id = me.executor_id
        WHERE me.status IN ('online', 'verified')
        AND me.gpu_count > 0
    )
    UNION ALL
    SELECT
        'Verified executors with 0 GPUs' as metric,
        COUNT(*) as count
    FROM miner_executors
    WHERE status = 'verified'
    AND gpu_count = 0;"
    echo

    echo "Quick Action Items:"
    PHANTOM_COUNT=$(run_query "SELECT COUNT(*) FROM miner_gpu_profiles mgp WHERE mgp.total_score > 0 AND mgp.miner_uid NOT IN (SELECT DISTINCT CAST(SUBSTR(ga.miner_id, 7) AS INTEGER) FROM gpu_uuid_assignments ga INNER JOIN miner_executors me ON ga.miner_id = me.miner_id AND ga.executor_id = me.executor_id WHERE me.status IN ('online', 'verified') AND me.gpu_count > 0);" | tail -1)
    VERIFIED_ZERO_COUNT=$(run_query "SELECT COUNT(*) FROM miner_executors WHERE status = 'verified' AND gpu_count = 0;" | tail -1)

    if [ "$PHANTOM_COUNT" -gt 0 ]; then
        echo "  CRITICAL: $PHANTOM_COUNT miners receiving weights without GPUs"
        echo "  Run cleanup SQL to reset phantom profiles"
    fi

    if [ "$VERIFIED_ZERO_COUNT" -gt 0 ]; then
        echo "  WARNING: $VERIFIED_ZERO_COUNT executors marked verified with 0 GPUs"
        echo "  Update executor status to offline"
    fi

    if [ "$PHANTOM_COUNT" -eq 0 ] && [ "$VERIFIED_ZERO_COUNT" -eq 0 ]; then
        echo "  No critical issues detected"
    fi
fi
