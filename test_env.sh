#!/bin/bash

echo "Testing environment variable override..."

# Test 1: Default config
echo "1. Default config:"
../../target/release/basilica ls 2>&1 | grep -o "https://api.basilica.ai\|http://localhost:8000" | head -1

# Test 2: With double underscore
echo "2. With BASILICA_API__BASE_URL:"
env BASILICA_API__BASE_URL=http://localhost:8000 ../../target/release/basilica ls 2>&1 | grep -o "https://api.basilica.ai\|http://localhost:8000\|401 Unauthorized" | head -1

# Test 3: Check what Auth0 domain is being used
echo "3. Auth0 domain being used:"
../../target/release/basilica login --device-code 2>&1 | grep -o "https://[^/]*\.auth0\.com" | head -1