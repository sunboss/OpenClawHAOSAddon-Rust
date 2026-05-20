# OpenClawHAOSAddon-Rust

Home Assistant add-on repository for running OpenClaw on HAOS.

Add this repository in Home Assistant:

```text
https://github.com/sunboss/OpenClawHAOSAddon-Rust
```

The add-on lives in [`openclaw_assistant_rust/`](./openclaw_assistant_rust/) using the standard Home Assistant add-on repository layout. Repository-level handoff and maintenance notes stay under [`docs/`](./docs/).

## Maintainer handoff

Future AI maintainers should read [`docs/AI_HANDOFF.md`](./docs/AI_HANDOFF.md) first.
It explains the current production-vs-Rust-rewrite distinction and the 2026-05-20
HAOS repair state.

As of 2026-05-20, the HAOS add-on currently installed on the user's Home
Assistant host is the production repository at
[`sunboss/openclaw-ha-addon`](https://github.com/sunboss/openclaw-ha-addon), not
this Rust rewrite repository. The installed production add-on is running
`2026.05.20.2` from `ghcr.io/sunboss/openclaw-ha-addon:2026.05.20.2`.

## Add-on

- Add-on docs: [`openclaw_assistant_rust/README.md`](./openclaw_assistant_rust/README.md)
- Install guide: [`openclaw_assistant_rust/INSTALL.md`](./openclaw_assistant_rust/INSTALL.md)
- Runtime notes: [`openclaw_assistant_rust/DOCS.md`](./openclaw_assistant_rust/DOCS.md)
- HAOS maintenance runbook: [`docs/HAOS_MAINTENANCE_RUNBOOK.md`](./docs/HAOS_MAINTENANCE_RUNBOOK.md)
- 2026-05-20 repair record: [`docs/RELEASE_2026-05-20.md`](./docs/RELEASE_2026-05-20.md)
