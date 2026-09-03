# 离线环境向已部署 AIO 沙箱补装工具 · 测试报告

> 场景:AIO 沙箱(caddy 网关 + axum app + code-server + vnc/Chromium,共享命名卷 `aio_workspace`
> 挂载在 `app/code-server/vnc` 的 `/root`,容器内统一 root)已部署到**内网离线机**。
> 需要在不重建、不联网的前提下,给运行中的系统补装各类开发工具。

## 0. 背景、约束与架构前提

**核心约束(全程遵守)**:
- 联网机做**所有**联网工作(下载 / 编译 / vendor);
- 运行中的 4 容器 stack **全程不碰网络**;
- 每个测试完成后**清理环境**,不留痕。

**架构关键点(决定安装策略)**:
- **共享卷 `/root`** 跨 `app/code-server/vnc` 三个容器即时可见;命名卷**抗容器 recreate**(容器删了卷还在)。
- Debian 默认 `~/.profile`(`if [ -d "$HOME/.local/bin" ]; then PATH=...`)在 login bash 里**自动**把 `~/.local/bin` 加进 PATH —— 所以共享卷上的 `~/.local/bin` 是「跨容器、抗 recreate、免配 PATH」的**通用安装位**。沙箱终端面板跑的就是 `/bin/bash -l`(login bash)。
- 容器**可写层**(`/usr`、`/etc`…)随容器删除而消失 —— 只适合临时用或镜像内置。
- `sandbox-base` 已含 `build-essential` / `pkg-config` / `libssl-dev` / `curl` / `xz-utils`(C 工具链齐全),所以离线编译 Rust 只差 rustc/cargo。

## 1. 测试总览

| # | 工具 | 类型 | 离线安装位 | 在线机准备 | 离线机安装 | 结论 |
|---|---|---|---|---|---|---|
| 1 | pi (`@earendil-works/pi-coding-agent`) | npm 全局包 | `~/.local/{bin,lib}` | tar 全局包 bin+lib/node_modules | 解压到 `~/.local` | ✅(测试后已回退) |
| 2 | ripgrep / fd | 静态 musl 二进制 | `~/.local/bin` | 下 musl 静态二进制 | 解压到 `~/.local/bin` | ✅ |
| 3 | nginx | apt deb 包 | 容器可写层(易失) | `apt-get install --download-only` 收集 .deb | `dpkg -i` | ✅(易失,持久需进镜像) |
| 4 | mdBook(预编译) | cargo 单二进制 | `~/.local/bin` | `cargo install --locked` 编译出一个二进制 | docker cp 到 `~/.local/bin` | ✅ |
| 5 | mdBook(源码离线编译) | cargo 源码 + vendor | 共享卷 build 目录 | `cargo vendor` 打依赖 + 下 rust 工具链 | `cargo build --offline` 从源码编译 | ✅ |
| 6 | rust + rustup + TUI | rustup 工具链 + 无依赖 TUI | `~/.cargo`+`~/.rustup`(共享卷) | rust 二进制 + manifest + 组件 tarball + host bundle | `rustup toolchain link`+`default`,`rustc` 编 TUI | ✅(rustup 真·频道装未跑通,见 §7) |
| 7 | python + uv | 静态二进制(uv)+ python wheelhouse | `~/.local/bin`(uv)+ venv(共享卷) | 下 uv 二进制 + `python:3.11-slim-bookworm` 容器 `pip download` 建 wheelhouse(含 native wheel) | `uv venv --python 系统 python` + `uv pip install --no-index --find-links --offline` | ✅ |

---

## 2. npm 全局包(pi)

**方法**:npm 全局安装 = `bin/`(可执行脚本)+ `lib/node_modules/`(实现),两者**相对引用、可整体搬迁**。
- 联网机:`npm install -g --ignore-scripts @earendil-works/pi-coding-agent` 装到一个临时 prefix,把 `bin/` 和 `lib/node_modules/` 一起 tar。
- 离线机:解压到 `~/.local`(`bin`→`~/.local/bin`,`lib/node_modules`→`~/.local/lib/node_modules`)。login bash 自动找到 `~/.local/bin/pi`。

**坑**:`~/.local` 里若已有 `share/`、`state/` 等旧数据,不能 `rm -rf ~/.local`,要**外科式**只删 `bin`/`lib`。

**结论**:npm 全局包 = 把「bin + lib/node_modules」搬到 `~/.local`,无需 node 重新装。用户反馈「这个只是用于测试」,已整体回退。

## 3. 静态 musl 二进制(ripgrep / fd)

**方法**:musl 静态链接,`ldd` 显示 `not a dynamic executable` —— 无任何系统库依赖,丢哪都能跑。
- 联网机:下 musl 静态二进制(GitHub release 的 `*-x86_64-unknown-linux-musl.tar.gz`)。
- 离线机:解压到 `~/.local/bin`,`rg --version` / `fd --version` 即用。

**坑**:一次性容器若用 `--user root` + bind mount,产出文件 root 属主,宿主删时要 `sudo rm`。非 root 工具建议 `--user $(id -u):$(id -g)` 避免属主问题。

**结论**:静态二进制是最省事的离线工具类型 —— 一个文件进 `~/.local/bin` 即可。

## 4. apt 包(nginx)

**方法**:apt 包有依赖树,要在**相同 debian 版本**的联网机上把 .deb 全收齐。
- 联网机(同 bookworm):`apt-get install --download-only nginx` → .deb 全部落到 `/var/cache/apt/archives/`(含依赖),tar 收齐。
- 离线机:`dpkg -i *.deb` 装到**容器可写层**(`/usr/sbin/nginx`、`/etc/nginx/…`)。`nginx -t` 通过。

**关键限制**:装在**容器可写层** = 容器 **recreate 即丢**。所以 apt 装的系统服务**不适合**「不重建」的持久场景。持久方案:把 apt 装进 `sandbox-base` 镜像(`docker save` → 离线机 `docker load` → 用新镜像重建容器),或离线 `docker build`。即:**apt 临时验证用 dpkg,持久用镜像**。

**结论**:apt 包离线 = 同版本联网机 `--download-only` + `dpkg -i`;但天生「易失」,持久要进镜像。

## 5. cargo 单二进制(mdBook · 预编译)

**方法**:在联网机用 cargo 编译一次,只搬**产物二进制**(不搬工具链)。
- 联网机:一次性 `rust:1-bookworm` 容器(`--rm`,自带 rustc/cargo)跑 `cargo install --locked mdbook` → 编译出 mdBook v0.5.4 + 依赖。产物 `/usr/local/cargo/bin/mdbook`,15M。
- `ldd` 确认只链接 glibc(`libgcc_s` / `libm` / `libc` / `ld-linux`,bookworm 自带)→ 任何 sandbox-base 派生容器裸跑,不需要 cargo/rustc。
- 离线机:`docker cp` 到 `~/.local/bin/mdbook`(共享卷),三个容器 login bash 立刻可用。`mdbook build` / `serve` 正常,vnc chromium 打开 demo 验证。

**结论**:自包含二进制工具(同 ripgrep/fd)= 联网机编译、只搬一个二进制到 `~/.local/bin`,离线机不需要工具链。

## 6. cargo 源码离线编译(mdBook · vendor)

**目的**:真正在**离线机**上用 cargo 从源码编译,验证 rust 工具链离线配置 + cargo 包离线安装。

**方法(教科书式离线 Rust 构建)**:
- 联网机:
  1. 下 **rust standalone 工具链 tarball**(`static.rust-lang.org`,自带 `install.sh`,1.97.1)。
  2. 下 mdBook 源码(GitHub codeload,含 `Cargo.lock`)。
  3. 一次性 `rust:1-bookworm` 容器跑 **`cargo vendor --versioned-dirs`** → 250 个依赖源码进 `vendor/`,并生成 `.cargo/config.toml`(把 crates.io 指向 `vendor/`)。源码 + vendor 打包。
- 离线机(stack 无网络):
  4. `docker cp` 工具链包 + vendored 源码包进 app 容器。
  5. `./install.sh --prefix=/root/.rust --without=rust-docs` → **rustc 1.97.1 + cargo 1.97.1** 装到共享卷(无 rustup、无网络)。
  6. `cargo build --offline` → 用 vendor 包编译 250 crate + mdBook → 产出 `target/debug/mdbook` v0.5.4。
  7. 拷到 `~/.local/bin/mdbook`,app 容器 `mdbook serve --hostname 0.0.0.0 --port 3000`,vnc chromium 开 `http://localhost:3000`(共享 netns 直通回环)。

**坑(重要)**:`cargo vendor --quiet` 会把**输出到 stdout 的那段 `.cargo/config.toml` 也吞掉**(config 就是 stdout 本体)→ 生成 0 字节 config,离线构建会去联网失败。**去掉 `--quiet`** 重跑才对。这段 config 是离线构建的关键映射,缺了不行。

**结论**:`cargo vendor` + `--offline` 是 Rust 离线源码编译的正解;工具链用 standalone `install.sh`(不是 rustup,rustup 要联网)。

## 7. rustup + 工具链 + TUI demo

**目的**:用 **rustup**(对照 §6 的 install.sh)装 rust,并跑通一个 TUI hello-world。

**联网机准备**:
- 从 `rust:1-bookworm` 镜像取出 `rustup` 二进制(rustup 1.29.0,本地无下载);
- 下 `channel-rust-stable.toml` + minimal 三组件 tarball(rustc / cargo / rust-std);
- 复用 §5/6 已下的 rust 1.97.1 host bundle;
- manifest 剥掉 `https://static.rust-lang.org/dist/` 前缀 → 相对路径,搭 `file://` 镜像。

**离线机尝试 1 —— 真·rustup-init 频道安装(失败)**:

```bash
RUSTUP_DIST_SERVER=file:///tmp/rustup-mirror/dist \
./rustup-init -y --profile minimal --default-toolchain stable
```

rustup 管理器装上了,但报 **`no release found for 'stable'`**。诊断:manifest 完全正常(`manifest-version=2`、`[pkg.rustc] version=1.97.1`、`available=true`、**0 个签名字段** → rustup 只验 sha256),URL 也已 relativize。但 **rustup 1.29 的 file:// 频道解析有 bug**:拿到 manifest 却认不出 release。这是 rustup 离线频道的老大难,非 manifest 问题。

**离线机尝试 2 —— `rustup toolchain link`(成功,稳健法)**:

```bash
# host bundle install.sh 离线 stage 工具链
./install.sh --prefix=/root/.rust-toolchain --without=rust-docs
# rustup 接管
rustup toolchain link mytool /root/.rust-toolchain
rustup default mytool
```

结果:
- `which rustc` → `/root/.cargo/bin/rustc`(**rustup 代理 shim**,不是裸二进制);
- `rustc --version` → 1.97.1(经 shim 解析到 mytool);
- `rustup show` → `mytool (active, default)`。

`rustup toolchain link` 是 rustup 官方为「非 rustup 分发的工具链」设计的注册接口 —— 把离线 stage 的工具链纳入 rustup 管理,`default` / `show` / shim 都是真在用。

**TUI demo**:无依赖 Rust(ANSI 备用屏 + 边框 + `stty raw` 原始按键),`rustc --edition 2021 hello_tui.rs` **离线编译** → `hello_tui`。在工作区终端面板 `~/hello_tui` 跑通(备用屏里居中框显示 Hello World,按 q/Enter 退出)。

**诚实地讲**:rustup 1.29 的 file:// 频道安装没跑通(工具链本体仍由 install.sh 离线 stage,rustup 用 `link` 接管)。这是 rustup 离线频道的已知 finickiness,不是 manifest/组件问题。

**结论**:离线 rustup 的稳妥姿势 = install.sh stage 工具链 + `rustup toolchain link` + `rustup default`;真·频道装待 rustup 修 file:// 解析。

---

## 7b. python + uv 离线开发

**目的**:验证 python(系统自带)+ uv 二进制 + 本地 wheelhouse 的离线开发链路,含 native wheel 的 ABI 匹配。

**方法**:
- 联网机:
  1. 下 uv 二进制(GitHub release,0.12.1,glibc-only,bookworm 能跑)。
  2. 用 `python:3.11-slim-bookworm` 一次性容器(与离线目标同 ABI:cp311 + manylinux + glibc 2.36)`pip download -d wheels --only-binary :all: pydantic rich requests` → 14 个 wheel(含 native `pydantic_core-2.46.4-cp311-cp311-manylinux_2_17_x86_64` 和 `charset_normalizer` native wheel)。
- 离线机(app 容器,stack 无网络):
  3. `docker cp` uv → `~/.local/bin/uv`(login/非 login 终端均见,见 §12)。
  4. `uv venv --python /usr/bin/python3 /root/pydemo/.venv` —— 用系统 python,**无网络**(uv venv 默认不 seed pip)。
  5. `uv pip install --python <venv> --no-index --find-links /tmp/wheels --offline pydantic rich requests` —— 从本地 wheelhouse 装,全 14 包含 native pydantic-core。
  6. demo:pydantic 模型 + rich 面板 + import requests/pydantic_core → native `.so` 加载成功、模型验证返回 `{'name':'AIO-offline','age':1}`。

**坑**:
- native wheel(pydantic-core)要匹配目标 ABI(cp311 + manylinux_2_17 + glibc)。用与离线目标**同镜像基**的容器建 wheelhouse 是最稳的保证(`python:3.11-slim-bookworm` 对 sandbox-base 的 3.11.2)。
- `uv venv` 默认不 seed pip(无需网络);但 `--seed` 会拉 pip → 离线别用。建 venv 要 `--python /usr/bin/python3` 显式指定系统解释器,否则 uv 可能去拉 managed python(联网)。
- wheelhouse 是 docker cp / bind mount 产出 → root 属主。容器内现在统一 root,可直接删除;旧 gem(uid 1000)时代才需要 `docker exec -u root rm` + 宿主 `sudo rm`(同 rg/fd 那次的坑)。

**结论**:python+uv 离线开发链路通:uv 静态二进制 + 系统 python + 本地 wheelhouse(`--no-index --find-links --offline`)。native wheel 的 ABI 匹配是关键,用同基容器建 wheelhouse 可保证。

---

## 8. 通用规律与决策树

```
要补装的工具是什么?
│
├─ 自包含二进制(静态 musl / glibc-only / cargo 单二进制)
│    → 联网机编译/下载,只搬一个二进制到 ~/.local/bin   (§3, §5)
│
├─ npm 全局包
│    → 联网机 npm i -g,tar bin+lib/node_modules,搬到 ~/.local  (§2)
│
├─ apt deb 包(系统服务)
│    → 同版本联网机 apt-get --download-only 收 .deb
│    → 临时:dpkg -i(容器层,recreate 即丢)           (§4)
│    → 持久:进 sandbox-base 镜像(docker save/load)
│
├─ cargo crate(要在离线机编译)
│    → 联网机 cargo vendor 打依赖 + 下 rust standalone 工具链
│    → 离线机 install.sh 装工具链 + cargo build --offline  (§6)
│
├─ python + uv
│    → 联网机:下 uv 二进制 + 同基 python 容器 `pip download` 建 wheelhouse
│    → 离线机:`~/.local/bin/uv` + `uv venv --python 系统 python` + `uv pip install --no-index --find-links --offline`  (§7b)
│
└─ rustup 管理的工具链
     → install.sh stage + rustup toolchain link + default  (§7)
     (真·rustup-init 频道装:rustup 1.29 file:// 未跑通)
```

## 9. 各安装位的持久性对比

| 安装位 | 跨容器可见 | 抗容器 recreate | 适合 |
|---|---|---|---|
| `~/.local/`(共享卷) | ✅ app/vnc/code-server | ✅(命名卷) | 自包含工具、npm 包、cargo 二进制 —— **首选** |
| 容器可写层(`/usr`、`/etc`) | ❌ 仅本容器 | ❌(删容器即丢) | apt 包临时验证;持久要进镜像 |
| 镜像(`docker save` → `load`) | ✅(派生容器) | ✅(随镜像) | 持久系统服务、基础环境固化 |

> 「抗 recreate」一行已于 2026-08-03 经 force-recreate + down/up 实测验证,见 §11。

## 10. 清理与终验

**最终终验**:4 容器 `Up 2 hours`(uptime 连续、未 recreate);app login bash `rustc` / `cargo` / `rustup` / `mdbook` 全 `none`;`~/.local` 仅剩原始 `bin share state`;vnc 主 chromium 正常;宿主 `/tmp` 测试目录全清。

---

## 11. 容器生命周期持久性实测(2026-08-03 补测)

离线补装的工具是否真能扛住容器生命周期,补做了两组实证(往共享卷 `~/.local/bin` 放一个工具,重建后再验):

| 场景 | 命令 | 命名卷 `aio_workspace` | `~/.local/bin` 工具 | login bash PATH |
|---|---|---|---|---|
| force-recreate | `docker compose --profile code-server --profile vnc up -d --force-recreate` | 不动(`createdAt` 不变) | ✅ 仍在 | ✅ |
| down + up(保卷) | `docker compose ... down`(不带 `-v`)→ `up -d` | 不动 | ✅ 仍在 | ✅ |
| down 毁卷 | `docker compose ... down -v` | **删除** | ❌ 没了 | — |

实证细节:force-recreate 后四容器 ID 全变(app `5fd3→6c10`、vnc `56ee→9d4b6`、code-server `279d→af10`、gateway `42cd→04d1`),卷 `createdAt=2026-07-28` 不变,`~/.profile` 的 `~/.local/bin` PATH 块完整,三容器 login bash 仍跑工具;`down`(无 `-v`)清掉全部容器+网络后再 `up`,marker 同样存活。**只有 `down -v` 会毁卷丢工具**。

命名卷 + `~/.profile` 自动 PATH 的持久性,至此从架构承诺变成实证结论。

## 12. code-server 非 login 终端的 PATH gap 与修复

**gap**:code-server 自带内置终端默认跑**非 login 交互 bash**(`SHELL=/bin/bash`、settings 无 terminal profile 覆盖),只 source `~/.bashrc`;而 Debian 默认 `~/.bashrc` **不加 `~/.local/bin`**(只有 `~/.profile` 加)。结果:在 code-server 终端里键入共享卷 `~/.local/bin` 的离线工具(rg / mdbook / …)会「找不到」。对照:AIO 工作区的终端面板(pty 跑 `/bin/bash -l`,login)不受影响。

**修复(已应用,带 marker 可摘除)**:在 `~/.bashrc` 追加幂等 PATH 块(共享卷 → 三容器即时生效、抗 recreate/down-up、免重建):

```bash
# >>> AIO_LOCAL_BIN_BLOCK >>>
# 让 ~/.local/bin 在非 login 交互终端(code-server 内置终端等)也可见。
# login bash 由 ~/.profile 处理;此处补非 login 场景。幂等:已在 PATH 则不重复加。
case ":$PATH:" in
  *":$HOME/.local/bin:"*) ;;
  *) export PATH="$HOME/.local/bin:$PATH" ;;
esac
# <<< AIO_LOCAL_BIN_BLOCK <<<
```

实测:修后非 login 交互 bash(`bash -ic`,三容器)均能在 `~/.local/bin` 找到并跑工具(`extbin` → `extbin-ok`),修前为 `NONE`。清理 marker 后保留 `~/.bashrc` 修复。

**更彻底的固化**(下次重建 base 时做):`Dockerfile.base` 加 `ENV PATH=/root/.local/bin:$PATH`,连非交互 shell 都覆盖,且不依赖用户 dotfiles。

## 13. 覆盖度小结

- **已闭环**:6 类工具离线搬移(含 python+uv,native wheel ABI 匹配实测) + 跨容器可见 + login/非 login 终端 PATH(`~/.profile` + `~/.bashrc`) + force-recreate / down-up 持久 + 跨容器服务可达。
- **已知硬边界**:apt 持久服务要进镜像(需重建)、rustup 1.29 真·频道装未通(用 `toolchain link` 替代)。
- **本报告未覆盖(需另行评估)**:arm64 / 版本漂移 / 换 base 的耦合、apt 进镜像端到端、真气隙传输通道(u盘/scp)、工具增量升级、共享卷配额、加新服务/面板(services.toml + app 重建)。

## 14. mise 管理工具的整目录搬迁(2026-09-03 新增)

> 背景:sandbox-base 的 L3 工具链(rust/go/uv/ruff/opencode)已统一由 `scenarios/mise`
> 用 mise 烘焙到 `/opt/mise`(见 mise-poc/FINDINGS.md Phase C 与 scenarios/mise/fragment.Dockerfile)。
> 基线版本升级走 `make build-base` + `make save/load`(§9 镜像位)不变;本节配方解决的是
> **不动镜像**、给已部署环境补装 mise 管理的新工具/新版本。

**三步配方**(PoC Phase C 实测,任务 09-02 复验):

```bash
# 1) 联网机:在与烘焙一致的 env 覆盖下安装(隔离路径,不与镜像 /opt/mise 混用)
MISE_DATA_DIR=/root/mise-x MISE_CONFIG_DIR=/root/mise-x mise use -g fd@10.2.0
tar -cf mise-bundle.tar -C /root mise-x     # data + config 单 tar(config 在 data dir 内)

# 2) 传输 mise-bundle.tar 到离线机

# 3) 离线机:解压回完全相同的绝对路径,同 env 覆盖后校验登记
MISE_DATA_DIR=/root/mise-x MISE_CONFIG_DIR=/root/mise-x MISE_OFFLINE=1 mise install
fd --version
```

**四条实测约束**(违反即失败,失败模式见 mise-poc/FINDINGS.md):

1. **同路径**:installs 内部是绝对路径 symlink(如 `shims/fd -> /root/mise-x/installs/fd/...`),
   搬迁换路径即断。解压目标必须与联网机路径完全一致。
2. **两 env 一致覆盖**:`MISE_DATA_DIR` 与 `MISE_CONFIG_DIR` 必须同时设且指向同一目录。
   只设 data 不设 config → mise 读默认 `~/.config/mise/config.toml`,shim 报
   "No version is set" 或误触发 auto_install。
3. **单 tar 含 config**:全局 `[tools]` 清单就是 `MISE_CONFIG_DIR/config.toml`,与 data dir
   同处一目录,一个 tar 全带走。
4. **`MISE_OFFLINE=1` 是硬失败开关**:缺什么直接报错,不会回退到 downloads cache
   (downloads cache 单独搬也无效,aqua 后端照样报 offline error)。

**注意**:本配方在**隔离目录**(如上例 `/root/mise-x`,在共享卷上,抗 recreate)验证;
「运行中 sandbox 里把卷路径 data dir 与镜像 `/opt/mise` 混用」(shims PATH 化后卷上
shims 不在 PATH、全局 config 跨 data dir 共享会误触发 auto_install)属后续任务,
本配方不承诺。镜像内 `/opt/mise` 的 auto_install 已在烘焙期关闭
(config.toml `[settings]` 段),离线机器缺工具时显式报错而非静默 hang。
