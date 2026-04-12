# OpenClawHAOSAddon-Rust

![OpenClawHAOSAddon-Rust](./logo.png)

[![Open your Home Assistant instance and show the add add-on repository dialog with a specific repository URL pre-filled.](https://my.home-assistant.io/badges/supervisor_add_addon_repository.svg)](https://my.home-assistant.io/redirect/supervisor_add_addon_repository/?repository_url=https%3A%2F%2Fgithub.com%2Fsunboss%2FOpenClawHAOSAddon-Rust)
[![GitHub last commit](https://img.shields.io/github/last-commit/sunboss/OpenClawHAOSAddon-Rust)](https://github.com/sunboss/OpenClawHAOSAddon-Rust/commits/main)
![Supports aarch64](https://img.shields.io/badge/aarch64-yes-green.svg)
![Supports amd64](https://img.shields.io/badge/amd64-yes-green.svg)
![Home Assistant Add-on](https://img.shields.io/badge/Home%20Assistant-Add--on-18bcf2?logo=homeassistant&logoColor=white)

这个仓库提供一个 Home Assistant Add-on，用来在 HAOS 中运行 OpenClaw，并尽量保持接近官方 OpenClaw 运行方式。

当前实现的核心思路是：

- 保留官方 `openclaw` 运行时
- 用 Rust 适配 HAOS 启动、Ingress、HTTPS 和状态面板
- 将首次安装、访问方式、配置保存路径收敛到更易维护的薄壳

## 安装

点击上面的按钮，把这个仓库添加到你的 Home Assistant 实例。

如果你想手动添加：

1. 打开 `Settings -> Add-ons -> Add-on Store`
2. 打开右上角菜单，选择 `Repositories`
3. 添加 `https://github.com/sunboss/OpenClawHAOSAddon-Rust`
4. 在商店里找到 **OpenClawHAOSAddon-Rust**

## 快速开始

1. 从这个仓库安装 add-on
2. 打开 add-on 的配置页，先配置模型和 API
3. 启动 add-on
4. 从首页点击 `打开网关` 进入 Web UI
5. 如果需要 CLI / TUI，点击 `OpenClaw CLI`
6. 如果需要本机维护 shell，点击 `维护 Shell`

## 当前访问方式

- Home Assistant 面板入口：HA Ingress
- 外部浏览器入口：`https://<HA_IP>:18789`
- 内部 Gateway：`127.0.0.1:18790`
- Browser control 派生端口：上游控制链自动派生

之所以保留外部 HTTPS 入口，是因为官方 Control UI 在远程浏览器场景下需要安全上下文。

## Add-on 内容

### [OpenClawHAOSAddon-Rust](./DOCS.md)

![Supports aarch64](https://img.shields.io/badge/aarch64-yes-green.svg)
![Supports amd64](https://img.shields.io/badge/amd64-yes-green.svg)

这个 add-on 负责：

- 启动官方 `openclaw gateway run`
- 管理首次安装引导和基础配置保存
- 暴露 HA Ingress 与 HTTPS 访问入口
- 提供首页资源采集、服务状态和基础运维入口

## 首次配置建议

建议首次安装时按这个顺序做：

1. 配置主模型
2. 配置 OpenAI / OpenRouter / 自定义 Base URL 等 API 信息
3. 如需 Web Search，再配置对应 provider 和 API Key
4. 如需 Memory Search，再配置 provider、embedding model 和后备策略
5. 启动 add-on 后，从首页进入 Gateway 或 CLI 验证

## 文档

- 运行说明：[DOCS.md](./DOCS.md)
- 安装说明：[INSTALL.md](./INSTALL.md)
- 维护上下文：[docs/MAINTAINER_CONTEXT.md](./docs/MAINTAINER_CONTEXT.md)
- 官方 OpenClaw 文档：[docs.openclaw.ai](https://docs.openclaw.ai/)
