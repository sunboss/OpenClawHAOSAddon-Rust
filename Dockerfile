FROM rust:1.94-bookworm AS builder

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --workspace

FROM node:24-bookworm-slim

ARG TARGETARCH
ARG OPENCLAW_VERSION=2026.4.14
ARG TTYD_VERSION=1.7.7
ARG BUILD_VERSION=dev
ARG BUILD_ARCH=amd64
ARG BUILD_DATE=unknown
ARG BUILD_REF=unknown
ENV ADDON_VERSION=${BUILD_VERSION}

LABEL \
  io.hass.type="addon" \
  io.hass.version="${BUILD_VERSION}" \
  io.hass.arch="${BUILD_ARCH}" \
  io.hass.name="OpenClawHAOSAddon-Rust" \
  io.hass.description="Rust rewrite of the local HAOS add-on layer while keeping upstream OpenClaw runtimes unchanged." \
  org.opencontainers.image.title="OpenClawHAOSAddon-Rust" \
  org.opencontainers.image.description="Rust rewrite of the local HAOS add-on layer while keeping upstream OpenClaw runtimes unchanged." \
  org.opencontainers.image.version="${BUILD_VERSION}" \
  org.opencontainers.image.created="${BUILD_DATE}" \
  org.opencontainers.image.revision="${BUILD_REF}"

RUN apt-get update && apt-get install -y --no-install-recommends \
    bash \
    ca-certificates \
    curl \
    git \
    jq \
    openssl \
    python3 \
    python3-pip \
    python3-venv \
    python-is-python3 \
    procps \
    iproute2 \
    rsync \
    tzdata \
    xz-utils \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

RUN set -eux; \
    case "${TARGETARCH}" in \
      amd64) ttyd_arch="x86_64" ;; \
      aarch64|arm64) ttyd_arch="aarch64" ;; \
      *) echo "unsupported TARGETARCH for ttyd: ${TARGETARCH}"; exit 1 ;; \
    esac; \
    curl -fsSL "https://github.com/tsl0922/ttyd/releases/download/${TTYD_VERSION}/ttyd.${ttyd_arch}" -o /usr/local/bin/ttyd; \
    chmod +x /usr/local/bin/ttyd

RUN npm config set fund false && npm config set audit false \
    && npm install -g pnpm mcporter openclaw@${OPENCLAW_VERSION} @xterm/xterm @xterm/addon-fit

# Pre-install all openclaw bundled plugin deps into openclaw's own node_modules so that
# `openclaw doctor --fix` reports them as already installed (no per-startup npm download).
# Packages are installed with --ignore-scripts to avoid native-build failures on ARM;
# jiti lazy-loads these at runtime so missing native addons degrade gracefully.
# Source: `openclaw doctor` "Bundled plugin runtime deps are missing" output (v2026.4.8).
RUN cd /usr/local/lib/node_modules/openclaw \
    && npm install --no-save --ignore-scripts \
      "@aws-sdk/client-s3@3.1024.0" \
      "@aws-sdk/s3-request-presigner@3.1024.0" \
      "@aws/bedrock-token-generator@^1.1.0" \
      "@azure/identity@^4.9.1" \
      "@buape/carbon@0.14.0" \
      "@clawdbot/lobster@2026.1.24" \
      "@discordjs/opus@^0.10.0" \
      "@discordjs/voice@^0.19.2" \
      "@grammyjs/runner@^2.0.3" \
      "@grammyjs/transformer-throttler@^1.2.1" \
      "@grammyjs/types@^3.26.0" \
      "@lancedb/lancedb@^0.27.2" \
      "@larksuiteoapi/node-sdk@^1.60.0" \
      "@microsoft/teams.api@2.0.6" \
      "@microsoft/teams.apps@2.0.6" \
      "@opentelemetry/api@^1.9.1" \
      "@opentelemetry/api-logs@^0.214.0" \
      "@opentelemetry/exporter-logs-otlp-proto@^0.214.0" \
      "@opentelemetry/exporter-metrics-otlp-proto@^0.214.0" \
      "@opentelemetry/exporter-trace-otlp-proto@^0.214.0" \
      "@opentelemetry/resources@^2.6.1" \
      "@opentelemetry/sdk-logs@^0.214.0" \
      "@opentelemetry/sdk-metrics@^2.6.1" \
      "@opentelemetry/sdk-node@^0.214.0" \
      "@opentelemetry/sdk-trace-base@^2.6.1" \
      "@opentelemetry/semantic-conventions@^1.40.0" \
      "@pierre/diffs@1.1.10" \
      "@pierre/theme@0.0.29" \
      "@slack/bolt@^4.6.0" \
      "@slack/web-api@^7.15.0" \
      "@snazzah/davey@^0.1.11" \
      "@tloncorp/tlon-skill@0.3.2" \
      "@twurple/api@^8.1.3" \
      "@twurple/auth@^8.1.3" \
      "@twurple/chat@^8.1.3" \
      "@urbit/aura@^3.0.0" \
      "@whiskeysockets/baileys@7.0.0-rc.9" \
      "acpx@0.5.2" \
      "discord-api-types@^0.38.44" \
      "fake-indexeddb@^6.2.5" \
      "grammy@^1.42.0" \
      "jimp@^1.6.0" \
      "jwks-rsa@^4.0.1" \
      "mpg123-decoder@^1.0.3" \
      "music-metadata@^11.12.3" \
      "nostr-tools@^2.23.3" \
      "opusscript@^0.1.1" \
      "silk-wasm@^3.7.1" \
      "zca-js@2.1.2"

COPY --from=builder /src/target/release/addon-supervisor /usr/local/bin/addon-supervisor
COPY --from=builder /src/target/release/haos-ui /usr/local/bin/haos-ui
COPY --from=builder /src/target/release/ingressd /usr/local/bin/ingressd
COPY --from=builder /src/target/release/oc-config /usr/local/bin/oc-config

COPY config.yaml /etc/openclaw-addon-config.yaml

RUN mkdir -p /run/nginx /run/openclaw-rs/public /config /share

CMD ["addon-supervisor", "haos-entry"]
