# 运行时自由配置：mise 数据目录卷化 + symlink 种子

> 父任务：`.trellis/tasks/09-22-scenario-items-layered-mise/`（需求与架构见其 prd.md / design.md）
>
> **依赖**：本任务依赖兄弟任务 `09-22-scenario-granularity-refactor` —— 它的
> L1 mise engine 拆分确定了 symlink 种子的目标路径 `/opt/mise/installs/*`。
> 需在该任务合入后再开始实现。

## Goal

让用户在容器内 `mise use -g <tool>` 自装的工具**跨 recreate 存活**，同时**不复制**
镜像中烘焙的 1.9G 内容进卷。

解除现有 `scenarios/mise/fragment.Dockerfile` 中的既有约束：

> 运行时 mise use 自装工具落容器可写层,recreate 即丢

## Background

### 现状约束

现设计把 mise 全量重定向到**镜像层** `/opt/mise`：

```
MISE_DATA_DIR=/opt/mise
MISE_CONFIG_DIR=/opt/mise
RUSTUP_HOME=/opt/mise/rustup
CARGO_HOME=/opt/mise/cargo
```

这是为了**躲卷遮盖**（共享卷 `aio_workspace` 挂 `/root`，`~/.local/share/mise`
会被盖住导致 symlink 悬空）。代价是：用户运行时 `mise use` 装的东西落在**容器可写层**，
`recreate` 即丢失。

### 实测验证（2026-09-22 规划期）

**方案可行，且卷开销极小**：

```
镜像烘焙 /opt/mise                    1.9G
  ↓ entrypoint 在卷内为每个 install 建 symlink 指回去
MISE_DATA_DIR → /root/.local/share/mise   ← 卷只占 3.7M
```

实测结果：

| 验证 | 结果 |
|---|---|
| 烘焙工具经 symlink 是否可见 | ✅ `mise ls` 列出 go/opencode/ruff/rust/uv |
| 实际执行 | ✅ `rustc 1.93.1`、`go version go1.23.4` 通过 |
| 用户自装落卷 | ✅ `mise use -g fd@latest` → 卷内 `installs/fd`（3.7M） |
| 卷体积 | ✅ **3.7M**（未复制 1.9G） |

### 配置分层已验证

设 `MISE_GLOBAL_CONFIG_FILE` 指向卷上路径后，`mise ls` **同时读到**镜像级 config
的工具与用户级 config 的工具：

```
fd        10.5.0 (missing)  ~/.config/mise/config.toml  latest   ← 用户装
go        1.23.4                                                 ← 镜像烘
opencode  1.18.24                                                ← 镜像烘
```

### ⚠️ 实现期发现的四个陷阱（全部实测踩到并解决）

规划期的 PoC 只验到「symlink installs 即可」，实现时发现远不止如此：

**陷阱 1 — 必须一并链接 `rustup/` 与 `cargo/`**
`installs/rust/<ver>` 只是指向 `RUSTUP_HOME` / `CARGO_HOME` 的 symlink，
这两个家目录不处理则 rust 全线不可用。

**陷阱 2 — `MISE_GLOBAL_CONFIG_FILE` 是「替换」而非「叠加」** ⚠️ 最反直觉
规划期误判为「配置分层生效」，实现时用 `mise config ls` 验证才发现：设了
`MISE_GLOBAL_CONFIG_FILE` 后它**取代** `MISE_CONFIG_DIR/config.toml`，只列出一个
文件。后果是所有烘焙工具报 "No version is set for shim"。**正解：不设该变量，
把 config 整体放在卷上**（`MISE_CONFIG_DIR` 也指向卷）。

**陷阱 3 — shims 目录不能用 symlink 复用镜像的**
`ln -s /opt/mise/shims $VOL/shims` 会让所有工具报
"rustc is not a valid shim" —— mise 校验 shim 是否属于当前 data dir。
必须 `mise reshim` 在卷上生成真实 shim 农场（~34 个小文件）。

**陷阱 4 — `mise activate` 会把 shims 目录从 PATH 移除**
activate 把 PATH 重写为「已激活工具的 install 目录」，shims 目录不再出现
（实测：activate 前 1 条，后 0 条）。后果是**本次 shell 内刚装的工具不可见**——
`mise use -g X && X` 会失败。**正解：在 `eval "$(mise activate bash)"` 之后
重新追加 shims 目录**。

### 配置过期问题

卷上的 config 是「烘焙 + 用户」的合并体。基座重建（新增工具）后，若不重新生成，
卷上的旧 config 会让新烘焙工具消失。故 seeder **每次启动都重新生成** config：
以镜像 config 为准（烘焙工具权威），awk 提取卷上用户独有的条目再追回。

### 为什么 seeder 放在 app 容器

profile.d 烘在 sandbox-base，被 code-server / vnc 共享。app 容器**总是启动**，
由它播种一次，所有容器的 login shell 都能探测到卷。若改由各容器自行播种，
code-server/vnc 不启动时就没有播种者。

## Requirements

### R1 — 卷内建立烘焙工具的可见性

- 1.1 entrypoint 启动时，在 `MISE_DATA_DIR`（卷）下的 `installs/` 中，为
      `/opt/mise/installs/*` 每个条目建立 symlink
- 1.2 **同时**处理 `rustup/` 与 `cargo/`（见已知陷阱）——否则 rust 不可用
- 1.3 symlink 建立必须**幂等**：容器多次重启不产生重复或错误链接
- 1.4 卷中已存在的用户自装工具**不得被覆盖或删除**

### R2 — 配置分层

- 2.1 `MISE_GLOBAL_CONFIG_FILE` 指向卷上的用户 config
- 2.2 镜像级 config（`/opt/mise/config.toml`）继续被读取
- 2.3 用户 `mise use -g` 写入的是卷上的 config

### R3 — 环境变量调整

- 3.1 `MISE_DATA_DIR` 从 `/opt/mise` 改为卷路径
- 3.2 四个 ENV 重定向（`MISE_DATA_DIR` / `MISE_CONFIG_DIR` / `RUSTUP_HOME` /
      `CARGO_HOME`）需重新评估各自应指向镜像还是卷
- 3.3 ENV + profile.d 双通道设计保持（login / 非 login shell 均可见）

### R4 — 兼容性

- 4.1 未挂载卷的场景（如 `docker run` 裸跑）需优雅降级，不得崩溃
- 4.2 幂等：删除卷后重建，行为与首次一致
- 4.3 离线分发（`make save/load`）路径不被破坏

## Acceptance Criteria

- [x] **AC7a** — 容器内 `mise use -g <新工具>` 后，工具**立即可用**
      （实测 `hyperfine` → 同 shell 内 `hyperfine 1.20.0`）
- [x] **AC7b** — `docker restart`（或 recreate）后该工具**仍可用**
      （restart ×2 + `--force-recreate` ×1 后仍在）
- [x] **AC7c** — 卷体积增量仅为该工具本身，**不含**烘焙内容副本
      （卷内 mise 足迹 **1.5M**，18 个烘焙条目均为 symlink）
- [x] **AC13** — 烘焙的 rust 在新数据布局下 `rustc` / `cargo` 可用（陷阱已解）
      （`rustc 1.93.1` / `cargo 1.93.1`）
- [x] **AC14** — 容器多次 restart 后 symlink 幂等，无重复/断裂
      （3 次启动后仍 18 symlink / 0 断裂 / 35 shims）
- [x] **AC15** — 未挂载卷时容器正常启动
      （`docker run --rm sandbox-base bash` → `/opt/mise`，`rustc 1.93.1` 正常）
- [x] **AC16** — 用户自装的工具与烘焙工具在 `mise ls` 中同时可见
      （login shell 下 19 项，全部来自卷 config）
- [x] **AC17** — 探测机制对**共享卷的兄弟容器**同样生效：`code-server`
      容器的 login shell 看到的是卷布局（播种只有 app 做，探测烘在
      profile.d 里被各容器共享）。实测：code-server 内 `MISE_DATA_DIR` 指卷，
      且 app 里装的 `hyperfine` 可直接执行。
- [x] **AC18** — 非 login shell（ENV 通道）保持**烘焙布局**不变：这是
      刻意的安全默认（裸跑无卷时 PATH/变量仍然正确），代价是
      `docker exec <c> bash -c 'mise use -g X'` 不落卷。已知取舍，记入文档。

## Out of Scope

- **scenario 粒度重构**（子任务 1）
- 用户自装工具的离线分发（卷内容不在 `make save` 范围内）
- mise 的多版本切换（`mise use` 局部版本）

## Notes

- 改动文件预估：`app/entrypoint.sh`、`scenarios/mise/fragment.Dockerfile`（ENV 段）、
  可能涉及 `docker-compose.yml`（卷定义）
- 与 `aio_workspace` 卷的交互需谨慎（`~/.local/share/mise` 位于 `/root` 下，
  正是该卷的挂载点）
- 规划期实测的完整命令序列见父任务 prd.md「关键实测结论」表
