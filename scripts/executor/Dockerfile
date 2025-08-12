FROM nvidia/cuda:12.6.0-devel-ubuntu22.04 AS builder

# Install Rust and system dependencies
RUN apt-get update && apt-get install -y \
    curl \
    pkg-config \
    libssl-dev \
    build-essential \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.87.0
ENV PATH="/root/.cargo/bin:${PATH}"

# Install rustfmt for code formatting
RUN rustup component add rustfmt

WORKDIR /workspace

# Copy workspace files for dependency resolution
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/

# Pre-build dependencies to cache them
RUN cargo fetch

# Build arguments
ARG BUILD_MODE=release
ARG FEATURES=""
ARG BITTENSOR_NETWORK="finney"
ARG METADATA_CHAIN_ENDPOINT=""

# Set environment variables for the build - default to finney network if not specified
ENV BITTENSOR_NETWORK=${BITTENSOR_NETWORK:-finney}
ENV METADATA_CHAIN_ENDPOINT=${METADATA_CHAIN_ENDPOINT}

# Set environment variables to reduce build size and use less disk space
ENV CARGO_TARGET_DIR=/tmp/target
ENV CARGO_INCREMENTAL=0
ENV RUST_BACKTRACE=1
# Add retry configuration for network operations during build
ENV CARGO_NET_RETRY=10
ENV CARGO_HTTP_TIMEOUT=120

# Build the executor directly (workspace should resolve dependencies)
# Set explicit endpoint if network is specified but endpoint is not
RUN if [ -n "$BITTENSOR_NETWORK" ] && [ -z "$METADATA_CHAIN_ENDPOINT" ]; then \
        case "$BITTENSOR_NETWORK" in \
            test) export METADATA_CHAIN_ENDPOINT="wss://test.finney.opentensor.ai:443" ;; \
            local) export METADATA_CHAIN_ENDPOINT="ws://subtensor:9944" ;; \
            finney|*) export METADATA_CHAIN_ENDPOINT="wss://entrypoint-finney.opentensor.ai:443" ;; \
        esac; \
    fi && \
    if [ "$BUILD_MODE" = "release" ]; then \
        if [ -n "$FEATURES" ]; then \
            cargo build --release -p basilica-executor --features "$FEATURES"; \
        else \
            cargo build --release -p basilica-executor; \
        fi; \
    else \
        if [ -n "$FEATURES" ]; then \
            cargo build -p basilica-executor --features "$FEATURES"; \
        else \
            cargo build -p basilica-executor; \
        fi; \
    fi && \
    # Clean up intermediate build artifacts to save space but keep the binary
    # Keep the binary, only remove build artifacts
    rm -rf /tmp/target/${BUILD_MODE}/deps \
           /tmp/target/${BUILD_MODE}/incremental \
           /tmp/target/${BUILD_MODE}/build \
           /tmp/target/${BUILD_MODE}/.fingerprint \
           /tmp/target/${BUILD_MODE}/examples \
           /tmp/target/${BUILD_MODE}/*.d \
           /tmp/target/${BUILD_MODE}/*.rlib && \
    rm -rf /root/.cargo/registry/cache /root/.cargo/git/db && \
    # Verify binary exists
    ls -la /tmp/target/${BUILD_MODE}/basilica-executor || echo "Binary not found at expected location"

# Runtime image
FROM nvidia/cuda:12.6.0-runtime-ubuntu22.04

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libssl3 \
    ca-certificates \
    curl \
    docker.io \
    && rm -rf /var/lib/apt/lists/*

# Create symlink for NVML library (required for GPU detection)
RUN ln -sf /usr/lib/x86_64-linux-gnu/libnvidia-ml.so.1 /usr/lib/x86_64-linux-gnu/libnvidia-ml.so || true

# Copy the built binary
ARG BUILD_MODE=release
COPY --from=builder /tmp/target/${BUILD_MODE}/basilica-executor /usr/local/bin/basilica-executor

# Make binary executable
RUN chmod +x /usr/local/bin/basilica-executor

EXPOSE 8080 8081

# Default command
ENTRYPOINT ["/usr/local/bin/basilica-executor"]
