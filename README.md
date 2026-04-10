# OpenClawHAOSAddon-Rust Official Thin

Parallel scheme-B workspace for a minimal Home Assistant OS add-on that stays close to the official OpenClaw runtime model.

## Goal

- keep OpenClaw Gateway as the real control plane
- keep HAOS-specific logic as a thin wrapper only
- expose the native WebUI and a native CLI terminal, nothing more

## Planned shape

- `addon-supervisor`: bootstrap, durable paths, runtime public artifacts, certs, token file, process supervision
- `ingressd`: HA ingress + HTTPS proxy + native CLI terminal
- `oc-config`: config helper for `openclaw.json`

Not included in this workspace:

- custom Rust dashboard UI (`haos-ui`)

The HA ingress root should proxy the official OpenClaw Control UI instead of a local replacement panel, and the wrapper should stick to `/healthz` and `/readyz` rather than inventing extra readiness semantics. Runtime-downloaded artifacts like `gateway.token` and `openclaw-ca.crt` now live in a neutral public runtime directory instead of an nginx-specific path.
