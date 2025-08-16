#!/bin/bash
set -euo pipefail

SERVICE="api"
SERVER_USER=""
SERVER_HOST=""
SERVER_PORT=""
DEPLOY_MODE="binary"
CONFIG_FILE="config/api.correct.toml"
FOLLOW_LOGS=false
HEALTH_CHECK=false
TIMEOUT=60

usage() {
    cat <<EOF
Usage: $0 [OPTIONS]

Deploy Basilica API to remote server.

OPTIONS:
    -s, --server USER@HOST:PORT      Server connection
    -d, --deploy-mode MODE           Deployment mode: binary, systemd, docker (default: docker)
    -c, --config FILE                Config file path (default: config/basilica-api.toml)
    -f, --follow-logs                Stream logs after deployment
    --health-check                   Perform health checks on service endpoints
    -t, --timeout SECONDS           SSH timeout (default: 60)
    -h, --help                       Show this help

DEPLOYMENT MODES:
    binary   - Deploy binary with nohup
    systemd  - Deploy binary with systemd service management
    docker   - Deploy using docker compose with public images (default)

EXAMPLES:
    # Deploy API with docker mode (default)
    $0 -s root@64.247.196.98:9001

    # Deploy API with binary mode
    $0 -s root@64.247.196.98:9001 -d binary

    # Deploy API with systemd
    $0 -s root@64.247.196.98:9001 -d systemd

    # Deploy with custom config and health checks
    $0 -s root@64.247.196.98:9001 -c config/api.prod.toml --health-check
EOF
    exit 1
}

log() {
    echo "[$(date '+%H:%M:%S')] $*"
}

ssh_cmd() {
    local cmd="$1"
    ssh -o ConnectTimeout=30 "$SERVER_USER@$SERVER_HOST" -p "$SERVER_PORT" "$cmd"
}

scp_file() {
    local src="$1"
    local dest="$2"
    scp -o ConnectTimeout=30 -P "$SERVER_PORT" "$src" "$SERVER_USER@$SERVER_HOST:$dest"
}

validate_config() {
    if [[ ! -f "$CONFIG_FILE" ]]; then
        log "ERROR: Config file not found: $CONFIG_FILE"
        exit 1
    fi
    log "Using config file: $CONFIG_FILE"
}

build_service() {
    if [[ "$DEPLOY_MODE" == "docker" ]]; then
        log "Docker mode: skipping local build"
        return
    fi

    log "Building API..."
    if [[ ! -f "scripts/api/build.sh" ]]; then
        log "ERROR: Build script scripts/api/build.sh not found"
        exit 1
    fi

    ./scripts/api/build.sh

    if [[ ! -f "./basilica-api" ]]; then
        log "ERROR: Binary ./basilica-api not found after build"
        exit 1
    fi
}

deploy_binary() {
    log "Deploying API in binary mode"

    log "Stopping existing API processes"
    ssh -o ConnectTimeout=5 "$SERVER_USER@$SERVER_HOST" -p "$SERVER_PORT" "pkill -f '/opt/basilica/basilica-api' 2>/dev/null || true" || log "WARNING: Could not connect to stop API processes"

    sleep 2

    # Force kill with shorter timeout
    ssh -o ConnectTimeout=5 "$SERVER_USER@$SERVER_HOST" -p "$SERVER_PORT" "pkill -9 -f '/opt/basilica/basilica-api' 2>/dev/null || true" || log "WARNING: Could not connect for force kill"

    sleep 3

    log "Removing old API files"
    ssh_cmd "cp /opt/basilica/basilica-api /opt/basilica/basilica-api.backup 2>/dev/null || true"

    # Try to move the current binary out of the way to avoid "Text file busy"
    ssh_cmd "mv /opt/basilica/basilica-api /opt/basilica/basilica-api.old 2>/dev/null || true"

    scp_file "basilica-api" "/opt/basilica/basilica-api"
    ssh_cmd "chmod +x /opt/basilica/basilica-api"

    log "Creating directories for API"
    ssh_cmd "mkdir -p /opt/basilica/config"
    scp_file "$CONFIG_FILE" "/opt/basilica/config/basilica-api.toml"

    ssh_cmd "mkdir -p /opt/basilica/data && chmod 755 /opt/basilica/data"

    local start_cmd="cd /opt/basilica && RUST_LOG=debug nohup ./basilica-api --config config/basilica-api.toml > api.log 2>&1 &"

    log "Starting API"
    ssh -o ConnectTimeout=5 "$SERVER_USER@$SERVER_HOST" -p "$SERVER_PORT" "$start_cmd" || true

    sleep 5
    if ssh_cmd "pgrep -f basilica-api > /dev/null"; then
        log "API started successfully"
    else
        log "ERROR: API failed to start"
        ssh_cmd "tail -10 /opt/basilica/api.log"
        exit 1
    fi
}

deploy_systemd() {
    log "Deploying API in systemd mode"

    log "Stopping existing API service"
    ssh_cmd "systemctl stop basilica-api 2>/dev/null || true"

    log "Removing old API files"
    ssh_cmd "cp /opt/basilica/basilica-api /opt/basilica/basilica-api.backup 2>/dev/null || true"
    ssh_cmd "mv /opt/basilica/basilica-api /opt/basilica/basilica-api.old 2>/dev/null || true"

    scp_file "basilica-api" "/opt/basilica/basilica-api"
    ssh_cmd "chmod +x /opt/basilica/basilica-api"

    log "Creating directories for API"
    ssh_cmd "mkdir -p /opt/basilica/config"
    scp_file "$CONFIG_FILE" "/opt/basilica/config/basilica-api.toml"

    ssh_cmd "mkdir -p /opt/basilica/data && chmod 755 /opt/basilica/data"

    log "Installing systemd service"
    if [[ ! -f "scripts/basilica-api/systemd/basilica-api.service" ]]; then
        log "Creating systemd service file"
        ssh_cmd "cat > /etc/systemd/system/basilica-api.service << 'EOF'
[Unit]
Description=Basilica API Service
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=/opt/basilica
ExecStart=/opt/basilica/basilica-api --config config/basilica-api.toml
Restart=always
RestartSec=10
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target
EOF"
    else
        scp_file "scripts/basilica-api/systemd/basilica-api.service" "/etc/systemd/system/"
    fi
    
    ssh_cmd "systemctl daemon-reload"
    ssh_cmd "systemctl enable basilica-api"

    log "Starting API service"
    ssh_cmd "systemctl start basilica-api"

    sleep 5
    if ssh_cmd "systemctl is-active basilica-api --quiet"; then
        log "API service started successfully"
    else
        log "ERROR: API service failed to start"
        ssh_cmd "journalctl -u basilica-api --lines=20 --no-pager"
        exit 1
    fi
}

deploy_docker() {
    log "Deploying API in docker mode"

    log "Stopping existing API containers"
    ssh_cmd "cd /opt/basilica && docker compose -f compose.prod.yml down 2>/dev/null || true"

    log "Creating directories for API"
    ssh_cmd "mkdir -p /opt/basilica/config"
    scp_file "$CONFIG_FILE" "/opt/basilica/config/basilica-api.toml"

    ssh_cmd "mkdir -p /opt/basilica/data && chmod 755 /opt/basilica/data"

    log "Deploying docker compose files"
    if [[ ! -f "scripts/basilica-api/compose.prod.yml" ]]; then
        log "ERROR: Docker compose file not found: scripts/basilica-api/compose.prod.yml"
        exit 1
    fi

    scp_file "scripts/basilica-api/compose.prod.yml" "/opt/basilica/"

    # Deploy .env file if it exists
    if [[ -f "scripts/basilica-api/.env" ]]; then
        scp_file "scripts/basilica-api/.env" "/opt/basilica/"
    fi

    log "Pulling and starting API container"
    ssh_cmd "cd /opt/basilica && docker compose -f compose.prod.yml pull"
    ssh_cmd "cd /opt/basilica && docker compose -f compose.prod.yml up -d"

    sleep 5
    if ssh_cmd "cd /opt/basilica && docker compose -f compose.prod.yml ps | grep -q 'Up'"; then
        log "API container started successfully"
    else
        log "ERROR: API container failed to start"
        ssh_cmd "cd /opt/basilica && docker compose -f compose.prod.yml logs --tail=20"
        exit 1
    fi
}

deploy_service() {
    case "$DEPLOY_MODE" in
        binary)
            deploy_binary
            ;;
        systemd)
            deploy_systemd
            ;;
        docker)
            deploy_docker
            ;;
        *)
            log "ERROR: Unknown deployment mode: $DEPLOY_MODE"
            exit 1
            ;;
    esac
}

health_check_service() {
    case "$DEPLOY_MODE" in
        binary)
            if ssh_cmd "pgrep -f basilica-api > /dev/null"; then
                local port=$(ssh_cmd "grep -E '^(port|bind)' /opt/basilica/config/basilica-api.toml | head -1 | cut -d'=' -f2 | tr -d ' \"'")
                log "API running (port $port)"
                # Try to check health endpoint
                ssh_cmd "curl -sf http://localhost:${port}/health >/dev/null && echo 'Health check passed' || echo 'Health check failed'"
            else
                log "API not running"
            fi
            ;;
        systemd)
            if ssh_cmd "systemctl is-active basilica-api --quiet"; then
                local port=$(ssh_cmd "grep -E '^(port|bind)' /opt/basilica/config/basilica-api.toml | head -1 | cut -d'=' -f2 | tr -d ' \"'")
                log "API service active (port $port)"
                ssh_cmd "curl -sf http://localhost:${port}/health >/dev/null && echo 'Health check passed' || echo 'Health check failed'"
            else
                log "API service not active"
            fi
            ;;
        docker)
            if ssh_cmd "cd /opt/basilica && docker compose -f compose.prod.yml ps | grep -q 'Up'"; then
                log "API container running"
                ssh_cmd "cd /opt/basilica && docker compose -f compose.prod.yml exec -T basilica-api curl -sf http://localhost:8000/health >/dev/null && echo 'Health check passed' || echo 'Health check failed'"
            else
                log "API container not running"
            fi
            ;;
    esac
}

follow_logs_service() {
    log "Following logs for API"
    case "$DEPLOY_MODE" in
        binary)
            ssh_cmd "tail -f /opt/basilica/api.log"
            ;;
        systemd)
            ssh_cmd "journalctl -u basilica-api -f"
            ;;
        docker)
            ssh_cmd "cd /opt/basilica && docker compose -f compose.prod.yml logs -f"
            ;;
    esac
}

while [[ $# -gt 0 ]]; do
    case $1 in
        -s|--server)
            IFS='@' read -r SERVER_USER temp <<< "$2"
            IFS=':' read -r SERVER_HOST SERVER_PORT <<< "$temp"
            shift 2
            ;;
        -d|--deploy-mode)
            DEPLOY_MODE="$2"
            if [[ "$DEPLOY_MODE" != "binary" && "$DEPLOY_MODE" != "systemd" && "$DEPLOY_MODE" != "docker" ]]; then
                echo "ERROR: Invalid deployment mode: $DEPLOY_MODE. Must be binary, systemd, or docker"
                exit 1
            fi
            shift 2
            ;;
        -c|--config)
            CONFIG_FILE="$2"
            shift 2
            ;;
        -f|--follow-logs)
            FOLLOW_LOGS=true
            shift
            ;;
        --health-check)
            HEALTH_CHECK=true
            shift
            ;;
        -t|--timeout)
            TIMEOUT="$2"
            shift 2
            ;;
        -h|--help)
            usage
            ;;
        *)
            echo "ERROR: Unknown option: $1"
            usage
            ;;
    esac
done

if [[ -z "$SERVER_USER" || -z "$SERVER_HOST" || -z "$SERVER_PORT" ]]; then
    echo "ERROR: Server connection required (-s)"
    usage
fi

log "Deployment mode: $DEPLOY_MODE"
validate_config

log "Building API"
build_service

deploy_service

if [[ "$HEALTH_CHECK" == "true" ]]; then
    log "Running health check on API"
    health_check_service
fi

log "Deployment completed successfully"

if [[ "$FOLLOW_LOGS" == "true" ]]; then
    follow_logs_service
fi
