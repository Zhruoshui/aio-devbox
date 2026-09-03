# 架构总览 / Architecture

[← 返回首页](Home)

本页计划涵盖 AIO devbox 的整体架构:各容器的职责与拓扑关系、共享基础镜像的
派生结构、网络命名空间与端口约定、工作区卷的数据持久化方式。

> **TODO** 待填充。计划小节:
>
> - 容器拓扑:gateway / app / code-server / vnc / base 各自的职责
> - 镜像派生关系:`sandbox-base` 与 app / code-server 的 FROM 链
> - 共享网络命名空间与保留端口约定
> - 工作区卷(`/root`)与容器重建后的数据存活
> - 网关鉴权与反向代理路径

[场景配置](Scenarios) 与 [离线分发](Offline-Bundle) 分别展开工具链预置与
离线运行,部署中遇到的具体问题见 [常见问题](FAQ)。
