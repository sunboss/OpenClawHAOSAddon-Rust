# Maintainer Context

This file is the handoff memory for future edits to `OpenClawHAOSAddon-Rust`.
Read this before making UI, runtime, or release changes.

## Project intent

- This repository rewrites the Home Assistant add-on layer in Rust.
- Upstream `openclaw` and `mcporter` are intentionally not rewritten.
- The add-on must feel native inside HAOS, not like an OpenWrt web panel.

## Versioning

- Add-on version format must be `YYYY.MM.DD.N`.
- Every pushed fix increments `N`.
- Always tell the user the version number and commit hash when pushing.
- When reporting a push, include the validation log summary too.
  - At minimum: what checks ran and whether they passed.

## Current runtime architecture

- `crates/addon-supervisor`
  - bootstraps config
  - writes runtime env
  - launches and supervises local services
- `crates/ingressd`
  - HA ingress
  - external HTTPS gateway proxy
  - browser terminal transport
- `crates/actiond`
  - local actions such as managed gateway restart
- `crates/haos-ui`
  - multi-page HAOS UI

## Important behavioral decisions

- The managed OpenClaw process is the foreground `openclaw gateway run` process.
- Startup self-heal should run `openclaw doctor --fix`.
  - This is intentional so config/runtime migrations such as `x_search` / Firecrawl changes do not depend on manual repair.
- Do not use `openclaw gateway restart` for the add-on restart button.
  - In this containerized setup that command prints guidance and does not restart
    the supervisor-managed foreground gateway process.
- The restart button must use the local action endpoint:
  - `POST http://127.0.0.1:48100/action/restart`
- `OpenClaw runtime` must be based on the real gateway PID written under:
  - `/run/openclaw-rs/openclaw-gateway.pid`
  - fallback `/run/openclaw-rs/openclaw-node.pid`
- If uptime does not reset after restart, the restart path is wrong.

## Native Gateway status

- The severe native Gateway `ws closed before connect` problem was largely fixed by:
  - preserving forwarded headers
  - allowing the correct control UI origins
  - opening the native dashboard with `#token=...`
- Embedded terminal and native Gateway are separate paths.
  - If embedded terminal works, that does not automatically mean native Gateway works.

## Known noisy logs

- `No pending device pairing requests to approve`
  - not an error
  - just the auto-approve poller finding nothing to approve
- `Health check failed: Error: gateway timeout after 10000ms` (in doctor output)
  - not an error
  - doctor runs 15s after boot; CLI WebSocket needs acpx runtime (ready in 90-120s)
  - health check always times out on startup doctor run; does not abort doctor
- `Gateway port: Port 18790 is already in use` (in doctor output)
  - not an error; expected — doctor detects the supervisor-managed gateway is running
- `systemd user services are unavailable` (in doctor output)
  - not an error; container has no systemd, gateway runs under our supervisor instead
- `Memory search is enabled, but no embedding provider is ready` (in doctor output)
  - not an error unless user wants memory search; requires configuring an embedding provider
- `amazon-bedrock failed to load`
- `Cannot find module '@slack/web-api'`
  - optional plugin dependency noise from upstream OpenClaw
  - usually not a primary add-on failure

## UI direction

- The UI should feel coordinated with Home Assistant.
- Keep the soft gradient / glow background if it still looks clean.
- Avoid obvious OpenWrt-style visual language in the header.
- Use Chinese for user-facing UI copy.
- Command labels can be Chinese, but actual executed commands stay in English.
- User-facing text should explain what to do, not internal architecture rationale.

## Current page structure

- `Home`
  - status overview
  - resource overview
  - concise quick entry points only
- `Config`
  - what the add-on manages
  - persistent directories
  - capability status
- `Commands`
  - operational buttons
  - embedded terminal
- `Logs`
  - log/doctor actions
  - log terminal

## Pending recurring cleanup themes

- Remove duplicated summary blocks if they repeat the same data.
- Prefer one clear source of truth per page.
- PID display should read like status badges, not generic pills.
- Do not keep fake controls that do nothing.
  - Example: the old fake log filter row (`source / lines / time range / keyword`) should stay removed.
- If a button cannot do real work reliably, replace it with guidance instead of a fake action.

## Command-page expectations

- `Check npm version`
  - should run a real version query
  - expected command: `npm view openclaw version`
- `Approve authorization`
  - only makes sense when there is a pending pairing request
  - otherwise prefer a user hint pointing to the command page:
    - `openclaw devices list`
    - `openclaw devices approve --latest`

## Terminal rendering

- The embedded terminal previously rendered ANSI/TUI output poorly.
- The terminal was upgraded to handle more complete ANSI/TUI behavior.
- If output becomes garbled again, check terminal rendering before blaming Chinese text.
- `新窗口打开终端` should behave like a native terminal page.
  - Do not depend on a separate input box at the bottom.
  - The page itself should take focus and accept direct keyboard input and paste.

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
