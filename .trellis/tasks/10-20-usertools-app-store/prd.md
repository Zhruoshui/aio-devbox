# usertools 应用商店化:配置与安装简化

## Goal

把「用户自定义工具」从手写 Dockerfile 片段的极客通道,升级为 mgr 管理区的**工具应用商店**:用户像上架应用一样用配方表单定义一次工具,之后在创建/编辑沙箱时像选购应用一样挑选安装。两条核心诉求:

- **配置简化**:不必懂 Dockerfile 与三条硬规则(安装位 / 禁写 /root / 双通道自检)也能上架工具——硬规则由生成器写死,想违反都难。
- **安装简化**:上架即入货架,创建/编辑向导里直接选购;不再面对裸 id 勾选列表,也不再有单文件手动编辑面。

## Background(现状事实,代码锚点)

- 现定义通道:mgr-web「自定义工具」页是 `usertools/fragment.Dockerfile` 单文件直编文本框(mgr-web/src/pages/UserToolsPage.tsx:1),工具用 `# --- tool: <name> ---` 标记块;保存时 mgr 校验 /root 遮盖规则并同步出 per-tool 目录(mgr/src/usertools.rs:170)。
- 现安装通道(维持不变):创建/编辑沙箱 env 选配 L5 `env.user_tools` → `assemble_with_tools` 读 per-tool 目录的 `usertools/<id>/fragment.Dockerfile` 合入 per-sandbox Dockerfile.base → env_hash → 镜像构建(mgr/src/envhash.rs:125-142,mgr/src/jobs.rs:158)。
- **per-tool 目录早就是构建事实来源**;根下单文件纯属编辑面。传统 `make gen`(aio-config)路径已不含用户工具:config/src 不识别 usertools,scenarios/user-tools/ 已清空。
- 结构化表单曾在 09-23 任务「修正 2」被刻意移除(表单无法 round-trip 任意内容);「修正 3」将工具提升为独立 L5 场景。本任务以「表单只管理自己生成的工具、raw 逃生口保留」的方式正确吸取该教训。
- 工具目录被删而沙箱 env 仍引用时,该沙箱下次重建在装配阶段报缺片段错误(mgr/src/envhash.rs:86-87)——删除保护的数据基础是扫 `sandboxes` 表 env_json。
- env_hash 由装配后 Dockerfile.base 内容决定:改安装字段 → fragment 变 → hash 变 → 引用沙箱下次重建自动换新镜像;纯元数据编辑不影响 hash。版本流动机制天然成立。
- 09-23 旧 non-goal「mgr 不触发重建」继续有效:商店只管目录,安装仍由用户在创建/编辑 env 时发起。

## Requirements

### R1 方向翻转:per-tool 目录为唯一事实来源 + recipe 数据模型

- `usertools/<id>/` 是工具的完整定义单元;构建侧读取路径(envhash.rs:134)不变。
- 表单上架的工具写 `usertools/<id>/recipe.toml`(schema 含元数据 `name/description/category/version/homepage` + 安装配方 `[install]`),fragment.Dockerfile **由生成器产出**;纯元数据编辑不得改变 fragment 字节(哈希稳定)。
- 配方类型(可扩展 enum,本期五类):`apt`(包名列表)/ `url`(静态二进制或 tarball,含 bin 名)/ `github-release`(repo+tag+asset)/ `script`(脚本内容+自检命令)/ `raw`(内嵌 Dockerfile 片段)。
- 无 recipe.toml 的目录 = 手写/存量工具(如 qpdf),商店标记「自定义片段」,仅 raw 编辑,继续可用。

### R2 片段生成器:硬规则由构造保证

- 每类配方有确定性模板,产出含 `# --- tool: <name> ---` 标记的 fragment:apt 走 `--no-install-recommends`+清 lists;url/release 类装 `/usr/local/bin`(单二进制)或 `/opt/<id>`+软链(目录树);全部自带 `bash -lc` / `bash -c` 双通道自检。
- 生成结果仍过现有 `validate()`(/root 遮盖守卫,mgr/src/usertools.rs:264)作为纵深防御;生成器是唯一写 recipe-owned fragment 的路径。

### R3 内置种子货架

- mgr 内置静态种子清单(纯 apt 类、零字段门槛,8~12 个常用工具,具体名单实现期定);商店页设「推荐货架」,点「添加」才物化为 `usertools/<id>/`——`usertools/` 保持纯用户领地,tarball/Release 类不进种子(URL 易失效)。

### R4 per-tool CRUD API 与旧面退役

- 新增:`GET /api/usertools/store`(货架视图:我的货架卡片含被引用沙箱数 + 未物化推荐货架)、`POST /api/usertools/tools`(表单上架/从种子物化,slug 冲突校验)、`PUT /api/usertools/tools/<id>`(改 recipe 重新生成)、`DELETE /api/usertools/tools/<id>`(引用保护)、`POST /api/usertools/preview`(配方→fragment 预览,生成器单一事实)。
- 退役:单文件 `GET/PUT /api/usertools`(routes.rs:65)、根下 `usertools/fragment.Dockerfile`、`mgr-data/usertools.json`。
- 删除语义:有引用默认 400 返回沙箱名清单,`force=true` 逃生口;不做级联改写 env。
- `GET /api/scenarios` 的 L5 条目(routes.rs:196-204)补 recipe 元数据(name/description/category)。

### R5 StorePage 前端

- rail 位原位替换 UserToolsPage(App.tsx:73 的 userTools 条目),改名「工具商店」;页面 = 我的货架(卡片:名称/分类/版本/被引用数/编辑·删除)+ 推荐货架(一键添加)+ 上架按钮。
- 上架/编辑 = 配方表单对话框:类型选择 → 动态字段 → 服务端预览生成 fragment;`raw` 类型与无 recipe 存量工具进 raw 编辑器(复用现有 textarea 交互)。
- 删除确认框展示被引用沙箱名清单;编辑被引用工具时轻提示「下次重建生效」。
- EnvPicker L5 区渲染 recipe 元数据,替换裸 id + 固定描述;zh/en i18n 完整。

### R6 迁移兼容

- 存量 per-tool 目录(qpdf 等)无感继承:无 recipe 也能上架选购与构建;已建沙箱 env 引用不受影响。
- 回滚 = revert 提交:无 DB schema 变更,recipe/fragment 均为普通文件。

## Acceptance Criteria

- [x] AC1 表单上架 apt 工具(如 htop)全程无 Dockerfile 知识:recipe.toml + fragment.Dockerfile 生成,fragment 含双通道自检、无 /root 写入、过 validate()。
- [x] AC2 上架后立即可在创建向导 L5 区选购,展示 recipe 的 name/description/category。
- [x] AC3 选装该工具创建沙箱,构建成功后容器内 `bash -lc` 与 `bash -c` 均能 `command -v` 到工具。
- [x] AC4 推荐货架条目添加前不在我的货架/选购列表,点添加后物化入 `usertools/<id>/` 并可选购。
- [x] AC5 删除无引用工具成功;删除被引用工具返回 400 + 沙箱名清单;`force=true` 后成功。
- [x] AC6 改安装字段 → fragment 变化;只改元数据 → fragment 字节不变(env_hash 稳定,有单测锚定)。
- [x] AC7 存量 qpdf 目录显示为「自定义片段」且 raw 可编辑;单文件 API 退役后旧端点 404,StorePage 接管 rail 位;`mgr-data/usertools.json` 不再读写。
- [x] AC8 `cargo test -p aio-mgr`(recipe 解析/生成器确定性/引用统计/路由)与 mgr-web `tsc --noEmit && vite build` 全绿。
- [x] AC9 StorePage/EnvPicker/表单对话框 zh/en 文案齐全。

## Out of Scope

- runtime 安装到已运行沙箱(exec 通道)——安装只发生在创建/编辑 env 的重建。
- mgr 主动触发重建(旧 non-goal 延续);级联自动卸载。
- 多机分享(导出/导入)——`usertools/<id>/` 目录即包,载体已就绪,本期不做皮。
- npm/pip 全局包一等安装类型——五类已覆盖,enum 可扩展,raw 逃生口可写。
- 商店条目图标(分类色块/首字母头像代替)。
