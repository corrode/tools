FROM rust:latest as builder
WORKDIR /app

RUN apt-get update
RUN apt-get install -y pkg-config libssl-dev libsqlite3-dev

COPY . .
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

# Create a non-root user
RUN useradd -m -u 1000 -U -s /bin/bash appuser

WORKDIR /app

# Create data directory and set permissions
RUN mkdir -p /app/data && chown -R appuser:appuser /app/data

# Copy binaries and assets
COPY --from=builder --chown=appuser:appuser /app/target/release/server /app/bin/server
COPY --from=builder --chown=appuser:appuser /app/target/release/crawler /app/bin/crawler
COPY --chown=appuser:appuser assets /app/assets

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