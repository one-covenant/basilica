#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "Starting Basilica Payments development environment..."

# Start the services
docker-compose -f "$SCRIPT_DIR/compose.dev.yml" up -d

echo "Waiting for PostgreSQL to be ready..."
sleep 5

# Check if PostgreSQL is ready
docker-compose -f "$SCRIPT_DIR/compose.dev.yml" exec -T postgres pg_isready -U payments -d basilica_payments

echo "Development environment started!"
echo ""
echo "Services:"
echo "  - PostgreSQL: localhost:5433"
echo "  - Payments gRPC: localhost:50061"
echo "  - Payments HTTP: localhost:8082"
echo ""
echo "To view logs: docker-compose -f $SCRIPT_DIR/compose.dev.yml logs -f"
echo "To stop: docker-compose -f $SCRIPT_DIR/compose.dev.yml down"