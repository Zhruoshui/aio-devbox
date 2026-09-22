# user-tools 独立场景层(非 mise 工具逃生通道)

## Goal

对应 issue #31:新建 `scenarios/user-tools/{scenario.toml, fragment.Dockerfile}`,给用户一个**可自由编辑的独立场景层**,装非 mise 工具。修改后只重建该层,不影响已烘焙内容。spike 已实测通过,不改 Rust 代码,`full.toml ["*"]` 自动包含。

## Background

当前 AIO sandbox 的工具安装分两派:
- **mise 管理**(L2/L3/L4 大多数工具):装 `/opt/mise`,版本可选
- **apt 系统管理**(c23 等):随镜像烘焙

对于非 mise 能管理的工具(apt 系统工具、源码编译、自定义 tarball),此前只能加进 `Dockerfile.base.head`(不可勾选、污染基础层)或手写新场景目录(门槛高)。本任务提供第 3 条逃生通道。

## Requirements

### R1: 新建场景目录

新建:
- `scenarios/user-tools/scenario.toml`
- `scenarios/user-tools/fragment.Dockerfile`

`scenario.toml` 字段:
- `id = "user-tools"`
- `name = "用户自定义工具"`
- `description`:说明用途 + 关键约束(装 /usr/local 或 /opt,勿写 /root)
- `category = "shell"`(挂 L2,跟随 shell 工具层排序)
- `installer = "apt"`(声明非 mise 通道)
- `always_on = false`(默认不勾选,用户主动启用)

### R2: fragment.Dockerfile 骨架

空骨架 + 详细注释模板,含:
- 文件用途与编辑说明
- 三条硬规则:安装到 `/usr/local/bin` 或 `/opt/<tool>`、**绝不写 `/root`**(被工作区卷遮盖)、`bash -lc` 和 `bash -c` 双通道自检
- 工作流注释:`make gen && docker compose build app`(而非 `make build-base`,避免重复跑 build-base)
- 至少 1 个完整可取消注释的示例(如 htop),含 `ARG` 固定版本 + 双通道自检 + 清理 apt cache
- 末尾空实现 `RUN echo "user-tools: no custom tools yet"`

### R3: skill 文档同步

- `.claude/skills/aio-env-config/references/recipes.md`:追加「用户自定义工具层」使用流程
- `.claude/skills/aio-env-config/references/scenario-authoring.md`:checklist 追加 user-tools 条目

### R4: 不改动项

- **不改任何 Rust 代码**(`config/src/scenario.rs`、`config/src/gen.rs`、`config/src/tui.rs` 均无需改动)
- **不改 `full.toml` / `minimal.toml`**:确认 `["*"]` 通配符能自动包含 `user-tools` 场景,若不能则补登

## Acceptance Criteria

- [ ] `scenarios/user-tools/scenario.toml` 存在,字段齐全,`category = "shell"`,`always_on = false`
- [ ] `scenarios/user-tools/fragment.Dockerfile` 存在,含三条硬规则注释 + 至少 1 个完整示例 + 末尾空实现
- [ ] `make gen` 通过,生成的 `Dockerfile.base` 里 `user-tools` fragment 位于 L2 shell 层片段之间(层序正确)
- [ ] `.aio/enabled.toml` 启用 `user-tools` 后 `docker compose build app` 成功,且**只有 user-tools 层重建**(其他层全部 CACHED)
- [ ] 容器内 `bash -lc` 和 `bash -c` 均能看到 fragment 里 echo 的提示文本(双通道自检通过)
- [ ] `.aio/presets/full.toml` 的 `["*"]` 能命中 `user-tools`(无需手动追加)
- [ ] `recipes.md` 和 `scenario-authoring.md` 已追加 user-tools 条目
- [ ] 任何场景作者 / 用户阅读 fragment 顶部注释后,无需看其他文档就能正确添加一个非 mise 工具

## Notes

- Spike 验证记录见 issue #31
- 关键发现:Docker BuildKit 缓存链是**内容哈希链**,不是位置链。修改 user-tools 只重建该层及后续层,只要后续层内容哈希不变仍能复用缓存
- 层序:`gen.rs` 的 `sort_by_layer` 按 `category_rank(os < shell < lang < app)` 再按 id 字母序。`user-tools` 的 `category="shell"` 决定其拼在 L2 片段之间
- 相关 issue: #30(宏观重构跟踪)、#31(本任务来源)
