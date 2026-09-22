# design.md — 运行时卷化

> 父任务架构见 `.trellis/tasks/09-22-scenario-items-layered-mise/design.md`。

## 1. 问题

烘焙的工具链在**镜像层** `/opt/mise`（因为工作区卷挂 `/root`，会遮住 mise 默认的
`~/.local/share/mise`）。这对烘焙工具正确，但导致用户运行期 `mise use -g <tool>`
落在**容器可写层**，recreate 即丢。

## 2. 方案：卷作为 data dir + symlink 引用镜像

```
镜像层                            卷层(/root，持久)
/opt/mise/installs/rust/1.93.1 →  ~/.local/share/mise/installs/rust  (symlink)
/opt/mise/rustup/              →  ~/.local/share/mise/rustup         (symlink)
/opt/mise/cargo/               →  ~/.local/share/mise/cargo          (symlink)
/opt/mise/config.toml          →  ~/.local/share/mise/config.toml    (复制+合并)
                                  ~/.local/share/mise/shims/         (reskim 生成)
                                  ~/.local/share/mise/installs/<用户新装>
```

卷开销 = symlink + ~34 个 shim 文件 + config（实测 **1.5MB**，其中大头是用户
自装工具本身；空载时 32KB）。

## 3. 职责划分

```
app/entrypoint.sh 启动 → 调 aio-mise-volume.sh → 播种(seed)
/etc/profile.d/mise.sh (每次 login shell)      → 探测(probe)并选 data dir
```

**为什么探测放 profile.d**：它烘在 sandbox-base，被 code-server / vnc 共享。
app 容器总是启动，播种一次，所有容器的 login shell 都受益。若在运行期改写
profile.d，只能修好改写者自己那个容器。

**为什么播种放 app**：app 是唯一总会被启动的容器。

## 4. 四个实测陷阱（本设计的核心知识）

| # | 陷阱 | 现象 | 正解 |
|---|---|---|---|
| 1 | 只链 `installs/` | rust 可用但 cargo/rustup 相关报错 | 一并链 `rustup/` `cargo/` |
| 2 | 设 `MISE_GLOBAL_CONFIG_FILE` | 全部烘焙工具 "No version is set for shim" | **不设**；config 整体放卷，`MISE_CONFIG_DIR` 也指卷 |
| 3 | `shims/` 用 symlink 指向镜像 | 全部 "is not a valid shim" | 必须 `mise reshim` 生成真实 shim |
| 4 | 只在 activate 前设 PATH | 本次 shell 内新装工具不可见 | activate **之后**重新追加 shims 目录 |

陷阱 2 最反直觉：规划期 PoC 读 `mise ls` 输出误判为「配置分层生效」，实现时用
`mise config ls` 才发现只列出一个文件。**PoC 结论必须用更精确的探针复核。**

## 5. 契约

### 5.1 环境变量

| 变量 | baked-only 模式 | 卷模式 |
|---|---|---|
| `MISE_DATA_DIR` | `/opt/mise` | `$HOME/.local/share/mise` |
| `MISE_CONFIG_DIR` | `/opt/mise` | `$HOME/.local/share/mise` |
| `RUSTUP_HOME` | `/opt/mise/rustup` | `$HOME/.local/share/mise/rustup` |
| `CARGO_HOME` | `/opt/mise/cargo` | `$HOME/.local/share/mise/cargo` |
| `MISE_GLOBAL_CONFIG_FILE` | **不设** | **不设** |
| `PATH` | activate + `/opt/mise/shims` | activate + `$VOL/shims:/opt/mise/shims` |

### 5.2 config 合并规则

每次启动重新生成 `$VOL/config.toml`：

```
镜像 config（权威，含全部烘焙工具）
  + 卷 config 中「镜像没有的」条目（用户自装）
```

保证：基座新增工具 → 卷自动获得；用户自装 → 不被覆盖。

### 5.3 幂等性

- symlink：`ln -sfn` 重指；用户自建的真实目录/文件**跳过**（不覆盖）
- config：每次重生成，结果确定
- shims：`mise reshim` 可重复执行

## 6. 兼容性

- **无卷裸跑**（`docker run --rm <base> bash`）：`mountpoint` 检查失败 → seeder
  直接 exit 0 → profile.d 探测不到 → 回落 baked-only。**零行为变化**
- **未烘 mise 的基座**：`[ -d /opt/mise ]` 失败 → 同样 no-op
- **失败隔离**：entrypoint 里 seeder 失败只打日志，不阻止容器启动；烘焙工具
  经镜像 shims 仍可用

## 7. 风险

| 风险 | 缓解 |
|---|---|
| seeder 失败导致用户工具丢失 | 双 shim 目录兜底；失败仅打日志 |
| config 合并的 awk 解析 TOML 不完整 | 只处理 `key = value` 与 `[section]`；镜像 config 由 fragment 生成，格式受控 |
| `install_name` 变化的工具 | 边缘情况；reskim 后仍按 config 解析 |
