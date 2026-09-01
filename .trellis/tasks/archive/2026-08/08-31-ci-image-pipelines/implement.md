# Implement: GitHub Actions 镜像自动构建流水线（最小/最大双预设）

> 执行顺序即依赖顺序；每步末尾有验证命令，过门再进下一步。
> 本地测试通配符时**先备份 `.aio/enabled.toml`**（cp 预设会覆盖你的本机选区，
> 测完还原）。

## 0. 前置

- [ ] 分支：`feat/ci-image-pipelines`（自 `feat/aio-sandbox-mvp` 切出；若 MVP 已
  并 main 则自 main 切）。
- [ ] `cp .aio/enabled.toml /tmp/enabled.toml.bak`（本机选区备份）。

## 1. aio-config 通配符（R1/R2）

- [ ] `config/src/manifest.rs`：新增 `expand(&self, discovered)` —— `scenarios`
  含 `"*"` 且为唯一元素时展开为全部非 always_on 场景 id；混合即报错；否则原样。
- [ ] `config/src/gen.rs` 与 `config/src/tui.rs` 调用点接入（gen 组装前展开；
  TUI 预检框时展开）。
- [ ] 单测：纯通配展开 / 混合报错 / 无通配不变 / 空 scenarios 目录展开为空，
      风格随 `config/src/scenario.rs` 既有测试。
- 验证：
  ```bash
  docker run --rm -v "$PWD/config:/src" -w /src rust:1-bookworm cargo test
  cp .aio/presets/full.toml .aio/enabled.toml   # 第 2 步建好预设后回来复跑
  make build-config && make gen
  grep -c '^# scenario:' Dockerfile.base        # 片段数 = 全部非 always_on 场景数
  ```

## 2. 预设文件 + 示例（R1/R2）

- [ ] `.aio/presets/minimal.toml`（`scenarios = []` + node 22.23.2 / python 3.13.0
      显式钉版）。
- [ ] `.aio/presets/full.toml`（`scenarios = ["*"]` + 同钉版块）。
- [ ] `.aio/enabled.toml.example` 注释补通配符一句。
- 验证：
  ```bash
  cp .aio/presets/minimal.toml .aio/enabled.toml && make gen \
    && ! grep -E 'c23|rust|shell-utils' Dockerfile.base
  cp .aio/presets/full.toml   .aio/enabled.toml && make gen \
    && grep -E 'rust|shell-utils|go|nvm' Dockerfile.base
  cp /tmp/enabled.toml.bak .aio/enabled.toml && make gen && git diff --stat Dockerfile.base  # 与提交版一致（幂等，AC3）
  ```

## 3. ARG BASE_IMAGE 化（R5）

- [ ] `app/Dockerfile`：顶部全局 `ARG BASE_IMAGE=sandbox-base`；
      `:43` web-builder 与 `:55` runtime 两处 FROM 改 `FROM ${BASE_IMAGE}`。
- [ ] `code-server/Dockerfile:25`：同样处理。
- 验证（默认路径不回归，AC3）：
  ```bash
  docker build -t sandbox-base -f Dockerfile.base .   # 若本地已有最新 sandbox-base 可跳过
  docker build --build-arg BASE_IMAGE=sandbox-base -f app/Dockerfile -t sandbox-app .
  ```
- **评审门 A**：Dockerfile 改动构建通过后再进 workflow。

## 4. workflow（R3/R4/R6/R7）

- [ ] `.github/workflows/images.yml`：按 design §1/§4 编写 —— prepare/vnc/images
      三 job；触发矩阵（main push / v* tag / PR / dispatch+干跑开关）；权限
      `contents:read + packages:write`；并发组按 ref（tag 不取消）；磁盘清理
      内联 rm；aio-config 与 app 缓存 `mode=max`，其余默认；超时 90/20min；
      PR 路径 `--load` + 派生镜像 `docker build`，push 路径 buildx `--push` 链；
      抽查探针 `bash -lc` + `command -v`；minimal 冒烟（retag → .env → make
      hash → compose up -d --no-build → curl 重试至 200 → down -v）。
- [ ] REGISTRY_PREFIX / prefix 计算用小写化的 `github.repository_owner`。
- 验证（本地静态）：`python3 -c "import yaml,sys;yaml.safe_load(open('.github/workflows/images.yml'))"`
  与 `actionlint`（若可安装）。
- **评审门 B（主门）**：向用户展示 workflow 全文再合入。

## 5. make pull + 文档（R8）

- [ ] Makefile 增 `pull` 目标（design §5；`VARIANT ?= full`、
      `REGISTRY_PREFIX ?= ghcr.io/<OWNER>` 占位）。
- [ ] README.md / README.zh-CN.md「预构建镜像安装」节。
- [ ] `.claude/skills/aio-env-config/references/paths-and-offline.md` 补 make pull 段。
- 验证：`make -n pull VARIANT=minimal` 干跑看命令序列正确。

## 6. 仓库卫生（design §6，单独确认项）

- [ ] 与用户确认后：`git rm --cached Dockerfile.base` + `.gitignore` 增
      `Dockerfile.base`（注释：生成物，规范路径 make build-base 先 gen）。

## 7. 首跑校准（推送后）

- [ ] 用户创建 GitHub 公开仓库、设 origin、推分支（REGISTRY_PREFIX 占位符若
      已知 owner 则第 5 步前先替换真名）。
- [ ] dispatch 干跑一次（push=false）验证全链绿 → 正式 dispatch 或推 main。
- [ ] 首跑后记录：full 冷缓存时长、GHA 缓存占用量；据此校准 mode=max 范围与
      超时（journal 记账）。
- [ ] `make pull VARIANT=minimal && make up NOBUILD=1` 本地实测（AC2）。

## 回滚点

- 每步独立成 commit；任何一步出问题单 revert 该步。
- workflow 出错可 GitHub UI 禁用；其余改动对本地流程零侵入。

## 完成定义

PRD AC1–AC5 全勾；spec 更新（3.3：若有 CI/发布新约定，沉淀 backend/guides）；
commit 按仓库规范（英文 conventional 前缀 + 中文主标题）。
