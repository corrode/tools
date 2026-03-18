FROM rust:latest AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json
# Ensure that `COPY --from=planner` relies purely on file content for caching.
RUN touch -t 197001010000 recipe.json

FROM chef AS builder
# Install build dependencies
RUN apt-get update && apt-get install -y pkg-config libssl-dev libsqlite3-dev

COPY --from=planner /app/recipe.json recipe.json
# Docker caching 
RUN cargo chef cook --release --recipe-path recipe.json

# Build application
COPY . .
RUN cargo build --release --workspace

# Compile the spellfix1 SQLite extension as a shared library
RUN apt-get update && apt-get install -y libsqlite3-dev && rm -rf /var/lib/apt/lists/* \
 && cc -fPIC -shared -o ext/spellfix.so ext/spellfix.c -I/usr/include \
 && echo "spellfix.so built OK"

# Runtime stage
FROM debian:trixie-slim AS runtime

# Install runtime dependencies
# Chromium is required for the crawler
# curl is required for health checks
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

# Copy binaries, assets, and the spellfix extension
COPY --from=builder --chown=appuser:appuser /app/target/release/server /app/bin/server
COPY --from=builder --chown=appuser:appuser /app/target/release/crawler /app/bin/crawler
COPY --from=builder --chown=appuser:appuser /app/ext/spellfix.so /app/ext/spellfix.so
COPY --chown=appuser:appuser static /app/static

# Set environment variables
ENV PORT=3000
ENV DATA_DIR=/app/data
ENV CHROME_NO_SANDBOX=true
ENV SPELLFIX_PATH=/app/ext/spellfix

# Switch to non-root user
USER appuser

# Expose the port
EXPOSE 3000

# Set entrypoint
CMD ["/app/bin/server"]