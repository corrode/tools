# syntax=docker/dockerfile:1
FROM rust:latest as builder
WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y pkg-config libssl-dev libsqlite3-dev

# Copy source code
COPY . .

# Build with cache mounting
# This replaces cargo-chef by letting Docker manage the cache volumes for cargo dependencies and build artifacts.
# We mount the cargo registry (for downloaded crates) and the target directory (for compiled artifacts).
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --workspace && \
    # We must copy the binaries out of the cache mount to preserve them in the image layer
    mkdir -p /out && \
    cp target/release/server /out/server && \
    cp target/release/crawler /out/crawler

# Runtime stage
FROM debian:trixie-slim AS runtime

# Install runtime dependencies including Chromium
RUN apt-get update && apt-get install -y \
    chromium \
    ca-certificates \
    libssl3 \
    libsqlite3-0 \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Create a non-root user
RUN useradd -m -u 1000 -U -s /bin/bash appuser

WORKDIR /app

# Create data directory and set permissions
RUN mkdir -p /app/data && chown -R appuser:appuser /app/data

# Copy binaries and assets
# Note: We copy from /out since the builder's target directory was a cache mount and not persisted
COPY --from=builder --chown=appuser:appuser /out/server /app/bin/server
COPY --from=builder --chown=appuser:appuser /out/crawler /app/bin/crawler
COPY --chown=appuser:appuser static /app/static

# Set environment variables
ENV PORT=3000
ENV DATA_DIR=/app/data
ENV CHROME_NO_SANDBOX=true

# Switch to non-root user
USER appuser

# Expose the port
EXPOSE 3000

# Set entrypoint
CMD ["/app/bin/server"]