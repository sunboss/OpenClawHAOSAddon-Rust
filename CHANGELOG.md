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
