# lgtm-agent worker image: a Rust-built binary plus the Claude Code CLI it
# shells out to. Build from the repo root:
#
#   docker build -f docker/agent.Dockerfile -t lgtm-agent .
#
# Run it (see docs/remote-workers.md for the full picture):
#
#   docker run --rm \
#     -e ANTHROPIC_API_KEY \
#     -e LGTM_TOKEN=change-me \
#     lgtm-agent ws://host:4750 --token change-me
#
# Required at runtime: ANTHROPIC_API_KEY, the orchestrator URL as the first
# argument, and a token via LGTM_TOKEN or --token.
# Optional: LGTM_CA, pointing at a mounted PEM, when the orchestrator serves
# a self-signed certificate (mount it, e.g. -v ./cert.pem:/ca.pem:ro -e
# LGTM_CA=/ca.pem).

FROM rust:1-bookworm AS builder
WORKDIR /build

# Only the workspace manifest and crate/app sources are needed to build
# lgtm; the frontend and the Tauri review app are left out. The apps/desktop
# manifest still has to be present for workspace resolution, but building
# -p lgtm-cli never compiles it (or GPUI). This also builds the orchestrator,
# which is fine — only the lgtm binary is copied into the runtime image.
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY apps ./apps

RUN cargo build --release --locked -p lgtm-cli

FROM node:22-bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends git ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN npm install -g @anthropic-ai/claude-code

COPY --from=builder /build/target/release/lgtm /usr/local/bin/lgtm

RUN useradd --create-home --shell /bin/bash lgtm
ENV HOME=/home/lgtm
USER lgtm
WORKDIR /home/lgtm

# The worker exits after finishing its work, expecting whatever started it
# (provisioning script, orchestrator) to clean up the container.
ENV LGTM_EPHEMERAL=1

ENTRYPOINT ["lgtm", "worker"]
