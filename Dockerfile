FROM rust:1.94-bookworm AS builder

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --workspace

FROM node:24-bookworm-slim

ARG TARGETARCH
ARG OPENCLAW_VERSION=2026.4.1

RUN apt-get update && apt-get install -y --no-install-recommends \
    bash \
    ca-certificates \
    curl \
    jq \
    nginx \
    openssl \
    procps \
    iproute2 \
    rsync \
    tzdata \
    xz-utils \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

RUN ARCH=$(echo ${TARGETARCH:-$(dpkg --print-architecture)} | sed 's|arm64|aarch64|;s|arm/v7|armhf|;s|armv7|armhf|;s|amd64|x86_64|') \
    && curl -fsSL "https://github.com/tsl0922/ttyd/releases/download/1.7.7/ttyd.${ARCH}" -o /usr/local/bin/ttyd \
    && chmod +x /usr/local/bin/ttyd

RUN npm config set fund false && npm config set audit false \
    && npm install -g pnpm mcporter openclaw@${OPENCLAW_VERSION}

COPY --from=builder /src/target/release/actiond /usr/local/bin/actiond
COPY --from=builder /src/target/release/addon-supervisor /usr/local/bin/addon-supervisor
COPY --from=builder /src/target/release/haos-ui /usr/local/bin/haos-ui
COPY --from=builder /src/target/release/oc-config /usr/local/bin/oc-config

COPY config.yaml /etc/openclaw-addon-config.yaml

RUN mkdir -p /run/nginx /etc/nginx/html /config /share

CMD ["addon-supervisor", "haos-entry"]
