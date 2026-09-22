# implement.md — 运行时卷化执行计划

## 阶段 0 · 前置

- [x] 子任务 1（scenario 粒度重构）已合入 —— L1 engine 已独立，目标路径确定
- [x] 记录回滚锚点（`git rev-parse HEAD`）
- [x] 磁盘余量检查（完整构建前 ≥25G）

## 阶段 1 · 播种脚本

- [x] **1.1** 新建 `app/aio-mise-volume.sh`
  - `link_tree` 幂等 symlink（不覆盖用户自建条目）
  - 链 `installs/` **+ `rustup/` + `cargo/`**（陷阱 1）
  - `mise reshim` 生成卷 shim 农场（陷阱 3）
  - config 合并生成：镜像权威 + 用户独有条目（awk）
  - `mountpoint` 检查 —— 非挂载点直接 no-op
- [x] **1.2** 语法检查 `sh -n`

## 阶段 2 · profile.d 探测

- [x] **2.1** 改写 `scenarios/mise/fragment.Dockerfile` 的 profile.d
  - 多行 heredoc（可读性）
  - 探测 `$HOME/.local/share/mise/installs` 存在与否
  - 卷模式：四个变量指卷，**不设 `MISE_GLOBAL_CONFIG_FILE`**（陷阱 2）
  - **`eval activate` 之后**追加 shims 目录（陷阱 4）
- [x] **2.2** 语法检查

## 阶段 3 · 接线

- [x] **3.1** `app/Dockerfile` COPY 脚本到 `/usr/local/bin/aio-mise-volume`
- [x] **3.2** `app/entrypoint.sh` 在 pi-web 自启**之前**调用（失败仅告警）

## 阶段 4 · 隔离环境实测（关键）

在 `sandbox-base` 容器内逐项验证，**每项都用真实卷 + `/root` 挂载**：

- [x] **4.1** 播种：卷体积 32KB（非 1.9G）
- [x] **4.2** AC13：烘焙 rust/go/uv 在卷模式下可用
- [x] **4.3** AC7a：同 shell 内 `mise use -g hyperfine && hyperfine --version` 通过
- [x] **4.4** AC7b：新容器（recreate）后用户装的 hyperfine 仍在
- [x] **4.5** AC7c：卷增量仅含该工具本身
- [x] **4.6** AC16：`mise ls` 同时列出烘焙与用户工具
- [x] **4.7** AC15：无卷裸跑回落 baked-only，rust 正常
- [x] **4.8** 无卷时 seeder no-op（mountpoint 检查生效）
- [x] **4.9** config 合并：删掉卷上某烘焙条目 → 重播种后恢复；用户条目保留

## 阶段 5 · 真实栈验证

> ⚠️ 首次 `make build-base` 时 `Dockerfile.base` 是旧的（上次构建绕过了 `gen`），
> 烘出的是**旧 profile.d**（无探测）。必须重跑 `make gen` 再建，否则真实栈里
> 探测逻辑根本不生效、卷装了也用不上——`make build-base` 本身带 gen 依赖，
> 但上一轮镜像是在 gen 之前建的。

- [x] **5.1** 重建 base + app 镜像（`make gen` → `make build-base` → `compose build app`）
- [x] **5.2** 重启 `aio` 栈，观察 entrypoint 日志出现播种行 —— `docker logs aio-app-1`
      出现 `mise volume seeded: /root/.local/share/mise`，无 warn
- [x] **5.3** AC7a：`bash -lc 'mise use -g hyperfine && hyperfine --version'` →
      `hyperfine 1.20.0`，路径 `/root/.local/share/mise/shims/hyperfine`（卷）
- [x] **5.4** AC7b：`docker restart` 与 `--force-recreate` 两轮后 hyperfine 仍在
- [x] **5.5** 回归：24 个二进制全 OK（rust/go/uv/ruff/rg/fzf/bat/fd/eza/zoxide/
      delta/starship/jq/yq/hyperfine/node/python3/opencode/pi/claude/codex/mise/git）
- [x] **5.6** AC17：code-server 容器的 login shell 也看到卷（`MISE_DATA_DIR=/root/.local/share/mise`，
      且 app 里装的 hyperfine 在 code-server 中可直接执行）
- [x] **5.7** AC18：非 login shell 保持烘焙布局（`MISE_DATA_DIR=/opt/mise`）——
      已确认为刻意取舍并写入文档
- [x] **5.8** 幂等：三次启动（2×restart + 1×recreate）后 18 symlink / 0 断裂 / 35 shims；
      卷内 mise 足迹 **1.5M**（仅用户装的 hyperfine，烘焙 1.9G 未复制）

## 阶段 6 · 收尾

- [x] **6.1** 更新 `docs/wiki/Scenarios.md`（新增「运行时自装工具的持久化」节 +
      四个陷阱 + login-shell 限定；删除原「已知取舍：recreate 即丢」）
- [x] **6.2** 更新 `paths-and-offline.md` 的 mise 段落（同上，含 login-shell 限定）
- [x] **6.3** 更新 `scenario-authoring.md` 的相关说明（新场景无需任何额外动作）
- [x] **6.4** 提交

## 回滚点

| 阶段 | 回滚 |
|---|---|
| 3 | `git checkout app/entrypoint.sh app/Dockerfile` |
| 2 | `git checkout scenarios/mise/fragment.Dockerfile` + 重建 base |
| 全部 | 删 `app/aio-mise-volume.sh`，还原上述文件，重建两个镜像 |

**注意**：卷上的数据（`~/.local/share/mise`）在回滚后**无害** —— 回落 baked-only
时 profile.d 探测不到…… 实际会探测到（目录还在）。若需彻底回退，用户需手动
`rm -rf /root/.local/share/mise`。这一条要写进回滚说明。

## 验证命令速查

```bash
# 隔离测试（带真实卷，/root 挂载）
docker run --rm -v <vol>:/root -v "$PWD/app/aio-mise-volume.sh:/tmp/seed.sh:ro" \
  sandbox-base bash -lc 'sh /tmp/seed.sh; bash -lc "<cmd>"'

# 真实栈
docker exec aio-app-1 bash -lc '<cmd>'
```
