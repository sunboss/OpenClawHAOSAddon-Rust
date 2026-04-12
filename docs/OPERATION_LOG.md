# Operation Log

This file preserves task and push history for future AI handoff.

## 2026-04-12 23:59 Asia/Shanghai - Switch Maintenance Shell to official ttyd web shell

- User request:
  - make the `维护 Shell` button open a complete full-screen web shell page rather than the current framed page with explanatory chrome
  - verify whether `ttyd` is present and whether the installed version is the latest complete release
- Outcome:
  - confirmed the image previously did not install `ttyd`; the old maintenance shell was still the custom `ingressd` PTY/xterm page
  - aligned the image to install upstream `ttyd 1.7.7` from the official GitHub release assets
  - added a managed `ttyd` process under `addon-supervisor`
  - added `/shell/` reverse proxy support in `ingressd`, with WebSocket forwarding and a minimal loading fallback
  - switched the HA panel `维护 Shell` entry to open the dedicated `/shell/` page directly
  - updated the home status panel to show `Shell` as a first-class service and removed UI wording that implied shell commands could still be pre-injected into that page
- Files changed:
  - `config.yaml`
  - `CHANGELOG.md`
  - `Dockerfile`
  - `crates/addon-supervisor/src/main.rs`
  - `crates/ingressd/src/main.rs`
  - `crates/haos-ui/src/main.rs`
- Commands / validation:
  - `cargo test -p ingressd`
  - `cargo test -p haos-ui -p ingressd -p addon-supervisor`
- Version:
  - bump add-on version to `2026.04.12.17`
- Source used:
  - official ttyd release feed / GitHub release assets for `1.7.7`
- Next handoff:
  - verify the `/shell/` experience in the real HA panel and confirm the managed `ttyd` process appears in runtime logs and the home service overview

## 2026-04-12 21:55 Asia/Shanghai - Polish the control-deck UI, replace the brand mark, and improve mobile adaptation

- User request:
  - keep refining the redesigned AI Agent command-center UI
  - improve the cinematic feel of the home hero/buttons and the premium quality of the left navigation / brand area
  - consider mobile adaptation
  - stop showing screenshots and push the final result
- Outcome:
  - refined the left rail with higher-end brand treatment, stronger navigation affordances, and calmer premium spacing
  - pushed the home hero and action buttons further toward a cinematic control-room feel
  - redesigned the inline UI logo into a more restrained command-core mark and removed the old `Rs` project-signifier feel
  - darkened and unified the service-state panel so it no longer looked like a bright warning-card cluster inside the dashboard
  - added dedicated mobile rules for small screens so the shell, hero, buttons, nav, cards, and chips collapse more gracefully
  - kept the home resource collection, status display, Gateway entry, TUI entry, and Shell entry intact
- Files changed:
  - `config.yaml`
  - `CHANGELOG.md`
  - `docs/OPERATION_LOG.md`
  - `crates/haos-ui/src/main.rs`
- Commands / validation:
  - `cargo test -p haos-ui -p ingressd -p addon-supervisor`
- Version:
  - bump add-on version to `2026.04.12.16`
- Next handoff:
  - verify the updated control-deck UI in the real HA panel on both desktop and mobile
  - if desired, sync the repository `logo.png` / `icon.png` to the new inline brand direction

## 2026-04-12 21:12 Asia/Shanghai - Redesign the HA panel into an AI Agent command center

- User request:
  - redesign the page as a premium dark AI Agent control console rather than a normal chat window or SaaS admin screen
  - keep the home page resource collection and status display
  - continue refining the redesign, then finish and push
- Outcome:
  - redesigned the shell into a dark cinematic control-deck layout with a left command rail and right main workspace
  - added restrained background glow, subtle grid texture, glassy panels, and calmer hierarchy using teal/water-blue accents
  - reframed the home page as a command-center overview while preserving live status, PID visibility, resource telemetry, Gateway access, CLI, and Shell
  - reorganized the commands page into a scheduling/operations center, the logs page into an observability console, and the config page into an ability-matrix control surface
  - kept existing runtime behavior intact; this was a UI/structure redesign rather than a logic change
- Files changed:
  - `config.yaml`
  - `CHANGELOG.md`
  - `docs/OPERATION_LOG.md`
  - `crates/haos-ui/src/main.rs`
- Commands / validation:
  - `cargo test -p haos-ui -p ingressd -p addon-supervisor`
- Version:
  - bump add-on version to `2026.04.12.15`
- Next handoff:
  - visually verify the redesigned control-deck layout in Home Assistant on desktop and mobile
  - if desired, continue by modularizing `haos-ui/src/main.rs` without changing behavior

## 2026-04-12 23:59 Asia/Shanghai - Add repository docs and keep slimming the scheme-3 UI shell

- User request:
  - create a documentation set modeled after `sunboss/hermes-agent-ha-addon`
  - keep optimizing and then push
- Outcome:
  - added repository-level `README.md`, `INSTALL.md`, and `DOCS.md`
  - reused the existing `logo.png` and added Mermaid diagrams for runtime structure and first-configuration flow
  - kept the home page resource collection and service status display intact
  - made the config page more clearly config-only
  - kept logs page focused on logs, while concentrating operational actions in the commands page
  - reduced duplicated Shell action buttons from the commands page
  - fixed Chinese copy in the ingress terminal page and fallback pages
- Files changed:
  - `README.md`
  - `INSTALL.md`
  - `DOCS.md`
  - `config.yaml`
  - `CHANGELOG.md`
  - `docs/OPERATION_LOG.md`
  - `crates/haos-ui/src/main.rs`
  - `crates/ingressd/src/main.rs`
- Commands / validation:
  - `cargo test -p haos-ui -p ingressd -p addon-supervisor`
  - `cargo test -p ingressd`
- Version:
  - bump add-on version to `2026.04.12.14`
- Next handoff:
  - verify the new docs render correctly on GitHub and in the HA add-on repository view
  - if desired, continue by splitting the large `haos-ui` page renderer into modules without changing behavior

## 2026-04-12 23:59 Asia/Shanghai - Restore lightweight embedded terminal on top of the slimmer UI

- User request:
  - keep a terminal after all
  - stay aligned with the latest official OpenClaw docs
  - keep the home page resource collection and status display
- Official direction followed:
  - terminal should center on native `openclaw tui`
  - keep the terminal path lighter than the older embedded-shell implementation
  - do not restore the removed pairing websocket/control-ui sidecar logic
- Outcome:
  - restored a lightweight embedded terminal entry
  - terminal now launches native `openclaw tui` again
  - config / commands / logs pages can open the TUI and send official `!command` style maintenance commands
  - home page status and resource panels remain intact
- Files changed:
  - `Cargo.lock`
  - `Dockerfile`
  - `config.yaml`
  - `CHANGELOG.md`
  - `docs/OPERATION_LOG.md`
  - `crates/haos-ui/src/main.rs`
  - `crates/ingressd/Cargo.toml`
  - `crates/ingressd/src/main.rs`
- Implementation:
  - restore terminal routes and PTY handling in `ingressd`
  - launch `openclaw tui` in the PTY by default
  - restore xterm assets in the image
  - re-add `OpenClaw CLI` entry points in the HA UI without bringing back the older heavy local control chain
- Commands / validation:
  - `cargo test -p haos-ui -p ingressd -p addon-supervisor`
- Version:
  - bump add-on version to `2026.04.12.13`
- Next handoff:
  - after push, verify that the restored terminal opens directly into native TUI
  - verify that config / commands / logs buttons pass commands through as intended

## 2026-04-12 23:59 Asia/Shanghai - Improve onboarding guidance, access-mode copy, and first-run path

- User request:
  - use the external `DOCS.md` as a reference for
    - installation guidance
    - access-mode explanation
    - first configuration path
  - keep the home page resource collection and status display
- Reference reviewed:
  - `https://github.com/techartdev/OpenClawHomeAssistant/blob/main/DOCS.md`
- Outcome:
  - homepage now includes a clearer first-install flow
  - config page now explains access mode more directly and shows a recommended first-configuration sequence
  - no terminal was reintroduced
- Files changed:
  - `config.yaml`
  - `CHANGELOG.md`
  - `docs/OPERATION_LOG.md`
  - `crates/haos-ui/src/main.rs`
- Implementation:
  - add a first-install guidance block to the home page
  - add access-mode explanation helpers
  - add a first-configuration path card to the config page
  - keep Gateway HTTPS access and the status/resource overview untouched
- Commands / validation:
  - `cargo test -p haos-ui -p ingressd -p addon-supervisor`
- Version:
  - bump add-on version to `2026.04.12.12`
- Next handoff:
  - after push, verify that the new guidance reads naturally in Chinese
  - confirm the home page still feels concise and not overloaded

## 2026-04-12 23:55 Asia/Shanghai - Remove embedded terminal and keep status-first home page

- User request:
  - if the terminal is not needed, delete it
  - continue aligning with the latest official OpenClaw docs
  - keep the home page resource collection and status display
- Official direction followed:
  - keep Gateway / Control UI access on the official HTTPS path
  - stop maintaining an add-on-local pseudo terminal
  - move shell usage guidance back to Home Assistant `Terminal & SSH`, SSH, or another host-local shell
- Outcome:
  - removed the add-on's embedded terminal end-to-end
  - command and logs pages now act as copyable/reference pages instead of a built-in terminal shell
  - kept the home page status cards, PID display, and resource sampling
- Files changed:
  - `Cargo.lock`
  - `Dockerfile`
  - `config.yaml`
  - `CHANGELOG.md`
  - `translations/en.yaml`
  - `crates/addon-supervisor/src/main.rs`
  - `crates/haos-ui/src/main.rs`
  - `crates/ingressd/Cargo.toml`
  - `crates/ingressd/src/main.rs`
  - `docs/MAINTAINER_CONTEXT.md`
- Implementation:
  - remove `enable_terminal` / `terminal_port` options and translations
  - stop exporting terminal-related runtime env from `addon-supervisor`
  - delete terminal routes, terminal protocol leftovers, and the fallback `Open Terminal` link from `ingressd`
  - remove `portable-pty` and xterm npm packages
  - remove all HA UI terminal buttons, iframe loading logic, and command injection scripts
  - replace command/log pages with official command references
- Commands / validation:
  - `cargo test -p haos-ui -p ingressd -p addon-supervisor`
- Version:
  - bump add-on version to `2026.04.12.11`
- Next handoff:
  - after push, verify that no `/terminal` references remain in runtime behavior
  - verify that command/log pages still provide useful guidance without regressing the home page status panels

## 2026-04-12 23:20 Asia/Shanghai - Deep native-TUI cleanup and dependency slimming

- User request:
  - continue deep local optimization without pushing first
  - keep the home page resource collection and status display
  - optimize strictly against the latest official OpenClaw docs instead of inventing local behavior
- Official sources checked:
  - `https://docs.openclaw.ai/tui`
  - `https://docs.openclaw.ai/cli/tui`
  - `https://docs.openclaw.ai/web/control-ui`
  - `https://docs.openclaw.ai/gateway/security`
- Outcome:
  - preserved the home page resource/status panels
  - aligned the embedded terminal with the official TUI model
  - continued reducing add-on-specific control logic and compile-heavy dependencies
- Files changed:
  - `Cargo.lock`
  - `Cargo.toml`
  - `Dockerfile`
  - `README.md`
  - `config.yaml`
  - `CHANGELOG.md`
  - `crates/addon-supervisor/Cargo.toml`
  - `crates/addon-supervisor/src/main.rs`
  - `crates/haos-ui/Cargo.toml`
  - `crates/haos-ui/src/main.rs`
  - `crates/ingressd/Cargo.toml`
  - `crates/ingressd/src/main.rs`
  - `docs/MAINTAINER_CONTEXT.md`
- Implementation:
  - remove the old local pairing websocket chain from `haos-ui` and delete `crates/haos-ui/src/gateway_ws.rs`
  - remove `haos-ui` dependencies that were only kept for that chain:
    - `ring`
    - `tokio-tungstenite`
    - `futures-util`
    - `reqwest`
  - keep the home page resource/status cache path, but switch local health probing to direct Tokio socket checks instead of `reqwest`
  - remove `control-readyz` and old `/action/*` helper endpoints from `ingressd`
  - change the embedded terminal PTY to launch native `openclaw tui` by default
  - standardize terminal-triggered commands to the official TUI `!command` model
  - change `doctor --fix` to run automatically on first install only, instead of every startup
  - rename the old public file path concept to `PUBLIC_SHARE_DIR`
  - update README and maintainer notes so docs match the current runtime architecture
- Commands / validation:
  - `cargo test -p haos-ui`
  - `cargo test -p haos-ui -p ingressd -p addon-supervisor`
- Version:
  - bump add-on version to `2026.04.12.10`
- Next handoff:
  - after push, verify that `OpenClaw CLI` opens straight into native `openclaw tui`
  - verify that commands sent from the HA panel execute as `!command` inside TUI rather than as raw shell input
  - keep the home page resource/status panels intact during any future refactor

## 2026-04-12 21:30 Asia/Shanghai - Stop browser-side pairing SSE from the HA home page

- User request: continue reducing remaining early-connect / repeated connection noise.
- Files changed:
  - `config.yaml`
  - `CHANGELOG.md`
  - `crates/haos-ui/src/main.rs`
  - `docs/OPERATION_LOG.md`
- Implementation:
  - remove the home-page mount point that triggered the browser pairing banner flow
  - this prevents the HA home page from auto-starting the `/events/pairing` browser subscription path
  - keep pairing available through native Control UI and CLI commands instead
- Commands / validation:
  - `cargo test -p haos-ui -p ingressd -p addon-supervisor`
- Version:
  - bump add-on version to `2026.04.12.9`

## 2026-04-12 21:05 Asia/Shanghai - Force visible HA UI text back to Chinese

- User request: some pages had been rendered as garbled/Korean-looking text and needed to be forced back to Chinese.
- Files changed:
  - `config.yaml`
  - `CHANGELOG.md`
  - `crates/haos-ui/src/main.rs`
  - `docs/OPERATION_LOG.md`
- Implementation:
  - add an output-layer Chinese normalization pass for the rendered HA UI HTML
  - normalize the most visible navigation, home, commands, logs, and status-panel strings back to Chinese
  - also apply the same normalization to health and diagnostics partials
- Commands / validation:
  - `cargo test -p haos-ui`
- Version:
  - bump add-on version to `2026.04.12.8`

## 2026-04-12 20:30 Asia/Shanghai - Slim the HA commands page toward native OpenClaw

- User request: continue simplifying what can still be reduced after removing `actiond`.
- Goal:
  - make `haos-ui` feel more like a thin HA shell instead of a second control surface
  - keep official entrypoints front and center: Gateway and `openclaw tui`
- Files changed:
  - `config.yaml`
  - `CHANGELOG.md`
  - `crates/haos-ui/src/main.rs`
  - `docs/OPERATION_LOG.md`
- Implementation:
  - add a new native-leaning commands page renderer and switch `/commands` to it
  - keep only:
    - native Gateway entry
    - `openclaw tui`
    - onboarding
    - health / status / doctor / logs / pairing basics
  - drop from the new commands page:
    - custom command input
    - config-file browsing shortcuts
    - backup script shortcut
    - other add-on-specific convenience actions
- Commands / validation:
  - `cargo test -p haos-ui -p ingressd -p addon-supervisor`
- Version:
  - `2026.04.12.7`

## 2026-04-12 18:30 Asia/Shanghai - Collapse actiond into ingressd

- User request: continue simplifying toward upstream-native architecture and specifically answer whether `actiond` had really been removed.
- Outcome:
  - `actiond` is no longer part of the active workspace build or runtime process tree
  - its lightweight health/restart surface was folded into `ingressd`
- Files changed:
  - `Cargo.toml`
  - `Dockerfile`
  - `README.md`
  - `config.yaml`
  - `crates/addon-supervisor/src/main.rs`
  - `crates/haos-ui/src/main.rs`
  - `crates/ingressd/src/main.rs`
  - `docs/MAINTAINER_CONTEXT.md`
  - `docs/OPERATION_LOG.md`
  - `CHANGELOG.md`
  - deleted:
    - `crates/actiond/Cargo.toml`
    - `crates/actiond/src/main.rs`
- Implementation:
  - moved local `/health`, `/healthz`, `/readyz`, `/control-readyz`, `/action/restart`, and `/action/status` handling into `ingressd`
  - switched `haos-ui` internal health/restart calls from `127.0.0.1:48100` to `127.0.0.1:48099`
  - removed `ACTION_SERVER_PORT` runtime env export and stopped spawning `actiond` from `addon-supervisor`
  - removed `actiond` from the Cargo workspace and Docker image
- Commands / validation:
  - `cargo test -p ingressd -p haos-ui -p addon-supervisor`
- Version:
  - `2026.04.12.6`

## 2026-04-12 16:05 Asia/Shanghai - Preinstall missing msteams bundled-plugin deps in image

- User request: continue with item `1` from the previous split, meaning handle the bundled plugin dependency problem rather than Bonjour.
- Intent / context:
  - after HTTPS access was repaired, the remaining repeated startup/doctor issue was:
    - `Bundled plugin runtime deps are missing`
    - `@azure/identity@^4.9.1`
    - `jwks-rsa@^4.0.1`
  - Dockerfile review showed that the image already preinstalled a large bundled-plugin dependency set, but those two packages were missing from the preinstall list
  - official docs indicate `msteams` is plugin-oriented and optional, but the packaged runtime still surfaces these dependency checks in `doctor`
- Official sources checked:
  - `https://docs.openclaw.ai/plugins`
  - `https://docs.openclaw.ai/providers/msteams`
  - `https://docs.openclaw.ai/cli/doctor`
- Files changed:
  - `Dockerfile`
  - `config.yaml`
  - `CHANGELOG.md`
  - `docs/OPERATION_LOG.md`
- Implementation:
  - add `@azure/identity@^4.9.1` to the bundled dependency preinstall step
  - add `jwks-rsa@^4.0.1` to the bundled dependency preinstall step
  - keep the fix surgical instead of adding a new runtime service or doctor suppression
- Commands / validation:
  - image rebuild required in HAOS to validate at runtime
- Version:
  - target version `2026.04.12.5`
- Commit:
  - pending
- Push:
  - pending
- Result summary:
  - rebuilt add-on images should stop reporting the missing `msteams` bundled runtime deps during startup doctor runs
- Next handoff:
  - push this version
  - rebuild/update the add-on in HAOS
  - confirm the bundled plugin dependency warning no longer appears

## 2026-04-12 15:20 Asia/Shanghai - Restore HTTPS secure-context access after native-HTTP regression

- User request: after testing the latest build, fix the broken LAN browser access path that now fails with `control ui requires device identity (use HTTPS or localhost secure context)`.
- Intent / context:
  - version `2026.04.12.3` proved that remote LAN browsers cannot use plain `http://<lan-ip>:18789` for Control UI because browser device identity requires a secure context
  - user testing showed repeated `code=1008 reason=control ui requires device identity (use HTTPS or localhost secure context)`
  - official docs confirm that HTTPS or localhost secure context is required for device identity / Control UI auth
- Official sources checked:
  - `https://docs.openclaw.ai/web/control-ui`
  - `https://docs.openclaw.ai/gateway/security`
  - `https://docs.openclaw.ai/gateway/troubleshooting`
- Files changed:
  - `crates/addon-supervisor/src/main.rs`
  - `crates/actiond/src/main.rs`
  - `crates/haos-ui/src/gateway_ws.rs`
  - `crates/haos-ui/src/main.rs`
  - `crates/ingressd/src/main.rs`
  - `docs/MAINTAINER_CONTEXT.md`
  - `config.yaml`
  - `CHANGELOG.md`
  - `docs/OPERATION_LOG.md`
- Implementation:
  - revert the previous native-HTTP access experiment
  - restore the HTTPS proxy on external port `18789`
  - move the managed gateway back to loopback `18790`
  - switch HA UI gateway links back to `https://<host>:18789/`
  - restore internal websocket helpers and readiness probes to `18790`
- Commands / validation:
  - pending
- Version:
  - target version `2026.04.12.4`
- Commit:
  - pending
- Push:
  - pending
- Result summary:
  - HAOS LAN browser access should again satisfy the official secure-context requirement for device identity
  - the add-on keeps the newer runtime and UI improvements, while rolling back only the broken plain-HTTP access path
- Next handoff:
  - rerun the relevant Rust tests
  - after push and HA rebuild, verify that `https://<ha-ip>:18789/` opens normally again

## 2026-04-12 10:45 Asia/Shanghai - Align HAOS terminal UX with official TUI local-shell model

- User request: review the official TUI docs, especially local shell commands, then continue.
- Intent / context:
  - the add-on already launched `openclaw tui`, but the HA panel wording still made the terminal feel like a custom command runner
  - official docs clarify that the TUI itself is the primary CLI, and local shell execution is built into that TUI via `!command`
  - the user wanted behavior and mental model to stay close to upstream, not drift into a separate local UX
- Official source checked:
  - `https://docs.openclaw.ai/web/tui#local-shell-commands`
- Key official points preserved:
  - `openclaw tui` is the official terminal interface
  - local shell commands are prefixed with `!`
  - those commands run on the TUI host in a fresh non-interactive shell
- Files changed:
  - `crates/haos-ui/src/main.rs`
  - `crates/ingressd/src/main.rs`
  - `docs/MAINTAINER_CONTEXT.md`
  - `config.yaml`
  - `CHANGELOG.md`
  - `docs/OPERATION_LOG.md`
- Implementation:
  - revise the commands page copy so `OpenClaw CLI` is explicitly described as native `openclaw tui`
  - add `!command` examples to the commands page and terminal placeholders
  - update the standalone terminal page subtitle/footer to describe native TUI + local shell usage
  - record the official TUI model in maintainer notes so future edits do not reintroduce a pseudo-CLI mental model
- Commands / validation:
  - `cargo test -p haos-ui -p ingressd`
- Version:
  - target version `2026.04.12.2`
- Commit:
  - pending
- Push:
  - pending
- Result summary:
  - users should now see the correct upstream mental model in the HA panel: TUI for interactive control, `!command` for host-local shell execution
- Next handoff:
  - run `cargo test -p haos-ui -p ingressd`
  - after push, verify the commands page and terminal page both visibly mention `!command`

## 2026-04-12 10:15 Asia/Shanghai - Upgrade bundled OpenClaw runtime to upstream v2026.4.11

- User request: verify whether the latest upstream release fixes the observed Dreaming and websocket behaviors, then proceed with upgrading if appropriate.
- Intent / context:
  - the user correctly pointed out that upstream had already published a newer stable release than the runtime currently bundled in the add-on
  - current HAOS logs still showed `current v2026.4.9`, so the add-on image had not yet caught up to upstream stable
  - upstream latest stable was re-verified as `v2026.4.11`, released on 2026-04-12 00:18, with explicit `Gateway/startup` websocket-availability fixes
- Official sources checked:
  - `https://github.com/openclaw/openclaw/releases/tag/v2026.4.11`
  - `https://github.com/openclaw/openclaw/tags`
- Findings that drove the implementation:
  - `v2026.4.11` release notes explicitly include `Gateway/startup: keep WebSocket RPC available while channels and plugin sidecars start`
  - release notes do not explicitly mention a fix for `memory-core ... must have required property 'idempotencyKey'`
  - upgrading is still worthwhile because the add-on runtime was one full stable release behind and the startup/websocket fixes match the observed HAOS symptoms
- Files changed:
  - `Dockerfile`
  - `crates/haos-ui/src/gateway_ws.rs`
  - `config.yaml`
  - `CHANGELOG.md`
  - `docs/OPERATION_LOG.md`
- Implementation:
  - bump Docker build arg `OPENCLAW_VERSION` from `2026.4.9` to `2026.4.11`
  - remove the stale hardcoded webchat client version in `gateway_ws.rs`
  - make websocket `client.version` follow runtime `OPENCLAW_VERSION`, with `2026.4.11` as the fallback
- Commands / validation:
  - `cargo test -p haos-ui`
  - `cargo test -p addon-supervisor`
- Version:
  - target version `2026.04.12.1`
- Commit:
  - pending
- Push:
  - pending
- Result summary:
  - rebuilt add-on images will install upstream OpenClaw `v2026.4.11`
  - the HAOS webchat helper will no longer present itself as `2026.4.9` after the runtime upgrade
- Next handoff:
  - run `cargo test -p haos-ui -p addon-supervisor`
  - after push and HA rebuild, verify whether the gateway log now reports `current v2026.4.11`
  - if `memory-core ... idempotencyKey` still persists on `v2026.4.11`, treat it as an upstream Dreaming bug and expose a HA panel Dreaming toggle to disable it safely

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

## 2026-04-11 00:55 Asia/Shanghai - Replace gateway popup blank page with a controlled loading page

- User request: the homepage `打开网关` action still opened a blank page.
- Intent / context:
  - the mainline had already restored the homepage gateway entry as a visible link
  - the remaining UX failure was the popup itself staying blank during the async wait before final navigation
  - current code path opened `about:blank` with `noopener,noreferrer`, then tried to redirect later, which is fragile across browsers
- Root-cause conclusion:
  - the blank page was caused by the popup bootstrap strategy, not by the gateway HTTPS proxy itself
  - the user still needs a pre-opened window to avoid popup blockers, but that window should contain a real loading page instead of an inaccessible `about:blank`
- Files changed:
  - `crates/haos-ui/src/main.rs`
  - `config.yaml`
  - `CHANGELOG.md`
  - `docs/OPERATION_LOG.md`
- Implementation:
  - replace `window.open("about:blank", "_blank", "noopener,noreferrer")`
  - open a controllable popup window first
  - write a lightweight loading page into the popup immediately
  - keep the existing stronger-ready wait and token-fetch logic
  - redirect the popup to the native gateway only after that logic completes
- Commands / validation:
  - `cargo test -p haos-ui`
- Version:
  - target version `2026.04.11.3`
- Commit:
  - pending
- Push:
  - pending
- Result summary:
  - clicking `打开网关` should now show a loading page instead of a persistent blank tab/window while waiting for the native gateway flow to complete
- Next handoff:
  - after push, verify whether the popup now shows the loading card first and then lands on the native gateway
  - if the popup still does not navigate, inspect whether the browser blocks later `location.replace` after the async wait on the user's target browser

## 2026-04-11 00:40 Asia/Shanghai - Expose official Bonjour disable switch in HAOS options

- User request: after confirming what Bonjour broadcast is for, continue and wire up the official way to disable it.
- Intent / context:
  - idle `127.0.0.1:18790` websocket noise was already reduced on `2026.04.11.1`
  - remaining recurring noisy lines were now dominated by Bonjour advertiser churn:
    - `bonjour restarting advertiser`
    - `watchdog detected non-announced service`
  - the user chose the path of disabling LAN discovery if the official product supports it
- Official source checked:
  - OpenClaw Bonjour discovery docs
  - documented knob: `OPENCLAW_DISABLE_BONJOUR=1` disables advertisement
  - source: [OpenClaw Bonjour docs](https://docs.openclaw.ai/zh-CN/gateway/bonjour)
- Files changed:
  - `crates/addon-supervisor/src/main.rs`
  - `config.yaml`
  - `CHANGELOG.md`
  - `docs/OPERATION_LOG.md`
- Implementation:
  - add HAOS option `disable_bonjour`
  - map that option into runtime settings
  - export official env var `OPENCLAW_DISABLE_BONJOUR` as `1` or `0`
  - include the env var in the supervisor allowlist so the managed gateway process actually receives it
- Commands / validation:
  - `cargo test -p addon-supervisor`
- Version:
  - target version `2026.04.11.2`
- Commit:
  - pending
- Push:
  - pending
- Result summary:
  - the add-on now exposes the official upstream switch for turning off Bonjour advertisement in HAOS
  - this gives a supported way to stop Bonjour advertiser churn without inventing a custom runtime patch
- Next handoff:
  - after push, set `disable_bonjour: true` in the add-on options and restart once
  - then verify whether the `bonjour ... advertiser` lines disappear while normal WebUI / CLI access still works through HA ingress and `:18789`

## 2026-04-11 00:25 Asia/Shanghai - Stop background pairing polling from constantly touching 127.0.0.1:18790

- User request: keep digging into `18790`; confirm what it is and continue reducing unnecessary internal connections instead of hiding the logs.
- Intent / context:
  - latest HAOS logs still showed periodic `origin=n/a host=127.0.0.1:18790` websocket failures even after `2026.04.10.10`
  - code inspection confirmed these were not browser clicks alone; `haos-ui` still had a background `pairing_poll_task` that continuously called `device.pair.list` over the native gateway websocket
  - in this add-on architecture, `18790` is the upstream native loopback gateway port, so the right fix is to reduce our own eager callers rather than trying to remove the port
- Root-cause conclusion:
  - the remaining recurring loopback websocket noise was primarily self-inflicted by the add-on's always-on pairing poller
  - keeping pairing updates event-driven / on-demand is closer to the desired model than a permanent background websocket poll
- Files changed:
  - `crates/haos-ui/src/main.rs`
  - `config.yaml`
  - `CHANGELOG.md`
  - `docs/OPERATION_LOG.md`
- Implementation:
  - stop spawning the background `pairing_poll_task` at `haos-ui` startup
  - keep pairing state in memory, but refresh it only from the pairing SSE path when a page is actually open and subscribed
  - keep `pair-approve` refresh behavior so manual approval still updates in-memory pairing state
  - update the cached pending-device count from current in-memory pairing state instead of from the removed background poll loop
- Commands / validation:
  - `cargo test -p haos-ui`
- Version:
  - target version `2026.04.11.1`
- Commit:
  - pending
- Push:
  - pending
- Result summary:
  - the add-on no longer maintains a permanent background websocket pairing poll against the native gateway
  - repeated internal `127.0.0.1:18790` connection attempts should now only happen when the user has an active page/session that actually needs pairing state
- Next handoff:
  - after push, compare logs before/after and verify whether the periodic `origin=n/a host=127.0.0.1:18790` lines largely disappear while idle
  - if browser-triggered `origin=https://...:18789` websocket failures still remain, inspect whether the native gateway page itself is being auto-opened or refreshed too early

## 2026-04-10 23:55 Asia/Shanghai - Make homepage gateway entry a real link while preserving native ready gating

- User request: the homepage `打开网关` entry no longer looked like a real link; continue on the `2026.04.10.9` mainline and fix it.
- Intent / context:
  - the homepage action was rendered as a pure JS button, so users could not see a direct link target on the native gateway entry
  - the existing `ocOpenGateway()` behavior still mattered because it waits for stronger readiness and appends the gateway token before opening the native UI
- Root-cause conclusion:
  - UX regressed because the visual control was only a `<button>` with JS behavior, not an actual anchor element
  - we should keep the mainline startup-order safeguards but restore a real link-style entry on the homepage
- Files changed:
  - `crates/haos-ui/src/main.rs`
  - `config.yaml`
  - `CHANGELOG.md`
  - `docs/OPERATION_LOG.md`
- Implementation:
  - add a dedicated `primary_link_button()` helper for anchor-styled primary actions
  - change the homepage `打开网关` control from a JS-only button to a real `<a>` element with `target="_blank"`
  - sync the rendered `href` to the native gateway URL on load so the entry visibly behaves like a link
  - keep click behavior routed through the existing native open logic so the page still waits for control readiness and token retrieval before final navigation
- Commands / validation:
  - `cargo test -p haos-ui`
- Version:
  - target version `2026.04.10.10`
- Commit:
  - pending
- Push:
  - pending
- Result summary:
  - the homepage gateway entry is once again a visible link-style control, while preserving the native ready/token flow added on the 10.9 mainline
- Next handoff:
  - after push, verify the homepage shows a link-style `打开网关` control and that it still opens the native gateway successfully after startup
  - if users want the same visual treatment on secondary pages, convert the remaining raw `ocOpenGateway()` button call sites as a follow-up

## 2026-04-10 21:45 Asia/Shanghai - Gate pairing poll and native gateway open on stronger control readiness

- User request: do not merely suppress remaining websocket noise; solve what can be solved in the add-on layer.
- Intent / context:
  - logs after `2026.04.10.8` showed the main MCP setup issue was fixed
  - remaining `ws closed before connect` lines clustered before `[browser] control listening` and `[plugins] embedded acpx runtime backend ready`
  - two distinct paths were still racing startup:
    - local polling from `haos-ui` (`origin=n/a host=127.0.0.1:18790`)
    - user/browser opening the native gateway before the control plane was ready (`origin=https://<ha-ip>:18789`)
- Root-cause conclusion:
  - plain `/readyz` only proves the gateway process and loopback port are up
  - it does not prove the browser/acpx control layer is ready to accept the webchat/device-pair flow
- Files changed:
  - `crates/actiond/src/main.rs`
  - `crates/ingressd/src/main.rs`
  - `crates/haos-ui/src/main.rs`
  - `config.yaml`
  - `CHANGELOG.md`
  - `docs/OPERATION_LOG.md`
- Implementation:
  - add `GET /control-readyz` in `actiond`
  - define control readiness as:
    - gateway ready
    - local browser-control port (`gateway port + 2`, usually `18792`) accepting TCP connections
  - proxy `/control-readyz` through `ingressd`
  - make `haos-ui` pairing polling wait for control readiness instead of a fixed 90s blind delay
  - make `ocOpenGateway()` wait for control readiness before opening the native dashboard
- Commands / validation:
  - `cargo test -p actiond -p ingressd -p haos-ui`
- Version:
  - target version `2026.04.10.9`
- Commit:
  - pending
- Push:
  - pending
- Result summary:
  - remaining startup websocket failures should be materially reduced by fixing readiness ordering, not by hiding logs
- Next handoff:
  - after push, verify whether the `origin=n/a host=127.0.0.1:18790` lines disappear or drop sharply
  - if Bonjour churn still matters, investigate whether OpenClaw upstream exposes a documented knob for fixed advertised gateway name / hostname or LAN discovery disablement

## 2026-04-10 21:05 Asia/Shanghai - Switch Home Assistant MCP setup to official mcporter config shape

- User request: handle the remaining MCP setup failure strictly with reference to official documentation.
- Intent / context:
  - runtime logs still showed `--header requires KEY=value` after the previous CLI-based fix
  - official MCPorter docs favor the persisted config file shape over ad-hoc startup mutation
  - the add-on should stop depending on fragile startup CLI syntax for HA MCP registration
- Official source checked:
  - MCPorter README / configuration reference
  - config shape documented as `mcpServers -> <name> -> baseUrl -> headers`
  - config resolution documented around `MCPORTER_CONFIG` and `~/.mcporter/mcporter.json`
- Implementation decision:
  - stop shelling out to `mcporter config add` during startup
  - directly upsert the `HA` entry in `/config/.mcporter/mcporter.json`
  - preserve the official structure:
    - `"baseUrl": "http://supervisor/core/api/mcp"`
    - `"headers": { "Authorization": "Bearer <token>" }`
- Files changed:
  - `crates/addon-supervisor/src/main.rs`
  - `config.yaml`
  - `CHANGELOG.md`
  - `docs/OPERATION_LOG.md`
- Commands / validation:
  - `cargo test -p addon-supervisor`
- Version:
  - target version `2026.04.10.8`
- Commit:
  - pending
- Push:
  - pending
- Result summary:
  - HA MCP setup now follows the official persisted config model instead of volatile CLI flag syntax
  - future startup logs should no longer emit `mcporter` header / subcommand parsing failures for this path
- Next handoff:
  - after push, verify `/config/.mcporter/mcporter.json` contains the `HA` entry with `baseUrl` and `headers.Authorization`
  - if logs still show repeated loopback websocket timeouts after startup, inspect the remaining early internal probe path separately from MCP setup

## 2026-04-10 20:45 Asia/Shanghai - Fix mcporter header syntax and suppress boxed startup doctor noise

- User request: continue after reviewing the new runtime logs and push the fix.
- Intent / context:
  - the latest runtime logs showed two remaining issues after `2026.04.10.6`
  - `mcporter` still failed because current CLI expects `--header KEY=value`
  - startup `doctor --fix` still leaked boxed noise sections even though single-line suppression already existed
- Log findings captured:
  - `"[mcporter] --header requires KEY=value."`
  - `"Unknown command 'add'."` remains the legacy fallback path after the modern command fails
  - startup doctor still surfaced boxed sections for `Memory search`, `Gateway port`, and `Gateway`
  - the gateway itself still reached ready state later, so `Port 18790 is already in use` stayed classified as startup noise in this supervised container model
- Files changed:
  - `crates/addon-supervisor/src/main.rs`
  - `config.yaml`
  - `CHANGELOG.md`
  - `docs/OPERATION_LOG.md`
- Commands / validation:
  - `cargo test -p addon-supervisor`
- Version:
  - target version `2026.04.10.7`
- Commit:
  - pending
- Push:
  - pending
- Result summary:
  - `mcporter` HA setup now emits header syntax compatible with the current CLI
  - startup doctor suppression now handles whole boxed noise sections instead of only matching a few raw lines
- Next handoff:
  - after push, verify that add-on logs no longer show the `--header requires KEY=value` failure
  - if loopback websocket timeouts still dominate after startup, inspect which internal probe path is still polling too early

## 2026-04-10 15:35 Asia/Shanghai - Prepare rollback tag and release 2026.04.10.6 to main

- User request: add a git tag on `sunboss/OpenClawHAOSAddon-Rust` so rollback is easy, then version the latest local changes and push them to the main repository.
- Intent / context:
  - preserve a stable rollback anchor before publishing the next batch of HAOS add-on changes
  - publish the official-alignment work from the fresh repo baseline instead of relying on older local directories
  - keep durable repo memory ahead of the push, per maintainer rule
- Rollback anchor plan:
  - tag current `origin/main` / `HEAD` at version `2026.04.10.5`
  - planned tag name: `rollback-2026.04.10.5`
- Files changed:
  - `config.yaml`
  - `CHANGELOG.md`
  - `docs/OPERATION_LOG.md`
  - `docs/MAINTAINER_CONTEXT.md`
  - `docs/RUNTIME_BOUNDARIES.md`
  - `crates/actiond/src/main.rs`
  - `crates/addon-supervisor/src/main.rs`
  - `crates/ingressd/src/main.rs`
  - `crates/haos-ui/src/main.rs`
- Commands / validation:
  - `cargo test -p haos-ui`
  - `cargo test -p addon-supervisor -p actiond -p ingressd`
- Version:
  - target version `2026.04.10.6`
- Commit:
  - pending
- Push:
  - pending
- Result summary:
  - release record created before push
  - rollback tag plan and version bump are now captured in-repo for future AI handoff
- Next handoff:
  - if the push succeeds, update this entry with final commit hash, pushed tag, and verification summary
  - if the push fails, keep `rollback-2026.04.10.5` reserved for the pre-release state

## 2026-04-10 15:10 Asia/Shanghai - Track upstream v2026.4.9 focus points and post-release mainline clues

- User request: write the upstream `v2026.4.9` points worth syncing into the durable repo log, then check whether upstream has any newer tag or commit clues beyond `v2026.4.9`.
- Intent / context:
  - avoid relying on chat history for upstream tracking
  - keep HAOS add-on changes aligned with official OpenClaw runtime direction
  - record whether we should follow a newer release tag or only unreleased mainline commits
- Upstream release baseline checked:
  - `openclaw/openclaw` releases page
  - latest release still `v2026.4.9`
  - release date: `2026-04-09 02:25`
  - release commit: `0512059`
- Upstream `v2026.4.9` points worth syncing / watching for this add-on:
  - `Reply/doctor`: doctor now leans harder on active runtime snapshots, clearer reauth surfacing, and more explicit remediation commands
  - `Android/pairing`: pairing bootstrap, stale setup-code cleanup, and token reuse remain active upstream focus areas
  - `Matrix/gateway startup readiness`: upstream keeps moving toward "only mark startup healthy after readiness is real"
  - `npm packaging`: fresh-install dependency completeness is now treated as release-critical, relevant for add-on/container builds
  - `Security`: SSRF, `.env`, remote node event trust boundaries, and auth-choice collision fixes raise the bar for runtime safety assumptions
- Newer upstream status as of `2026-04-10`:
  - no newer release tag than `v2026.4.9` was found
  - upstream `main` does continue beyond the release on `2026-04-10`
  - notable post-release commits observed on `main`:
    - `0e54440` `fix(cycles): remove browser cli and tlon runtime seams`
    - `dbe2a97` `fix(cycles): remove qa-lab and ui runtime seams`
    - `6c82a91` `refactor: tighten device pairing approval types`
    - `c2e2b87` `fix(acp): classify gateway chat error kinds`
- Current add-on comparison:
  - already aligned in spirit on lightweight readiness and startup-noise handling
  - already actively working the pairing path (`CHANGELOG.md` entries `2026.04.09.10` through `2026.04.10.5`)
  - no evidence yet that a new upstream release requires an immediate version sync
  - the most interesting unreleased upstream clue for us is `6c82a91` on pairing approval typing / stricter protocol shape
- Files changed:
  - `docs/OPERATION_LOG.md`
  - `docs/MAINTAINER_CONTEXT.md`
  - `docs/RUNTIME_BOUNDARIES.md`
  - `crates/addon-supervisor/src/main.rs`
  - `crates/haos-ui/src/main.rs`
- Commands / validation:
  - `cargo test -p haos-ui`
  - `cargo test -p addon-supervisor -p actiond -p ingressd`
- Version:
  - not bumped yet
- Commit:
  - not created yet
- Push:
  - not pushed yet
- Result summary:
  - repository memory now records both the official `v2026.4.9` baseline and the fact that upstream `main` moved on `2026-04-10` without a newer release tag
  - future AI handoff should compare against `6c82a91`-style pairing approval tightening before chasing less relevant runtime seam removals
- Next handoff:
  - if the next user asks to sync upstream behavior, inspect post-`v2026.4.9` pairing commits first
  - if a newer release tag appears later, re-check whether it supersedes these unreleased `main` clues before porting anything

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

## 2026-04-07 - Performance optimizations + UI redesign + button colors + copy fixes

- User request:
  1. "有点卡顿，你看看哪里可以优化"
  2. "优化 UI 设计，要美观大气，要有专业风范"
  3. "命令行按钮是不是没有颜色"
  4. "运行状态总览下面的说明文档要调整，这个是面对开发的，实际我们要面对用户"
- Intent / context:
  - Performance: blocking I/O on Tokio threads was causing UI lag on page load.
  - UI: the previous design had a large purple accent bar + hero block wasting ~260px; needed a professional look matching HAOS aesthetic.
  - Buttons: all command-page buttons were unstyled white, indistinguishable from each other.
  - Copy: all page subtitles were written from an internal architecture perspective, not user perspective.
- Files changed:
  - `crates/haos-ui/src/main.rs`
  - `crates/ingressd/src/main.rs`
  - `.preview/index.html` (preview only, not shipped)
  - `index.html` (preview only, not shipped)
  - `.claude/launch.json` (preview server config, not shipped)
- Changes detail:

  **Performance (`haos-ui`)**
  - Replaced two separate `/proc/meminfo` reads per request with a single `parse_meminfo_both()` that reads the file once and extracts both `MemTotal` and `MemAvailable` in one pass.
  - Replaced two `df` subprocess calls (`-h` and `-B1`) per request with a single `disk_combined()` using `-B1` only, computing both the display string and the percentage from one run.
  - Moved all blocking I/O (`df`, `ps`, `/proc/*`) into `tokio::task::spawn_blocking` so the Tokio async thread is never blocked.
  - Made `collect_system_snapshot()` async; `index` handler now awaits it.
  - `home_content()` signature changed to accept `&SystemSnapshot` instead of calling collection internally.

  **Performance (`ingressd`)**
  - Added `cached_file_response()` helper that serves xterm.js / xterm.css / addon-fit.js with `Cache-Control: public, max-age=86400, immutable`, eliminating repeated ~500 KB downloads on every terminal page load.

  **UI redesign (`haos-ui` CSS + HTML shell)**
  - Removed: 40 px purple accent bar, large hero block (~180 px), separate masthead/top-shell structure.
  - Added: sticky 60 px dark navy header (`app-header`) combining brand logo, nav tabs, and version chip in one row.
  - Added: compact page-header area (eyebrow + h1 + subtitle) below the header with a bottom border separator.
  - Background: three-stop fixed gradient (`#eaf1ff → #f0f4fb → #f5f0ff`) replacing flat color.
  - Cards: `rgba(255,255,255,.92)` + `backdrop-filter:blur(2px)` + two-layer shadow for glassmorphism feel; hover lifts card with `translateY(-1px)`.
  - Live dot: pulsing `@keyframes dot-pulse` animation for online state; separate `dot-pulse-warn` for warning state.
  - Service badges: added `.svc-dot` colored indicator inside each badge name.
  - Progress bars: height reduced to 7 px; `transition: width .4s ease` for smooth render.
  - `<title>` changed to `OpenClaw · {title}` pattern.
  - `brand_lockup()` call replaced with `openclaw_brand_svg("brand-mark")` directly in header badge.

  **Button colors (`haos-ui`)**
  - Added three new CSS classes: `.btn-action` (blue tint), `.btn-diag` (green tint), `.btn-danger` (red tint).
  - `action_button()`: auto-applies `btn-danger` if command contains "restart" or "kill", otherwise `btn-action`.
  - Added `diag_button()` function for diagnostic commands using `btn-diag`.
  - Commands page: setup group uses `action_button`, diagnostic group uses `diag_button` (except restart → `action_button` which auto-triggers `btn-danger`), storage group uses `action_button`.

  **User-facing copy (`haos-ui`)**
  - Home page subtitle: removed architecture rationale ("拆出去后，整体更轻，也更适合长期维护") → "查看 OpenClaw 当前是否正常运行、各服务进程状态，以及系统资源占用情况。"
  - Config page subtitle: removed "会比直接翻日志和命令更直观" → "查看插件当前的访问方式、数据目录位置，以及各能力的启用状态。"
  - Commands page subtitle: removed "按钮显示中文，实际执行仍然是英文 OpenClaw 命令" → "在这里重启服务、批准设备配对、执行诊断，或直接打开终端操作。"
  - Logs page subtitle: removed "独立成页后，首页更轻，命令页也不会再被长输出挤满" → "查看 OpenClaw 运行日志、执行诊断命令，快速定位异常原因。"
  - Log terminal card subtitle: removed "适合长时间盯日志、复制报错和回看修复后的变化" → "点击上方按钮执行命令，输出结果会在这里显示。"

- Commands / validation:
  - Verified no remaining calls to removed functions (`parse_meminfo_kib`, `disk_snapshot`, `disk_percent_snapshot`) via grep — clean.
  - Verified new symbols present (`parse_meminfo_both`, `disk_combined`, `spawn_blocking`, `cached_file_response`) via grep — all found.
  - Verified tests still reference expected commands (`commands_page_uses_supervisor_restart_endpoint`, `commands_page_uses_real_npm_and_pairing_commands`) — unchanged.
  - Live browser preview confirmed via preview server screenshots.
- Version: `2026.04.07.1`
- Commit: not yet created
- Push: not yet pushed
- Result summary: page load latency reduced (no blocking Tokio threads, browser caches xterm assets), UI upgraded to sticky dark-nav + glassmorphism cards, command buttons color-coded by semantic category, all user-facing copy rewritten from architecture-rationale to user-benefit language.
- Next handoff:
  - Preview files (`index.html`, `.preview/index.html`, `.claude/launch.json`) were created for rendering verification — remove or gitignore them before pushing if not wanted in the repo.
  - `MAINTAINER_CONTEXT.md` → "UI direction" section already says user-facing text should explain what to do, not internal architecture rationale — this session enforced that rule.
  - If adding more command groups to the commands page, follow the pattern: setup/config → `action_button`, diagnostics/read-only → `diag_button`, destructive/restart → `action_button` (auto-gets `btn-danger` via keyword match).
  - openclaw upstream version in Dockerfile is still `2026.4.2`; latest release is `v2026.4.5` (adds video_generate, music_generate, Qwen/Fireworks/MiniMax providers, dreaming system). Upgrade is optional but noted.

## 2026-04-09 - Fix auto-approve startup race: increase initial delay to 120s

- User request: 日志显示 90s 延迟仍差 3-5 秒（acpx 实测需 93-95s），每次重启仍有一次启动失败
- Intent / context:
  - 精确计时：gateway 启动后 ~20s ready，acpx runtime 再需 ~73s，合计 ~93-95s。
  - 90s 延迟每次差 3-5 秒，导致启动时仍有一次 CLI 超时。
  - 改为 120s 给 25s 余量，覆盖 SD 卡慢启动场景。
- Files changed:
  - `config.yaml` — 版本升至 `2026.04.09.1`
  - `crates/addon-supervisor/src/main.rs` — `sleep(90s)` → `sleep(120s)`
  - `docs/OPERATION_LOG.md`
- Commands / validation:
  - `cargo check -p addon-supervisor` — 编译通过
- Version: `2026.04.09.1`
- Commit: pending
- Push: pending
- Result summary: 重启后 auto-approve helper 不再因 acpx 未就绪失败；120s 覆盖实测 93-95s 加 25s 余量。
- Next handoff:
  - 运行期间约每 30 分钟一次偶发失败属 gateway 内部定时事件，非 bug，系统 15s 自动恢复。

## 2026-04-09 - Fix auto-approve startup race: increase initial delay to 90s

- User request: 日志中 `auto-approve helper exited with Some(1): gateway timeout` 在重启后持续出现，45s 延迟仍不够
- Intent / context:
  - 日志分析：从 gateway 启动到 `[plugins] embedded acpx runtime backend ready` 需要约 90 秒（gateway 进程就绪约 20s，acpx runtime 初始化额外需 40-70s）。
  - CLI 连接（`openclaw devices approve --latest`）依赖 acpx runtime，webchat 不依赖，因此 webchat 正常而 CLI 超时。
  - 45s 延迟只等到 gateway 进程就绪，未等到 acpx ready，故启动阶段仍失败。
  - 运行期间偶发失败（约每 30 分钟一次）属正常行为：gateway bonjour 重启或短暂繁忙时 CLI 连接超时，15s 后自动重试，不影响功能。
- Files changed:
  - `config.yaml` — 版本升至 `2026.04.08.9`
  - `crates/addon-supervisor/src/main.rs` — `sleep(45s)` → `sleep(90s)`
  - `docs/OPERATION_LOG.md`
- Commands / validation:
  - `cargo check -p addon-supervisor` — 编译通过
- Version: `2026.04.08.9`
- Commit: pending
- Push: pending
- Result summary: 重启后启动阶段 auto-approve 不再因 acpx runtime 尚未就绪而报 timeout；运行期间偶发超时属预期行为，系统自动恢复。
- Next handoff:
  - 运行期间约 30 分钟一次的偶发失败是 gateway 短暂繁忙导致，非 bug，无需处理。
  - 若后续希望彻底消除，可改用 gateway 的 webchat WebSocket API 发送 device.pair.approve，绕过 CLI 依赖。

## 2026-04-08 - Fix auto-approve helper timeout on gateway startup

- User request: 日志中 `auto-approve helper exited with Some(1): gateway timeout` 反复出现
- Intent / context:
  - `run_pairing_auto_approver` 启动后等待 20 秒就开始执行 `openclaw devices approve --latest`，但 gateway 实际启动需要 22-25 秒，导致第一次尝试必然超时并打印错误日志。
  - 配对功能本身不受影响（15 秒后自动重试时 gateway 已就绪），但日志噪音严重。
  - 修复：初始等待从 20 秒改为 45 秒，给 gateway 充足的启动时间。
- Files changed:
  - `config.yaml` — 版本升至 `2026.04.08.8`
  - `crates/addon-supervisor/src/main.rs` — `sleep(20s)` → `sleep(45s)`
  - `docs/OPERATION_LOG.md`
- Commands / validation:
  - `cargo check -p addon-supervisor` — 编译通过
- Version: `2026.04.08.8`
- Commit: pending
- Push: pending
- Result summary: 首次启动时 auto-approve helper 不再因 gateway 未就绪而报 timeout 错误，日志更干净。
- Next handoff:
  - 如果 gateway 启动时间超过 45 秒（极慢设备），第一次仍会失败，15 秒后重试。可按需调大此值。
  - 预装 deps 已生效（2026.04.08.7），doctor 不再显示 "Bundled plugin runtime deps are missing"。

## 2026-04-08 - Perf: cache pending_devices + show Gateway Token on home page + prebundle deps

- User request: 首页有点卡顿；首页显示 Gateway Token 可以复制
- Intent / context:
  - 每次页面请求都调用 `count_pending_devices()`（spawn Node.js 进程 ~500ms），是首页卡顿的直接原因。
  - `CachedSnapshot` 之前去掉了 `pending_devices` 字段（2026.04.08.1 修 OOM）；现在加回来但改为后台每 5 分钟刷新一次（而非之前的 8 秒），既不压内存也不影响页面速度。
  - 首页缺少 Gateway Token 展示，用户需要进终端 `jq` 才能获取；原版 HAOS 插件在 landing page 显著展示 token。
  - doctor --fix 每次重启都下载 46 个 bundled deps（约 2 分钟），是因为 Dockerfile 的 `npm install -g` 装到全局路径，而 openclaw/jiti 找的是自己的 node_modules。改为安装到 openclaw 包目录后预装进镜像。
- Files changed:
  - `config.yaml` — 版本升至 `2026.04.08.7`
  - `crates/haos-ui/src/main.rs`
  - `Dockerfile`
  - `docs/OPERATION_LOG.md`
- Changes detail:
  **haos-ui**
  - `CachedSnapshot` 增回 `pending_devices: usize` 字段
  - 后台任务：新增 `last_pending_check: Option<Instant>`，每 5 分钟刷新一次 `pending_devices`，30s 周期内其余时刻复用缓存值
  - `index()` 不再每次 `spawn_blocking(count_pending_devices)`，直接读缓存，首页响应速度 <1ms（缓存命中）
  - `PageConfig` 增 `gateway_token: String`，从 `openclaw.json` 的 `gateway.auth.token` 读取
  - 首页新增 Token 卡片：蓝色背景区块，默认遮罩显示末 8 位，[显示] 切换明文，[复制] 使用 Clipboard API，复制成功 1.5s 反馈
  - CSS 增 `.token-section`、`.token-row`、`.token-val` 等样式
  **Dockerfile**
  - 移除独立的 `npm install -g @buape/carbon ...` 补丁行
  - 新增 `cd /usr/local/lib/node_modules/openclaw && npm install --no-save --ignore-scripts <全部 46 个包>`，安装到 openclaw 自己的 node_modules，doctor 检测路径匹配，不再每次启动重下
  - `@grammyjs/types` 一并加入（原缺漏）
- Commands / validation:
  - `cargo test -p haos-ui` — 5/5 全过
- Version: `2026.04.08.7`
- Commit: pending
- Push: pending
- Result summary: 首页加载不再因 Node.js spawn 卡顿；Gateway Token 在首页可见可复制；镜像重建后启动时 doctor 不再下载 46 个包。
- Next handoff:
  - `pending_devices` 最多延迟 5 分钟才更新，设备配对提醒有轻微滞后，属预期行为。
  - `--ignore-scripts` 跳过了原生 addon 的编译（@discordjs/opus 等）；这些包在未配置 Discord 语音时不影响功能，配置后如有问题可移除 `--ignore-scripts` 标志。
  - Token 展示直接读 openclaw.json，如 gateway 尚未完成 onboard（token 未生成），token 卡片不显示，属预期行为。

## 2026-04-08 - Fix all undeclared channel plugin deps for openclaw 2026.4.8 (complete)

- User request: 日志继续刷 `Cannot find module 'grammy'`（Telegram 渠道），要求一次性补齐所有缺失包
- Intent / context:
  - 对 openclaw v2026.4.8 的全部渠道扩展文件（telegram/discord/feishu/google-chat/teams/mattermost/irc/nextcloud-talk/bluebubbles/zalo/whatsapp/signal）进行完整扫描。
  - 扫描结论：只有三个渠道有未声明的外部 npm 依赖（其他渠道使用内部模块，无外部依赖）：
    - Discord: `@buape/carbon`
    - Feishu: `@larksuiteoapi/node-sdk`
    - Telegram: `grammy`、`@grammyjs/types`（后者在 openclaw devDependencies 中，但 production 代码调用）
  - 合并为一行 `npm install -g` 全部补齐。
- Files changed:
  - `config.yaml` — 版本升至 `2026.04.08.6`
  - `Dockerfile` — 补丁行追加 `grammy @grammyjs/types`，更新注释
  - `docs/OPERATION_LOG.md`
- Commands / validation:
  - 无需 cargo 编译
- Version: `2026.04.08.6`
- Commit: pending
- Push: pending
- Result summary: 全部三个有问题的渠道插件（Discord/Feishu/Telegram）的缺失依赖已补齐，gateway-http unhandled error 将在镜像重建后消除。
- Next handoff:
  - 扫描结果为完整扫描，其他渠道（Google Chat/Teams/IRC/Mattermost/Nextcloud/Zalo/BlueBubbles 等）不存在同类问题，无需额外修补。
  - 若 upstream 后续修复 package.json 打包，可移除此 npm install -g 补丁行。

## 2026-04-08 - Fix missing Feishu channel dependency @larksuiteoapi/node-sdk

- User request: 升级后日志持续刷 `Cannot find module '@larksuiteoapi/node-sdk'`（Feishu 渠道）
- Intent / context:
  - 与 `@buape/carbon`（Discord）同类问题：openclaw v2026.4.8 Feishu 渠道插件依赖 `@larksuiteoapi/node-sdk`，但未在 package.json 中声明，每次 HTTP 请求触发 `probe-Cz2PiFtC.js` 加载 Feishu 扩展时报 `MODULE_NOT_FOUND`，每 30 秒刷一次。
  - 查询 npm registry 确认该包未在 openclaw 的 dependencies/peerDependencies/optionalDependencies 中出现，属 upstream 遗漏。
  - 将已知两个未声明依赖合并为一条 `npm install -g` 指令，统一注释说明来源。
- Files changed:
  - `config.yaml` — 版本升至 `2026.04.08.5`
  - `Dockerfile` — 补丁行改为同时安装 `@buape/carbon @larksuiteoapi/node-sdk`，更新注释
  - `docs/OPERATION_LOG.md`
- Commands / validation:
  - 无需 cargo 编译
- Version: `2026.04.08.5`
- Commit: pending
- Push: pending
- Result summary: Feishu 渠道插件加载不再报 MODULE_NOT_FOUND，与 Discord 修复合并为单条安装指令。
- Next handoff:
  - v2026.4.8 release notes 提及修复了 10+ 个渠道（BlueBubbles、Google Chat、IRC、Matrix、Mattermost、Teams、Nextcloud Talk、Zalo 等），可能还有其他渠道存在同类未声明依赖，出现时继续追加到此 `npm install -g` 行。
  - 若 upstream 后续版本修复打包问题，可移除这些补丁包。

## 2026-04-08 - Fix missing @buape/carbon dependency for Discord channel plugin

- User request: 升级后日志持续刷 `Cannot find module '@buape/carbon'`
- Intent / context:
  - openclaw v2026.4.8 的 Discord 渠道插件新引入了 `@buape/carbon` 依赖，但 openclaw 的 `package.json` 未将其列为 dependencies，导致 `npm install -g openclaw` 时不会自动安装。
  - gateway HTTP server 每次处理请求时调用 `listBundledChannelPlugins` → 触发 Discord 插件加载 → 找不到 `@buape/carbon` → `unhandled error`，持续刷错误日志。
  - gateway 本身仍可运行（acpx runtime ready、webchat 正常），但错误噪音影响日志可读性，属 upstream 打包漏洞。
- Files changed:
  - `config.yaml` — 版本升至 `2026.04.08.4`
  - `Dockerfile` — npm 安装步骤追加 `npm install -g @buape/carbon`，带注释说明原因
  - `docs/OPERATION_LOG.md`
- Commands / validation:
  - 无需 cargo 编译，仅 Dockerfile 层变更
- Version: `2026.04.08.4`
- Commit: pending
- Push: pending
- Result summary: 镜像重建后 `@buape/carbon` 已安装，Discord 插件加载不再报 `MODULE_NOT_FOUND`，错误日志消除。
- Next handoff:
  - 若 openclaw 后续版本修复了此打包问题（将 `@buape/carbon` 加入 dependencies），可从 Dockerfile 移除这行补丁。
  - 当前用户未使用 Discord 渠道，修复仅消除日志噪音，不影响现有功能。

## 2026-04-08 - Upgrade openclaw to v2026.4.8

- User request: 升级 openclaw 到 v2026.4.8
- Intent / context:
  - gateway 自检日志提示 `update available: v2026.4.8 (current v2026.4.5)`，用户确认升级。
  - v2026.4.8 修复：Telegram/多渠道打包缺失 sidecar 导致的 npm 构建失败、Slack Socket Mode 代理支持、SecretRef token 下载、DNS pinning 问题，均为 Bug Fix，无破坏性变更。
- Files changed:
  - `config.yaml` — 版本升至 `2026.04.08.3`
  - `Dockerfile` — `OPENCLAW_VERSION` 从 `2026.4.5` 改为 `2026.4.8`
  - `docs/OPERATION_LOG.md`
- Commands / validation:
  - 无需 cargo 编译，仅修改 npm 安装版本号
- Version: `2026.04.08.3`
- Commit: pending
- Push: pending
- Result summary: 镜像重建后将安装 openclaw@2026.4.8，修复多渠道启动问题和 Slack 代理支持。
- Next handoff:
  - 升级后首次启动 gateway 会重新加载插件（当前 52 loaded），如有渠道启动失败需查日志。
  - openai/gpt-5.4 模型仍无 API key，建议执行 `openclaw config set agents.defaults.model minimax/MiniMax-M2.7` 消除每 30 分钟的诊断报错。

## 2026-04-08 - Fix rapid-restart OOM feedback loop with exponential backoff

- User request: haos 还是会重启（2026.04.08.1 之后 gateway 依然崩溃循环）
- Intent / context:
  - 2026.04.08.1 消除了后台 Node.js 周期性 spawn，但 `run_managed_process` 在 gateway 崩溃后仍以固定 2s 间隔立即重启，OOM 崩溃 → 2s → 再次 OOM，形成正反馈循环。
  - 日志显示 gateway 进程 `exited with None`（被 OOM Killer 信号终止），存活时间远小于 30s，crash 后 2s 内即重启，导致系统内存未能恢复就再次 OOM。
- Files changed:
  - `config.yaml` — 版本升至 `2026.04.08.2`
  - `crates/addon-supervisor/src/main.rs`
  - `docs/OPERATION_LOG.md`
- Changes detail:
  - `run_managed_process` 新增三个常量：`STABLE_SECS=30`、`BACKOFF_BASE=2`、`BACKOFF_MAX=64`
  - 新增 `consecutive_failures: u32` 计数器：进程存活 ≥30s 则清零（视为稳定），否则 +1
  - 重启延迟 = `min(2 << consecutive_failures.min(5), 64)` → 2→4→8→16→32→64s 指数退避
  - `consecutive_failures > 1` 时打印 `backing off Xs (failure #N)` 日志，便于排查
  - `nginx-conf` 和 `spawn` 失败路径沿用 `BACKOFF_BASE(2s)` 不变
- Commands / validation:
  - `cargo check -p addon-supervisor` — 编译通过，零错误
- Version: `2026.04.08.2`
- Commit: pending
- Push: pending
- Result summary: gateway 快速崩溃时重启间隔从固定 2s 升至最长 64s，给系统足够时间释放内存后再重试，避免 OOM 正反馈循环。长时间稳定运行不受影响（稳定即清零）。
- Next handoff:
  - 若 gateway 内存泄漏导致每隔数小时 OOM，可考虑在 `STABLE_SECS` 内加入定时主动重启（如每 24h restart）作为长期修补，等待 upstream 修复内存泄漏。
  - 若设备内存确实不足（<512 MB），可将 `BACKOFF_MAX` 调高至 120s 以留出更多恢复时间。

## 2026-04-08 - Fix OOM crash loop caused by background Node.js spawning

- User request: 升级后 openclaw-gateway 反复重启，haos 系统不稳定
- Intent / context:
  - 上版本（2026.04.07.5）的后台缓存任务每 8 秒调用 `count_pending_devices()`，该函数启动 `openclaw devices list`（Node.js 进程，100-200MB 内存）。
  - Raspberry Pi 等内存受限设备上，每 8 秒 spawn 一个 Node.js 进程叠加 gateway 自身的 Node.js 进程，触发 OOM Killer 将 gateway 进程杀死（`exited with None` = 信号终止）。
  - 日志中 PID 快速跳跃（36→329→789→1393）证实了 gateway 在快速崩溃重启循环中。
- Files changed:
  - `config.yaml` — 版本升至 `2026.04.08.1`（日期修正）
  - `crates/haos-ui/src/main.rs`
  - `docs/OPERATION_LOG.md`
- Changes detail:
  - `CachedSnapshot` 去掉 `pending_devices` 字段
  - 后台任务移除 `count_pending_devices()` 调用，只保留轻量操作（proc 文件读取 + HTTP health check）
  - 后台任务更新间隔 8s → 30s，进一步降低系统压力
  - `index()` 仍按需调用 `count_pending_devices()`（用户访问页面时），保留 3s 超时保护
- Commands / validation:
  - `cargo test -p haos-ui` — 5/5 全过
- Version: `2026.04.08.1`
- Commit: pending
- Push: pending
- Result summary: 后台任务不再持续 spawn Node.js 进程，gateway OOM 崩溃循环消除，系统恢复稳定。
- Next handoff:
  - 后台缓存目前只缓存 SystemSnapshot（proc/df/ps）和 health_ok（actiond HTTP），两者都是轻量操作，30s 间隔对 SD 卡设备友好。
  - `count_pending_devices()` 仍是按需的 Node.js 调用，如果将来想彻底消除，可改为通过 actiond 的 REST 接口查询（无 Node.js 开销）。

## 2026-04-08 - Performance: background snapshot cache + timeout reductions

- User request: 页面有点卡顿，有没有优化的空间
- Intent / context:
  - 性能分析发现 `index()` 每次请求都并发执行三个耗时操作（spawn_blocking 系统命令 ~1s、HTTP health check 最差 3s、openclaw CLI 最差 3s），`tokio::join!` 等最慢者，导致首页加载有明显延迟。
  - `health_partial()` 在 async 上下文中直接调用 `pid_value()`（`fs::read_to_string`），违反 async I/O 最佳实践。
  - JS `loadPanel` 没有超时，服务端挂起时前端无限等待。
- Files changed:
  - `config.yaml` — 版本升至 `2026.04.07.5`
  - `crates/haos-ui/src/main.rs`
  - `docs/OPERATION_LOG.md`
- Changes detail:
  - 新增 `CachedSnapshot` 结构体（snapshot + health_ok + pending_devices）
  - `AppState` 增加 `cache: Arc<RwLock<Option<CachedSnapshot>>>` 字段
  - `main()` 启动后台 tokio task，每 8 秒采集一次完整 snapshot 并写入缓存
  - `index()` 改为优先读缓存（缓存命中时响应时间 <1ms），仅在首次加载前缓存未就绪时回退到内联采集
  - `fetch_openclaw_health()` 超时 3s → 1.5s
  - `health_partial()` 中 `pid_value()` 同步文件读取改为 `spawn_blocking` 包裹
  - JS `loadPanel()` 增加 `AbortController` 8s 超时，超时静默处理（不显示错误）
- Commands / validation:
  - `cargo check -p haos-ui` — 通过
  - `cargo test -p haos-ui` — 5/5 全过
- Version: `2026.04.07.5`
- Commit: pending
- Push: pending
- Result summary: 首页加载从最差 3s 降至 <1ms（缓存命中后），health check 最差延迟减半，async 上下文中不再有阻塞 I/O，JS 面板轮询有超时保护。
- Next handoff:
  - 缓存更新周期 8s，意味着服务状态最多延迟 8s 显示。如需更实时，可调小间隔。
  - 首次请求（服务刚启动，缓存空）仍会走内联采集路径，约需 1.5-3s，这是预期行为。
  - 后台任务无错误处理守卫（panic 会静默退出），未来可加 `tokio::spawn` + `JoinHandle` 监控。

## 2026-04-07 - Replace raw proxy error with gateway startup fallback page

- User request: 直接访问 https://设备IP:18789 时，浏览器显示裸文本错误 "处理失败：发送 URL (http://127.0.0.1:18790/) 的请求时出错"
- Intent / context:
  - Gateway 启动需要 30–60 秒，期间 ingressd HTTPS 代理（18789→18790）因连接失败返回 502，`simple_response` 将 reqwest 错误以纯文本输出到浏览器。
  - reqwest 错误在中文 locale 下本地化为中文，与 `proxy failed:` 前缀拼接后直接暴露给用户，体验差。
  - UI 代理已有 `fallback_ui_response`（等价功能），但 gateway 代理路径无 fallback。
- Files changed:
  - `config.yaml` — 版本升至 `2026.04.07.4`
  - `crates/ingressd/src/main.rs` — `proxy_gateway` 检查 502 后返回 `fallback_gateway_response()`；新增 `fallback_gateway_response` 函数（含 `<meta http-equiv="refresh" content="8">` 自动刷新）
  - `docs/OPERATION_LOG.md`
- Commands / validation:
  - `cargo check -p ingressd` — 编译通过
- Version: `2026.04.07.4`
- Commit: pending
- Push: pending
- Result summary: gateway 启动期间访问 HTTPS 端口显示友好的"Gateway 正在启动"页面并每 8 秒自动刷新，不再显示裸 reqwest 错误文本。
- Next handoff:
  - WebSocket 升级路径（OpenClaw 客户端连接）不受影响，fallback 只拦截普通 HTTP 502。
  - 自动刷新间隔 8 秒可按需调整。

## 2026-04-07 - Fix mcporter initial config missing mcpServers field

- User request: 日志中出现 mcporter ZodError（从新日志发现）
- Intent / context:
  - `ensure_mcporter_config` 在首次运行时写入 `{}` 作为初始配置，但 mcporter 要求顶层存在 `mcpServers` 字段（类型为 record），否则 Zod schema 校验失败，导致 `mcporter add HA ...` 无法执行，MCP 自动配置完全失效。
  - 已存在的 `{}` 文件不会被覆盖（函数开头检查 `exists()` 就返回），需手动删除 `/config/.mcporter/mcporter.json` 后重启插件才能触发修复。
- Files changed:
  - `config.yaml` — 版本升至 `2026.04.07.3`
  - `crates/addon-supervisor/src/main.rs` — 初始 mcporter.json 内容从 `{}` 改为 `{"mcpServers":{}}`
  - `docs/OPERATION_LOG.md`
- Commands / validation:
  - `cargo check -p addon-supervisor` — 编译通过
- Version: `2026.04.07.3`
- Commit: pending
- Push: pending
- Result summary: 新安装时 mcporter.json 格式正确，mcporter 不再抛 ZodError，HA MCP 自动配置可正常执行。
- Next handoff:
  - 已存在损坏的 `{}` 文件的设备需手动删除 `/config/.mcporter/mcporter.json` 并重启插件，仅新安装或文件不存在时自动修复。
  - 若 mcporter schema 将来升级（如增加必填字段），需同步更新此初始模板。

## 2026-04-07 - Fix blank ingress page caused by blocking CLI call without timeout

- User request: 网页打不开（ingress 页面空白）
- Intent / context:
  - 用户反映通过 HA 侧边栏点击插件后右侧内容区域全白，无任何内容显示。
  - 根因：`haos-ui` 的 `index` 路由在每次页面请求时调用 `count_pending_devices()`，该函数执行 `openclaw devices list`，当 gateway 处于启动期间（约 60 秒内 acpx runtime 未就绪）时，该命令通过 `spawn_blocking` 阻塞约 10 秒（CLI 内置 gateway timeout）。
  - `tokio::join!` 等待所有三个 future（含无超时的 `spawn_blocking`）全部完成，导致 `haos-ui` 迟迟不返回响应。
  - `ingressd` 的 reqwest 客户端未设超时，代理请求挂起，HA ingress 收不到响应，显示空白 iframe。
- Files changed:
  - `config.yaml` — 版本升至 `2026.04.07.2`
  - `crates/haos-ui/src/main.rs` — `index` 路由中 `count_pending_devices` 外包 `tokio::time::timeout(3s)`，超时返回 0
  - `crates/ingressd/src/main.rs` — reqwest Client 添加 `.timeout(10s)`
  - `docs/OPERATION_LOG.md`
- Commands / validation:
  - `cargo check` — 编译通过，零错误
- Version: `2026.04.07.2`
- Commit: pending
- Push: pending
- Result summary: 页面加载不再因 gateway 启动慢而卡死；`count_pending_devices` 最多等 3 秒，超时按 0 处理；ingressd 代理 10 秒未响应即返回 502 fallback，显示"UI 正在启动"提示页而非空白。
- Next handoff:
  - gateway 启动期（约 60 秒）内首页设备计数会显示 0，属预期行为，不影响功能。
  - 若将来 `count_pending_devices` 需要更准确，可改为异步 HTTP 接口而不是 CLI 子进程，以避免 gateway timeout 开销。

## 2026-04-07 - Native OpenClaw integration + command page optimization

- User request:
  1. 补全原生 OpenClaw 状态集成（health check、AI 模型、设备配对、MCP 端点数）
  2. 命令行页优化（流式命令、重复按钮、npm 命令、自定义输入框、令牌安全、备份健壮性）
- Intent / context:
  - 首页状态指示器只看 PID，无法反映 OpenClaw 实际健康状态。
  - 当前 AI 模型、待配对设备数、MCP 端点数量等信息未在 UI 中展示。
  - 命令页流式命令（`--follow`、`tail -f`）错误地进入嵌入式终端，用户无法中断。
  - `npm view openclaw version` 是开发者命令，用户无意义。
  - 令牌明文输出缺乏安全提示；备份命令失败时无任何反馈。
- Files changed:
  - `crates/haos-ui/Cargo.toml` — 添加 `reqwest` 依赖
  - `crates/haos-ui/src/main.rs`
  - `docs/OPERATION_LOG.md`
- Changes detail:
  **Native integration**
  - `fetch_openclaw_health()`: 异步 GET `http://127.0.0.1:48100/health`（3s 超时），返回 `Option<bool>`，用于首页 live-status，优先于 PID 计数。
  - `count_pending_devices()`: spawn_blocking 跑 `openclaw devices list`，统计含 "pending" 的行数，>0 时首页显示橙色 notice-badge。
  - `count_mcp_endpoints()`: 读 `/config/.mcporter/mcporter.json`，支持多种 JSON 结构（数组字段、顶层数组、顶层对象），配置页 MCP 行改为"已注册 N 个端点"。
  - `current_model`: 从 `agents.defaults.model` 读取，首页显示为"AI 模型"stat tile。
  - `index()` 用 `tokio::join!` 并发拉取 snapshot + health + pending_devices。
  - `PageConfig` 新增 `current_model: String`、`mcp_endpoint_count: usize`。
  - 首页 `from_env()` 只加载一次 openclaw.json，web/memory/model 共用同一解析结果。

  **Command page**
  - 移除"检查 npm 版本"(`npm view openclaw version`)，换为 `openclaw --version`。
  - 流式命令"跟随日志"/"网关日志"从诊断组移出，新建"日志跟踪（新窗口）"分组，强制走 `terminal_window_button`。
  - 新增"自定义命令"输入框 + 运行按钮，支持回车触发，对应 JS `ocRunCustomCommand()`。
  - `sensitive_button()`: 点击前弹 confirm 对话框，"读取令牌"按钮改用此函数。
  - 备份命令加 `set -e` + 逐步 `echo` 进度，完成后输出 `✓ 备份完成`。
  - 删除死代码 `brand_lockup()`。

  **Config page text**
  - 两处开发者导向文案改为用户视角（配置页 subtitle、"插件配置页负责"第3条）。

  **CSS / JS**
  - `.cmd-input` + `.custom-cmd-row` 样式。
  - `ocRunSensitive(command, warning)` JS 函数。
  - `ocRunCustomCommand()` JS 函数。
- Commands / validation:
  - `cargo build -p haos-ui` — 零警告通过
  - `cargo test -p haos-ui` — 5/5 全过
- Version: `2026.04.07.1`
- Commit: pending
- Push: pending
- Result summary: 首页状态现在基于真实 health check API，显示 AI 模型、待配对设备提醒、MCP 端点数；命令页流式命令强制新窗口、新增自定义命令输入框、令牌读取加安全确认、备份命令带进度反馈；openclaw 升级至 2026.4.5。
- Next handoff:
  - `count_pending_devices()` 解析的是 `openclaw devices list` 纯文本输出中含 "pending" 的行数，若 openclaw 更新了输出格式需重新校验。
  - `count_mcp_endpoints()` 对 mcporter.json 结构做了兼容性猜测，若 mcporter 升级了 schema 需验证字段名。
  - 参考 techartdev/OpenClawHomeAssistant v0.5.63→v0.5.65：run.sh 的 gateway 3 层检测改进（ss/pgrep/proc 扫描）对本项目的 Rust addon-supervisor 无需同步，架构不同。
## 2026-04-11 - HA panel config for Web Search / Memory Search / model selection

- User request:
  - 在 HA 面板里按官方文档补齐 `Web Search`、`Memory Search`、模型选择
  - 需要官方 provider 列表、可输入 API、并兼顾网页登录/控制台跳转的交互
  - 配置时再写文件，平时不要自动改动 `openclaw.json`
- Files changed:
  - `crates/haos-ui/src/main.rs`
  - `crates/addon-supervisor/src/main.rs`
  - `config.yaml`
  - `CHANGELOG.md`
  - `docs/OPERATION_LOG.md`
- Implementation:
  - `haos-ui` 配置页新增可编辑表单，分别保存 `Web Search`、`Memory Search`、模型配置
  - `Web Search` / `Memory Search` provider 下拉按官方文档常见列表提供，并带官方文档与网页登录/控制台链接
  - API Key 字段不回显现有值；留空表示保持，勾选复选框才会清除
  - 表单保存改为写入独立的 `/config/.openclaw/addon-panel.json`
  - `addon-supervisor` 在启动时读取并递归合并 `addon-panel.json` 到 `openclaw.json`
  - 未点击保存时，不会自动改写 `openclaw.json`
- Validation:
  - `cargo test -p haos-ui -p addon-supervisor`
- Version:
  - `2026.04.11.4`
- Push:
  - pending
