#!/bin/bash

# BDD Scenarios Test Runner
# This script provides a convenient way to run BDD scenarios with proper configuration

set -e

USER_ID=${1:-"test-user-$(date +%s)"}
CREDIT_AMOUNT=${2:-"100.0"}

echo "=== Basilica BDD Scenarios Test Runner ==="
echo "User ID: $USER_ID"
echo "Credit Amount: $CREDIT_AMOUNT"
echo

# Export environment variables for tests
export TEST_USER_ID="$USER_ID"
export TEST_CREDIT_AMOUNT="$CREDIT_AMOUNT"

# Check if environment variables are set
if [ -n "$PAYMENTS_ENDPOINT" ]; then
    echo "Payments endpoint: $PAYMENTS_ENDPOINT"
fi
if [ -n "$BILLING_ENDPOINT" ]; then
    echo "Billing endpoint: $BILLING_ENDPOINT"
fi
echo

echo "Running BDD scenarios as integration tests..."
echo

# Run the complete flow test
echo "1. Running complete payment and billing flow test..."
cargo test -p integration-tests --test bdd_payment_billing_flow test_complete_payment_billing_flow -- --nocapture --test-threads=1

echo
echo "2. Running individual scenario tests..."

# Run wallet creation test
cargo test -p integration-tests --test bdd_payment_billing_flow test_wallet_creation_scenario -- --nocapture --test-threads=1

# Run balance operations test  
cargo test -p integration-tests --test bdd_payment_billing_flow test_balance_operations_scenario -- --nocapture --test-threads=1

# Run deposits listing test
cargo test -p integration-tests --test bdd_payment_billing_flow test_deposits_listing_scenario -- --nocapture --test-threads=1

echo
echo "✓ All BDD scenario tests completed"
echo

echo "=== Alternative: Run individual scenario unit tests ==="
echo "To run individual scenario unit tests:"
echo "  cargo test -p integration-tests payments::tests::test_wallet_create_scenario -- --nocapture"
echo "  cargo test -p integration-tests payments::tests::test_deposits_list_scenario -- --nocapture"
echo "  cargo test -p integration-tests billing::tests::test_balance_check_scenario -- --nocapture"
echo "  cargo test -p integration-tests billing::tests::test_credits_apply_scenario -- --nocapture"
echo

echo "=== Test Setup Information ==="
echo "To test with live services, ensure the following are running:"
echo "  Payments service on: \${PAYMENTS_ENDPOINT:-http://localhost:50061}"
echo "  Billing service on: \${BILLING_ENDPOINT:-http://localhost:50051}"
echo
echo "Example with custom endpoints:"
echo "  PAYMENTS_ENDPOINT=https://api.basilica.ai/payments \\"
echo "  BILLING_ENDPOINT=https://api.basilica.ai/billing \\"
echo "  $0 production-user-123 50.0"

echo "=== Test Complete ==="