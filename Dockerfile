# syntax=docker/dockerfile:1.7
#
# Minimal static image for the Rust Tool Index server. There is no database,
# no headless browser, and no native extensions: just the compiled `server`
# binary plus the committed `data/` (TOML source of truth) and `static/`
# assets baked in. A merged metrics PR rebuilds this image and redeploys.

FROM lukemathwalker/cargo-chef:latest-rust-1.94.0-slim-trixie AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json
RUN touch -t 197001010000 recipe.json

FROM chef AS builder
# utoipa-swagger-ui's build script downloads the Swagger UI assets and shells
# out to `curl`, which isn't in the slim chef image. Install it for the build.
RUN apt-get update && apt-get install -y --no-install-recommends curl \
    && rm -rf /var/lib/apt/lists/* /var/cache/apt/*
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release -p server

FROM debian:trixie-slim AS runtime
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/* /var/cache/apt/*

RUN useradd -m -u 1000 -U -s /bin/bash appuser
WORKDIR /app

COPY --chown=appuser:appuser static /app/static
COPY --chown=appuser:appuser data /app/data
COPY --from=builder --chown=appuser:appuser /app/target/release/server /app/bin/server

ENV PORT=3000 \
    DATA_DIR=/app/data

USER appuser
EXPOSE 3000

CMD ["/app/bin/server"]
