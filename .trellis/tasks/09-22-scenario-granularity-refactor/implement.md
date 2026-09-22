# implement.md — Scenario 粒度重构执行计划

## 阶段 0 · 前置检查

- [ ] **0.1** 记录回滚锚点
  ```bash
  git rev-parse HEAD
  git status --short          # 应为 clean
  df -h /var/lib/docker       # 需 ≥25G 余量
  ```
- [ ] **0.2** 确认 BuildKit 前端镜像在本地（`builder prune` 会连带清掉它）
  ```bash
  docker images docker/dockerfile --format '{{.Repository}}:{{.Tag}}'
  # 缺失则：docker pull docker/dockerfile:1
  ```

---

## 阶段 1 · L1 mise engine 拆分（最高风险，先做）

**理由**：后续所有 mise fragment 都依赖 engine 提供的 `/opt/mise` 与 `MISE_*` env。
先拆并单独验证，避免其余 fragment 建立在错误基础上。

- [ ] **1.1** 改写 `scenarios/mise/scenario.toml`
  - `category` 从 `"lang"` 改为 `"os"`
  - 加 `always_on = true`
  - 移除 `[[versions]]`（engine 版本走 fragment 内 `ARG`，不需 UI 选择）
  - `description` 改为说明「engine only」
  - name 改为「mise (工具链管理器 engine)」

- [ ] **1.2** 改写 `scenarios/mise/fragment.Dockerfile`
  - **保留**：`ARG MISE_VERSION`、mise 二进制安装、四个 ENV、profile.d
  - **删除**：5 个工具版本 ARG、`[settings]`/`[tools]` 写入、`mise install`、
    `rustup component add`、`rm -rf /opt/mise/downloads`
  - **保留** `mkdir -p /opt/mise`（否则工具 fragment 追加 config 会失败）
  - 自检改为只验 `mise`，仍走双通道（`bash -lc` + `bash -c`）

- [ ] **1.3** 单点验证 AC1
  ```bash
  # 只选 mise 构建
  make config      # TUI 里只勾 mise
  make build-base
  docker run --rm sandbox-base:latest bash -lc 'mise --version'
  docker run --rm sandbox-base:latest bash -lc 'ls /opt/mise/installs 2>&1 || echo "EMPTY(ok)"'
  docker run --rm sandbox-base:latest bash -lc 'echo $MISE_DATA_DIR $RUSTUP_HOME'
  docker run --rm sandbox-base:latest bash -c 'command -v mise'   # 非 login shell
  ```

**回滚点**：若 engine 拆分失败，`git checkout scenarios/mise/` 还原。

---

## 阶段 2 · L3 mise 派（rust / go / uv / ruff）

- [ ] **2.1** 建 `scenarios/rust/`
  - `scenario.toml`：`category = "lang"`，带 `[[versions]]`（1.93.1）
  - `fragment.Dockerfile`：**模板 2**（直接写 config + 两个补偿）
- [ ] **2.2** 建 `scenarios/go/`（模板 1，1.23.4）
- [ ] **2.3** 建 `scenarios/uv/`（模板 1，0.5.11）
- [ ] **2.4** 建 `scenarios/ruff/`（模板 1，0.8.4）
- [ ] **2.5** 验证 AC3
  ```bash
  # 选 mise + rust
  docker run --rm sandbox-base:latest bash -lc 'rustc --version && cargo --version && cargo clippy --version && rustfmt --version && rust-analyzer --version'
  ```

---

## 阶段 3 · L2 shell 工具（10 个）

- [ ] **3.1** 建 10 个 scenario（均模板 1，`category = "shell"`）
  | id | 版本 | 自检 bin |
  |---|---|---|
  | `fzf` | 0.74.4 | `fzf` |
  | `ripgrep` | 15.2.0 | **`rg`** ⚠️ |
  | `bat` | 0.26.1 | `bat` |
  | `fd` | 10.5.0 | `fd` |
  | `eza` | 0.23.5 | `eza` |
  | `zoxide` | 0.10.0 | `zoxide` |
  | `delta` | 0.19.2 | `delta` |
  | `starship` | 1.26.0 | `starship` |
  | `jq` | 1.8.2 | `jq` |
  | `yq` | 4.53.6 | `yq` |

  ⚠️ `ripgrep` 的 bin 名是 `rg`（实测确认），自检必须写 `rg`。
  `bat`/`fd` 是规范名，**无** Debian 的 `batcat`/`fdfind` 问题。

- [ ] **3.2** 删除 `scenarios/shell-utils/`
  ```bash
  git rm -r scenarios/shell-utils
  ```

- [ ] **3.3** 验证 AC2（只选 fzf）
  ```bash
  docker run --rm sandbox-base:latest bash -lc 'fzf --version'
  docker run --rm sandbox-base:latest bash -lc 'for t in rg bat fd eza zoxide delta starship jq yq; do command -v $t >/dev/null && echo "UNEXPECTED: $t"; done; echo done'
  ```

---

## 阶段 4 · L4 agent（3 个）

- [ ] **4.1** 建 `scenarios/opencode/`（1.18.24，`category = "app"`）
- [ ] **4.2** 建 `scenarios/claude-code/`（2.1.278）
- [ ] **4.3** 建 `scenarios/codex/`（0.155.1）
- [ ] **4.4** 验证 AC5
  ```bash
  docker run --rm sandbox-base:latest bash -lc 'opencode --version && claude --version && codex --version'
  ```
- [ ] **4.5** 确认 `EnvPicker.tsx` 未改动（新 agent 不在 `SERVICE_SCENARIOS` 内，
      应自动落 L4 场景区）

---

## 阶段 5 · pi 改 mise 装

- [ ] **5.1** 改 `scenarios/pi/fragment.Dockerfile`
  - CLI 本体改为 `mise use -g "pi@${PI_VERSION}"`
  - **锁 `PI_VERSION=0.84.2`**（对齐现有 agent-browser 插件基线）
  - **其余全部保留**（见 design.md §4.2 清单）
- [ ] **5.2** 验证 AC6
  ```bash
  docker run --rm sandbox-base:latest bash -lc 'pi --version && aio-pi-extensions --help 2>&1 | head -3'
  docker run --rm sandbox-base:latest bash -lc 'ls /opt/pi-extensions/node_modules | head'
  docker run --rm sandbox-base:latest bash -lc 'command -v agent-browser agent-browser-real pi-agent-browser-doctor'
  ```

---

## 阶段 6 · 迁移与清理

- [ ] **6.1** 更新仓库 `.aio/enabled.toml`
  - `mise` 保留（语义已变 engine）
  - `shell-utils` 替换为 10 个新 id
  ```bash
  make config   # 或手工编辑
  ```
- [ ] **6.2** 全仓 grep 硬编码系统路径（父任务 Q1）
  ```bash
  grep -rn "usr/local/bin/rg\|usr/bin/rg\|batcat\|fdfind" --include=* \
    --exclude-dir=.git --exclude-dir=target --exclude-dir=node_modules .
  ```

---

## 阶段 7 · 集成验收

- [ ] **7.1** `cargo test`（config + mgr）全绿（AC11）
  ```bash
  cd config && cargo test
  cd ../mgr && cargo test
  ```
- [ ] **7.2** `make config`（TUI）正常展示四层分组与全部目录（AC9）
- [ ] **7.3** 完整构建：最小集 + 全选各一次
- [ ] **7.4** AC4：c23 与 mise 派同时选中互不干扰
  ```bash
  docker run --rm sandbox-base:latest bash -lc 'clang --version && rustc --version'
  echo 'int main(){return 0;}' > /tmp/t.c
  docker run --rm -v /tmp/t.c:/tmp/t.c sandbox-base:latest bash -lc 'clang -std=c23 /tmp/t.c -o /tmp/t && /tmp/t && echo OK'
  ```
- [ ] **7.5** AC8：不同选择产生不同 env_hash
  ```bash
  # 通过 mgr API 或直接对比 gen 产物
  ```

---

## 验证命令速查

```bash
# 单场景构建后的验证(替换 <cmd>)
docker run --rm sandbox-base:latest bash -lc '<cmd>'

# 只重建 base(不动其他服务)
make build-base

# 完整构建
docker build --no-cache -t sandbox-base -f Dockerfile.base .
```

## 回滚点

| 阶段 | 回滚 |
|---|---|
| 1 | `git checkout scenarios/mise/` |
| 3 | `git checkout scenarios/shell-utils/` |
| 5 | `git checkout scenarios/pi/` |
| 全部 | `git reset --hard <阶段 0.1 的 HEAD>` + 重建镜像 |
