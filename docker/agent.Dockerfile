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

# rust:1-bookworm
FROM rust@sha256:82150a52ec202c1b14d7817e14516c392bb7f5cfebd88f1ed531cb37ebd39922 AS builder
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

# node:22-bookworm-slim
FROM node@sha256:83f487e0a63425e5b4d146fb5e5be574bcbe1b7b843d3ebafdd95eaf7767a7e5

RUN apt-get update \
    && apt-get install -y --no-install-recommends git ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN npm install -g @anthropic-ai/claude-code@2.1.251

COPY --from=builder /build/target/release/lgtm /usr/local/bin/lgtm

RUN useradd --create-home --shell /bin/bash lgtm
ENV HOME=/home/lgtm
USER lgtm
WORKDIR /home/lgtm

# The worker exits after finishing its work, expecting whatever started it
# (provisioning script, orchestrator) to clean up the container.
ENV LGTM_EPHEMERAL=1

ENTRYPOINT ["lgtm", "worker"]
