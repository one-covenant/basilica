#!/bin/bash

# Test script for Basilica API examples
# This script demonstrates the complete payment and billing flow

set -e

USER_ID=${1:-"test-user-$(date +%s)"}
CREDIT_AMOUNT=${2:-"100.0"}

echo "=== Basilica API Flow Test ==="
echo "User ID: $USER_ID"
echo "Credit Amount: $CREDIT_AMOUNT"
echo

# Check if services are running
echo "1. Checking service connectivity..."
echo "Payments service (port 50061):"
if nc -z localhost 50061 2>/dev/null; then
    echo "  Available"
    PAYMENTS_AVAILABLE=true
else
    echo "  Not available"
    PAYMENTS_AVAILABLE=false
fi

echo "Billing service (port 50051):"
if nc -z localhost 50051 2>/dev/null; then
    echo "  Available"
    BILLING_AVAILABLE=true
else
    echo "  Not available"
    BILLING_AVAILABLE=false
fi

echo

if [ "$PAYMENTS_AVAILABLE" = true ] && [ "$BILLING_AVAILABLE" = true ]; then
    echo "2. Running full flow with live services..."

    echo "Step 1: Create/get wallet"
    cargo run -p integration-tests --bin scenario_wallet_create "$USER_ID"
    echo

    echo "Step 2: List deposits"
    cargo run -p integration-tests --bin scenario_deposits_list "$USER_ID"
    echo

    echo "Step 3: Apply test credits"
    cargo run -p integration-tests --bin scenario_credits_apply "$USER_ID" "$CREDIT_AMOUNT"
    echo

    echo "Step 4: Check balance"
    cargo run -p integration-tests --bin scenario_balance_check "$USER_ID"
    echo

    echo "Full flow completed successfully"
else
    echo "2. Services not available - showing example commands..."
    echo
    echo "To test with running services:"
    echo "# Start services:"
    echo "docker-compose -f scripts/billing/compose.dev.yml up -d"
    echo "docker-compose -f scripts/payments/compose.dev.yml up -d"
    echo
    echo "# Run examples:"
    echo "cargo run -p integration-tests --bin scenario_wallet_create $USER_ID"
    echo "cargo run -p integration-tests --bin scenario_deposits_list $USER_ID"
    echo "cargo run -p integration-tests --bin scenario_credits_apply $USER_ID $CREDIT_AMOUNT"
    echo "cargo run -p integration-tests --bin scenario_balance_check $USER_ID"
    echo
    echo "# Manual TAO deposit flow:"
    echo "1. Run wallet creation to get deposit address"
    echo "2. Send TAO to that address using your wallet"
    echo "3. Run deposits list to see the transaction"
    echo "4. Run balance check to see credited amount"
fi

echo "=== Test Complete ==="
