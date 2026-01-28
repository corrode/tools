FROM ghcr.io/railwayapp/nixpacks:ubuntu-1745885067

ENTRYPOINT ["/bin/bash", "-l", "-c"]
WORKDIR /app/


COPY .nixpacks/nixpkgs-ef56e777fedaa4da8c66a150081523c5de1e0171.nix .nixpacks/nixpkgs-ef56e777fedaa4da8c66a150081523c5de1e0171.nix
RUN nix-env -if .nixpacks/nixpkgs-ef56e777fedaa4da8c66a150081523c5de1e0171.nix && nix-collect-garbage -d
RUN sudo apt-get update && sudo apt-get install -y --no-install-recommends libssl-dev ca-certificates libsqlite3-0
COPY .nixpacks/assets /assets/
ARG NIXPACKS_METADATA ROCKET_ADDRESS
ENV NIXPACKS_METADATA=$NIXPACKS_METADATA ROCKET_ADDRESS=$ROCKET_ADDRESS

# setup phase
COPY . /app/.
RUN  rustup default stable

# build phase
COPY . /app/.
RUN --mount=type=cache,id=IfckFZmxqWo-/root/cargo/git,target=/root/.cargo/git --mount=type=cache,id=IfckFZmxqWo-/root/cargo/registry,target=/root/.cargo/registry . $HOME/.cargo/env && cargo build --release --workspace
RUN --mount=type=cache,id=IfckFZmxqWo-/root/cargo/git,target=/root/.cargo/git --mount=type=cache,id=IfckFZmxqWo-/root/cargo/registry,target=/root/.cargo/registry mkdir -p /app/bin
RUN --mount=type=cache,id=IfckFZmxqWo-/root/cargo/git,target=/root/.cargo/git --mount=type=cache,id=IfckFZmxqWo-/root/cargo/registry,target=/root/.cargo/registry cp target/release/server /app/bin/
RUN --mount=type=cache,id=IfckFZmxqWo-/root/cargo/git,target=/root/.cargo/git --mount=type=cache,id=IfckFZmxqWo-/root/cargo/registry,target=/root/.cargo/registry cp target/release/crawler /app/bin/
RUN --mount=type=cache,id=IfckFZmxqWo-/root/cargo/git,target=/root/.cargo/git --mount=type=cache,id=IfckFZmxqWo-/root/cargo/registry,target=/root/.cargo/registry ls -la /app/bin
RUN --mount=type=cache,id=IfckFZmxqWo-/root/cargo/git,target=/root/.cargo/git --mount=type=cache,id=IfckFZmxqWo-/root/cargo/registry,target=/root/.cargo/registry file /app/bin/server




# start
FROM ubuntu:noble
ENTRYPOINT ["/bin/bash", "-l", "-c"]
WORKDIR /app/
COPY --from=0 /etc/ssl/certs /etc/ssl/certs
RUN true
COPY --from=0 /app/bin/server /app/bin/server

CMD ["/app/bin/server"]

