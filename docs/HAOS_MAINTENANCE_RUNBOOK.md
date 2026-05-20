# HAOS Maintenance Runbook

This runbook captures the working process used on 2026-05-20 for the OpenClaw
HA add-on upgrade and cleanup.

## Production repository

Use this repository for the add-on currently installed on the user's HAOS host:

```text
https://github.com/sunboss/openclaw-ha-addon
```

The current Rust rewrite repository is:

```text
https://github.com/sunboss/OpenClawHAOSAddon-Rust
```

Do not mix them up. The production HAOS UI showed the old add-on name:

```text
OpenClaw HA Add-on
slug: 3dc2fc14_openclaw_ha_addon
repository: 3dc2fc14
```

## Version rules

- Add-on version format: `YYYY.MM.DD.N`
- Increment `N` for every pushed fix on the same day.
- Record the commit, CI URL, and HAOS verification result in
  `docs/OPERATION_LOG.md`.

## Upgrade checklist

1. Check upstream OpenClaw release.
2. Update production add-on metadata and build logic.
3. Push to `sunboss/openclaw-ha-addon`.
4. Wait for GitHub Actions:
   - `meta`
   - `linux/amd64`
   - `linux/arm64`
   - `manifest`
5. Verify GHCR manifest exists.
6. On HAOS:
   - `ha store reload`
   - `ha apps info 3dc2fc14_openclaw_ha_addon --raw-json`
   - confirm `version_latest`
   - `ha apps update 3dc2fc14_openclaw_ha_addon`
7. Verify:
   - `version == version_latest`
   - `update_available == false`
   - container image tag matches the release
   - gateway returns `HTTP/2 200`
   - logs do not show fresh security or OAuth loops

## Known HAOS commands

```sh
ha apps info 3dc2fc14_openclaw_ha_addon --raw-json
ha apps logs 3dc2fc14_openclaw_ha_addon --lines 120
ha store reload
ha apps update 3dc2fc14_openclaw_ha_addon
ha apps restart 3dc2fc14_openclaw_ha_addon
```

Docker checks:

```sh
docker ps --format '{{.ID}} {{.Image}} {{.Names}}' | grep openclaw
docker images --format '{{.Repository}}:{{.Tag}} {{.ID}} {{.Size}}' | grep openclaw
docker manifest inspect ghcr.io/sunboss/openclaw-ha-addon:VERSION
```

Gateway check:

```sh
curl -k -I --max-time 10 https://127.0.0.1:18789/
```

## 2026-05-20 repair details

Production version `2026.05.20.2` was created to harden the Control UI config:

- Force `gateway.controlUi.allowInsecureAuth=false`
- Force `gateway.controlUi.dangerouslyDisableDeviceAuth=false`
- Keep startup normalizer and config writer aligned so the insecure-auth warning
  does not return after restart.

The user's installed runtime config was also repaired in place:

- Stale OpenAI Codex OAuth profiles were backed up and removed.
- `thinkingDefault=adaptive` was changed to `medium`.
- Legacy broken cron job `cc1cf5dc-f611-47a4-925e-5d6339934e9f` was disabled.

## Cron troubleshooting

If logs show:

```text
cron: isolated agent setup timed out before runner start
```

Check cron jobs inside the add-on:

```sh
docker exec addon_3dc2fc14_openclaw_ha_addon openclaw cron list
docker exec addon_3dc2fc14_openclaw_ha_addon openclaw cron show JOB_ID
docker exec addon_3dc2fc14_openclaw_ha_addon openclaw cron disable JOB_ID
```

The 2026-05-20 failing job was:

```text
id: cc1cf5dc-f611-47a4-925e-5d6339934e9f
name: hermes-haos-2min-current-session-reminder
schedule: every 2m
session: session:agent:main:main
status before fix: error
status after fix: disabled
```

This cron was unrelated to the running OpenClaw add-on. It was an old Hermes
project progress reminder and was safe to disable.

## Store detection troubleshooting

If HAOS does not detect a new version:

1. Check Supervisor internet health.
2. Run:

   ```sh
   ha network reload
   ha resolution healthcheck
   ha store reload
   ```

3. Check Supervisor logs for:

   ```text
   GitRepo.pull blocked from execution, no supervisor internet connection
   ```

4. Confirm `ha apps info ... --raw-json` shows the expected `version_latest`.

## Disk cleanup notes

Cleanup performed on 2026-05-20:

- Removed old automatic/manual backups.
- Pruned stopped containers.
- Pruned unused images.
- Pruned Docker builder cache.

Do not delete active large images blindly. Active large images observed included
OpenClaw, Frigate, Hermes Agent, and Home Assistant Core.

Safe checks:

```sh
df -h / /data /addon_configs 2>/dev/null || df -h
docker system df
docker ps --format '{{.Image}} {{.Names}}'
```

## Security note

Do not write tokens, passwords, OAuth refresh tokens, or SSH credentials into any
documentation or git history. If credentials were exposed in chat or logs, treat
them as compromised and rotate them outside this repository.

