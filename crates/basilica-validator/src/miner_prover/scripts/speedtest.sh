#!/usr/bin/env bash

set -euo pipefail

# Configuration
readonly DOWNLOAD_SIZE_MB=50
readonly UPLOAD_SIZE_MB=15
readonly UPLOAD_TIMEOUT=30
readonly DOWNLOAD_URL="https://speed.cloudflare.com/__down"
readonly UPLOAD_URL="https://speed.cloudflare.com/__up"

# Check for required commands
check_dependencies() {
    local missing_deps=()

    # Check for curl
    if ! command -v curl &> /dev/null; then
        missing_deps+=("curl")
    fi

    # Check for awk
    if ! command -v awk &> /dev/null; then
        missing_deps+=("awk")
    fi

    # Check for date
    if ! command -v date &> /dev/null; then
        missing_deps+=("date")
    fi

    # Check for dd
    if ! command -v dd &> /dev/null; then
        missing_deps+=("dd")
    fi

    if [ ${#missing_deps[@]} -ne 0 ]; then
        echo "Error: Missing required dependencies: ${missing_deps[*]}" >&2
        return 1
    fi
}

# Calculate download speed
download_test() {
    local bytes=$((DOWNLOAD_SIZE_MB * 1024 * 1024))
    local url="${DOWNLOAD_URL}?bytes=${bytes}"

    echo "Running download test (${DOWNLOAD_SIZE_MB}MB)..." >&2

    # Use bash built-in time for better compatibility
    local start_time
    local end_time

    # Get start time in nanoseconds (works on most Linux systems)
    if [ -r /proc/uptime ]; then
        start_time=$(awk '{print $1}' /proc/uptime)
    else
        start_time=$(date +%s.%N 2>/dev/null || date +%s)
    fi

    # Download and verify size
    local received_bytes
    received_bytes=$(curl -sf --max-time 60 "$url" | wc -c)

    # Get end time
    if [ -r /proc/uptime ]; then
        end_time=$(awk '{print $1}' /proc/uptime)
    else
        end_time=$(date +%s.%N 2>/dev/null || date +%s)
    fi

    if [[ "$received_bytes" -ne "$bytes" ]]; then
        echo "Error: Expected $bytes bytes, received $received_bytes" >&2
        return 1
    fi

    # Calculate speed in Mbps
    local elapsed
    local speed_mbps
    elapsed=$(awk "BEGIN {printf \"%.6f\", $end_time - $start_time}")
    speed_mbps=$(awk "BEGIN {printf \"%.2f\", ($bytes * 8) / ($elapsed * 1000000)}")

    echo "$speed_mbps"
}

# Calculate upload speed
upload_test() {
    local bytes=$((UPLOAD_SIZE_MB * 1024 * 1024))

    echo "Running upload test (${UPLOAD_SIZE_MB}MB)..." >&2

    # Generate random data
    local temp_file
    temp_file=$(mktemp /tmp/speedtest.XXXXXX)
    trap "rm -f $temp_file" EXIT

    # Use dd to generate random data (more compatible than /dev/urandom on some systems)
    if [ -r /dev/urandom ]; then
        dd if=/dev/urandom of="$temp_file" bs=1M count="$UPLOAD_SIZE_MB" 2>/dev/null
    else
        # Fallback to /dev/zero if /dev/urandom is not available
        dd if=/dev/zero of="$temp_file" bs=1M count="$UPLOAD_SIZE_MB" 2>/dev/null
    fi

    # Get start time
    local start_time
    local end_time

    if [ -r /proc/uptime ]; then
        start_time=$(awk '{print $1}' /proc/uptime)
    else
        start_time=$(date +%s.%N 2>/dev/null || date +%s)
    fi

    # Upload with timeout
    if curl -sf \
        --max-time "$UPLOAD_TIMEOUT" \
        -X POST \
        -H "Content-Type: application/octet-stream" \
        --data-binary "@$temp_file" \
        "$UPLOAD_URL" > /dev/null; then

        # Get end time
        if [ -r /proc/uptime ]; then
            end_time=$(awk '{print $1}' /proc/uptime)
        else
            end_time=$(date +%s.%N 2>/dev/null || date +%s)
        fi

        local elapsed
        local speed_mbps
        elapsed=$(awk "BEGIN {printf \"%.6f\", $end_time - $start_time}")
        speed_mbps=$(awk "BEGIN {printf \"%.2f\", ($bytes * 8) / ($elapsed * 1000000)}")
        echo "$speed_mbps"
    else
        echo "Upload test failed or timed out" >&2
        echo "0.0"
    fi
}

# Main execution
main() {
    # Check dependencies first
    if ! check_dependencies; then
        exit 1
    fi

    # Run tests
    local download_speed
    local upload_speed

    # Run download test with error handling
    download_speed=$(download_test) || download_speed="0.0"

    # Run upload test with error handling
    upload_speed=$(upload_test) || upload_speed="0.0"

    # Output results in a consistent format
    echo "Download: ${download_speed} Mbps"
    echo "Upload: ${upload_speed} Mbps"
}

# Run if executed directly
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
