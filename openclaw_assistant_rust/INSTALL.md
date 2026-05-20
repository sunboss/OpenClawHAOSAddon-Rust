# 安装指南

## 添加仓库

1. 打开 Home Assistant
2. 进入 `Settings -> Add-ons -> Add-on Store`
3. 打开右上角菜单，点击 `Repositories`
4. 添加：

```text
https://github.com/sunboss/OpenClawHAOSAddon-Rust
```

5. 刷新商店并打开 **OpenClawHAOSAddon-Rust**

## 安装前说明

这个 add-on 不是把 OpenClaw 整体重写成 Home Assistant 风格，而是：

- 保留官方 OpenClaw 运行时
- 用 Rust 做 HAOS 适配层
- 通过 HTTPS 和 Ingress 让 Control UI 能在 HAOS 里稳定访问

## 第一次启动前建议

建议至少先准备：

- 一个主模型
- 对应的 API Key 或 Base URL
- 明确是否需要 Web Search
- 明确是否需要 Memory Search

如果你只是先跑起来，推荐最小起步：

1. 只配主模型
2. 只配一组 API
3. 先不要打开太多高级能力

## 第一次启动后怎么验证

启动 add-on 后，优先检查：

1. 首页服务状态是否全部在线
2. 首页资源采集和系统状态是否正常显示
3. `打开网关` 是否能正常进入 HTTPS Gateway
4. `OpenClaw CLI` 是否能进入原生 TUI
5. `维护 Shell` 是否能打开本机 shell

## 常见入口

- 打开官方 Gateway：首页 `打开网关`
- 打开原生 TUI：首页 `OpenClaw CLI`
- 打开维护 Shell：首页 `维护 Shell`
- 查看配置说明：配置页
- 查看命令参考和诊断入口：命令页 / 日志页

## 如果启动较慢

首次安装时，add-on 会自动执行一次启动期修复流程，因此第一次启动可能比后续启动更慢。

这是预期行为；后续正常重启默认不会再每次都运行同样的修复步骤。
