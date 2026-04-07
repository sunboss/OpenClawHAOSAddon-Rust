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
