# Design：L3 全量替换为 mise 管理方案

> 2026-09-03 修订：采纳 issue #6 评审意见——D1 补 `MISE_CONFIG_DIR` 第四 env、
> D2 可见性改 ENV 主导 + profile.d 补偿、D3 补 rust `profile="default"`、D4 移除
> 「装到卷」逃生门、D5 配方改两 env 一致覆盖、新增 D9 粒度取舍、风险表补两项。

## 架构总览

```
before                                     after
─────────────────────────────              ─────────────────────────────
L3 lang (5 个手写场景)                     L3 lang (1 个场景)
  scenarios/rust    rustup→/opt/rust         scenarios/mise
  scenarios/go      tarball→/usr/local/go      ├─ mise 二进制 → /usr/local/bin
  scenarios/nvm     nvm.sh→/opt/nvm            ├─ mise use 烘焙 rust/go/uv/ruff
  scenarios/uv      二进制→/usr/local/bin       │    → /opt/mise (系统路径)
  scenarios/python-dev uv+ruff→/usr/local/bin  └─ ENV 4×env+shims PATH; profile.d(activate)
L4 app                                       L4 app
  scenarios/opencode tarball→/usr/local/bin    (删除,由 mise 场景一并烘焙)
```

## 核心决策与依据

### D1 烘焙数据布局：全部家目录与配置进 /opt/mise

```
/opt/mise/                    ← MISE_DATA_DIR + MISE_CONFIG_DIR(镜像层,躲卷遮盖)
├── installs/{rust,go,uv,ruff,opencode}/<ver>/   mise 后端安装物
├── downloads/                ← 构建期缓存(烘焙末尾清理)
├── shims/                    ← shim 目录(ENV 烘进 PATH,activate 时也接管)
├── config.toml               ← 全局 [tools] 清单(MISE_CONFIG_DIR 重定向至此)
├── rustup/                   ← RUSTUP_HOME(core:rust 的真实工具链,~1.4GB)
└── cargo/                    ← CARGO_HOME(rustup 代理二进制)
```

依据（PoC 实测 + issue #6 评审）：
- 只重定向 MISE_DATA_DIR 不够：core:rust 内部跑 rustup，工具链默认落 `~/.rustup`
  （被卷遮盖），运行时 symlink 悬空触发 auto_install 静默重下 1.4GB。
- **config 落点是 PoC 盲区（issue #6 点 5）**：全局 config.toml 由 MISE_CONFIG_DIR
  控制，默认 `~/.config/mise/config.toml`，与 MISE_DATA_DIR 相互独立。烘焙期
  `mise use --global` 会把 [tools] 写进镜像 `/root/.config/mise/config.toml`。
  PoC Phase B 只测了空白卷（Docker copy-on-first-use 恰好把 config 带进卷）；
  旧卷被用户自己的 config 遮盖时 shim 读不到 [tools] → 报 "No version is set" /
  触发 auto_install，恰好是要防的失败模式。修法：`MISE_CONFIG_DIR=/opt/mise`
  第四个 env 一起重定向，配置随镜像走，卷遮盖无从触及。实现期按
  「卷上预置无 [tools] 的 config」补负向验证。
- 与原 scenarios/rust 的 `/opt/rust/{rustup,cargo}` 布局同构，心智模型不变。

### D1.1 rust 后端的 profile 与组件（issue #6 点 6）

- mise 的 `[tools.rust]` 不设 `profile` 时走 rustup 配置，全新 RUSTUP_HOME 即
  **minimal**（仅 rustc/rust-std/cargo），会丢 clippy/rustfmt——现 rust 场景是显式
  `--profile default`。config.toml 写
  `[tools] rust = { version = "...", profile = "default" }`。
- rust-analyzer 不在任何 profile 里，仍需显式
  `mise exec -- rustup component add rust-analyzer`（缺组件时 rustup 代理沿
  PATH fallback 撞 shim → 死循环，PoC 实测）。

### D2 可见性：ENV 主导 + profile.d 补偿（issue #6 点 2 修订）

原设计「profile.d activate 单通道」被评审推翻：被收编的 6 个场景里 **5 个
（rust/go/uv/ruff/opencode）现状是全 shell 可见**——rust/go 显式 symlink 进
`/usr/local/bin`（fragment 注释原话 "EVERY shell"），uv/ruff/opencode 直接安装到
`/usr/local/bin`；只有 nvm 是 login-only。「与 nvm 同病、不新增恶化」不成立，
回归面实际扩大：code-server 非 login 终端、被编辑器/agent spawn 的非交互子进程
都会失去工具。

修订为双保险（对齐原 rust 场景「ENV PATH + symlink」手法，symlink 农场换成
shims 目录）：

```dockerfile
ENV MISE_DATA_DIR=/opt/mise \
    MISE_CONFIG_DIR=/opt/mise \
    RUSTUP_HOME=/opt/mise/rustup \
    CARGO_HOME=/opt/mise/cargo \
    PATH=/opt/mise/shims:$PATH
```

- **ENV 通道**：容器内全部进程继承（非 login shell、非交互子进程、code-server
  终端全覆盖）；
- **profile.d 通道**：`/etc/profile.d/mise.sh` 重新导出四个 env +
  `eval "$(mise activate bash)"`，补偿 login shell 被 `/etc/profile` 重置 PATH；
  activate 的 hook-env 实时计算动态环境（core:rust 注入的 RUSTUP_TOOLCHAIN、
  go 的 GOPATH 提示），比静态 PATH 更正确；
- shims 目录替代 symlink 农场：工具集增减自动维护，无需逐二进制 ln；
- activate 与 shims 并存是 mise 认可的组合（activate 注入路径优先），但 **PoC 未
  实测该组合**，列为 Step 2 验证项；
- 覆盖面核对：app pty 终端面板 `bash -l`（含 `?cmd=opencode` 走 `bash -l -c`）、
  CI probe `bash -lc` 走 profile.d 通道；其余进程走 ENV 通道。

### D3 版本管理：ARG 块，不做 TUI [[versions]]

原 TUI 版本下拉是「单工具选版本」语义；mise 场景是「rust+go+uv+ruff+opencode 联合体」，
下拉无意义。版本固定在 fragment 顶部 ARG 块（MISE_VERSION/RUST_VERSION/GO_VERSION/
UV_VERSION/RUFF_VERSION/OPENCODE_VERSION），升级=改一行。scenario.toml description
注明此约定。node/python 的 [[versions]] 机制不受影响。config.toml 的 [tools] 由
fragment 按 ARG 生成，rust 带 `profile = "default"`（见 D1.1）。

### D4 运行时用户自装工具的落点（关键取舍；issue #6 点 3 修订）

MISE_DATA_DIR/MISE_CONFIG_DIR 在全部 shell 里指向 /opt/mise（镜像层只读语义），
运行时 `mise use -g X` 写容器可写层 → recreate 即丢。这是与原 nvm/uv「运行时装到卷」
语义的**已知回归**，本任务**接受**该取舍，理由：
- 烘焙工具集是本任务主目标；运行时增装属于增强。
- ~~「装到卷」逃生门~~（评审推翻）：原方案称可显式
  `MISE_DATA_DIR=~/.local/share/mise mise use X` 按调用覆盖——不可行。全局
  config 的 [tools] 清单跨 data dir 共享：装完回普通 shell，mise 按
  /opt/mise/installs 解析不到 → 误触发 auto_install（在线静默重下、离线报错），
  恰是 Phase B 要防的失败模式；且 shims PATH 化后，卷上的 shims 目录也不在 PATH。
- 该能力与「镜像种子→卷活态」是同类问题（绝对路径 symlink 语义、首次登录延迟、
  config 双目录合一），一并留待后续任务单独设计，本任务不承诺。

### D5 离线分发：镜像级 + 目录级两条路，均已 PoC 验证

1. 基线升级（换 rust/go 版本）：改 ARG → `make build-base` → `make save`/`load`。
   与现有离线模型完全一致，无新增步骤。
2. 离线补装新工具/新版本（不动镜像）：三步搬迁配方——
   - 联网机：在 `MISE_DATA_DIR` 与 `MISE_CONFIG_DIR` **一致覆盖**的环境里
     `mise install X`，tar 整个 data dir（config 落其内，单 tar）；
   - 传输到离线机；
   - 离线机：解压回**相同绝对路径**（installs 内部是绝对路径 symlink，换路径即断），
     同样覆盖两个 env 后 `MISE_OFFLINE=1 mise install` 校验登记。
   - 注意：配方验证在隔离容器做（两 env 一致覆盖，不与烘焙的 /opt/mise 混用）；
     「运行中 sandbox 里混用卷路径 data dir」不承诺（D4 后续任务），文档按此措辞。
3. auto_install 在烘焙期关闭（`mise settings set auto_install false`），离线机缺工具
   时显式报错而非静默 hang。
4. MISE_OFFLINE 不设为镜像级默认：在线机器上 activate 遇缺工具时仍可自动补（体验优先），
   离线机由用户/文档显式设 MISE_OFFLINE=1。

### D6 opencode 的 upstream 迁移

mise registry 解析为 `aqua:anomalyco/opencode`（原场景用 sst/opencode GitHub release）。
实现期验证点：`opencode --version` 输出版本号正常、`app/services.toml` 的
type=agent 按钮探测（command -v opencode）通过。若 anomalyco 资产异常，回退方案是
mise 场景里保留原 sst tarball 安装行（不收编 opencode，恢复 scenarios/opencode）。

### D7 CI probe 适配

full 变体 tools 列表改为：
`node python3 mise rustc cargo go gofmt rustfmt uv ruff clang gcc gdb cmake ninja fzf rg bat fd opencode pi pi-web fc-list`
（移除 nvm——场景已删；新增 mise；rustc/cargo/go 等改由 mise 提供）。
probe 本身仍是 `bash -lc + command -v` 语义，守护 profile.d 通道；非 login 的
ENV+shims 通道由 AC 的 `bash -c` 抽查守护（D2 双保险各测一路）。

### D9 粒度坍缩（issue #6 点 4，已接受为取舍）

现状 TUI 可按场景粒度勾选（只装 go 不装 rust）；合并后启用 mise = 五工具全家桶
（rust 1.4GB + go + uv + ruff + opencode），minimal 侧用户失去按需粒度、镜像体积
不可按工具裁剪。**接受**：full 预设本就面向全套工具链；场景内 ARG 子集开关会让
TUI 语义（一个场景一份配置）与 fragment 复杂度明显上升，等真实需求出现再评估。
scenario.toml description 注明「启用即安装全部五工具」。

### D8 删除场景的连带清理

- `.aio/enabled.toml` 本地文件（若存在）删去 rust/go/nvm/uv/python-dev/opencode id
  ——gen 对未知 id 报错，属于期望的失败模式，但要避免用户本地文件踩坑（文档提醒）。
- presets（minimal/full）用 `["*"]` 或空列表，无显式 id，无需改。
- 技能 references 里 layers.md 的 L3 示例、scenario-authoring.md/recipes.md 的
  rust/go/nvm/uv 引用改为 mise 说法。

## 回滚设计

单 commit 完成全部变更（场景增删 + CI + 文档），git revert 即整体回滚；
`.aio/enabled.toml` 为本地未跟踪文件，回滚后需手工把旧 id 加回（文档注明）。
镜像层回滚 = 用旧 tag 重新 load（现有 make save 产物保留策略不变）。

## 风险清单

| 风险 | 概率 | 缓解 |
|---|---|---|
| mise registry 的 go/uv/ruff 后端行为与 PoC 的 rust/opencode 不一致 | 中 | 实现首步即验证五工具安装（见 implement.md Step 1） |
| aqua:anomalyco/opencode 资产不可用/不等价 | 中 | D6 回退方案 |
| activate + shims PATH 并存组合未实测（PoC 只测过 activate） | 中 | Step 2 双通道验证：`bash -lc`（activate）与 `bash -c`（ENV+shims）各过一遍 |
| MISE_CONFIG_DIR 重定向后 mise 行为异常（PoC 未测过该 env） | 中 | Step 2 首个验证项：config.toml 确认落 /opt/mise 且卷遮盖下不触发 auto_install（负向验证） |
| rust profile 生效姿势与 mise 版本相关 | 低 | config.toml `profile="default"` + AC 的 `cargo clippy --version` 探针兜底 |
| activate hook 与 code-server/vnc 面板的边缘交互 | 低 | PoC 已在 pty login shell 验证；CI probe 守护 |
| 烘焙体积膨胀（/opt/mise 含 downloads 缓存） | 低 | fragment 末尾 rm -rf /opt/mise/downloads 再验证 |
| 断网 build CI（无 cache）拉 mise 资产失败 | 低 | URL 全 HTTPS GitHub release，与现有场景同源 |
