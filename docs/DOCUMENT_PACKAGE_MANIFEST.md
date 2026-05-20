# Document Package Manifest

Package purpose: give another AI or maintainer enough context to pull, inspect,
and continue OpenClaw HAOS add-on maintenance without relying on chat history.

## Read order

1. `README.md`
2. `docs/AI_HANDOFF.md`
3. `docs/RELEASE_2026-05-20.md`
4. `docs/HAOS_MAINTENANCE_RUNBOOK.md`
5. `docs/MAINTAINER_CONTEXT.md`
6. `docs/OPERATION_LOG.md`
7. `MIGRATION.md`

## Included document groups

- Repository overview:
  - `README.md`
  - `MIGRATION.md`
- HAOS add-on metadata:
  - `config.yaml`
  - `build.yaml`
  - `repository.yaml`
  - `translations/en.yaml`
- Maintainer handoff:
  - `docs/AI_HANDOFF.md`
  - `docs/MAINTAINER_CONTEXT.md`
  - `docs/OPERATION_LOG.md`
  - `docs/HAOS_MAINTENANCE_RUNBOOK.md`
  - `docs/RELEASE_2026-05-20.md`
- Package metadata:
  - `docs/DOCUMENT_PACKAGE_MANIFEST.md`

## Excluded content

The archive intentionally excludes:

- `.git/`
- build outputs
- `.claude/`
- `.codex-worktrees/`
- credentials, tokens, SSH passwords, OAuth refresh tokens
- HAOS runtime config files from the user's host

## Current package name

Generated archives should use this pattern:

```text
docs/archives/openclaw-haos-maintenance-docs-YYYYMMDD-HHMMSS.zip
docs/archives/openclaw-haos-maintenance-docs-YYYYMMDD-HHMMSS.tar.gz
```

