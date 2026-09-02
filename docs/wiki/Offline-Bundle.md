# 离线分发 / Offline-Bundle

[← 返回首页](Home)

本页计划涵盖整套栈的离线运行路径:如何在联网机构建并打包、离线机如何恢复
并启动,以及不联网给运行中的栈补装工具的思路。

> **TODO** 待填充。计划小节:
>
> - `make save` 打包内容(镜像 + `.env` + 网关哈希 + 场景选择)
> - `make load` + `make up NOBUILD=1` 的离线启动流程
> - 从 GHCR 拉预构建镜像的替代路径(`make pull`)
> - 离线补装工具的手册指引(指向 `docs/offline-install-guide.md`)
> - 烘入 `pi` 场景后的首次启动登记步骤

打包依赖的镜像构建方式见 [架构总览](Architecture) 与
[场景配置](Scenarios);离线环境的常见坑见 [常见问题](FAQ)。
