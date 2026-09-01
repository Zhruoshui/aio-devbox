# Design: GitHub Actions 镜像自动构建流水线（最小/最大双预设）

## 1. 总体架构

```
.github/workflows/images.yml
│
├─ prepare（轻量 job，算出下游入参）
│   outputs: matrix(variants JSON), ref, push(bool), prefix(小写 owner 的
│            ghcr.io/<owner>), base_image 命名
│
├─ vnc（不依赖 base，独立并行）
│   buildx 构建 sandbox-vnc → 镜像抽查 → [push] :latest + :<ref>
│
└─ images（strategy.matrix.variant ∈ [minimal, full]）
    ① 磁盘清理（回收 runner ~20GB）
    ② buildx 构建 aio-config 镜像（gha cache, --load）
    ③ cp .aio/presets/<variant>.toml .aio/enabled.toml
       docker run aio-config gen --repo $PWD   （= Makefile gen 的命令行）
    ④ buildx base（--load + gha cache）→ 镜像内工具抽查（bash -lc 探针）
    ⑤ buildx app（--build-arg BASE_IMAGE=…，gha cache mode=max）
       → 抽查（aio-app / /app/static）
    ⑥ buildx code-server（同 app）→ 抽查
    ⑦ [push 路径] 二次 buildx --push（层全命中，秒级）：
       base/app/cs → :<variant> + :<variant>-<ref>（v* 时 full 家族加 :latest）
    ⑧ [仅 minimal] 栈冒烟：retag 本地名 → cp .env.example .env → make hash
       → docker compose up -d --no-build → curl basic-auth 200 → down -v
```

事件→参数映射（prepare 内实现）：

| 事件 | variants | push | ref |
|------|----------|------|-----|
| push main | [minimal, full] | true | sha7 |
| push tag v* | [minimal, full] | true | 标签名（v1.2.0） |
| pull_request | [minimal] | **false** | sha7 |
| workflow_dispatch | 输入选择（both/minimal/full） | 输入开关（默认 true，可干跑） | sha7 |

PR 路径与 push 路径的差异只在「输出方式」：`--load`（本地验证，不推）vs `--push`；
以及 app/cs 的 `BASE_IMAGE`（PR 用 daemon 本地 `sandbox-base`，push 用
`ghcr.io/<owner>/sandbox-base:<variant>-<ref>`）。

## 2. 变更集 A：预设文件 + manifest 通配符

### 2.1 预设文件（新增，提交进 git）

`.gitignore` 只排除 `.aio/enabled.toml` 精确路径，`.aio/presets/` 可正常跟踪。

- `.aio/presets/minimal.toml`：
  ```toml
  scenarios = []

  [[versions]]
  id = "node"
  label = "22.23.2"

  [[versions]]
  id = "python"
  label = "3.13.0"
  ```
- `.aio/presets/full.toml`：同上，但 `scenarios = ["*"]`。

两份都显式钉版（虽等于当前 default_version，但可复现性优先）。

### 2.2 通配符语义（aio-config Rust 改动）

- **归属**：展开逻辑放 `manifest.rs`（选区清单的唯一属主，符合既有
  "single owner of each cross-boundary payload" 约定），签名形如
  `Manifest::expand(&self, discovered: &[ScenarioMeta]) -> Result<Vec<String>>`。
- **规则**：
  - `scenarios` 含 `"*"` 时，展开为**全部 discovered 且非 always_on** 的场景 id
    （always_on 排除规则仍唯一归属 aio-config，不外泄到 CI）。
  - `"*"` 必须是唯一元素；`["*", "rust"]` 报错（清晰契约优于宽容去重）。
  - 不含 `"*"` 时行为与现状逐字节一致（AC3 的 gen 幂等基础）。
- **调用点**：`gen.rs`（组装前展开）与 `tui.rs`（加载已有选区预检框时展开——
  手写 `["*"]` 再进 TUI，表现为全部可选框已勾选；保存时写回显式清单）。
- **单测**：`config/src/` 既有测试风格（fixture scenarios 目录）；覆盖
  纯通配展开、混合报错、无通配不变、空目录展开为空。
- `.aio/enabled.toml.example` 注释补一行通配符说明。

### 2.3 CI 中的 gen 调用

workflow 直接复刻 `Makefile:56-58` 的 docker run 行（不调 `make gen`，因其
`build-config` 前置依赖会走无缓存的 `docker build`）。命令逐字一致：

```bash
docker run --rm --user "$(id -u):$(id -g)" -v "$PWD:/repo" aio-config gen --repo /repo
```

漂移风险（Makefile 该行将来变动）在 workflow 注释中标注引用 Makefile 行号。

## 3. 变更集 B：ARG BASE_IMAGE 化

- `app/Dockerfile`：顶部加全局 `ARG BASE_IMAGE=sandbox-base`；两处
  `FROM sandbox-base`（web-builder `app/Dockerfile:43`、runtime `:55`）改
  `FROM ${BASE_IMAGE}`。全局 ARG 对所有 FROM 生效，且不在任何 RUN 中使用，
  无需 stage 内重声明。
- `code-server/Dockerfile:25`：同样处理（一处 FROM）。
- 本地构建零变化：不传 `--build-arg` 即默认 `sandbox-base`（compose/build 均
  不传）。CI 的 buildx（docker-container 驱动）通过全限定 GHCR 名解析父镜像，
  解决该驱动看不见 daemon 本地镜像的问题。
- vnc/Dockerfile 不动（不从 base 派生）。

## 4. 变更集 C：workflow 详设（.github/workflows/images.yml）

- **权限**：`permissions: contents: read, packages: write`（顶级别声明）。
- **并发**：`group: images-${{ github.ref }}`；`cancel-in-progress` 对 push 分支
  与 PR 为 true，tag 与 dispatch 为 false（不可变 tag 排队跑完）。
- **步骤要点**：
  - 登录：`docker/login-action@v3`（ghcr.io，`github.actor` + `GITHUB_TOKEN`），
    仅 push 路径执行。
  - 磁盘清理：内联 `sudo rm -rf` 清 android/dotnet/ghc/hostedtoolcache
    （不用第三方 action，供应链干净）。
  - buildx：`docker/setup-buildx-action@v3`（docker-container 驱动）。
  - tag 组装：bash 小函数生成 tag 列表（`docker/metadata-action` 需 7 次调用，
    shell 循环更透明可审）。
  - 抽查探针：`docker run --rm --entrypoint bash <img> -lc 'for t in node
    python3 …; do command -v $t >/dev/null || exit 1; done'` —— 用 `command -v`
    而非 `--version`（对无标准 version 旗标的工具稳健）；`bash -lc` 保持登录
    shell 语义，顺带守护场景编写的 PATH 规则。
- **tag 方案**（GHCR 仓库 = 镜像名，变体进 tag）：

  | 镜像 | 浮动标签 | ref 标签 | v* 追加 |
  |------|---------|----------|---------|
  | sandbox-base / -app / -code-server | `:minimal` `:full` | `:<variant>-<ref>` | full 家族加 `:latest` |
  | sandbox-vnc | `:latest` | `:<ref>` | — |

- **缓存**（`type=gha`）：
  - aio-config、app：`mode=max`（多阶段，builder 层的 cargo/npm 缓存最值钱）。
  - base、code-server、vnc：默认 `mode=min`（单阶段 Dockerfile 的 min≈max，
    省缓存预算）。GHA 缓存 10GB/仓库，7 镜像全 max 会互相驱逐——首跑后按
    命中率观测调整。
- **超时**：images job 90min（full 冷缓存预留），vnc job 20min。
- **冒烟细节**（minimal、push 与 PR 路径都跑）：
  `docker tag` 回本地名 → `cp .env.example .env` → `make hash`（拉 caddy:2
  再生默认口令）→ `docker compose up -d --no-build`（无 profile：仅 gateway+app；
  base 服务在 `build` profile 下不会启动）→ 重试循环
  `curl -fsS -u admin:admin http://localhost:8080/` 至 200 → `docker compose
  down -v`。app 的 30141 直发端口在 runner 上无冲突；minimal 无 pi-web，
  entrypoint 自启分支跳过。

## 5. 变更集 D：make pull（消费侧）

```makefile
VARIANT ?= full
REGISTRY_PREFIX ?= ghcr.io/<OWNER>   # 仓库创建后替换为真实 owner
pull: ensure-hash
	@test -f .env || cp .env.example .env
	for img in sandbox-base sandbox-app sandbox-code-server; do \
	  docker pull $(REGISTRY_PREFIX)/$$img:$(VARIANT) && \
	  docker tag  $(REGISTRY_PREFIX)/$$img:$(VARIANT) $$img; \
	done
	docker pull $(REGISTRY_PREFIX)/sandbox-vnc:latest && docker tag … sandbox-vnc
	@echo '→ make up NOBUILD=1 PROFILES="code-server vnc"'
```

- 与 `make load`（离线恢复）对称：同样只做「镜像就位 + 本地名 + 备料」。
- 不触碰 `.aio/enabled.toml`（纯消费者无所谓选区；NOBUILD=1 也不跑 gen）。
- 文档：README.md / README.zh-CN.md 各加「预构建镜像安装」一节（pull → up
  两步 + 口令/端口说明 + VARIANT 选择）；
  `.claude/skills/aio-env-config/references/paths-and-offline.md` 补一段
  make pull（技能自述覆盖离线/安装流，保持其准确性）。

## 6. 变更集 E：仓库卫生

- `Dockerfile.base` 改为 gitignore + `git rm --cached`：它是生成物，输入
  （enabled.toml）不入库导致提交副本不可复现，且在 PR diff 里全是噪音。
  唯一受影响的边缘路径是「fresh clone 后不跑 gen 直接
  `docker compose --profile build build base`」——非规范路径（规范路径
  `make build-base` 先跑 gen）。**此条为设计层决策，提交评审时单独确认。**
- 推送前检查（零改动，仅确认）：`.env`、`gateway/secrets/`、`.trellis/`、
  `aio-offline-bundle/`、`app/static/` 均已忽略。

## 7. 权衡记录（否决的替代方案）

| 决策 | 否决项 | 理由 |
|------|--------|------|
| 预设拷贝 | 给 gen 加 `--selection <path>` 旗标 | 零 Rust 改动即达成；避免 CLI 面积膨胀 |
| 通配符在 manifest 展开 | CI 脚本枚举 scenarios/ 生成清单 | always_on 排除等规则知识会复制到 CI，双头维护 |
| CI 直跑 docker run gen | `make gen` | 其 build-config 前置依赖在 runner 上无缓存必重编 Rust；命令行逐字复刻 + 注释锚定，漂移风险可控 |
| app/cs 走 buildx+push 链 | 全程默认 docker 驱动 | docker 驱动不支持 gha 缓存导出；FROM 依赖链经 GHCR 全限定名最干净 |
| PR 路径 base --load + 派生镜像 docker build | PR 也推临时 tag | 避免注册表污染；PR 车道无 app 层缓存是可接受代价（验证车道，非发布车道） |
| vnc 独立 job | 放进 matrix 双跑 | 与变体无关，双跑纯浪费 |
| amd64 单架构 | 多架构 | 无 arm64 需求；buildx 结构已留 platforms 位 |

## 8. 兼容与回滚

- 所有改动对本地流程零侵入：Dockerfile 默认 ARG、Makefile 只增目标、
  manifest 无通配符时行为不变。单 commit revert 即整体回滚。
- workflow 可在 GitHub UI 一键禁用；预设文件留存无害。
- 通配符特性独立于 CI 存在（本地手写 `["*"]` 同样合法）。
- 风险与观测：GHA 10GB 缓存预算的驱逐行为、full 冷缓存时长（首跑后校准
  mode=max 范围与超时）；pi/c23 等场景的外网下载在 GHA 网络下无沙箱防火墙
  限制（沙箱 403 策略仅本地环境）。
