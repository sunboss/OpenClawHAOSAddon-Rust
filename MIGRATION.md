# Migration map

This project is the Rust rewrite for the non-upstream parts of the add-on.

## Old file -> Rust replacement

- `openclaw_assistant/action_server.py`
  - `crates/actiond`

- `openclaw_assistant/oc_config_helper.py`
  - `crates/oc-config`

- `openclaw_assistant/render_nginx.py`
  - split between:
    - `crates/haos-ui`
    - `crates/ingressd`

- `openclaw_assistant/landing.html.tpl`
  - `crates/haos-ui`
  - archived legacy copy remains in the original repo

- `nginx`
  - `crates/ingressd`

- `ttyd`
  - `crates/ingressd`

- `openclaw_assistant/run.sh`
  - primary startup path is now `crates/addon-supervisor`
  - remaining `run.sh` is only a thin compatibility wrapper

- `openclaw_assistant/oc-cleanup.sh`
  - planned future utility crate or subcommand under `addon-supervisor`

- `openclaw_assistant/brew-wrapper.sh`
  - planned future wrapper binary

## Not planned for Rust rewrite

- `openclaw` upstream runtime
- `mcporter`
- Home Assistant add-on YAML metadata
- `openclaw-proxy-shim.cjs` unless upstream Node proxy bootstrap is removed

## Current status

- `actiond`: baseline working
- `oc-config`: baseline working
- `haos-ui`: prototype working
- `addon-supervisor`: startup bootstrap, backup sync, local process supervision, OpenClaw runtime launch, startup doctor, and auto-approve helper now handled in Rust
- `ingressd`: replaces `nginx` and `ttyd` for HA ingress routing, external HTTPS gateway proxying, static helper files, and browser terminal transport
