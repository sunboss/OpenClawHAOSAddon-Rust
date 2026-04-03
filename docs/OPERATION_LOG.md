# Operation Log

This file preserves task and push history for future AI handoff.

## Entry template

Copy this block before each push and fill it in:

```md
## YYYY-MM-DD HH:MM TZ - short title

- User request:
- Intent / context:
- Files changed:
  - `path`
- Commands / validation:
  - `command`
- Version:
- Commit:
- Push:
- Result summary:
- Next handoff:
```

## 2026-04-03 13:12 Asia/Shanghai - Backfill previous push for OpenClaw CLI window launch

- User request: fix the main Rust add-on so the `OpenClaw CLI` button opens a new window and runs the command there.
- Intent / context: the in-page terminal injection path was not the desired UX; the button should launch a standalone terminal window and immediately run `openclaw tui`.
- Files changed:
  - `config.yaml`
  - `crates/haos-ui/src/main.rs`
  - `crates/ingressd/src/main.rs`
- Commands / validation:
  - `cargo test -p haos-ui -p ingressd`
- Version: `2026.04.03.7`
- Commit: `6593f34`
- Push: `origin/main` pushed successfully
- Result summary: `OpenClaw CLI` now opens `/terminal/` in a new window with a boot command, and the terminal page auto-executes `openclaw tui` after connect.
- Next handoff: keep using this log before every future push; if terminal command bootstrapping changes again, re-check both `haos-ui` button wiring and `ingressd` terminal query handling together.

## 2026-04-03 13:20 Asia/Shanghai - Add durable pre-push operation logging workflow

- User request: preserve conversation and operation records, especially save them before every push for other AI or external callers.
- Intent / context: chat history may disappear, so the repository itself needs a durable operation trail.
- Files changed:
  - `config.yaml`
  - `docs/MAINTAINER_CONTEXT.md`
  - `docs/OPERATION_LOG.md`
  - `crates/haos-ui/src/main.rs`
- Commands / validation:
  - `Get-Content docs\\MAINTAINER_CONTEXT.md`
  - `git status --short --branch`
  - `cargo test -p haos-ui -p ingressd`
- Version: `2026.04.03.8`
- Commit: not created yet
- Push: not pushed yet
- Result summary: adds a standing rule plus a reusable log file in the repo, and bumps the add-on version for the process change release.
- Next handoff: before any push, either fill in this entry after creating the release commit or add a fresh entry that reflects the final push scope.
