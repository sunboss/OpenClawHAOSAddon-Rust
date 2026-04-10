# Operation Log

This file preserves task and push history for future AI handoff.

## 2026-04-10 22:05 Asia/Shanghai - Create parallel scheme-B workspace

- User request: choose scheme B and open a new directory for a thinner system.
- Intent / context:
  - stop layering more behavior into the existing architecture
  - create a separate workspace that can converge on an official-thin HAOS runtime model
- Workspace created:
  - `C:\Users\SunBoss\Desktop\555\OpenClawHAOSAddon-Rust-official-thin`
- Seeded components:
  - copied reusable crates: `addon-supervisor`, `actiond`, `ingressd`, `oc-config`
  - intentionally did not copy `haos-ui`
  - added new top-level manifest, Dockerfile, add-on config, and architecture notes
- Result summary:
  - scheme B now has an independent filesystem root and can evolve without destabilizing the current repo

## 2026-04-10 22:18 Asia/Shanghai - Remove leftover custom control semantics

- Intent / context:
  - keep the scheme-B workspace aligned with official OpenClaw probe semantics
  - remove leftover `haos-ui` and `control-readyz` assumptions copied from the older architecture
- Changes made:
  - removed `control-readyz` from `actiond` and `ingressd`
  - removed `ui_bin`, `ui_port`, and `UI_PORT` remnants from `addon-supervisor`
  - updated docs to state that the thin wrapper should stick to `/healthz` and `/readyz`
- Expected outcome:
  - the new workspace is easier to reason about as a thin HAOS shell around the official gateway/control UI

## 2026-04-10 22:29 Asia/Shanghai - Remove dead nginx orchestration and rename public artifact path

- Intent / context:
  - continue stripping concepts that only made sense in the old nginx-heavy architecture
  - keep the thin workspace understandable as ingressd-first instead of nginx-first
- Changes made:
  - removed dead `RenderNginx` command handling and the unused `render_nginx_conf` process field
  - removed unused `mcporter_bin` and related dead configuration wiring
  - renamed the exposed token/cert artifact directory from `nginx_html_dir` to `public_share_dir`
  - switched the default runtime artifact path to `/run/openclaw-rs/public`
  - updated `ingressd` and the image layout to read those artifacts from the new runtime public path
- Expected outcome:
  - thinner supervisor code, fewer misleading names, and less accidental coupling to an nginx implementation that this workspace no longer uses

## 2026-04-10 22:41 Asia/Shanghai - Collapse runtime to native WebUI plus native CLI

- Intent / context:
  - user explicitly wants only the native WebUI and a native CLI entrypoint
  - remove middle-layer behavior that is not required to boot the official gateway
- Changes made:
  - removed `actiond` from the workspace build and image copy list
  - stopped `addon-supervisor` from spawning `actiond`
  - changed ingress health probes to call the native gateway directly instead of a helper daemon
  - set `run_doctor_on_start` default to `false` in the thin add-on config
- Expected outcome:
  - the thin workspace runtime is now conceptually just `gateway + ingressd`, with ingress acting as a transport shell rather than a second control plane

## 2026-04-10 22:48 Asia/Shanghai - Remove unused actiond source tree

- Intent / context:
  - avoid leaving dead crates around after the runtime was simplified to `gateway + ingressd`
- Changes made:
  - removed the unused `crates/actiond` source files from the thin workspace
- Expected outcome:
  - future maintenance should see the runtime shape directly from the directory layout, without stale helper-daemon code suggesting extra layers
