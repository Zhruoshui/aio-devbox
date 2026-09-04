# 离线分发 / Offline-Bundle

[← 返回首页](Home)

本页描述整套栈的离线运行路径:联网机构建并打包、离线机恢复启动、从 GHCR 拉
预构建镜像,以及不联网给运行中的栈补装工具。

## `make save`:联网机打包

```sh
make build        # 先构建最新镜像(场景变更要重烘)
make hash         # 若还没有网关哈希
make save         # 产出 aio-offline-bundle/
```

产物 `aio-offline-bundle/` 四件套:

| 文件 | 内容 |
|---|---|
| `images.tar` | `docker save` 的全部业务镜像(sandbox-base 及派生) |
| `env` | `.env` 副本(网关凭据等) |
| `hash` | 网关 Caddyfile 哈希(离线机校验一致) |
| `enabled.toml` | 场景选择,离线机可按需改选后重建 |

## `make load` + `make up NOBUILD=1`:离线机恢复

```sh
make load                 # docker load images.tar,装回 env / hash / enabled.toml
make up NOBUILD=1         # 跳过一切构建,纯用本地镜像起栈
```

也可以不走 Makefile,裸 `docker load -i aio-offline-bundle/images.tar` +
`docker compose up`。

## 替代路径:`make pull` 拉 GHCR 预构建镜像

不想传大 tar 时,联网机 push 过的镜像可以直接拉(main 分支的 CI 自动发布到
GHCR):

```sh
make pull VARIANT=full    # 或 minimal;默认 full,拉取后 retag 为本地名
make up NOBUILD=1
```

两个变体:`minimal` = 仅 always_on 基线(node + python + pi/pi-web 工作台核心);
`full` = `scenarios = ["*"]` 全部场景(mise [rust/go/uv/ruff/opencode] / c23 /
shell-utils / fonts…,pi / pi-web 为 always_on 基线已含)。镜像有
`:latest` + `:<ref>` 双标签。

## 离线补装工具(不动镜像)

镜像级分发(`make save`/`load`)解决"整套更新";给**已部署**环境补装单个
工具走 `docs/offline-tool-install.md` 的分工具手册:

- 通用套路:联网机把自包含二进制/目录搬过去;落 `~/.local/bin`(卷上,有
  auto-PATH,扛 recreate);
- **mise 管理的工具**(rust/go/uv/ruff/opencode 及 `mise use` 装的任何东西)
  走 §14 的**整目录搬迁**配方,四条实测约束:
  1. **同绝对路径**解压(installs 内部是绝对路径 symlink,换路径即断);
  2. `MISE_DATA_DIR` 与 `MISE_CONFIG_DIR` **必须同时覆盖**且指向同一目录;
  3. **单 tar 同时含 data + config**(全局 `[tools]` 清单就是
     `MISE_CONFIG_DIR/config.toml`);
  4. 离线校验登记用 `MISE_OFFLINE=1 mise install`(缺什么显式报错,不回退)。

  镜像内 `/opt/mise` 的 auto_install 已在烘焙期关闭,离线缺工具不会静默
  hang。

## pi 场景的首次启动登记

`pi` 场景(always_on,任何变体都烘)把 agent 二进制和扩展烘进镜像
(`/opt/pi-extensions`),但登记数据在卷上的 `~/.pi`。**离线首次使用**在终端跑一次:

```sh
aio-pi-extensions         # 把烘好的扩展离线登记进 ~/.pi(卷),之后扛重建
```

打包依赖的镜像构建方式见 [架构总览](Architecture) 与 [场景配置](Scenarios);
离线环境的常见坑见 [常见问题](FAQ)。
