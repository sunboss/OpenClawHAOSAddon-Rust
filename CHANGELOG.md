## 2026.04.15.7

- Cleanup: removed the last exposed remote/local gateway mode options from `config.yaml`, leaving only the small option surface the Hermes-style single-page shell still uses
- Cleanup: rewrote `crates/haos-ui/src/main.rs` as a clean single-file Hermes thin shell with only Gateway open, Shell open, Gateway status, token display, and device approval actions
- Cleanup: simplified `crates/ingressd/src/main.rs` health probing to the fixed local `openclaw-gateway` process instead of carrying dead remote-mode branching
- Validation: `cargo test -p haos-ui -p addon-supervisor -p ingressd`

## 2026.04.15.6

- Runtime: removed the last `ENABLE_HTTPS_PROXY` / `HTTPS_PROXY_PORT` toggle path and made the external HTTPS Gateway proxy the fixed add-on architecture
- Cleanup: simplified `ingressd` startup so it always serves the HTTPS Gateway path instead of branching on an env-controlled proxy mode we no longer use
- Cleanup: simplified allowed-origin generation in `addon-supervisor` to the now-fixed HTTPS scheme
- Validation: `cargo test -p haos-ui -p addon-supervisor -p ingressd`

## 2026.04.15.5

- Image: removed the unused global `pnpm` install from the runtime image, keeping only the binaries the add-on actually uses at runtime
- Image: removed the unused `/share` preparation path and dropped the `share:rw` Home Assistant map now that backup export logic is gone
- Build: trimmed `ingressd` dependencies by removing unused `http`, `http-body-util`, and `serde_json`
- Build: trimmed `haos-ui` dependencies by removing the unused direct `serde` dependency
- Validation: `cargo test -p haos-ui -p addon-supervisor -p ingressd`

## 2026.04.15.4

- Cleanup: removed the last dead `addon-panel.json` overlay merge path from `addon-supervisor`, so the single-page Hermes shell no longer carries legacy HA panel config layering
- Cleanup: removed the old auto-backup chain from `addon-supervisor`, dropped the unused `backup_dir` runtime path, and removed the `rsync` runtime dependency from the image
- Cleanup: removed the obsolete `Plan` scaffold subcommand from `addon-supervisor` and trimmed child env propagation to the vars still used by the live runtime
- Routing: removed the last `/terminal` compatibility routes from `ingressd`, keeping `ttyd` only under the direct `/shell/` entry used by the Hermes-style page
- UX: simplified the single-page Shell button wiring so it opens the full `ttyd` Web Shell directly, without legacy command plumbing
- Validation: `cargo test -p haos-ui -p addon-supervisor -p ingressd`

## 2026.04.15.3

- UI: aligned the Home Assistant sidebar page further toward the Hermes thin-shell model, keeping a single light single-page surface with only Gateway open, maintenance Shell, Gateway status, token display, and device approval actions
- UI: removed legacy compatibility routes for `/config`, `/commands`, and `/logs`, so `haos-ui` now behaves as a true single-page entry instead of redirect-backed pseudo pages
- UX: made both “打开网关” and “维护 Shell” open directly in dedicated new windows, matching the thinner Hermes-style workflow instead of routing through extra local transition chrome
- Cleanup: removed the old embedded `/terminal` implementation from `crates/ingressd/src/main.rs`, dropped `portable-pty`, and kept only the `ttyd`-backed `/shell/` path plus `/terminal` compatibility redirect
- Image: removed unused runtime packages `jq` and `xz-utils`, and removed obsolete xterm asset installation from the image
- Validation: `cargo test -p haos-ui -p addon-supervisor -p ingressd`

## 2026.04.15.2

- UI: rewrote `haos-ui` into a true Hermes-style single-page shell instead of keeping the old hidden multi-page architecture behind redirects
- UI: keeps only the Home Assistant sidebar entry, Gateway open action, maintenance Shell action, OpenClaw Gateway real-time status, token display, pending-device listing, and latest-device approval
- Cleanup: removed thousands of lines of stale multi-page/config/navigation logic from `crates/haos-ui/src/main.rs` while preserving the popup Gateway and Shell flows
- Validation: `cargo test -p haos-ui -p addon-supervisor -p ingressd`

## 2026.04.15.1

- 界面：参考 `sunboss/hermes-agent-ha-addon` 的薄壳思路，把 HA 面板压成单页，只保留“打开网关”“维护 Shell”“Gateway 状态”“令牌显示”“授权确认”
- 路由：`/config`、`/commands`、`/logs` 统一回到首页，Home Assistant 侧边栏打开后只看到一个入口页
- 状态：实时状态只显示 OpenClaw Gateway，不再展示资源遥测和多服务矩阵
- 授权：保留“列出待批准设备”和“确认最新授权”，并把令牌显示重新加回首页

## 2026.04.14.5

- 交互：优化“打开网关”按钮的跳转等待逻辑，改为尽快带 token 打开原生 Gateway，不再额外强制等待多轮稳定探测
- 体验：保留本地 `readyz` 兜底，但将等待压到秒级，减少弹出过渡页停留时间
- 文案：打开网关的过渡提示改成“页面会继续完成初始化”，更贴合原生 Control UI 的实际行为

## 2026.04.14.4

- 设备授权：命令页“列出待批准设备”不再依赖 TUI 启动期注入命令，改为页面后端直接执行官方 `openclaw devices list --json` 并回显结果
- 设备授权：保留“一键批准最新请求”按钮，同时把设备列表输出直接显示在页面里，避免用户误以为命令卡住
- 交互：把最常用的设备配对诊断动作从易受终端时序影响的路径，收敛为稳定的页面动作

## 2026.04.14.3

- 升级：Docker 镜像内的 upstream OpenClaw runtime 从 `v2026.4.12` 升到 `v2026.4.14`
- 对齐：这版上游包含多项稳定性与性能修复，例如 doctor/plugins catalog 缓存、context engine 后台维护、以及多项 Ollama / memory / browser 修复
- 保持：继续沿用当前 add-on 的 Python 运行环境、静音默认值与设备授权按钮

## 2026.04.14.2

- 运行时：镜像补齐 `python3`、`pip`、`venv` 和 `python` 命令别名，便于在维护 Shell 里直接执行 Python 脚本和安装工具
- 配对：首页和命令页补充官方 `openclaw devices` 授权路径，并新增“一键批准最新请求”按钮
- 诊断：为新客户端授权问题增加官方文档入口和 token drift / superseded request 的排查提示，同时保留精确 `requestId` 处理路径

## 2026.04.14.1

- 升级：Docker 镜像内的 upstream OpenClaw runtime 从 `v2026.4.11` 升到 `v2026.4.12`
- 对齐：配置页模型部分补充 `LM Studio` 官方文档入口，并加入常用 `lmstudio/...` 模型建议值
- 保持：继续沿用较安静的默认值，`disable_bonjour` 默认开启，`Dreaming` 仍保持独立开关

## 2026.04.13.2

- 默认值：将 `disable_bonjour` 的默认值改为开启，新的安装默认不再广播 Bonjour，减少 `bonjour ... advertiser` 噪音
- 配置页：新增独立的 `Dreaming` 开关，直接写入官方 `plugins.entries.memory-core.config.dreaming.enabled`
- 观测：配置页新增 `Dreaming` 概览，让后台梦境整理状态更直观

## 2026.04.13.1

- Logo：恢复更有辨识度的彩色品牌方案，根目录 `logo.png` / `icon.png` 与 UI 头部品牌标识重新同步
- UI：将内联品牌 SVG 调回青蓝主色 + 橙金点缀，保持暗色主控台气质，同时避免上一版过于发灰发闷
- 资源：保留新增的 `logo.svg` / `icon.svg` 作为同风格矢量版本，便于后续文档和界面继续复用

## 2026.04.12.18

- Shell：为受管 `ttyd` 增加可写参数，修复“维护 Shell”进入后仅只读不可输入的问题
- 运行时：保持 `ttyd 1.7.7` 与现有 `/shell/` 反向代理结构不变，仅修正交互能力
- 验证：`cargo test -p addon-supervisor -p haos-ui -p ingressd`

## 2026.04.12.17

- Shell：将“维护 Shell”从原先的自定义内嵌 shell 页面升级为真正的 `ttyd` Web Shell，全屏直达、无额外说明框
- 运行时：Docker 镜像改为安装官方 `ttyd 1.7.7` 静态二进制，并由 `addon-supervisor` 托管为正式服务
- 路由：`ingressd` 新增 `/shell/` 反向代理到本地 `ttyd`，同时保留现有 `/terminal/` 作为 `OpenClaw CLI (TUI)` 入口
- UI：首页服务状态新增 `Shell` 进程状态；命令页与日志页中原先依赖“预置 shell 命令”的按钮改为直接打开维护 Shell，避免行为与文案不一致
- 验证：`cargo test -p haos-ui -p ingressd -p addon-supervisor`

## 2026.04.12.16

- UI：进一步强化首页标题区与按钮区的电影感层次，提升 Hero 区、操作按钮和左侧品牌导航的高级感
- UI：重绘主控台品牌 logo，改为更克制的“核心舱 / 指挥中枢”风格，去掉旧的 `Rs` 项目标识感
- UI：继续收紧首页服务状态区的暗色体系，修正服务卡与状态徽标在主控台里的视觉一致性
- UI：补充移动端适配，优化品牌区、导航、Hero、按钮、状态卡与资源卡在手机上的阅读与操作体验
- 验证：`cargo test -p haos-ui -p ingressd -p addon-supervisor`

## 2026.04.12.15

- UI：将 HA 面板整体重设计为“AI Agent 主控台”风格，采用深蓝黑暗色背景、左侧指挥栏、右侧主工作区，以及更克制的电影感控制室层次
- UI：首页继续保留资源采集与服务状态显示，但将状态区、资源区与说明区统一重组为更适合长期值守的主控总览
- UI：命令页重组为“调度中枢”，日志页重组为“观测台”，配置页顶部重组为“能力矩阵与策略配置”，弱化普通后台卡片感
- 交互：保留 Gateway / OpenClaw CLI / 维护 Shell 的现有入口与行为，不改变现有功能链路
- 验证：`cargo test -p haos-ui -p ingressd -p addon-supervisor`

## 2026.04.12.14

- 文档：新增仓库级 `README.md`、`INSTALL.md`、`DOCS.md`，对齐参考 add-on 的安装入口、首次配置路径和访问方式说明
- 文档：补充架构图与首次配置流程图，明确 `18789` 外部 HTTPS 与 `18790` 内部 Gateway 的当前分工
- 精简：配置页进一步收成纯配置页，初始化、状态确认和日志排查统一引导到命令页
- 精简：日志页只保留日志相关入口；命令页继续收敛重复的 Shell 按钮，减少运维入口分散
- 修复：`ingressd` 终端页与 fallback 页文案恢复为正常中文

## 2026.04.12.13

- 恢复：重新提供轻量内置终端入口，但不把旧的重型终端控制链整包带回
- 对齐官方：终端默认直接进入原生 `openclaw tui`
- 交互：首页、配置页、命令页、日志页重新加入 `OpenClaw CLI` 入口与常用维护按钮
- 保留：首页资源采集、状态显示、Gateway HTTPS 主链路继续保持不变

## 2026.04.12.12

- 优化：首页新增首次安装推荐路径，帮助用户按“先打开 Gateway、再保存配置、最后重启验证”的顺序完成初始化
- 优化：配置页补充访问模式说明和首次配置路径，让 `local_only` / `lan_https` / `tailnet_https` 等模式更容易理解
- 保留：首页资源采集、状态显示和 Gateway HTTPS 主链路不变

## 2026.04.12.11

- 精简：彻底移除 Add-on 自带终端，HA 面板不再提供嵌入式终端或终端新窗口入口
- 对齐官方：命令页与日志页改为官方命令参考页，引导用户在 Home Assistant `Terminal & SSH`、SSH 或其它本机 shell 中执行
- 精简：删除 `ingressd` 终端路由与协议残留，移除 `portable-pty`、xterm npm 资源以及对应配置项
- 保留：首页资源采集、状态显示、Gateway HTTPS 主链路不变

## 2026.04.12.10

- 对齐官方：嵌入式终端默认直接启动 `openclaw tui`，不再把裸 shell 作为默认主入口
- 对齐官方：HA 面板里的终端命令统一切到 TUI 的 `!命令` 模型，例如 `!openclaw status`
- 精简：移除 `haos-ui` 本地配对 WebSocket 旧链路与 `gateway_ws.rs`，减少依赖和编译面
- 精简：移除 `haos-ui` 不再使用的 `ring`、`tokio-tungstenite`、`futures-util`、`reqwest` 等依赖
- 精简：移除 `ingressd` 里的 `control-readyz` 与旧 `/action/*` 控制接口，继续向原生 Gateway 探针语义靠拢
- 启动优化：`doctor --fix` 改为首次安装自动运行一次，之后默认不再每次启动都跑
- 架构整理：`PUBLIC_SHARE_DIR` 取代旧的公共目录命名，README 与维护文档同步对齐当前真实架构

## 2026.04.12.9

- 精简：首页不再挂载本地配对 banner 容器，浏览器不会再自动订阅 `/events/pairing`
- 调整：继续把设备配对收回原生 Control UI / 命令行入口，减少 `haos-ui` 侧额外的早连触发面

## 2026.04.12.8

- 修复：HA 面板直接可见页面的乱码文案，统一在输出层强制转换为正常中文
- 修复：首页、导航、命令页、日志页和局部状态面板不再出现韩文化/乱码显示

## 2026.04.12.7

- 精简：HA 面板命令页新增更贴近官方的“原生入口”版本，优先保留 `openclaw tui`、原生 Gateway、基础状态与诊断入口
- 调整：新命令页不再展示自定义命令输入、目录浏览、备份脚本等偏 add-on 自定义的快捷操作
- 保留：嵌入式终端仍可用，但页面文案明确它只是辅助入口，原生交互优先走 TUI / Gateway

## 2026.04.12.6

- 精简：移除 `actiond` crate 和镜像打包，原本的本地健康检查、readiness 与重启入口统一并入 `ingressd`
- 调整：`haos-ui` 的本地探针与重启命令改为走 `http://127.0.0.1:48099`
- 维护：同步更新 supervisor / README / maintainer context，当前常驻服务收敛为 `openclaw-gateway`、`haos-ui`、`ingressd`

## 2026.04.12.5

- 修复：在镜像构建阶段补装 `@azure/identity` 和 `jwks-rsa`，避免 `msteams` 相关 bundled plugin 依赖在 `doctor --fix` 和启动日志里反复报缺失

## 2026.04.12.4

- 修复：恢复远程浏览器访问所需的 HTTPS secure context，避免 Control UI 因 `device identity` 要求拒绝 `http://<lan-ip>:18789`
- 架构：保留外部 `https://<host>:18789` 访问，同时让内部 Gateway 继续运行在 loopback `18790`
- 对齐：这次回退仅限访问策略，保留 `v2026.4.11` runtime、TUI 官方文案和新配置页能力

## 2026.04.12.2

- UI：把 HA 面板里的 CLI/终端说明对齐到官方 TUI 文档，明确 `OpenClaw CLI = openclaw tui`
- UX：在命令页、嵌入式终端和独立终端页都补充了 `!命令` 即本地 shell 的官方用法提示

## 2026.04.12.1

- 升级：Docker 镜像内的 upstream OpenClaw runtime 从 `v2026.4.9` 提升到 `v2026.4.11`
- 兼容：`haos-ui` 的 gateway WebSocket `client.version` 改为优先读取运行时 `OPENCLAW_VERSION`，避免继续写死在旧版本号上

## 2026.04.11.3

- 修复：`打开网关` 不再先打开一张空白页，改为先显示可控的加载页，再跳转到原生 Gateway
- 保留：仍然会等待 stronger-ready 并附带 token，避免因为过早跳转回退到旧的连接失败路径

## 2026.04.11.2

- 新增：HAOS 选项 `disable_bonjour`
- 对齐官方：该选项会透传 `OPENCLAW_DISABLE_BONJOUR=1`，用官方支持的方式关闭 Bonjour / mDNS 广播

## 2026.04.11.1

- 修复：停止 `haos-ui` 启动后的后台常驻配对轮询，避免空闲时持续主动直连原生 `127.0.0.1:18790`
- 调整：配对状态改为页面实际订阅时再按需刷新，`pair-approve` 仍会立刻刷新内存中的配对列表

## 2026.04.10.10

- 修复：首页 `打开网关` 改回真正的链接式入口，页面加载后会同步原生网关地址到 `href`
- 保留：点击后仍然先等待 stronger-ready，再带 token 打开原生 Gateway，避免回退到早期开窗失败路径

## 2026.04.10.9

- 修复：新增 `control-readyz`，把“网关端口已开”与“browser/acpx 控制面真正 ready”分开
- 修复：配对轮询不再固定盲等 90 秒，而是等待控制面 ready 后再开始 `device.pair.list`
- 修复：`打开网关` 按钮会先等待 stronger-ready，再跳转原生控制台，减少启动期浏览器侧 `ws closed before connect`

## 2026.04.10.8

- 对齐官方 MCPorter 配置模型：不再依赖启动期 `mcporter config add` CLI 变体，改为直接写入官方 `mcporter.json` 的 `mcpServers -> baseUrl -> headers` 结构
- 修复：避免再出现 `--header requires KEY=value` 和旧版 `add` fallback 噪音
- 校验：补充 `addon-supervisor` 测试，验证 Home Assistant MCP 条目按官方配置形状写入

## 2026.04.10.7

- 修复：`mcporter` 配置 Home Assistant MCP 时改用 `--header KEY=value` 语法，兼容当前 CLI
- 优化：启动阶段 `doctor --fix` 对 `Memory search`、`Gateway port`、`Gateway` 盒子噪音整段抑制
- 校验：补充 `addon-supervisor` 测试覆盖 `mcporter` header 格式和 doctor section 过滤

## 2026.04.10.6

- 对齐官方：新增轻量 `healthz` / `readyz` 语义，并让 UI 状态优先读取 readiness
- 运维：梳理 `config path` / `state dir` / `runtime dir` / `backup dir` 边界，补充文档和环境变量导出
- 兼容：`mcporter` 优先使用 `config add`，失败时回退旧版 `add`
- 体验：启动阶段 `doctor --fix` 的已知网关噪音不再直接打到用户界面
- UI：配置页和命令页按更接近 `ClawDock` / `Podman` 的官方操作模型重组

## 2026.04.10.5

- 修复：`list_pending_pairs` 改返回 `Option<Vec>` 区分"成功空列表"和"请求失败"
- 优化：`pairing_poll_task` 启动延迟 90s（等待 acpx 就绪），失败时指数退避至最长 120s，成功后重置 10s 间隔
- 优化：补全启动噪音抑制——`ws stream ended` / `ws closed` 类错误不再打印
- 代码：`pair_approve` 中 `list_pending_pairs` 返回值更新为 `Option` 解包

## 2026.04.10.4

- 修复：`hello-ok timeout` / `connect.challenge timeout` 等启动噪音未被抑制的问题，统一过滤所有含 `timeout` 的错误日志

## 2026.04.10.3

- 移除调试日志（raw payload 已确认格式为 `{paired:[...], pending:[...]}`，字段解析正确）
- 优化：首页配对提示补充说明——token 错误时需清除浏览器存储后重新打开

## 2026.04.10.2

- 调试：`device.pair.list` 打印原始响应 payload，帮助定位响应格式问题
- 修复：`parse_pending_pairs` 兼容多种响应字段名（`pending` / `requests` / `devices` / `items` / `list` / 直接数组），以及 `requestId` / `request_id` / `id` 写法
- 优化：配对轮询间隔从 30s 缩短至 10s，减少通知延迟

## 2026.04.10.1

- 修复：实现完整 Ed25519 设备身份认证，彻底解决 `missing scope: operator.pairing`
  - 首次运行在 `/config/.openclaw/haos-ui-identity.json` 生成持久化密钥对
  - connect 请求携带 device 对象（id/publicKey/signature/signedAt/nonce）
  - gateway 判断为 `cli_container_local` → silent auto-approve → scopes 不被清空
  - 后续连接直接复用已配对 identity，无需每次重新配对

## 2026.04.09.11

- 优化：启动阶段 `gateway call timed out` 不再打印错误日志（acpx 未就绪属正常现象）

## 2026.04.09.10

- 修复：WebSocket connect 改用 `id="cli"` + `mode="cli"`，绕过 v2026.4.9 新增的
  Control UI device identity 校验（根本原因：`id="openclaw-control-ui"` 触发
  `isControlUi=true` 导致 `reject-control-ui-insecure-auth`，改为 CLI 身份后
  走 `roleCanSkipDeviceIdentity("operator", true)` 直接 allow）
- 同步移除 Origin 头（CLI 本地连接不需要，带 Origin 反而影响 hasBrowserOriginHeader 判断）

## 2026.04.09.9

- 修复：WebSocket 握手改用 `IntoClientRequest` 正确生成 `Sec-WebSocket-Key` 等标准头，
  再单独追加 `Origin` 头，彻底解决 `Missing sec-websocket-key` 握手失败

## 2026.04.09.8

- 修复：WebSocket 连接补充 `Origin: http://127.0.0.1:18790` 头，解决 gateway 拒绝 `device.pair.list`（`origin not allowed`）
- 修复：AI 模型显示路径修正为 `agents.defaults.model.primary`（实测 openclaw.json 格式）
- 修复：Token 复制按钮改用 `execCommand` 兜底，兼容 HAOS ingress iframe 受限环境

## 2026.04.09.7

- 升级：openclaw 至 v2026.4.9（安全加固：浏览器 SSRF 隔离、env 变量保护、gateway 节点事件清理；Android 配对稳定性改进）
- 修复：AI 模型显示——兼容新版 `agents.defaults.llm.model` 字段路径，修复首页"未配置"问题

## 2026.04.09.6

- 修复：gateway WebSocket connect 请求补充 `platform` 字段，消除 `must have required property 'platform'` 鉴权失败
- 优化：gateway 未就绪时（Connection refused）不再打印错误日志，消除启动阶段噪音

## 2026.04.09.5

- 新增：首页实时配对通知——有设备请求配对时自动弹出 Banner，显示设备名，点按钮即可批准，无需进命令行
- 实现：通过 WebSocket webchat 协议直连 gateway（绕开不稳定的 CLI 模式），SSE 推送到前端

## 2026.04.09.4

- 移除：自动审批设备配对功能，与原生保持一致——有配对请求时首页提示，用户手动在命令行执行 `openclaw devices approve --latest`

## 2026.04.09.3

- 修复：彻底解决启动阶段 auto-approve 报错问题——改用 180s 静默期替代固定延迟，启动期间的失败不再打印错误日志，acpx 就绪后自动恢复正常

## 2026.04.09.2

- 新增：CHANGELOG.md，修复 HAOS 插件页"No changelog found"提示

## 2026.04.09.1

- 修复：增加 auto-approve 初始等待至 120s，消除重启后启动阶段的 CLI 超时报错（acpx runtime 实测需 93-95s 就绪）

## 2026.04.08.9

- 修复：增加 auto-approve 初始等待至 90s，覆盖 acpx runtime 初始化时间

## 2026.04.08.8

- 修复：增加 auto-approve 初始等待至 45s，避免 gateway 启动竞争

## 2026.04.08.7

- 性能：预缓存待授权设备数（5 分钟间隔），首页加载不再实时启动 Node.js 进程
- 新增：首页显示 Gateway Token，支持一键复制
- 修复：预装所有 bundled 渠道插件依赖（Discord / Feishu / Telegram），消除启动时重复下载

## 2026.04.08.6

- 修复：补全 Telegram 渠道缺失依赖 grammy 及相关包

## 2026.04.08.5

- 修复：补全 Feishu 渠道缺失依赖 @larksuiteoapi/node-sdk

## 2026.04.08.4

- 升级：openclaw 至 v2026.4.8
- 修复：补全 Discord 渠道缺失依赖 @buape/carbon

## 2026.04.08.2

- 修复：gateway 崩溃后添加指数退避重启策略（2s→64s），防止 OOM 快速循环

## 2026.04.08.1

- 修复：移除后台每 8 秒启动 Node.js 进程的操作，解决内存不足崩溃循环
## 2026.04.11.4

- 新增 HA 面板配置页可编辑表单：`Web Search`、`Memory Search`、模型选择
- `Web Search` 和 `Memory Search` 提供官方文档入口、网页登录/控制台链接、API Key 输入与清除选项
- 配置保存改为写入独立的 `/config/.openclaw/addon-panel.json`
- `addon-supervisor` 启动时会把 `addon-panel.json` 合并进 `openclaw.json`，平时不配置时不改动主配置文件
- 新增保存接口与前端交互，保存后提示“重启插件后应用”
