#!/bin/bash
set -euo pipefail

# Simple Basilica API Deployment Script

if [ $# -lt 1 ]; then
    echo "Usage: $0 <user@host> [port]"
    echo "Example: $0 root@192.168.1.10 22"
    exit 1
fi

BASILICA_API_HOST="$1"
BASILICA_API_PORT="${2:-22}"
REMOTE_DIR="/opt/basilica"

echo "Deploying Basilica API to $BASILICA_API_HOST:$BASILICA_API_PORT"

# Create remote directory
ssh -p "$BASILICA_API_PORT" "$BASILICA_API_HOST" "mkdir -p $REMOTE_DIR"

# Copy necessary files
scp -P "$BASILICA_API_PORT" -r \
    scripts/basilica-api/compose.prod.yml \
    config/basilica-api.toml \
    "$BASILICA_API_HOST:$REMOTE_DIR/"

# Deploy
ssh -p "$BASILICA_API_PORT" "$BASILICA_API_HOST" << 'EOF'
    cd /opt/basilica
    docker compose -f compose.prod.yml pull
    docker compose -f compose.prod.yml up -d
    docker compose -f compose.prod.yml ps
EOF

echo "Basilica API deployed successfully"
