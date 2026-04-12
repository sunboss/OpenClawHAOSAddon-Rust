# OpenClawHAOSAddon-Rust

Rust rewrite project for the Home Assistant OpenClaw add-on.

This project is intentionally separate from the current production repository.
The goal is to replace our local Python/Bash/UI layer with Rust components while
continuing to consume upstream `openclaw` and `mcporter`.

## Crates

- `haos-ui`: Rust + htmx control page
- `oc-config`: JSON config helper for `openclaw.json`
- `ingressd`: Rust ingress, native TUI terminal transport, external HTTPS gateway proxy, and lightweight health/readiness endpoints replacing `nginx` + `ttyd`
- `addon-supervisor`: runtime orchestrator replacing shell startup glue

## Status

- UI prototype: working baseline
- Config helper: working baseline
- Supervisor: now handles startup bootstrap, certificate/token prep, backup sync, first-install `doctor --fix`, `openclaw gateway run` / `openclaw node run`, and supervision of `haos-ui` and `ingressd`
- Add-on wrapper: `Dockerfile`, `config.yaml`, `build.yaml`, and a thin fallback `run.sh` remain, but the container default entry now goes straight to `addon-supervisor haos-entry`

## Repository shape

- `repository.yaml`: Home Assistant custom repository metadata
- `config.yaml`: HA add-on manifest for the Rust rewrite project
- `Dockerfile`: builds the Rust binaries and bundles upstream `openclaw`
- `run.sh`: ultra-thin compatibility wrapper; no longer the primary startup path
- `docs/MAINTAINER_CONTEXT.md`: persistent handoff notes for future edits and debugging
