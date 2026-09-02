# AIO DevBox Wiki / AIO 开发沙箱 Wiki

欢迎来到 AIO DevBox 的项目 Wiki。这里是文档的主入口,内容与仓库内的
`docs/wiki/` 目录保持同步(由 GitHub Action 自动发布)。

**AIO devbox** 是一个 Docker + WebUI 的个人开发沙箱,受
[agent-infra/sandbox](https://github.com/agent-infra/sandbox) 启发:一条
`docker compose up` 拉起 Caddy 网关和若干可插拔服务容器,浏览器里呈现一个
带可折叠侧边栏按钮的工作区——浏览器版 VSCode、VNC 里的 Chromium、终端、
按需打开的 AI agent TUI,以及构建期场景预置的工具链,并支持离线分发。

- 主仓库:<https://github.com/Zhruoshui/aio-devbox>
- 快速上手请看仓库根目录的 README([English](https://github.com/Zhruoshui/aio-devbox/blob/main/README.md) ·
  [中文](https://github.com/Zhruoshui/aio-devbox/blob/main/README.zh-CN.md))

## 页面导航

- [架构总览](Architecture) —— 容器拓扑、镜像派生、网络与卷
- [场景配置](Scenarios) —— 分层模型、TUI 勾选、版本化运行时
- [离线分发](Offline-Bundle) —— `make save` / `make load` 与离线运行
- [常见问题](FAQ) —— 安装、启动、排错

## 状态说明

本 Wiki 目前是骨架阶段:各页面已建好目录与计划小节,正文将逐步填充。
具体内容以仓库 README 为准。
