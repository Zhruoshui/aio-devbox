# Design: Scenario-based preset dev environment profiles

方案 A 已定(见 prd.md)。本文记录技术设计:配置工具形状、装配契约、Makefile/离线流程、Dockerfile.base 拆分迁移、以及 rust/python-dev 两个示例场景片段。

## 1. 架构与边界

新增一个**构建/宿主侧** Rust crate `config/`(与 `app/` 平级,不属于运行栈),产出单一二进制 `aio-config`,两个子命令:

- `aio-config tui` -- ratatui 交互勾选,读 `scenarios/*/scenario.toml` 列出可选场景,写 `.aio/enabled.toml`。
- `aio-config gen` -- 读 `.aio/enabled.toml` + `Dockerfile.base.head` + `Dockerfile.base.tail` + `scenarios/<id>/fragment.Dockerfile`,拼出 `Dockerfile.base`。

`tui` 与 `gen` 共享:scenario 发现(扫 `scenarios/*/scenario.toml`)、manifest 读写(`.aio/enabled.toml` 的 TOML schema)。共享代码放 `config/src/scenario.rs` + `config/src/manifest.rs`。

边界:
- `config/` 不依赖 `app/`(独立 crate,独立 `Cargo.toml`)。
- 生成器**只做拼接**,不解析片段内容、不渲染安装步骤(片段是裸 Dockerfile)。
- `Dockerfile.base.head`/`.tail` 是手工维护的真相源;`Dockerfile.base` 是生成产物(见 §6 是否提交)。

```
config/                     新 Rust crate(aio-config 二进制)
  Cargo.toml                ratatui + toml + serde + anyhow
  Dockerfile                多阶段:rust 构建 -> slim 运行镜像
  src/
    main.rs                 clap 子命令分发 tui/gen
    scenario.rs             扫 scenarios/*/scenario.toml -> Scenario{id,name,desc,fragment_path}
    manifest.rs             .aio/enabled.toml 读写(Enabled{scenarios:Vec<String>})
    tui.rs                  ratatui 勾选 UI
    gen.rs                  装配 Dockerfile.base
scenarios/                  场景库(每场景一目录)
  rust/
    scenario.toml
    fragment.Dockerfile
  python-dev/
    scenario.toml
    fragment.Dockerfile
Dockerfile.base.head        现有 Dockerfile.base 第 1..68 行(FROM..chown /home/gem),全 root
Dockerfile.base.tail        现有第 70..71 行(USER gem + WORKDIR /home/gem)
Dockerfile.base             生成产物 = head + enabled fragments + tail
.aio/enabled.toml           TUI 写的选择清单(machine-local,gitignore)
```

## 2. 数据流与契约

### 2.1 manifest `.aio/enabled.toml`

```toml
# 由 `aio-config tui` 写;`aio-config gen` 读。
scenarios = ["python-dev", "rust"]   # 任意顺序;gen 按字母序拼装
```

- 空/缺失 = 不启用任何场景(gen 退化为 head+tail,等价现状)。
- `gen` 对 enabled id 做存在性校验:某 id 在 `scenarios/` 找不到 -> 报错退出(防 TUI 与场景库不一致)。

### 2.2 `scenarios/<id>/scenario.toml`

```toml
id = "rust"                       # 必须与目录名一致(gen 校验)
name = "Rust 工具链"               # TUI 显示名
description = "rustup stable + rustfmt + clippy + rust-analyzer,装到 /opt/rust(gem 可写)"
```

### 2.3 `scenarios/<id>/fragment.Dockerfile`

裸 Dockerfile 片段(字面 `ARG`/`ENV`/`RUN`)。契约:
- **以 root 执行**(head 未设 `USER gem`,片段在 head 之后、tail 之前)。
- 工具装**系统路径**(`/opt`、`/usr/local`),**禁止**装 `/home/gem/*`(卷遮盖,见 prd R6)。
- 需 gem 可写的系统路径(如 `/opt/rust`),片段末尾 `chown -R 1000:1000 <path>`。
- 自包含:不依赖另一场景的产物(MVP 假设场景互不依赖;文档注明)。

### 2.4 装配顺序(gen)

```
Dockerfile.base =
    Dockerfile.base.head
  + Σ(按 enabled id 字母序) scenarios/<id>/fragment.Dockerfile
  + Dockerfile.base.tail
```

- 字母序保证可复现构建(同选择 -> 同 Dockerfile.base)。
- 片段间用注释分隔(`# >>> scenario: rust >>>` / `# <<< scenario: rust <<<`),便于人读/debug。

## 3. 示例场景片段

### 3.1 `scenarios/rust/fragment.Dockerfile`

```dockerfile
# >>> scenario: rust >>>
ARG RUST_VERSION=stable
ENV RUSTUP_HOME=/opt/rust/rustup \
    CARGO_HOME=/opt/rust/cargo \
    PATH=/opt/rust/cargo/bin:$PATH
# 在线 rustup(构建机有网);离线靠整镜像 save/load,不需要 vendor。
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
      | sh -s -- -y --profile default --default-toolchain ${RUST_VERSION} \
                 --component rust-analyzer \
 && rustup default ${RUST_VERSION} \
 && rustc --version && cargo --version && rustfmt --version && cargo clippy --version \
 && chown -R 1000:1000 /opt/rust
# <<< scenario: rust <<<
```

要点:
- `RUSTUP_HOME`/`CARGO_HOME` 指 `/opt/rust/*`(系统路径),**不**用默认 `~/.cargo`(会被卷遮盖)。
- `chown 1000:1000 /opt/rust` -> gem 可 `rustup`/`cargo install` 自由写。
- `--profile default` = rustc+cargo+rustfmt+clippy(+docs);`rust-analyzer` 组件供 code-server LSP。
- 版本 `stable`(rustup 解析最新);如需可复现可钉 `RUST_VERSION=1.82.0`。
- **login-shell PATH 陷阱(验证发现,已修)**:rustup 代理在 `/opt/rust/cargo/bin`(自定义 PATH),`ENV PATH` 只被非 login shell 继承;**login shell**(`bash -l`,即 AIO 终端面板 pty)source `/etc/profile` 会把 PATH **重置**为标准集,丢掉 `/opt/rust/cargo/bin` -> `cargo` 找不到(AC1 的 AIO 终端面板会挂)。修:把 `/opt/rust/cargo/bin/*` 代理 `ln -sf` 进 `/usr/local/bin`(login/非 login/交互/非交互**所有** shell 的 PATH 都含 `/usr/local/bin`,见 D4 落点反转)。代理是稳定路径,rustup 升级工具链不影响软链。直接装 `/usr/local/bin` 的工具(uv/ruff/opencode/node)无此问题。

### 3.2 `scenarios/python-dev/fragment.Dockerfile`

```dockerfile
# >>> scenario: python-dev >>>
ARG UV_VERSION=0.5.11
ARG RUFF_VERSION=0.8.4
# base 已有 python3/pip/venv;本场景补 uv + ruff(单二进制,装 /usr/local/bin,系统路径不被卷遮盖)。
RUN curl -fsSL "https://github.com/astral-sh/uv/releases/download/${UV_VERSION}/uv-x86_64-unknown-linux-gnu.tar.gz" -o /tmp/uv.tgz \
 && tar -xzf /tmp/uv.tgz -C /tmp \
 && install -m 0755 /tmp/uv-x86_64-unknown-linux-gnu/uv /usr/local/bin/uv \
 && curl -fsSL "https://github.com/astral-sh/ruff/releases/download/${RUFF_VERSION}/ruff-x86_64-unknown-linux-gnu.tar.gz" -o /tmp/ruff.tgz \
 && tar -xzf /tmp/ruff.tgz -C /tmp \
 && install -m 0755 /tmp/ruff-x86_64-unknown-linux-gnu/ruff /usr/local/bin/ruff \
 && rm -rf /tmp/uv.tgz /tmp/uv-x86_64-unknown-linux-gnu /tmp/ruff.tgz /tmp/ruff-x86_64-unknown-linux-gnu \
 && uv --version && ruff --version
# <<< scenario: python-dev <<<
```

> 上述版本号为示意,实现时钉当时最新稳定版并验证下载 URL 与架构(x86_64)。

## 4. Makefile 改动

新增/调整目标(`config/` 是构建/宿主侧工具,在线机构建):

```makefile
AIO_CONFIG_IMAGE := aio-config

# 构建 aio-config 镜像(在线机,rust 拉依赖)。
build-config:
	docker build -t $(AIO_CONFIG_IMAGE) -f config/Dockerfile config/

# 交互勾选场景 -> 写 .aio/enabled.toml。
config: build-config
	mkdir -p .aio
	docker run --rm -it \
	  -v $(PWD)/.aio:/aio \
	  -v $(PWD)/scenarios:/scenarios:ro \
	  $(AIO_CONFIG_IMAGE) tui --manifest /aio/enabled.toml --scenarios /scenarios

# 内部:按选择生成 Dockerfile.base。
gen: build-config
	docker run --rm \
	  -v $(PWD):/repo \
	  $(AIO_CONFIG_IMAGE) gen --repo /repo

# build-base 现在先 gen 再 docker build。
build-base: gen
	docker build -t sandbox-base -f Dockerfile.base .

# NOBUILD=1 跳过 build-base + compose --build(离线机:镜像已 load)。
up: 
ifndef NOBUILD
up: build-base ensure-hash
	$(COMPOSE) $(PROFILE_FLAGS) up -d --build
else
up: ensure-hash
	$(COMPOSE) $(PROFILE_FLAGS) up -d
endif
```

- `build` / `down` / `restart` / `logs` / `clean` 同前(`build` 依赖 `build-base` 自动带上 gen)。
- `clean` 增删 `docker rmi aio-config`。

## 5. 离线流程(R4)

1. 联网机:`make config`(勾选)-> `make build`(gen + build sandbox-base + build app/code-server/vnc)-> `docker save sandbox-base sandbox-app sandbox-code-server sandbox-vnc aio-config -o aio-images.tar`。
2. 传 `aio-images.tar` + 仓库(`scenarios/`、`Dockerfile.base.head/.tail`、compose、Makefile)到离线机。
3. 离线机:`docker load -i aio-images.tar` -> `make up NOBUILD=1`(跳过 build-base/compose --build,用已 load 镜像)。
4. 离线机改选:`make config`(aio-config 镜像已 load,TUI 可跑,不联网)-> 但改选需重建 base -> 离线机无法 build -> **改选必须回联网机重建+save+load**。这是方案 A 的固有代价(prd 已声明改选=重建)。

> 离线机 `make config` 仅用于查看/不改场景时的浏览,或接受"改选回联网机"。文档注明。

## 6. 兼容性与迁移

- **拆分 `Dockerfile.base`**:把现 repo 根的 `Dockerfile.base` 按行拆成 `Dockerfile.base.head`(第 1..68 行:FROM..`chown 1000:1000 /home/gem`)+ `Dockerfile.base.tail`(第 70..71 行:`USER gem` + `WORKDIR /home/gem`)。第 69 行空行归 tail。原 `Dockerfile.base` 删除(改为生成产物)。
- **`Dockerfile.base` 是否提交**:推荐**提交生成产物**(保持 `docker compose --profile build build base` 与直接 `docker build -f Dockerfile.base` 仍可用,不强制走 Makefile)。head/tail/fragments 是真相源;`make build-base` 重生成覆盖。加一致性检查(可选):CI/`make check` 跑 gen 比对已提交 Dockerfile.base。
- **`.aio/`**:gitignore(`.aio/enabled.toml` 机器本地选择)。提交 `.aio/enabled.toml.example`(空 `scenarios = []`)作示例。
- **现有 `make up` 行为**:不带 `NOBUILD` 时行为不变(gen 是幂等无副作用的拼接,选择为空时 gen 出的 Dockerfile.base 与原文件等价)。

## 7. 权衡与待定实现细节

- **rust 装系统路径且 gem 可写**(`/opt/rust` chown gem):个人单用户沙箱的最简解;多用户场景才需分离。MVP 接受。
- **场景间顺序/依赖**:MVP 字母序、假设独立。若将来某场景依赖另一场景(如 `wasm` 依赖 `rust`),用片段内自包含安装或显式文档约束,不在生成器引入依赖图。
- **版本钉死**:片段用 `ARG` 钉版本(可复现);`stable` 这类浮动 tag 可接受(个人用)。实现时确认每个 URL + 架构。
- **TUI 仅 ratatui 勾选**:无搜索/分类(MVP 场景少);后续场景多了再加分类。
- **gen 容器挂整个 repo**(`-v $(PWD):/repo`):gen 只读 head/tail/scenarios/.aio,只写 `Dockerfile.base`;容器以非 root 跑时写权限靠宿主 uid 映射,实现时验证写权限(必要时 `--user $(id -u):$(id -g)`)。
