FROM rust:1.94-bookworm AS builder

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --workspace

FROM node:24-bookworm-slim

ARG OPENCLAW_VERSION=2026.4.9
ARG BUILD_VERSION=dev
ARG BUILD_ARCH=amd64
ARG BUILD_DATE=unknown
ARG BUILD_REF=unknown
ENV ADDON_VERSION=${BUILD_VERSION}

LABEL \
  io.hass.type="addon" \
  io.hass.version="${BUILD_VERSION}" \
  io.hass.arch="${BUILD_ARCH}" \
  io.hass.name="OpenClawHAOSAddon-Rust Official Thin" \
  io.hass.description="Thin HAOS wrapper around the official OpenClaw gateway and control UI." \
  org.opencontainers.image.title="OpenClawHAOSAddon-Rust Official Thin" \
  org.opencontainers.image.description="Thin HAOS wrapper around the official OpenClaw gateway and control UI." \
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
    procps \
    iproute2 \
    rsync \
    tzdata \
    xz-utils \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

RUN npm config set fund false && npm config set audit false \
    && npm install -g pnpm mcporter openclaw@${OPENCLAW_VERSION} @xterm/xterm @xterm/addon-fit

COPY --from=builder /src/target/release/addon-supervisor /usr/local/bin/addon-supervisor
COPY --from=builder /src/target/release/ingressd /usr/local/bin/ingressd
COPY --from=builder /src/target/release/oc-config /usr/local/bin/oc-config

COPY config.yaml /etc/openclaw-addon-config.yaml

RUN mkdir -p /run/openclaw-rs/public /config /share

CMD ["addon-supervisor", "haos-entry"]
