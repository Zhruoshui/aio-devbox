# design — usertools 应用商店化

## 架构与边界

```
mgr-web StorePage ──表单──▶ mgr-api /api/usertools/*
                               │  recipe.rs   schema 解析/序列化 + 校验
                               │  render.rs   配方 → fragment 生成器(唯一写入口)
                               │  builtins.rs 静态种子清单
                               │  store.rs    货架视图扫描 + 引用统计(扫 sandboxes.env_json)
                               └──写──▶ usertools/<id>/{recipe.toml, fragment.Dockerfile}

创建/编辑沙箱 env(user_tools)──▶ envhash::assemble_with_tools(不变)
                               └─读─▶ usertools/<id>/fragment.Dockerfile → Dockerfile.base → env_hash → 镜像
```

**不变式(本设计的全部杠杆)**:构建侧只认 per-tool 目录的 fragment.Dockerfile(envhash.rs:134)。商店、recipe、生成器全部活动在该文件的「上游」;`assemble_with_tools`、`env_hash`、jobs 管线**零改动**。

## 模块划分(mgr/src/usertools/ 目录化)

现 usertools.rs 单文件拆为目录模块(旧文件里仍在用的函数迁入,标注来源):

| 模块 | 职责 | 承接旧代码 |
|---|---|---|
| `mod.rs` | `UserTool`、`slug`、`list_tools`/`open_name`、`blocks_well_formed`、`split_tools`、`validate`(root 守卫)、`write_atomic` | 原 usertools.rs 骨干全保留 |
| `recipe.rs` | recipe.toml 的 serde schema、按类型字段校验、slug 冲突检查 | 新增 |
| `render.rs` | 配方 → fragment 确定性生成;唯一写 recipe-owned fragment 的路径 | 新增(替代 sync_scenario_dirs 的生成侧职责) |
| `builtins.rs` | `const BUILTINS: &[Recipe]` 种子清单 + 物化 | 新增 |
| `store.rs` | 货架卡片扫描(合并 recipe 元数据 / 无 recipe 回退 `# --- tool:` 名)、跨沙箱引用统计 | picker_tools 的升级版 |

`sync_scenario_dirs`、单文件 `fragment_path`、`ToolMeta`/`save_meta`/`load_meta` 随单文件面退役而删除。

## recipe.toml 契约

```toml
schema = 1                      # 预留版本位,破坏性演进时 bump
name = "qpdf"                   # 显示名(上架表单主字段)
description = "PDF 转换/加密工具"
category = "document"           # 枚举:shell|document|media|network|dev|other(前端色块映射)
version = "bookworm 源"          # 展示用自由文本
homepage = "https://qpdf.sourceforge.io/"

[install]
type = "apt"                    # apt|url|github-release|script|raw(serde tag)
# apt:      packages = ["qpdf"]
# url:      url, kind(binary|tarball), bins(软链名列表, tarball 必填)
# github-release: repo, tag, asset, kind, bins   # URL 由模板拼出
# script:   script(内嵌), check(自检命令名, 默认取 id)
# raw:      dockerfile(内嵌片段)                 # 表单「自定义片段」类型
```

- `#[serde(tag = "type", rename_all = "kebab-case")]` 的 enum;**新增类型 = 加一个变体 + 一个模板**,上游无感(Out of Scope 里 npm/pip 的扩展位)。
- id = 目录名,复用现有 `slug()` 规则;POST 时校验与现存目录、种子 id 不冲突。
- raw 类型也走 recipe(表单发布的 raw 存 `install.dockerfile`,fragment 由生成器包上标记块产出)——**全商店只剩一种「无 recipe」形态:存量/手写目录**,它们 fragment-only、raw 编辑器直改文件。

## render.rs 生成器

- 每类型一个确定性模板(同输入必同输出字节,AC6 的元数据稳定性由此保证:模板只消费 `[install]`,不碰元数据字段)。
- 产出结构:`# --- tool: <name> ---` 标记块 + 注释头(类型/来源)+ 类型化 RUN + 双通道自检(`bash -lc`/`bash -c` 各查 `command -v <check>`),`# --- /tool ---` 收尾。
- 路径纪律(硬规则 1 的构造保证):单二进制 → `/usr/local/bin/<bin>`;tarball → 解 `/opt/<id>/` 逐个软链 `bins`;apt 清 `/var/lib/apt/lists/*`。
- 生成后调 `validate()`(mod.rs 的 /root 守卫)做纵深防御;recipe-owned fragment 的每次写都经 render.rs,表单保存时直接覆盖(不与手改 drift 合并——recipe 即事实,预览可见最终态)。

## API 契约(routes.rs)

| 端点 | 行为 |
|---|---|
| `GET /api/usertools/store` | `{mine: [Card], builtin: [Recipe 未物化者]}`;Card = recipe 元数据 + `type` + `has_recipe` + `refs: Vec<沙箱名>` |
| `POST /api/usertools/tools` | body = Recipe(或 `{"from_builtin": "htop"}`);校验 slug/类型字段 → 写 recipe.toml + render fragment;已有 id → 409 |
| `PUT /api/usertools/tools/{id}` | 全量替换 recipe → 重新生成 fragment;无 recipe 的存量目录 → 405(走 raw 端点) |
| `PUT /api/usertools/tools/{id}/fragment` | raw 通道:直写 fragment.Dockerfile(过 validate + blocks_well_formed),用于存量工具与 raw 类型 |
| `DELETE /api/usertools/tools/{id}` | 扫 `sandboxes` 表 env_json 统计引用;有引用且无 `?force=true` → 400 `{referenced_by: [...]}`;否则删目录 |
| `POST /api/usertools/preview` | body = Recipe → 返回将生成的 fragment 文本(表单实时预览;生成器单一事实来源) |
| `GET /api/scenarios` | L5 条目补 `description`(recipe 的 description,回退现有固定文案)、`category`(routes.rs:196-204 就地增强) |

退役:`GET/PUT /api/usertools`(单文件,routes.rs:65)删除;`mgr-data/usertools.json` 停读停写(mgr 启动时 best-effort 删除)。

## 数据流与关键时序

1. 上架:表单 → `POST preview`(逐字段实时预览)→ `POST tools` → recipe.toml + fragment 落盘 → 货架立即可见(无缓存层,目录扫描即事实)。
2. 选购:创建/编辑向导 L5(EnvPicker)读 `GET /api/scenarios` → `env.user_tools` → 既有 job 管线。
3. 更新:`PUT tools/{id}` → fragment 重生成 → 引用沙箱**下次重建**时 env_hash 变化换新镜像;前端保存时提示。不主动重建。
4. 删除:引用统计阻断 → force 逃生口 → 该沙箱下次重建前须先在编辑 env 移除(与 envhash.rs:87 现有报错语义衔接)。

## 兼容与迁移

- 存量 qpdf 目录:无 recipe → Card 标「自定义片段」,`PUT .../fragment` raw 可编,构建无感。
- 根下 `usertools/fragment.Dockerfile` 在本任务提交中删除;`scenarios/user-tools/` 空目录顺手清掉。
- 无 DB schema 变更;无镜像构建语义变化(env_hash 对同一 fragment 内容不变,已建沙箱不重建)。
- aio-env-config skill 的 user-tools 相关参考文档(recipes.md / scenario-authoring.md 条目)同步改为指向商店流程。

## 取舍记录

- **不做级联卸载**:删工具自动改写他人 env 会出现「env 与镜像不一致」中间态,不可预期;阻断+force 语义最干净。
- **raw 也入 recipe(表单路径)**:保证「生成器是唯一写入口」的简单心智;仅存量目录保留 fragment-only 形态,数量收敛不增长。
- **预览走服务端**:生成逻辑单一事实,前端永不复刻模板(避免双实现漂移)。
- **种子不物化预放目录**:usertools/ 纯用户领地,git 只见真上架物;种子随 mgr 二进制走版本。

## 回滚

revert 单提交即可:文件全部为普通仓库文件/mgr-data 内产物;无数据迁移、无 schema、无镜像标签语义变化。唯一外部行为变化(单文件 API 404)无已知消费方(唯一消费者 UserToolsPage 同提交内删除)。
