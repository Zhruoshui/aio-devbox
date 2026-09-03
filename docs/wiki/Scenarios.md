# 场景配置 / Scenarios

[← 返回首页](Home)

本页计划涵盖构建期场景预置系统:如何用 TUI 勾选工具链、分层模型如何组织
场景、版本化运行时的选择方式,以及预设与通配符的用法。

> **TODO** 待填充。计划小节:
>
> - 分层模型(L1–L5)与 `category` 分组
> - TUI 勾选工作流:`make config` → `.aio/enabled.toml` → `make build-base`
> - `always_on` 版本化运行时(Node / Python)的选择方式
> - 预设(`minimal` / `full`)与 `["*"]` 通配符
> - 新增一个场景需要放哪些文件

架构背景见 [架构总览](Architecture),离线机上的场景选择随 bundle 分发,
见 [离线分发](Offline-Bundle);相关问题见 [常见问题](FAQ)。
