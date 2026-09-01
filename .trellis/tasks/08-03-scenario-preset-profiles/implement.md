# Implement: Scenario-based preset dev environment profiles

执行计划。设计见 `design.md`,需求/AC 见 `prd.md`。顺序执行,每步带验证命令。回滚点标注 §R。

## 前置:留回滚锚点

- §R0 当前分支 `feat/aio-sandbox-mvp` 干净?`git status` 确认 `docs/` 外无未提交改动。原 `Dockerfile.base` 已在 git 中(回滚靠 `git checkout -- Dockerfile.base`)。

## Phase 1:拆分 Dockerfile.base + 场景库骨架

1. 拆 `Dockerfile.base` -> `Dockerfile.base.head`(现第 1..68 行,末尾含 `chown 1000:1000 /home/gem` 的 RUN)+ `Dockerfile.base.tail`(`USER gem` + `WORKDIR /home/gem`)。删 repo 根 `Dockerfile.base`(改生成产物)。
   - **风险**:head/tail 切点错会丢精细引导段(HTTPS sed / ca-cert 自举)。切完用 `cat Dockerfile.base.head Dockerfile.base.tail` 与 git 里原 `Dockerfile.base` diff 比对(应仅缺原中间空行)。
2. 建 `scenarios/rust/{scenario.toml,fragment.Dockerfile}`、`scenarios/python-dev/{scenario.toml,fragment.Dockerfile}`(内容见 design §3)。
3. 建 `.aio/enabled.toml.example`(`scenarios = []`)、`.gitignore` 加 `.aio/enabled.toml`。

## Phase 2:config crate 骨架(gen 先行)

4. `config/Cargo.toml`:bin name `aio-config`;deps `clap`(derive)、`serde`/`toml`、`anyhow`、`ratatui`、`crossterm`(后端)。
5. `config/src/main.rs`:clap `enum Cmd { Tui{..}, Gen{..} }` 分发。
6. `config/src/scenario.rs`:`scan(dir) -> Vec<Scenario{dir,id,name,description}>`,扫 `scenarios/*/scenario.toml`,校验 `id == 目录名`。
7. `config/src/manifest.rs`:`Enabled{scenarios:Vec<String>}` 的 load/save(`.aio/enabled.toml`);缺失视作空。
8. `config/src/gen.rs`:读 enabled -> 解析每个 id 的 fragment 路径(校验存在)-> `head + Σ(字母序,带分隔注释) fragments + tail` 写 `Dockerfile.base`。未知 id 报错退出。
   - 验证(空选择):`cargo run -p aio-config -- gen --repo .` 后,`diff <(cat Dockerfile.base.head Dockerfile.base.tail) Dockerfile.base` 应无差异(仅分隔注释除外)。
   - 验证(rust):写 `.aio/enabled.toml` = `scenarios=["rust"]` -> `cargo run -- gen` -> `grep "sh.rustup.rs" Dockerfile.base` 命中、`grep "USER gem" Dockerfile.base` 在 rust 片段之后。

## Phase 3:config Dockerfile + Makefile

9. `config/Dockerfile`:多阶段,`FROM rust:1-bookworm AS build` -> `cargo build --release` -> `FROM debian:bookworm-slim` 拷 `aio-config`。镜像名 `aio-config`。
   - 验证:`docker build -t aio-config -f config/Dockerfile config/` 成功,`docker run --rm aio-config --help` 出子命令。
10. `Makefile` 加 `build-config`/`config`/`gen`(见 design §4);改 `build-base` 依赖 `gen`;`up` 加 `NOBUILD` 条件分支;`clean` 加 `docker rmi aio-config`。
    - 验证:`make build-config` ok;`make gen` 生成 Dockerfile.base;`make build-base` 成功构建 `sandbox-base`。

## Phase 4:TUI

11. `config/src/tui.rs`:ratatui 勾选列表。扫 `scenarios/*/scenario.toml` 渲染 `{name} — {description}` 复选框;空格切换;`s` 存 `.aio/enabled.toml` 退出;`q` 不存退出。读已有 enabled.toml 预勾选。
    - 验证:`make config` 起界面,勾 rust 存盘 -> `.aio/enabled.toml` 含 `rust`;再跑预勾 rust。

## Phase 5:端到端 AC 验证

12. **AC1**:`make config` 勾 rust -> `make up`。
    - `docker exec aio-app-1 bash -lc 'rustc --version && cargo --version'`(AIO 终端面板 login -- **关键**:login shell 重置 PATH,靠 /usr/local/bin 软链)
    - `docker exec aio-app-1 bash -ic 'rustc --version && cargo --version' 2>/dev/null`(非 login 模拟 code-server 终端)
    - code-server 面板内置终端 `rustc --version`(真非 login)。
    - **关键**:确认 `which cargo` 在 login 下指 `/usr/local/bin/cargo`(软链)、非 login 下指 `/opt/rust/cargo/bin/cargo`;均非 `~/.cargo`(证明未落卷遮盖区)。
13. **AC2**:勾 rust + python-dev -> `make up` -> 两路 `cargo --version` 与 `uv --version`。
14. **AC3**:勾选取消 rust(留 python-dev)-> `make build-base` + `make up --force-recreate` -> `docker exec aio-app-1 bash -lc 'rustc --version'` 应 command not found;`uv --version` 仍在。
15. **AC4**(离线):联网机 `make build` -> `docker save sandbox-base sandbox-app aio-config -o /tmp/aio.tar`(含 app 即可验 AC;code-server/vnc 按需)-> 另起离线机/另一环境 `docker load` -> `make up NOBUILD=1` -> AC1/AC2 复验通过(全程离线)。
16. **AC5**(扩展):加 `scenarios/go/{scenario.toml,fragment.Dockerfile}`(go 官方 tarball 装 `/usr/local/go`)-> `make config` 勾 go -> `make up` -> `go version`。确认未改 `config/src/gen.rs`/`tui.rs`/`scenario.rs`(`git diff config/src` 应空)。

## Phase 6:收尾

17. 一致性检查:`make gen` 后 `git diff Dockerfile.base` 应与提交版一致(若选提交生成产物);不一致则说明 gen 与提交漂移,修。
18. `.trellis/spec/` 更新:新增 `config/` crate 的目录结构约定(backend/directory-structure.md 或新 `build-tools/` 层);记录"场景工具落系统路径避卷遮盖"约定(step 3.3 spec update)。
19. 整理 `implement.jsonl`/`check.jsonl`(若走 sub-agent dispatch):放 `design.md`、`prd.md`、`.trellis/spec/backend/directory-structure.md`、`docs/offline-install-guide.md` 作 context。
20. 提交(中文 subject + 英文前缀,见 commit-guidelines):拆成逻辑 commit--`feat: 新增 aio-config TUI 与场景预置(rust/python-dev 示例)`、`refactor: Dockerfile.base 拆为 head/tail 由生成器装配`。

## 验证命令速查

```bash
# gen 幂等(空选择 == head+tail)
diff <(cat Dockerfile.base.head Dockerfile.base.tail) Dockerfile.base
# 装配含 rust
grep -c 'sh.rustup.rs' Dockerfile.base   # >=1
# 落点正确(系统路径,非 ~/.cargo)
docker exec aio-app-1 bash -lc 'which cargo'   # /opt/rust/cargo/bin/cargo
# 两路终端
docker exec aio-app-1 bash -lc  'rustc --version'   # login (AIO 终端面板)
docker exec aio-app-1 bash -ic 'rustc --version' 2>/dev/null  # 非 login (code-server 终端)
# 离线
make up NOBUILD=1
```

## 风险与回滚

- **§R1 Dockerfile.base 拆分错**:切点丢引导段 -> `docker build sandbox-base` 失败或镜像缺 ca-cert。回滚:`git checkout -- Dockerfile.base`,重切。
- **§R2 gen 写坏 Dockerfile.base**:有 git 提交版兜底;gen 幂等可重跑。若 gen 容器写权限失败,加 `--user $(id -u):$(id -g)`。
- **§R3 rust 落 `~/.cargo`(卷遮盖)**:AC1 `which cargo` 指向 `~/.cargo` 即失败。修 fragment 强制 `CARGO_HOME=/opt/rust/cargo` + `chown`。
- **§R4 Makefile `up` NOBUILD 分支语法错**:Makefile 条件 `ifdef`/`else`/`endif` 易错 -> `make -n up NOBUILD=1` dry-run 查命令链。
- **§R5 离线机误跑 `make up`(带 build)**:`build-base` 联网失败。文档/README 注明离线必须 `NOBUILD=1`。

## 起步前检查门

- [ ] prd.md 收敛通过(无重复事实、AC 可测、open questions 清空)。
- [ ] design.md + implement.md 存在(复杂任务必需)。
- [ ] jsonl(若 sub-agent dispatch)有真实条目,非 seed-only。
- [ ] 用户确认后再 `task.py start`。
