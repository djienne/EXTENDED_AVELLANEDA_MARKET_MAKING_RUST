# Multi-stage build for Extended DEX Market Making Bot
# Stage 1: Builder
FROM rust:1.91-bookworm AS builder

# Install build dependencies (following INSTALL.md system dependencies)
RUN apt-get update && apt-get install -y \
    build-essential \
    pkg-config \
    libssl-dev \
    python3 \
    python3-pip \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create app directory
WORKDIR /app

# Copy Python requirements first (following INSTALL.md Step 3.1)
COPY requirements.txt ./
RUN pip3 install -r requirements.txt --break-system-packages

# Copy and install python_sdk-starknet (following INSTALL.md Step 3.2)
COPY python_sdk-starknet ./python_sdk-starknet
RUN cd python_sdk-starknet && pip3 install -e . --break-system-packages && cd ..

# Copy Cargo files for dependency caching
COPY Cargo.toml Cargo.lock ./

# Copy source code
COPY src ./src
COPY examples ./examples

# Copy Python signing script
COPY scripts ./scripts

# Build release binary (following INSTALL.md Step 4.3 - cargo build --release)
RUN cargo build --release --bin market_maker_bot

# Stage 2: Runtime
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    python3 \
    python3-pip \
    && rm -rf /var/lib/apt/lists/*

# Copy Python requirements and install (following INSTALL.md)
COPY requirements.txt ./
RUN pip3 install -r requirements.txt --break-system-packages --no-cache-dir

# Copy and install python_sdk-starknet
COPY python_sdk-starknet ./python_sdk-starknet
RUN cd python_sdk-starknet && pip3 install -e . --break-system-packages --no-cache-dir && cd ..

# Create app user for security
RUN useradd -m -u 1000 botuser

# Create app directory
WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/market_maker_bot /app/market_maker_bot

# Copy Python signing script
COPY --from=builder /app/scripts /app/scripts

# Create data directory with proper permissions
RUN mkdir -p /app/data && chown -R botuser:botuser /app

# Switch to non-root user
USER botuser

# Health check (optional - checks if process is running)
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
    CMD pgrep -f market_maker_bot || exit 1

# Run the bot
CMD ["/app/market_maker_bot"]
