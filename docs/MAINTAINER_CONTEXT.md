# Maintainer Context

This file is the durable handoff memory for future edits to `OpenClawHAOSAddon-Rust`.
Read it before changing UI, runtime, release flow, or the HAOS integration layer.

## Project intent

- This repository rewrites the Home Assistant add-on layer in Rust.
- Upstream `openclaw` and `mcporter` are intentionally not rewritten.
- The add-on should feel native inside HAOS while staying close to official OpenClaw behavior.

## Versioning

- Add-on version format must be `YYYY.MM.DD.N`.
- Every pushed fix increments `N`.
- When reporting a push, always include:
  - version number
  - commit hash
  - what validation ran and whether it passed

## Current runtime architecture

- `crates/addon-supervisor`
  - bootstraps config
  - writes runtime env
  - launches and supervises local services
- `crates/ingressd`
  - HA ingress
  - external HTTPS gateway proxy
  - local health/readiness endpoints
- `crates/haos-ui`
  - Hermes-style single-page HAOS shell
  - keeps only the Gateway open path, maintenance Shell, token display, device approval, and a small status block
- `crates/oc-config`
  - JSON helpers for `openclaw.json`

## Important behavioral decisions

- The managed OpenClaw process is the foreground `openclaw gateway run` process.
- For HAOS LAN browser access, keep the official secure-context requirement in mind:
  - remote Control UI over plain `http://<lan-ip>:18789` is rejected because device identity requires HTTPS or localhost secure context
  - in this add-on, external dashboard access should remain `https://<host>:18789`
  - keeping an internal loopback gateway port is acceptable when it is required to preserve remote HTTPS access
- Startup self-heal should run `openclaw doctor --fix`, but only automatically on first install.
  - After the first successful run, do not force `doctor --fix` on every startup.
  - This keeps migrations safe without turning normal boot into a heavy repair path.
- `OpenClaw runtime` must be based on the real gateway PID written under:
  - `/run/openclaw-rs/openclaw-gateway.pid`
  - fallback `/run/openclaw-rs/openclaw-node.pid`

## Probe semantics

- `GET /healthz`
  - liveness only
  - confirms the local ingress path is alive
  - must stay lightweight and unauthenticated
- `GET /readyz`
  - readiness probe for the managed gateway path
  - checks supervisor-managed PID presence first
  - in local mode it also requires `127.0.0.1:$GATEWAY_INTERNAL_PORT` to accept connections
  - the home page and gateway-facing proxy readiness should prefer this endpoint
- `GET /health`
  - JSON wrapper around the same lightweight readiness result
  - do not turn this back into a heavy `openclaw health --json` startup probe
- Keep probe semantics close to official OpenClaw docs:
  - lightweight `healthz` / `readyz` for startup and routing
  - deeper CLI health only when the user explicitly asks for diagnostics

## Config and state boundaries

- Keep moving toward the official `config path / state dir` mental model.
- Current working boundary in this add-on:
  - OpenClaw config file:
    - `/config/.openclaw/openclaw.json`
  - MCPorter config file:
    - `/config/.mcporter/mcporter.json`
    - prefer writing the official persisted config shape directly:
      - `mcpServers.<name>.baseUrl`
      - `mcpServers.<name>.headers`
    - do not rely on startup-time `mcporter config add` flag syntax unless upstream explicitly requires it
  - OpenClaw persistent state root:
    - `/config/.openclaw`
  - Workspace:
    - `/config/.openclaw/workspace`
  - Runtime-only pid files:
    - `/run/openclaw-rs`
  - Shared public runtime files:
    - `/run/openclaw-rs/public`
  - Runtime compile cache:
    - `/var/tmp/openclaw-compile-cache`
  - Certificates:
    - `/config/certs`

## Native Gateway status

- Native Gateway and HA panel are separate paths.
  - If the HA panel loads, that does not automatically mean native Gateway works.
- The most important native Gateway fixes already in place are:
  - preserve forwarded headers
  - allow the correct control UI origins
  - open the native dashboard with `#token=...`
  - keep remote browser access on HTTPS

## Known noisy logs

- `actiond`
  - no longer part of the runtime architecture
  - if it appears in logs or docs again, treat that as regression drift
- `Health check failed: Error: gateway timeout after 10000ms` during startup doctor
  - not a primary add-on failure
  - doctor can race early startup while browser/acpx sidecars are still warming up
- `Gateway port: Port 18790 is already in use` in doctor output
  - expected in the current HTTPS-preserving architecture
- `Memory search is enabled, but no embedding provider is ready`
  - not an error unless the user explicitly wants Memory Search
- optional plugin dependency warnings
  - often upstream plugin noise, not a primary add-on failure

## UI direction

- The UI should feel coordinated with Home Assistant and stay close to the thin Hermes shell model.
- Prefer a light, calm, utility-first page instead of a heavy console or multi-page dashboard.
- Use Chinese for user-facing UI copy.
- Command labels can be Chinese, but executed commands stay in English.
- User-facing copy should explain what to do, not internal architecture rationale.
- The single page must keep:
  - 打开网关
  - 维护 Shell
  - Gateway 状态
  - Gateway Token 显示
  - 待批准设备列表与最新授权确认

## Current page structure

- Single page only
  - Gateway open action
  - Shell open action
  - small Gateway runtime status block
  - token reveal/copy
  - device list / approve-latest actions

## Pending recurring cleanup themes

- Prefer one clear source of truth per behavior.
- Do not keep compatibility layers once the single-page shell no longer links to them.
- Do not add new local control panels when the upstream Gateway or Shell already provides the real surface.

- Group command actions in a way that feels close to official helper flows:
  - `Native entrypoints`
    - `openclaw tui`
    - native Gateway
    - `openclaw onboard`
  - `Health / Status`
    - `curl .../healthz`
    - `curl .../readyz`
    - `openclaw status --deep`
    - `openclaw health --json`
  - `Maintenance`
    - doctor
    - doctor --fix
    - security audit
    - memory status
  - `Logs`
    - `openclaw logs --follow`
    - gateway log tail
- `Check npm version`
  - should run a real version query
  - expected command: `npm view openclaw version`
- Device pairing / approval should stay with native Control UI or upstream TUI flows.
- Do not rebuild a separate HA-only pairing control surface.

## Shell boundary

- The add-on no longer ships its own embedded terminal.
- Commands and logs pages should stay as guidance/reference pages, not become another pseudo-shell.
- If users need a shell, guide them to Home Assistant `Terminal & SSH`, SSH, or another host-local shell.
- Keep the command examples aligned with official upstream flows:
  - `openclaw tui`
  - `openclaw onboard`
  - `openclaw doctor`
  - `openclaw status --deep`

## Workflow note

- Temporary preview HTML files should not stay in the repo root.
- Keep handoff memory in this document instead of relying on conversation history.
- Keep a push-ready operation log in `docs/OPERATION_LOG.md`.
  - Before every push, append a new entry with:
    - date/time
    - user request / conversation intent
    - files changed
    - commands/checks run
    - version + commit hash if already created
    - push target / result
    - next handoff note for the next AI
  - This log is the durable bridge for future AI handoff and external tool calls.
  - Do not rely on chat history being available later.
