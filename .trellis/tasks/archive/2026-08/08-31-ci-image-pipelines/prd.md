# PRD: GitHub Actions 镜像自动构建流水线（最小/最大双预设）

## Goal

MVP 完毕，准备推送 GitHub（公开仓库）。建立一套完善的 GitHub Actions 自动构建机制，
在 CI 中顺利构建并发布全部沙箱镜像到 GHCR；核心子问题是 `make config`（交互式 TUI）
在 CI 中的替代，以及「最小可运行 / 最大全量」两条流水线的预设机制。

## User value

- 推送代码后镜像自动构建发布到 GHCR（`GITHUB_TOKEN` 免配置推送、匿名可拉取），
  无需本地长时构建。
- 双预设持续验证：always_on 基线可构建可运行（minimal）＋ 全部场景 fragment 集成
  健康（full，自动纳入未来新增场景）。
- 他人（或未来的自己）`make pull` 拉全套预构建镜像，零编译一键起栈。

## Background（代码勘察确认的事实）

### 配置系统 —— CI 化的钥匙

- `make config`（TUI）唯一职责是写 `.aio/enabled.toml`；`make gen`（`aio-config gen
  --repo /repo`）本身**非交互**，读 `<repo>/.aio/enabled.toml` 组装 `Dockerfile.base`
  （head + L1→L4 排序的启用 fragment + tail）。`config/src/main.rs:29-44` 仅
  `tui`/`gen` 两子命令、各只 `--repo` 旗标，无 `--selection` 类参数 → CI 物化预设
  文件为 `.aio/enabled.toml` 即可，无需 TUI。
- `.aio/enabled.toml` 被 gitignore（`.example` 已提交）→ **CI 必须自行物化该文件**。
- 版本解析：manifest `[[versions]]` → `default_version` → `versions[0]`
  （`config/src/gen.rs:189-204`）→ 预设显式钉版可复现。
- always_on（node、python）由 gen 无条件并入，不出现在 `scenarios` 列表
  （`config/src/gen.rs:50-52`）；`scenarios = []` 是合法选区（纯 head+tail 基线）。
  node 必装（app web-builder 与 code-server 依赖）。
- aio-config 本身 Docker 化（`make build-config` → `config/Dockerfile`）→
  CI 无需 Rust 工具链。

### 镜像与构建关系

- 自建镜像 4 个：`sandbox-base`（`Dockerfile.base`）、`sandbox-app`、
  `sandbox-code-server`（`FROM sandbox-base` 硬编码，app 的 web-builder 阶段也是，
  见 `app/Dockerfile:43,55`、`code-server/Dockerfile:25`）、`sandbox-vnc`
  （`FROM debian:bookworm-slim`，与 base 无关）。gateway 用官方 `caddy:2`，不构建。
- compose `image:` 全部硬编码本地名（`docker-compose.yml:24,62,100,116`）。
- app 容器即工作台（终端 pane 跑在 app 容器内）→ **派生镜像逐变体构建**：
  minimal/full 各需自己的 base + app + code-server；vnc 单份。
- 场景全集（scenarios/）：c23, fonts, go, nvm, opencode, pi, pi-web, python-dev,
  rust, shell-utils, uv（+always_on: node, python）。
- `.env` 仅 `SANDBOX_USER`；gateway hash `make hash` 可再生 → CI 冒烟物料易备。
- 仓库现状：**无任何 git remote**（GitHub repo 未建）、无 `.github/`；
  `Dockerfile.base`（生成物）当前已提交；`.env`/`gateway/secrets/`/`.trellis/`/
  `aio-offline-bundle/` 均已 gitignore，推送卫生良好。
- GHA ubuntu runner 自带 Docker + buildx；docker-container 驱动下 `FROM
  sandbox-base` 无法解析本地 daemon 镜像 → 派生镜像需 `ARG BASE_IMAGE` 化。

### 已定产品决策（访谈结论）

| # | 决策 |
|---|------|
| D1 | GHCR + 公开仓库；GITHUB_TOKEN 推送，匿名拉取 |
| D2 | 全量触发矩阵：push main→双变体推镜像；tag v*→semver+latest；PR→仅 minimal 不推送；workflow_dispatch 手动可选变体 |
| D3 | minimal = 纯 always_on 基线（`scenarios = []`，仅 node+python） |
| D4 | full = 自动全量：manifest 通配符 `scenarios = ["*"]`，未来新增场景目录零配置纳入 |
| D5 | 7 标签开箱即用：base/app/code-server × 双变体 + vnc ×1；aio-config 不推 |
| D6 | 推送前两级验证：镜像内工具抽查 + minimal 栈 compose 冒烟 |
| D7 | 本期实现 `make pull` 消费侧一键拉取起栈 + README 安装文档 |

## Requirements

- **R1 预设机制**：仓库提交 `.aio/presets/minimal.toml`（`scenarios = []` + node/
  python 显式钉版）与 `.aio/presets/full.toml`（`scenarios = ["*"]` + 同样钉版）；
  CI 拷贝为 `.aio/enabled.toml` 后走既有 `gen` → build 链路。
- **R2 manifest 通配符**：`aio-config` 支持 `scenarios` 含 `"*"` —— gen 展开为全部
  非 always_on 场景；TUI 读到 `*` 时预检全部可选框；显式清单行为与现状完全一致
  （只增不改）；单测覆盖展开与去重；`.aio/enabled.toml.example` 文档化该语法。
- **R3 双流水线**：一个 matrix（variant ∈ minimal/full）job 群 + 独立 vnc job；
  触发矩阵按 D2；PR 事件全程不 push。
- **R4 镜像发布**：7 标签推 GHCR（D5）；tag 命名含浮动变体标签（`:minimal`/`:full`）
  与固定 ref 标签（`:<variant>-<ref>`）；vnc 用 `:latest`+`:<ref>`；tag v* 构建时
  full 家族加 `:latest` 别名。
- **R5 ARG BASE_IMAGE 化**：`app/Dockerfile`（web-builder + runtime 两处 FROM）与
  `code-server/Dockerfile` 改为 `ARG BASE_IMAGE=sandbox-base` + `FROM ${BASE_IMAGE}`；
  本地构建零回归（默认值不变），CI 用 GHCR 全限定名注入。
- **R6 构建可靠性与效率**：buildx docker-container 驱动 + `type=gha` 层缓存；
  最大流水线前置磁盘清理；同 ref 并发取消（tag 除外）；合理超时（full 90min）。
- **R7 运行级验证**：推送前 —— ① 双变体镜像内工具抽查（`bash -lc` 版本探针，
  full 加查 rustc/go/uv 等，登录 shell 语义顺带守护 PATH 规则）；② minimal 栈
  compose up（--no-build、无 profile）→ curl 网关 basic-auth 200 → down -v。
- **R8 消费侧**：`make pull VARIANT=minimal|full` —— 拉 7 镜像 → retag 为 compose
  本地名 → 缺失时补 `.env`/hash → 提示 `make up NOBUILD=1`；README 与
  README.zh-CN 各补「预构建镜像安装」一节；与 `make load`（离线恢复）对称。

## Acceptance Criteria

- [ ] AC1: push `main` 后两条 variant 流水线 + vnc job 全绿，GHCR 出现 7 镜像的
  约定标签（浮动 + ref 固定）。
- [ ] AC2: `make pull VARIANT=minimal` 于干净环境（无本地构建产物）执行后，
  `make up NOBUILD=1` 起栈，basic-auth curl 网关返回 200；CI 冒烟同款验证。
- [ ] AC3: 本地零回归 —— `make config`/`gen`/`build-base`/`up`/`save`/`load` 行为
  不变；显式 `scenarios` 清单产物与改动前逐字节一致（gen 幂等）。
- [ ] AC4: 新增 `scenarios/<新id>/` 目录后 full 流水线自动纳入（通配符展开单测 +
  本地 `aio-config gen` 验证 Dockerfile.base 含新 fragment）。
- [ ] AC5: 安全 —— workflow 权限最小（contents:read + packages:write）；PR 事件
  不触发任何 push；`.env`/hash 不入仓库与镜像。

## Out of scope

- 多架构（arm64）—— amd64 单架构，workflow 结构留 `platforms` 扩展位。
- 离线 bundle（`make save` 产物）的 CI 生成与 GitHub Release 附件。
- 代码级测试/lint 流水线（另一条 CI 线）。
- 消费侧 `make pull` 的版本钉定 UI（VARIANT 用浮动标签，ref 钉定靠手动 docker tag）。
