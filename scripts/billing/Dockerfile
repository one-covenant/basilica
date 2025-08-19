# Build stage
FROM rustlang/rust:nightly-slim AS builder

# Install build dependencies and rustfmt
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && rustup component add rustfmt

WORKDIR /app

# Copy workspace files but only needed crates
COPY Cargo.toml Cargo.lock ./
COPY crates/basilica-billing ./crates/basilica-billing
COPY crates/basilica-common ./crates/basilica-common
COPY crates/basilica-protocol ./crates/basilica-protocol
COPY crates/basilica-payments ./crates/basilica-payments
COPY crates/bittensor ./crates/bittensor

# Remove problematic workspace members from Cargo.toml
RUN sed -i '/crates\/basilica-executor/d' Cargo.toml && \
    sed -i '/crates\/basilica-miner/d' Cargo.toml && \
    sed -i '/crates\/basilica-validator/d' Cargo.toml && \
    sed -i '/crates\/basilica-api/d' Cargo.toml && \
    sed -i '/crates\/basilica-cli/d' Cargo.toml && \
    sed -i '/crates\/collateral/d' Cargo.toml && \
    sed -i '/crates\/integration-tests/d' Cargo.toml

# Build the billing service
RUN cargo build --release -p basilica-billing

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    libpq5 \
    wget \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the binary from builder to standard location
COPY --from=builder /app/target/release/basilica-billing /usr/local/bin/basilica-billing

# Create non-root user
RUN useradd -m -u 1000 billing

USER billing

# Expose ports
EXPOSE 50051 8080 9090

# Set the entrypoint
ENTRYPOINT ["/usr/local/bin/basilica-billing"]