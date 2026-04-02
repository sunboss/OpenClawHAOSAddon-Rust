# OpenClawHAOSAddon-Rust

Rust rewrite project for the Home Assistant OpenClaw add-on.

This project is intentionally separate from the current production repository.
The goal is to replace our local Python/Bash/UI layer with Rust components while
continuing to consume upstream `openclaw` and `mcporter`.

## Crates

- `haos-ui`: Rust-rendered control page
- `actiond`: local action API for gateway and diagnostics
- `oc-config`: JSON config helper for `openclaw.json`
- `ingressd`: Rust ingress, browser terminal, and external HTTPS gateway proxy replacing `nginx` + `ttyd`
- `addon-supervisor`: runtime orchestrator replacing shell startup glue

## Status

- UI shell: working
- Action server: working
- Config helper: working
- Supervisor: handles startup bootstrap, certificate/token prep, backup sync, `openclaw gateway run` / `openclaw node run`, and supervision of `haos-ui`, `actiond`, and `ingressd`
- Add-on wrapper: `Dockerfile`, `config.yaml`, `build.yaml`, and a thin fallback `run.sh` remain, but the container default entry now goes straight to `addon-supervisor haos-entry`

## Repository shape

- `repository.yaml`: Home Assistant custom repository metadata
- `config.yaml`: HA add-on manifest for the Rust rewrite project
- `Dockerfile`: builds the Rust binaries and bundles upstream `openclaw`
- `run.sh`: ultra-thin compatibility wrapper; no longer the primary startup path
