#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "Starting Basilica Billing development environment..."

# Start the services
docker-compose -f "$SCRIPT_DIR/compose.dev.yml" up -d

echo "Waiting for PostgreSQL to be ready..."
sleep 5

# Check if PostgreSQL is ready
docker-compose -f "$SCRIPT_DIR/compose.dev.yml" exec -T postgres pg_isready -U billing -d basilica_billing

echo "Development environment started!"
echo ""
echo "Services:"
echo "  - PostgreSQL: localhost:5432"
echo "  - Billing gRPC: localhost:50051"
echo "  - Billing HTTP: localhost:8081"
echo ""
echo "To view logs: docker-compose -f $SCRIPT_DIR/compose.dev.yml logs -f"
echo "To stop: docker-compose -f $SCRIPT_DIR/compose.dev.yml down"
