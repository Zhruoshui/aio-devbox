# Plan: L1 版本化(node/python 可选版本)+ nvm/uv 可选场景

## 目标(已与用户确认)
1. **L1 进 TUI**(推翻 Q2=A、回答 Q5):node/python 作为"始终启用、不可取消、带版本下拉"的条目可见。
2. **node/python 走 tarball 装系统路径,版本可选**(node: nodejs.org;python: python-build-standalone)。不用 nvm/uv 装 L1 默认版本。
3. **nvm/uv 作为 L3(lang)可选场景**,勾选才烘,运行时管多版本(装卷上,抗 recreate)。

## 模型扩展(沿用统一机制,不另起一套)

### `config/src/scenario.rs` — `ScenarioMeta` 增字段
```rust
#[serde(default)] pub always_on: bool,                  // L1: 不可取消, gen 必装
#[serde(default)] pub versions: Vec<Version>,           // 可选版本; 空=不可版本化
#[serde(default)] pub default_version: Option<String>,  // 默认版本 label
```
`Version { label: String, #[serde(flatten)] vars: HashMap<String,String> }`:label 是下拉显示,其余 key 作模板变量(gen 替换 `{{key}}`)。

### `config/src/manifest.rs` — `Enabled` 增版本选择
```rust
#[serde(default)] pub versions: Vec<VersionSelect>,  // { id, label }
```
manifest 只存 id+label;gen 按 (id,label) 回 scenario.toml 查 vars。`scenarios` 仍只存可勾选场景 id(always_on 不进)。

### `config/src/gen.rs` — 三处改动
1. **必装 always_on**:`ids = always_on_ids ∪ manifest.scenarios`,sort_by_layer 后去重(L1=os rank 0 自然排最前)。
2. **版本模板替换**:对有 versions 的场景,按选中 label 查 vars,把片段 `{{version}}`/`{{tag}}` 占位符替换成值(无占位符的片段不变,向后兼容)。
3. assemble 不变(head + 片段 + tail)。

### `config/src/tui.rs` — 版本下拉行
- always_on + 版本化:`🔒 Node.js  [20.18.0]  描述`。←→ 循环 versions;Space 无效(不可取消)。
- 普通:`[x]/[ ] name 描述`,Space 切换。
- 新增 `version_sel: HashMap<id,label>`,从 manifest 或 default_version 初始化;保存时写 `scenarios` + `versions`。
- L1 分组头自动出现(node/python 是 category="os")——现有分组逻辑已支持,无需改。标题/帮助加 `←→=改版本`。

## `Dockerfile.base.head` 瘦身
移除 node 块(→ node 场景)和 `python3 python3-pip python3-venv`(→ python 场景)。保留:apt 源 HTTPS、ca-cert 自举、apt 装 curl/git/gnupg2/xz-utils/build-essential/pkg-config/libssl-dev/locales/tzdata/sudo、locale-gen、用户 gem。head 变纯基础设施(无语言运行时)。

## 新场景

### `scenarios/node/`(L1 always_on 版本化)
fragment 用 `ARG NODE_VERSION={{version}}` + nodejs.org tarball(原 head 逻辑搬来,参数化)。versions: 20.18.0 / 22.11.0 / 18.20.4。

### `scenarios/python/`(L1 always_on 版本化)
fragment 用 `{{version}}`+`{{tag}}` 下 python-build-standalone `install_only.tar.gz`,解压到 `/usr/local`。versions: 3.11.10 / 3.12.7 / 3.13.0(各带 tag)。

### `scenarios/nvm/`(L3 可选,不版本化)
烘 nvm.sh 到 `/opt/nvm`(系统路径,躲卷);profile.d 运行时 `mkdir -p ~/.nvm && ln -sf /opt/nvm/nvm.sh ~/.nvm/nvm.sh` + `export NVM_DIR=~/.nvm && source` —— 使 `nvm install` 写卷、抗 recreate,nvm 在 `$NVM_DIR/nvm.sh` 找到自己。

### `scenarios/uv/`(L3 可选,不版本化)
单二进制装 `/usr/local/bin/uv`(躲卷);运行时 `uv python install` 装到 `~/.local/share/uv`(卷,抗 recreate)。

## 依赖核实(已读 Dockerfile 确认)
- code-server `FROM sandbox-base`:需 node,不需 python ✓
- app:Rust builder 独立;web-builder 需 node(vite/tsc 纯 JS 不需 python);runtime `FROM sandbox-base` ✓
- 结论:node/python 都 always_on 烘进 base 满足全部依赖;移除 apt python3 安全(实现时跑完整 build 复验)

## 风险点(实现时重点验证)
1. **python-build-standalone URL**:version+tag 耦合,asset 命名须核对(参考 uv 的 python-versions 映射),先钉一个已知可用 tag。
2. **nvm 卷 vs 系统自定位**(最易踩坑):nvm.sh 期望自己在 `$NVM_DIR/nvm.sh`。用 profile.d 软链方案解决;须实测 `nvm install`/`use` + recreate 存活。
3. **网络可达性**:nodejs.org(现 head 已用,通)、github releases(python-build-standalone/uv/nvm;opencode 场景已用 github,通)。
4. **TUI ←→ 交互**:无 tty 环境 smoke 到 raw_mode 即可(同现状)。

## 测试
- `cargo test`:scenario(always_on/versions 解析、default)、gen(模板替换、always_on 必装、L1 排序)。
- `make gen`:启用 node+python,检查 Dockerfile.base 含替换后版本、无 `{{}}` 残留。
- `make build-base` + 进容器:`node --version`/`python3 --version` 匹配所选;code-server + app web-builder 仍 build 通过。
- 启用 nvm/uv:build + 运行时 `nvm install`/`uv python install` + recreate 存活。

## 文档
README:L1 分层表(node/python 成可见可版本化场景;head 只剩基础设施)、场景清单加 nvm/uv、版本下拉用法。`docs/offline-*.md`:补 node/python/uv/nvm 制品离线预置。

## 不做(范围外)
- L5 外部服务、opencode 死面板自动检测、build-essential 下放(留 head)。
- nvm/uv 本身不版本化(跟 latest release;管理的运行时版本由用户运行时装)。
