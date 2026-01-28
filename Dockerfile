# Use the latest stable Rust version
FROM rust:latest as builder
WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y pkg-config libssl-dev libsqlite3-dev

# Copy source code
COPY . .

# Build release binaries
RUN cargo build --release --workspace

# Runtime stage
FROM debian:trixie-slim

# Install runtime dependencies including Chromium
RUN apt-get update && apt-get install -y \
    chromium \
    ca-certificates \
    libssl3 \
    libsqlite3-0 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binaries
COPY --from=builder /app/target/release/server /app/bin/server
COPY --from=builder /app/target/release/crawler /app/bin/crawler

# Copy assets
COPY assets /app/assets

# Set entrypoint
CMD ["/app/bin/server"]