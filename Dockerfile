# syntax=docker/dockerfile:1.7

FROM lukemathwalker/cargo-chef:latest-rust-1.94.0-slim-trixie AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json
# Ensure that `COPY --from=planner` relies purely on file content for caching.
RUN touch -t 197001010000 recipe.json

FROM chef AS builder
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config libssl-dev libsqlite3-dev cmake git g++ curl ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

COPY . .
RUN cargo build --release --workspace

# Compile the spellfix1 SQLite extension as a shared library
RUN cc -fPIC -shared -o ext/spellfix.so ext/spellfix.c -I/usr/include

# whisper.cpp builder (isolated so its toolchain doesn't leak into runtime)
FROM debian:trixie-slim AS whisper-builder
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates git cmake g++ make \
    && rm -rf /var/lib/apt/lists/*
RUN git clone --depth 1 https://github.com/ggerganov/whisper.cpp.git /tmp/whisper.cpp \
    && cd /tmp/whisper.cpp \
    && cmake -B build -DCMAKE_BUILD_TYPE=Release \
    && cmake --build build --config Release --target whisper-cli -j

FROM debian:trixie-slim AS runtime

ENV DEBIAN_FRONTEND=noninteractive

# Runtime dependencies. `--no-install-recommends` saves >150 MB on chromium's
# recommended packages alone.
RUN apt-get update && apt-get install -y --no-install-recommends \
        chromium \
        ca-certificates \
        libssl3 \
        libsqlite3-0 \
        curl \
        ffmpeg \
        fonts-liberation \
    # Vulkan validation layer is a dev/debug aid only (~22 MB).
    && rm -f /usr/lib/chromium/libVkLayer_khronos_validation.so \
    && rm -rf /var/lib/apt/lists/* /var/cache/apt/*

# Create a non-root user
RUN useradd -m -u 1000 -U -s /bin/bash appuser

WORKDIR /app
RUN mkdir -p /app/data && chown -R appuser:appuser /app/data

# Download the whisper model directly in the runtime stage. Because this RUN
# step is cache-keyed on the command string alone, the resulting layer blob
# hash is stable across rebuilds, so the Coolify host only downloads this
# ~548 MB layer once instead of on every deploy.
ARG WHISPER_MODEL_URL=https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin
RUN curl --fail --location --silent --show-error \
        -o /usr/local/share/ggml-large-v3-turbo-q5_0.bin \
        "${WHISPER_MODEL_URL}"

COPY --from=whisper-builder /tmp/whisper.cpp/build/bin/whisper-cli /usr/local/bin/whisper-cli

COPY --from=builder --chown=appuser:appuser /app/ext/spellfix.so /app/ext/spellfix.so
COPY --chown=appuser:appuser static /app/static
COPY --from=builder --chown=appuser:appuser /app/target/release/server /app/bin/server
COPY --from=builder --chown=appuser:appuser /app/target/release/crawler /app/bin/crawler

ENV PORT=3000 \
    DATA_DIR=/app/data \
    CHROME_NO_SANDBOX=true \
    SPELLFIX_PATH=/app/ext/spellfix

USER appuser
EXPOSE 3000

CMD ["/app/bin/server"]
