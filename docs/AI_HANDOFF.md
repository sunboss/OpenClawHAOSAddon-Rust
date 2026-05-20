# AI Handoff

Read this first when another AI or maintainer pulls the repository.

## What this repository is

`sunboss/OpenClawHAOSAddon-Rust` is the Rust rewrite of the local Home Assistant
OpenClaw add-on layer. It is not the current production add-on that the user's
HAOS installation is looking at.

Current production add-on repository:

- Repository: `https://github.com/sunboss/openclaw-ha-addon`
- HAOS repository id observed on the user's host: `3dc2fc14`
- Installed add-on slug: `3dc2fc14_openclaw_ha_addon`
- Current production add-on version after the 2026-05-20 repair: `2026.05.20.2`
- Production image: `ghcr.io/sunboss/openclaw-ha-addon:2026.05.20.2`

This Rust rewrite repository is useful as a long-term replacement and reference,
but do not assume changes here affect the installed HAOS add-on unless this repo
is explicitly added to the HAOS store.

## 2026-05-20 production state

The installed HAOS add-on was successfully upgraded and verified.

- HAOS add-on state: `started`
- Installed version: `2026.05.20.2`
- Latest version: `2026.05.20.2`
- Update available: `false`
- Gateway check: `curl -k -I https://127.0.0.1:18789/` returned `HTTP/2 200`
- Disk after cleanup: about `57.4G` used, `51.9G` available, `53%`

Production repository commits created during the repair:

- `8bb2830` - upgraded OpenClaw HA add-on to upstream `openclaw/openclaw` `2026.5.18`
- `692ab68` - fixed CI by using `pnpm` for browser extension production deps
- `5301ffc` - hardened HAOS Control UI config and bumped add-on to `2026.05.20.2`

GitHub Actions run for `2026.05.20.2`:

- Run: `https://github.com/sunboss/openclaw-ha-addon/actions/runs/26158357337`
- Result: success
- Jobs: `meta`, `linux/amd64`, `linux/arm64`, `manifest` all succeeded

## Main things that went wrong

1. HAOS initially did not detect the update because the Supervisor had
   `supervisor_internet: false`, and store reloads were blocked.
2. A duplicate Rust rewrite add-on repository had been installed temporarily,
   confusing the visible add-on list. It was uninstalled and removed from the HAOS
   store so the user sees only `OpenClaw HA Add-on`.
3. The installed production add-on had old runtime config drift:
   `gateway.controlUi.allowInsecureAuth=true`, unsupported
   `thinkingDefault=adaptive`, and stale OpenAI Codex OAuth profiles.
4. A legacy cron job named `hermes-haos-2min-current-session-reminder` kept
   failing with `cron: isolated agent setup timed out before runner start`.

## Fixes already applied on the HAOS host

- Upgraded production add-on to `2026.05.20.2`.
- Verified the running image is `ghcr.io/sunboss/openclaw-ha-addon:2026.05.20.2`.
- Removed duplicate Rust rewrite add-on from HAOS.
- Deleted old backups and pruned Docker containers/images/build cache.
- Backed up stale OpenAI Codex OAuth files before removing expired profiles.
- Set agent thinking defaults from `adaptive` to `medium`.
- Set:
  - `gateway.controlUi.allowInsecureAuth=false`
  - `gateway.controlUi.dangerouslyDisableDeviceAuth=false`
- Disabled cron job `cc1cf5dc-f611-47a4-925e-5d6339934e9f`.

## Sensitive information policy

Do not commit secrets. The user previously pasted credentials in chat. They are
not recorded in these docs. If future work needs SSH or GitHub access, obtain it
from the user's secure environment, keychain, or an explicit one-time input.

## Recommended next checks

Run these after pulling the repo or touching release files:

```sh
git status --short --branch
rg -n "2026\\.05\\.20|openclaw|gateway|cron|allowInsecureAuth" docs README.md config.yaml
```

On the HAOS host, verify:

```sh
ha apps info 3dc2fc14_openclaw_ha_addon --raw-json
ha apps logs 3dc2fc14_openclaw_ha_addon --lines 120
curl -k -I https://127.0.0.1:18789/
docker ps --format '{{.ID}} {{.Image}} {{.Names}}' | grep openclaw
docker images --format '{{.Repository}}:{{.Tag}} {{.ID}} {{.Size}}' | grep openclaw
```

For OpenClaw cron state inside the running add-on container:

```sh
docker exec addon_3dc2fc14_openclaw_ha_addon openclaw cron list
docker exec addon_3dc2fc14_openclaw_ha_addon openclaw cron status
```

Expected current cron result: no enabled cron jobs and no next wake.

