# base 层扩充: Noto 字体默认必装 + L2 常用 shell 工具(非 mise)

## Goal

1. 把容器字体从 Maple Mono NF CN 换成 **Noto 系列**,并**默认必装**(从可勾选场景移入 `Dockerfile.base.head`,不再是用户可关闭的选项)。
2. 在 **L2 shell 层级**追加一批常用非 mise 系统工具(wget 等),走 apt 烘焙,作为一个新的可勾选场景 `shell-tools`。

## Background

Issue #30 摩擦点 1 指出 `category` 承担了两个语义轴:既决定分组,又隐式决定「能否关闭」。`scenarios/fonts/` 正是这个问题的实例——它是 `category = "os"` 的 L1 行,却是可勾选的。Issue #30 声称「本轮已把字体移出场景(E1)」,但仓库实测 `scenarios/fonts/` **仍在**(L1 os,可勾选),说明该决策未落地。本任务补上。

现有 L2 shell 层 10 个工具(**全部 mise 管理**):fzf / ripgrep / bat / fd / eza / zoxide / delta / starship / jq / yq。base 镜像里 `wget` 等常用系统工具**确实缺失**(实测 `wget tree less file zip vim` 全部 MISS)。

## Decisions (2026-09-22, 已与用户确认)

| 问题 | 决策 |
|---|---|
| Noto 字体归属 | **移入 `Dockerfile.base.head`**,删除 `scenarios/fonts/` + `enabled.toml` 条目 |
| shell 工具归属 | **新建 `scenarios/shell-tools/`**,`category = "shell"`,`always_on = false`(可勾选) |
| shell 工具清单 | **精简版**:`wget tree less file zip vim` |
| Noto 范围 | `fonts-noto-cjk`(Sans+Serif CJK) + `fonts-noto-color-emoji` |

## Requirements

### R1: 字体迁入 head(Noto,默认必装)

- 删除 `scenarios/fonts/` 整个目录(scenario.toml + fragment.Dockerfile)
- 从 `.aio/enabled.toml` 的 `scenarios` 数组移除 `"fonts"`
- 在 `Dockerfile.base.head` 追加字体安装段:
  - `apt-get install -y --no-install-recommends fontconfig fonts-noto-cjk fonts-noto-color-emoji`
  - `fc-cache -f`
  - **断言**(不是打印):`monospace` / `sans-serif:lang=zh` 必须解析到 Noto
  - 必须能命中中文字形(如 `fc-list ':charset=4e2d'`)与 emoji 字形
  - 保留 `rm -rf /var/lib/apt/lists/*`
- head 注释说明:为什么字体在 head 而不在场景(非版本化基础设施,默认必装)

### R2: 新建 L2 shell-tools 场景

新建 `scenarios/shell-tools/`:
- `scenario.toml`:`id = "shell-tools"`,`category = "shell"`,`installer = "apt"`,`always_on = false`
- `fragment.Dockerfile`:安装 `wget tree less file zip vim`
  - `apt-get update` 开头(head 已清空 lists)
  - 结尾 `rm -rf /var/lib/apt/lists/*`
  - 每个工具 `command -v` 双通道自检(`bash -lc` + `bash -c`)
  - 遵循 scenario-authoring.md 的 fragment 契约(`# >>> scenario: shell-tools >>>` 包裹)

### R3: 文档同步

- `layers.md`:L1 表格里 `fonts` 从场景列移除(head 行更新);L2 成员补 `shell-tools`
- `layers.md` 的「Picking the layer for a new request」需说明 head 与 `shell-tools` 场景的差异
- `recipes.md`:Recipe 3「add an L2 apt utility」的示例当前用 `shell-extras`,与实际场景名不一致,需校正
- `scenario-authoring.md`:checklist 中 L2 相关条目需同步

### R4: L2 阶梯渲染的影响确认

`scenario.rs::has_multiple_installers` 会在**层内混用多种 installer** 时渲染子标题。加入 `shell-tools`(apt) 后,L2 从「全是 mise」变成「mise + apt 混用」,**会触发 L2 的阶梯子标题渲染**——这是上一轮(1107e4e)刚做的功能,属预期内的正向效果,验收时需确认渲染正确、无噪声。

## Acceptance Criteria

- [ ] `scenarios/fonts/` 已删除,`.aio/enabled.toml` 不再含 `"fonts"`
- [ ] `Dockerfile.base.head` 含 Noto 安装段 + 断言,`make build-base` 成功
- [ ] 新镜像里 `fc-match monospace` 解析到 Noto;中文字形断言通过;emoji 字形断言通过
- [ ] `fc-list | grep -i noto` 能看到 CJK 与 emoji 字体
- [ ] `scenarios/shell-tools/` 的 6 个工具在容器内 `bash -lc` 与 `bash -c` **双通道**均可见
- [ ] `make gen` 后 `shell-tools` 位于 L2 分组内(层序正确)
- [ ] L2 阶梯分组渲染正确:`shell-tools`(apt)与 10 个 mise 工具分成两阶,无渲染噪声
- [ ] `.aio/presets/full.toml` 的 `["*"]` 自动包含 `shell-tools`(无需手动追加)
- [ ] CI 探针列表(`.github/workflows/images.yml`)如有必要补 `shell-tools` 的代表工具
- [ ] `layers.md` / `recipes.md` / `scenario-authoring.md` 已同步

## Notes

- **head 变更的副作用**:mgr 按 `env_hash`(取自组装后 Dockerfile.base 字节)为每个沙箱建镜像。改 head → **所有**沙箱 hash 变化 → 各自全量重建。这是 issue #30 摩擦点的实例,本任务接受该代价(字体属基础设施,变更频率低)。
- **NF 图标风险**:Maple Mono NF CN 含 Nerd Font 图标字形,Noto 没有。若终端/编辑器依赖 NF 图标(如 starship prompt),改用 Noto 后可能变豆腐块。验收时需确认;若确实依赖,考虑 Noto + Nerd Fonts Symbols 并存。
- 相关 issue: #30(宏观重构跟踪)、#31(已完成的前置 user-tools)
- 前序任务: `09-22-user-tools-scenario`(已归档),暴露了 BuildKit 位置链缓存行为
