#!/bin/bash

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
COMPOSE_FILE="$SCRIPT_DIR/compose.dev.yml"
DATABASE_URL="postgres://billing:billing_dev_password@localhost:5432/basilica_billing"

KEEP_DB=false
TEST_FILTER=""
NO_DOCKER=false

usage() {
    cat << EOF
Usage: $0 [OPTIONS]

OPTIONS:
    -h, --help          Show this help message
    -k, --keep-db       Keep PostgreSQL running after tests
    -c, --clean         Stop and remove PostgreSQL container
    -f, --filter PATTERN  Run only tests matching the pattern
    --no-docker         Don't manage Docker, assume PostgreSQL is already running

EOF
}

while [[ $# -gt 0 ]]; do
    case $1 in
        -h|--help)
            usage
            exit 0
            ;;
        -k|--keep-db)
            KEEP_DB=true
            shift
            ;;
        -c|--clean)
            docker compose -f "$COMPOSE_FILE" down -v
            exit 0
            ;;
        -f|--filter)
            TEST_FILTER="$2"
            shift 2
            ;;
        --no-docker)
            NO_DOCKER=true
            shift
            ;;
        *)
            echo "Unknown option: $1"
            usage
            exit 1
            ;;
    esac
done

wait_for_postgres() {
    local max_attempts=30
    local attempt=0

    while [ $attempt -lt $max_attempts ]; do
        if docker compose -f "$COMPOSE_FILE" exec -T postgres pg_isready -U billing -d basilica_billing >/dev/null 2>&1; then
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 1
    done

    return 1
}

start_postgres() {
    docker compose -f "$COMPOSE_FILE" up -d postgres

    if ! wait_for_postgres; then
        echo "Failed to start PostgreSQL"
        docker compose -f "$COMPOSE_FILE" logs postgres
        return 1
    fi


    return 0
}

run_tests() {
    cd "$PROJECT_ROOT"

    local test_cmd="BILLING_DATABASE_URL=\"$DATABASE_URL\" BILLING_AWS__SECRETS_MANAGER_ENABLED=false cargo test -p basilica-billing --test bdd_integration_tests"

    if [ -n "$TEST_FILTER" ]; then
        test_cmd="$test_cmd $TEST_FILTER"
    fi

    test_cmd="$test_cmd -- --test-threads=1"

    eval $test_cmd
}

# Main
cd "$PROJECT_ROOT"

if [ "$NO_DOCKER" = false ]; then
    if [ "$KEEP_DB" = false ]; then
        trap 'docker compose -f "$COMPOSE_FILE" stop postgres' EXIT
    fi

    if ! start_postgres; then
        exit 1
    fi
fi

run_tests
test_exit_code=$?

if [ "$KEEP_DB" = true ] && [ "$NO_DOCKER" = false ]; then
    echo "PostgreSQL is still running. To stop: $0 --clean"
fi

exit $test_exit_code
