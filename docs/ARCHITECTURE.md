# Architecture

## Intent

This workspace follows the official OpenClaw mental model as closely as HAOS allows:

- one real runtime: `openclaw gateway run`
- one persisted OpenClaw config: `/config/.openclaw/openclaw.json`
- one persisted MCPorter config: `/config/.mcporter/mcporter.json`
- standard probes first: `/healthz` and `/readyz`
- no add-on-specific readiness API unless HAOS proves it is strictly necessary

## Thin wrapper boundary

Kept in the add-on layer:

- HA ingress routing
- HTTPS wrapper for LAN access
- persistent path preparation
- cert/token file exposure from a neutral runtime public directory
- native CLI terminal page

Explicitly avoided:

- custom replacement dashboard
- startup-time CLI mutation when a persisted config file is enough
- large local control-plane state machines
- extra helper daemons between HA and the native gateway unless strictly necessary

## Runtime shape

1. `addon-supervisor` seeds config/state directories and starts the gateway
2. `ingressd` proxies HA ingress and HTTPS traffic to the official Control UI
3. the only extra UX layer is a terminal page for running the native `openclaw` CLI inside HAOS

If the gateway is healthy, the add-on should feel like “official OpenClaw inside HAOS”, not like a separate product.
