# Implement: base 层扩充(Noto 字体 + L2 shell 工具)

## 前置检查

- [ ] `git status` 干净(或在预期分支上)
- [ ] `python3 ./.trellis/scripts/task.py current` 指向本任务
- [ ] 磁盘余量足够(当前约 16G 可用;新 base 约 +24MB 字体 + 6 个 apt 包)

## 执行清单

### Step 1: 字体迁入 head

- [ ] 读 `Dockerfile.base.head`,在末尾(locale-gen 之后)追加 Noto 安装 RUN
  - 安装 `fontconfig fonts-noto-cjk fonts-noto-color-emoji`
  - `rm -rf /var/lib/apt/lists/*` + `fc-cache -f`
  - 四条断言:`monospace` → noto、`sans-serif:lang=zh` → noto、CJK 字形(`:charset=4e2d`)、emoji 字形(`:charset=1f600`)
- [ ] 更新 head 顶部注释:说明字体为何在 head(非版本化基础设施,默认必装)
- [ ] 删除 `scenarios/fonts/` 目录
- [ ] `.aio/enabled.toml` 移除 `"fonts"` 条目

**验证**:
```bash
make build-base 2>&1 | tail -20
docker run --rm sandbox-base:latest bash -lc 'fc-match -f "%{family}\n" monospace && fc-list | grep -i noto | head'
```
预期:`Noto Sans CJK SC`(或 Mono CJK);`fc-list` 列出 Noto CJK 与 emoji。

> 若断言失败(字体没被选为默认)→ 补 `/etc/fonts/local.conf` 的 strong alias 段(参考原 fonts 场景),重跑。

### Step 2: 新建 L2 shell-tools 场景

- [ ] 建 `scenarios/shell-tools/scenario.toml`
- [ ] 建 `scenarios/shell-tools/fragment.Dockerfile`(6 工具 + 双通道自检)
- [ ] `.aio/enabled.toml` 追加 `"shell-tools"`

**验证**:
```bash
make gen 2>&1 | tail -3
grep -n "scenario: shell-tools" Dockerfile.base    # 应位于 L2 分组内(mise 工具之间/之后)
docker compose build app && docker compose up -d app
docker exec aio-app-1 bash -lc 'for t in wget tree less file zip vim; do command -v $t >/dev/null || { echo "MISSING(login): $t"; exit 1; }; done; echo login-ok'
docker exec aio-app-1 bash -c  'for t in wget tree less file zip vim; do command -v $t >/dev/null || { echo "MISSING(non-login): $t"; exit 1; }; done; echo nonlogin-ok'
```

### Step 3: 验证 L2 阶梯渲染

- [ ] `make config`(TUI)观察 L2 是否正确分成两阶:`mise 托管 · /opt/mise shims` 与 `系统路径 · apt / 官方源`
- [ ] 确认 `shell-tools`(与 `user-tools`,如启用)归在 apt 阶下

### Step 4: 文档同步

- [ ] `layers.md`
  - L1 表格:移除 `fonts` 场景,head 行补上字体
  - L2 表格成员:补 `shell-tools`
  - 「Picking the layer for a new request」:区分 head(纯系统 infra,无版本无叙事)与 `shell-tools`(可勾选的命名工具集)
- [ ] `recipes.md` Recipe 3:把示例场景名 `shell-extras` 校正为 `shell-tools`
- [ ] `scenario-authoring.md`:如 checklist 提到 L2 全 mise,需补 apt 情形
- [ ] `README.md` / `README.zh-CN.md` / `docs/wiki/Scenarios.md`:字体行改为 Noto + 标注「在 head,默认必装」;场景表补 `shell-tools`
- [ ] `.github/workflows/images.yml`:full-variant 探针列表补 `shell-tools` 的代表工具(如 `wget`)

### Step 5: 全量验收

- [ ] 逐条过 PRD 的 Acceptance Criteria
- [ ] NF 图标风险实测:容器内跑 `starship` 或检查 prompt 有无豆腐块
- [ ] `git diff` 复核,确认无 Rust 代码改动

## 验证命令汇总

```bash
# 构建
make build-base

# 字体
docker run --rm sandbox-base:latest bash -lc \
  'fc-match -f "%{family}\n" monospace && fc-match -f "%{family}\n" "sans-serif:lang=zh" && fc-list | grep -ci noto'

# shell 工具(双通道)
docker run --rm sandbox-base:latest bash -lc 'for t in wget tree less file zip vim; do command -v $t || exit 1; done; echo ok'

# 生成
make gen && grep -c "scenario: shell-tools" Dockerfile.base

# 全量探针(参照 CI)
docker run --rm sandbox-base:latest bash -lc 'for t in mise rustc cargo go uv ruff opencode wget tree less file zip vim starship fzf rg bat fd eza zoxide delta jq yq; do command -v $t >/dev/null || { echo "MISSING: $t"; exit 1; }; done; echo all-ok'
```

## 门禁

- **构建通过** = `make build-base` 成功且四条字体断言全过
- **双通道通过** = login 与 non-login 都能找到全部 6 个工具
- **无 Rust 改动** = `git diff --stat` 中不含 `.rs` 文件
- **文档一致** = `grep -rn "fonts" README.md README.zh-CN.md docs/wiki/Scenarios.md .claude/skills/aio-env-config/references/*.md` 无残留的 Maple/fonts 场景描述

## 回滚点

- Step 1 后若字体断言反复失败 → 先回退 head 改动,单独排查 fontconfig 排序
- 整体回滚:`git revert <commit>` 即可(单 commit,无状态残留)
