# Runtime Boundaries

This note keeps the HAOS add-on aligned with the official OpenClaw install docs.

## Probe model

| Endpoint | Meaning | Intended use |
| --- | --- | --- |
| `/healthz` | Liveness only | Confirms ingress / action path is alive |
| `/readyz` | Gateway readiness | Confirms the supervisor-managed gateway is actually ready |
| `/health` | JSON readiness wrapper | For UI / API callers that want structured output |

Rules:

- Keep all three endpoints lightweight.
- Do not call heavy CLI health commands during boot just to answer readiness.
- Startup doctor output is not the same thing as readiness.

## Directory boundary

| Category | Path | Notes |
| --- | --- | --- |
| OpenClaw config file | `/config/.openclaw/openclaw.json` | Main user-visible config entry |
| MCPorter config file | `/config/.mcporter/mcporter.json` | MCP server registration |
| OpenClaw state root | `/config/.openclaw` | Transitional mixed root: config file plus mutable state |
| Workspace | `/config/.openclaw/workspace` | User work/output area |
| Runtime pid dir | `/run/openclaw-rs` | Ephemeral runtime state only |
| Compile cache | `/var/tmp/openclaw-compile-cache` | Rebuild / startup helper cache |
| Certificates | `/config/certs` | Persistent TLS assets |
| Backup root | `/share/openclaw-backup/latest` | Durable export/copy target |

Current interpretation:

- Treat `openclaw.json` and `mcporter.json` as config files.
- Treat sessions, identity, memory, and workspace as state.
- The add-on has not fully split config and state roots yet, so code and docs should describe this as a transition, not as a finished architecture.

## UI grouping

The command page should stay close to the official helper mental model:

- `控制台与配对`
  - OpenClaw CLI
  - devices list
  - approve latest
  - onboard
- `状态与健康`
  - healthz / readyz
  - health --json
  - status --deep
- `维护与审计`
  - doctor
  - doctor --fix
  - security audit
  - memory status
- `配置与状态目录`
  - show config
  - MCP list
  - workspace
  - backup
- `网关控制`
  - restart

This keeps the UI closer to the official `ClawDock` / `Podman` operational split:

- dashboard / shell
- devices / approve
- health / show-config / workspace
- logs / restart
