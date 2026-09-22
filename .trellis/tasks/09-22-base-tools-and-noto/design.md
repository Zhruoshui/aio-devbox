# Design: base 层扩充(Noto 字体 + L2 shell 工具)

## 范围边界

**改动**:
- `Dockerfile.base.head` — 追加 Noto 字体安装段
- `scenarios/fonts/` — **删除**
- `scenarios/shell-tools/` — **新建**
- `.aio/enabled.toml` — 移除 `"fonts"`,追加 `"shell-tools"`
- 文档:`layers.md` / `recipes.md` / `scenario-authoring.md` / `README*` / `docs/wiki/Scenarios.md`
- 可选:`.github/workflows/images.yml` 探针列表

**不改**:
- 任何 Rust 代码(`gen.rs` / `scenario.rs` / `tui.rs` / `envhash.rs` 均无改动)
- `Dockerfile.base.tail`
- mgr / mgr-web(本任务不碰 UI;L2 阶梯渲染是既有功能,只验证不修改)

## 1. 字体:为什么进 head 而不是 always_on 场景

`layers.md` 的教条:

> **Non-versioned infrastructure** lives hardcoded in `Dockerfile.base.head` and
> NEVER appears as a scenario

`always_on = true` 的语义是「**有版本可选**、但要无条件烘焙」——node/python/mise-engine 用它是为了 TUI 里给用户一个版本下拉(左/右方向键循环)。字体**没有版本维度**,放进场景只会重现 issue #30 摩擦点 1(`category` 兼任「分组」与「能否关闭」两个语义轴)。因此进 head。

**代价(已知并接受)**:head 字节变化 → 所有沙箱的 `env_hash` 变化 → 各自全量重建。字体是低频变更的基础设施,该代价一次性。

## 2. head 里放什么

追加在 locale-gen 之后(head 末尾),新增一个独立 `RUN`:

```dockerfile
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
        fontconfig fonts-noto-cjk fonts-noto-color-emoji \
 && rm -rf /var/lib/apt/lists/* \
 && fc-cache -f \
 && fc-match -f '%{family}\n' monospace | grep -qi noto \
 && fc-match -f '%{family}\n' 'sans-serif:lang=zh' | grep -qi noto \
 && fc-list ':charset=4e2d' family | grep -qi noto \
 && fc-list ':charset=1f600' family | grep -qi noto
```

设计要点:

- **断言而非打印**。沿用原 fonts 场景的做法:任何一条失败即构建失败(fail-fast),不留隐性错误。`4e2d` = 中,`1f600` = 😀,分别验证 CJK 与 emoji 覆盖。
- **不改 `/etc/fonts/local.conf`**。原 Maple 场景需要它是因为要把三个通用族**钉**到 Maple;Noto 装上后 fontconfig 的默认排序就能让 `monospace`/`sans-serif:lang=zh` 落到 Noto(DejaVu 无中文字形,Noto CJK 是唯一覆盖者)。**但这条需实测确认**——若实测发现仍落到 DejaVu,则补 `local.conf`,断言会兜住这个失败。
- `fonts-noto-cjk` 同时含 Sans CJK 与 Serif CJK(单包);`fonts-noto-color-emoji` 补 emoji。合计约 102MB(原 Maple 约 78MB)。
- 位置:head 末尾,在所有场景片段之前 —— 满足「字体是最基础的地基」语义。

## 3. L2 shell-tools 场景

```
scenarios/shell-tools/
├── scenario.toml
└── fragment.Dockerfile
```

`scenario.toml`:
```toml
id = "shell-tools"
name = "常用 Shell 工具"
description = "wget/tree/less/file/zip/vim 等常用系统工具(apt 装系统路径)。与 mise 托管的 L2 工具互补:这些是几乎所有发行版都自带的基线工具"
category = "shell"
installer = "apt"
always_on = false
```

`fragment.Dockerfile` 要点(遵循 scenario-authoring.md):
- `# >>> scenario: shell-tools >>>` / `# <<< scenario: shell-tools <<<` 包裹
- `apt-get update` 开头(head 末尾已 `rm -rf /var/lib/apt/lists/*`)
- 6 个工具 `command -v` **双通道**自检(login + non-login)
- 结尾清 apt lists

## 4. 与既有架构的交互

### 4.1 L2 阶梯渲染(预期触发)

`scenario.rs::has_multiple_installers(scenarios, "shell")` 原本返回 `false`(L2 全是 mise)。加入 `shell-tools`(apt) 后返回 `true` → **TUI 与 mgr-web EnvPicker 都会在 L2 渲染两个子标题**:

```
L2 · Shell 命令
  mise 托管 · /opt/mise shims
    [ ] fzf  [ ] ripgrep  ...
  系统路径 · apt / 官方源
    [ ] shell-tools
```

这是 1107e4e 刚做的阶梯功能的**正向收益**(把「装在哪」显式化),不是回归。但需实测确认渲染无噪声(尤其子标题在只有 1 个成员的 apt 阶下是否显得空)。

### 4.2 「能不能重命名场景」

**用 `shell-tools` 而非 `shell-extras`**。`recipes.md` Recipe 3 的示例用的是 `shell-extras`,但仓库里从没有过这个场景(它只是文档示例)。本任务用 `shell-tools` 作为真实场景名,并把 Recipe 3 的示例校正为一致的名称,消除「文档示例名 ≠ 真实场景名」的漂移。

### 4.3 与 user-tools 的关系

`user-tools`(上一任务)是**用户自由编辑**的逃生通道(空骨架);`shell-tools` 是**我们预置**的常用工具集。两者互补:预置的进 `shell-tools`,用户自己想加的进 `user-tools`。二者都在 L2、都是 apt,`has_multiple_installers` 会看到 L2 有两种 installer(apt 与 mise),但 `shell-tools` 与 `user-tools` 同为 apt 会归到同一阶。

## 5. 风险

| 风险 | 影响 | 缓解 |
|---|---|---|
| **NF 图标缺失** | starship/eza 等若依赖 Nerd Font PUA 字形,改用 Noto 后会显示豆腐块 | 实测:base 里无 starship.toml,无带 `--icons` 的别名;starship 默认 preset 的 `❯` 是标准 Unicode。风险面小,**验收时实测确认** |
| **head 变更连锁重建** | 所有沙箱 env_hash 变化 → 全量重建 | 已接受(PRD Notes 记录);字体低频变更 |
| **字体断言依赖 fontconfig 默认排序** | 若 Noto 未被自动选为 mono/sans 默认,断言失败 | 断言会 fail-fast;若失败则补 `/etc/fonts/local.conf`(原 Maple 做法) |
| **apt 包体积增加** | +24MB(102 vs 78) | 可接受;emoji 可按需砍掉 |

## 6. 回滚

单 commit 可回滚:恢复 `scenarios/fonts/`、去掉 head 的字体段、`.aio/enabled.toml` 加回 `"fonts"`。无数据迁移、无状态残留。
