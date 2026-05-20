# AI Maintenance Checklist

Use this checklist when taking over maintenance for the OpenClaw HAOS add-on
workstream.

## 1. Orient before editing

- Read `README.md`.
- Read `docs/AI_HANDOFF.md`.
- Read `docs/MAINTAINER_CONTEXT.md`.
- Read the latest entries near the top of `docs/OPERATION_LOG.md`.
- Confirm whether the user is asking about:
  - production add-on: `sunboss/openclaw-ha-addon`
  - Rust rewrite add-on: `sunboss/OpenClawHAOSAddon-Rust`

Do not assume these repositories are interchangeable.

## 2. Protect existing work

- Run `git status --short --branch` before edits.
- Do not overwrite local dirty code changes unless the user explicitly asks.
- If pushing documentation from a temporary clone, compare remote docs first.
- Never rsync an old local `docs/` directory over a fresh clone without checking
  line counts or diffs.

Useful guard commands:

```sh
git status --short --branch
git log --oneline -5
wc -l docs/OPERATION_LOG.md docs/MAINTAINER_CONTEXT.md
git diff --stat
```

## 3. Production HAOS verification

For the current production add-on:

```sh
ha apps info 3dc2fc14_openclaw_ha_addon --raw-json
ha apps logs 3dc2fc14_openclaw_ha_addon --lines 120
curl -k -I --max-time 10 https://127.0.0.1:18789/
docker ps --format '{{.ID}} {{.Image}} {{.Names}}' | grep openclaw
```

Expected after the 2026-05-20 repair:

```text
version: 2026.05.20.2
version_latest: 2026.05.20.2
update_available: false
gateway: HTTP/2 200
```

## 4. Cron check

If logs mention cron setup timeout, check inside the running add-on:

```sh
docker exec addon_3dc2fc14_openclaw_ha_addon openclaw cron list
docker exec addon_3dc2fc14_openclaw_ha_addon openclaw cron status
```

Expected current state:

```text
No enabled cron jobs
nextWakeAtMs: null
```

The obsolete job disabled on 2026-05-20:

```text
cc1cf5dc-f611-47a4-925e-5d6339934e9f
hermes-haos-2min-current-session-reminder
```

## 5. OAuth/auth check

Old OpenAI Codex OAuth profiles were invalid and removed after backup. If the
user needs agent turns through OpenClaw again, they must re-authenticate through
the UI. Do not try to revive expired refresh tokens from docs, chat, or backups.

## 6. Release/push checklist

Before pushing any release:

1. Update the appropriate repository, not both by accident.
2. Increment add-on version using `YYYY.MM.DD.N`.
3. Record the planned change in `docs/OPERATION_LOG.md`.
4. Run the smallest meaningful validation.
5. Push.
6. Record commit hash, CI URL, and validation result.
7. If HAOS is involved, verify `version`, `version_latest`,
   `update_available`, container image tag, and gateway health.

## 7. Documentation package checklist

When asked to package docs:

1. Include all `*.md`, `*.yaml`, and `*.yml` files, excluding `.git/`,
   generated build outputs, and old archives.
2. Include a file list in the archive.
3. Exclude secrets and runtime token files.
4. Regenerate both `.zip` and `.tar.gz`.
5. Verify archive contents with:

```sh
unzip -l docs/archives/openclaw-haos-maintenance-docs-*.zip
tar -tzf docs/archives/openclaw-haos-maintenance-docs-*.tar.gz
```

## 8. Secret hygiene

Never write these to git or docs:

- GitHub tokens
- SSH passwords
- OpenAI access tokens
- OAuth refresh tokens
- Gateway auth tokens
- Home Assistant Supervisor tokens

If a credential appears in chat or logs, treat it as exposed and ask the user to
rotate it through the relevant provider.

