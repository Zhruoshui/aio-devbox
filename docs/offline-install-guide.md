# AIO 沙箱离线安装指南

> 本文件是**通用操作手册**:在内网离线机已部署 AIO 沙箱后,如何给运行中的系统补装任意工具/包,**不重建、不联网**。
> 阅读顺序:先看 §1–§3 的原理与跨切规则(理解后可外推到任何类型),需要现抄命令时查 §4 的参考配方。各方法的实测记录见 `docs/offline-tool-install.md`。

---

## 第一部分:原理与方法论

## 0. 前提与术语

| 术语 | 含义 |
|---|---|
| **联网机**(online) | 有外网的工作机,负责所有下载/编译/vendor/打包。 |
| **离线机**(offline) | 已部署的 AIO 沙箱(caddy + axum app + code-server + vnc),**全程不联网**。 |
| **stack** | 离线机上运行的 4 容器(gateway/app/code-server/vnc)。 |
| **共享卷** | 命名卷 `aio_workspace`,挂载在 app/code-server/vnc 的 `/home/gem`(uid 1000 gem)。 |

**铁律**:
1. 所有联网工作在**联网机**做;离线 stack **不碰网络**。
2. 工具默认装共享卷 `~/.local/bin`(跨容器可见、抗 recreate、login/非 login 终端自动上 PATH,见 §3.2)。
3. 临时测试装的东西验证后清理;持久需求固化进镜像(见 §3.4)。
4. 制品要匹配离线机的**架构(x86_64)/ glibc(bookworm 2.36)/ 语言 ABI(cp311 等)**。

## 1. 通用方法论

### 1.1 三原语

任何离线补装,不管装什么,都归约成三步:

```
① 联网机准备   →   ② 传到离线机   →   ③ 离线机安装
```

- **① 联网机准备**:把目标变成**自包含制品**(单二进制 / 预编译 wheel / 完整 tarball / vendor 包)。能拿预编译就别现场编译;必须编译的,在联网机(与离线机同基)编译好再打包。
- **② 传到离线机**:同 docker daemon 用 `docker cp`;真气隙用 U 盘/scp 到离线机 host 再 `docker cp`。
- **③ 离线机安装**:落到共享卷(首选)或对应位;`chown 1000:1000` + `chmod +x`;再 login + 非 login 两路验证;临时则清理。

这三步是**骨架**,§4 的每类配方只是把这三步具体化。遇到没见过的新类型,先想清这三步各自怎么做,再对号入座。

### 1.2 两个安装目标(决定装哪、能不能持久)

| 目标 | 位置 | 抗 recreate | 何时用 |
|---|---|---|---|
| **共享卷** | `~/.local/bin` 等(`/home/gem`) | ✅ | 自包含工具、脚本、venv、用户级运行时 —— **默认** |
| **镜像** | `Dockerfile.base` 烘进层,`docker save/load` 重建 | ✅(随镜像) | 持久系统服务、apt 服务、基础环境固化 |
| 容器可写层 | `/usr`、`/etc` | ❌(recreate 即丢) | 只用于临时验证,不依赖其持久 |

> 经验:装在容器可写层的东西,**容器一 recreate 就没了**。要持久要么进共享卷(自包含工具),要么进镜像(系统服务)。

### 1.3 两个判别维度(决定方法)

装一个东西前,先回答两个问题:

1. **需不需要运行时?**(它自己能不能跑,还是要 node/python/jvm 在场)
   - 不需要(静态/单二进制)→ 最简单,直接 `~/.local/bin`。
   - 需要 → 确认运行时在不在 base(node/python 在 sandbox-base;jvm **不在**,要一起 stage)。
2. **需不需要现场编译?**(拿到的是源码还是预编译)
   - 预编译(二进制/wheel/预编译 npm 产物)→ 离线机直接用。
   - 源码 → 联网机把**源码 + 依赖 + 工具链**一起打包,离线机用对应工具链离线编译(Rust 用 `cargo vendor`;C/C++ 要 stage 依赖与 build-essential,base 已有)。

这两个维度的组合,加上「是不是系统包」,就能把任何工具归到 §4 的某一类。

## 2. 按工具属性归类(决策框架)

**按属性归,不按「测没测过」枚举**。逐项判断:

```
要装 X。先判属性:
│
├─ 单个可执行 + 无外部依赖(静态 musl / glibc-only)?
│    → 方法 A(~/.local/bin)
│
├─ 需运行时才跑得动?
│    ├─ 运行时 = node,且 X 是 npm 包        → 方法 B
│    ├─ 运行时 = python                     → 方法 F(uv/pip)
│    ├─ 运行时 = jvm,且 base 无 jvm         → 把 jvm 当运行时一起 stage(类方法 A/G 组合)
│    └─ 运行时已在 base(node/python 有)    → 产物按方法 A/G 放 bin
│
├─ 拿到的是源码,需现场编译?
│    ├─ Rust crate                          → 方法 D(cargo vendor + 工具链)
│    ├─ C/C++ 源 + 依赖                     → 联网机编译好再 tar(类方法 D 思路,build-essential 已有)
│    └─ install 时编译的 native 包(npm 等) → 联网机正常装(带 scripts)再 tar,别用 --ignore-scripts
│
├─ 是系统包(deb)?
│    → 方法 C(apt --download-only + dpkg -i;持久要进镜像)
│
└─ 单文件脚本 / 数据 / 资源?
     → 方法 G(docker cp 到 ~/.local/bin 或项目目录)
```

**遇到没见过的新类型怎么办**:照上面逐项判断属性,落到最接近的方法;**无法自包含的,把「运行时」和「产物」分开 stage**(运行时进 `~/.local/bin` 或 base,产物进项目目录)。框架本身不依赖你测过没测过 —— §4 的配方只是把框架对 7 个常见类型的实例化。

## 3. 跨切规则(每次都适用,集中在此)

### 3.1 架构 / ABI / glibc 匹配

所有制品钉死在离线机的**架构(x86_64)+ glibc(bookworm 2.36)+ 语言 ABI(cp311 等)**。换 arm64、升级 base、换 python 版本 → 制品可能不兼容,要在联网机**按新目标重新准备**。native 编译产物(wheel / `.so` / 预编译二进制)尤其敏感。

### 3.2 终端 PATH(login + 非 login 都要覆盖)

离线机终端有两种 shell,PATH 来源不同,工具要两者都见:

| 终端 | shell | PATH 来源 | `~/.local/bin` 可见? |
|---|---|---|---|
| AIO 工作区终端面板(pty) | `/bin/bash -l`(login) | `~/.profile` | ✅(Debian 默认加) |
| code-server 内置终端 | 非 login 交互 bash | `~/.bashrc` | ✅(**需 §3.2.1 兜底**) |
| 非 login 非交互(`bash -c`) | 无 dotfile | — | ❌(除非进 ENV) |

**3.2.1 必做:`~/.bashrc` 兜底** —— Debian 默认只有 `~/.profile` 加 `~/.local/bin`,`~/.bashrc` 不加 → code-server 终端看不到共享卷工具。在 `~/.bashrc` 追加(共享卷 → 三容器即时生效、抗 recreate/down-up):

```bash
# >>> AIO_LOCAL_BIN_BLOCK >>>
case ":$PATH:" in
  *":$HOME/.local/bin:"*) ;;
  *) export PATH="$HOME/.local/bin:$PATH" ;;
esac
# <<< AIO_LOCAL_BIN_BLOCK <<<
```

**3.2.2 固化(下次重建 base)**:`Dockerfile.base` 加 `ENV PATH=/home/gem/.local/bin:$PATH`,连非交互 shell 都覆盖;固化后 3.2.1 的 block 可摘除。

**验证两路都过**:
```bash
docker exec aio-app-1 bash -lc  '<tool> --version'   # login(AIO 终端面板)
docker exec aio-app-1 bash -ic '<tool> --version' 2>/dev/null   # 非 login(code-server 终端)
```

### 3.3 root 属主清理

`docker cp` / bind mount 产出的文件常是 **root 属主**,gem 删不掉。清理要用 `docker exec -u root rm` + 宿主 `sudo rm`。一次性容器尽量 `--user $(id -u):$(id -g)` 避免产出 root 属主文件。

### 3.4 持久性与容器生命周期

| 操作 | 共享卷 `aio_workspace` | `~/.local/bin` 工具 |
|---|---|---|
| `up --force-recreate` | 不动 | ✅ 仍在 |
| `down`(不带 `-v`)→ `up` | 不动 | ✅ 仍在 |
| `down -v` | **删除** | ❌ 没了 |

共享卷工具扛住 recreate / down-up;只有 `down -v` 毁卷。**容器可写层(apt)不在此列** —— recreate 即丢,要持久进镜像。

### 3.5 供应链 / 签名

离线补装绕过正常验签(apt `dpkg -i` 不验来源、cargo vendor 信任联网机内容、rust/uv 制品只验 sha256)→ **联网机是受信暂存点**,它被污染会污染所有下游。气隙场景要把联网机本身管好。

---

## 第二部分:参考配方(7 类,均已实测)

> 下面每类 = §1 三原语的具体化。命令可直接照抄,把 `<ver>`/`<pkg>`/`<repo>` 换成你的值。

## 4.1 方法 A:自包含二进制(rg / fd / uv / 静态工具)

**属性**:单个可执行 + 无外部依赖(或仅 glibc)。**装哪**:共享卷 `~/.local/bin`。

**① 联网机**:
```bash
curl -L -o rg.tgz https://github.com/BurntSushi/ripgrep/releases/download/<ver>/ripgrep-<ver>-x86_64-unknown-linux-musl.tar.gz
tar -xzf rg.tgz
BIN=$(find . -name rg -type f)
file "$BIN"; ldd "$BIN" 2>/dev/null || echo "static(无依赖)"
```

**② 传输 + ③ 离线机安装**:
```bash
docker exec aio-app-1 mkdir -p /home/gem/.local/bin
docker cp "$BIN" aio-app-1:/home/gem/.local/bin/rg
docker exec -u root aio-app-1 chown 1000:1000 /home/gem/.local/bin/rg
docker exec -u root aio-app-1 chmod +x /home/gem/.local/bin/rg
```

**验证**:见 §3.2.2(login + 非 login 两路)。

**坑**:musl 静态最省事;glibc-only 二进制确认离线机 glibc ≥ 要求(bookworm 2.36 够新)。arm64 换对应架构包。

## 4.2 方法 B:npm 全局包

**属性**:需 node 运行时(sandbox-base 已有 node 20),X 是 npm 包。**装哪**:共享卷 `~/.local/{bin,lib}`。

**① 联网机**(装到临时 prefix 再 tar):
```bash
PREFIX=/tmp/pkg
# 纯 JS / 预编译产物包:可 --ignore-scripts
npm install -g --ignore-scripts --prefix "$PREFIX" <pkg>
# install 时编译的 native 包(如 better-sqlite3):别用 --ignore-scripts,正常装再 tar
#   npm install -g --prefix "$PREFIX" <pkg>
tar -czf pkg.tar.gz -C "$PREFIX" bin lib
```

**② 传输 + ③ 离线机安装**:
```bash
docker cp pkg.tar.gz aio-app-1:/tmp/
docker exec aio-app-1 tar -xzf /tmp/pkg.tar.gz -C /home/gem/.local   # bin→~/.local/bin, lib→~/.local/lib/node_modules
docker exec aio-app-1 bash -lc '<pkg> --version'
```
> 全局 bin 是 `#!/usr/bin/env node`,靠 base 的 node(PATH)运行。

**坑**:`--ignore-scripts` **只对纯 JS / 预编译产物包安全**;native 编译包用它会让 `.node` 缺失、运行时报错 → native 包在联网机**正常装(带 scripts)**再 tar 已构建的 `lib/node_modules`。包若要求更新/更老 node,要换 base 的 node。

## 4.3 方法 C:apt deb 包

**属性**:系统包(deb),有依赖树。**装哪**:容器可写层(**易失**)→ 持久要进镜像。

**① 联网机**(与离线机**同 debian suite**,如都 bookworm):
```bash
docker run --rm -v "$PWD/debs":/out debian:bookworm sh -c '
  apt-get update
  apt-get install --download-only -y -o Dir::Cache=/out <pkg>
  cp /var/cache/apt/archives/*.deb /out/'
tar -czf debs.tar.gz -C debs .
```

**② 传输 + ③ 离线机安装**:
```bash
docker cp debs.tar.gz aio-app-1:/tmp/
docker exec aio-app-1 sh -c 'mkdir -p /tmp/debs && tar -xzf /tmp/debs.tar.gz -C /tmp/debs'
docker exec -u root -e DEBIAN_FRONTEND=noninteractive aio-app-1 dpkg -i /tmp/debs/*.deb
```

**坑**(apt 是离线最麻烦的一类):
- **必须同 debian suite + 最好同已装状态**,否则 deb 版本与离线机已装的冲突 → `dpkg -i` 报版本错。
- **容器无 systemd**:apt 装的服务不能 `systemctl start`,要**手动启二进制**或塞进 entrypoint/supervisor。
- **conffiles** 弹提问 → `DEBIAN_FRONTEND=noninteractive`。
- **持久性**:容器可写层,recreate 即丢;持久 → 进镜像(`docker save`/离线 `docker build` + 重建)。

## 4.4 方法 D:cargo crate(离线机从源码编译)

**属性**:Rust 源码 + 需现场编译。**装哪**:产物进共享卷 `~/.local/bin`,工具链进 `~/.rust`(共享卷)。

**① 联网机**:
```bash
# 1) rust standalone 工具链(带 install.sh,不是 rustup)
curl -L -o rust.tar.xz https://static.rust-lang.org/dist/<date>/rust-<ver>-x86_64-unknown-linux-gnu.tar.xz
# 2) crate 源码(含 Cargo.lock)
curl -L -o src.tar.gz https://github.com/<org>/<repo>/archive/refs/tags/<ver>.tar.gz
# 3) vendor 依赖(rust:1-bookworm 自带 cargo)
docker run --rm -v "$PWD":/work rust:1-bookworm bash -c '
  cd /work && tar -xzf src.tar.gz && cd <repo>-<ver>
  mkdir -p .cargo
  cargo vendor --versioned-dirs > .cargo/config.toml   # ⚠️ 别加 --quiet,会把 config(stdout)吞成 0 字节
  cd .. && tar -cJf vendored.tar.xz <repo>-<ver>'
```

**② 传输 + ③ 离线机安装**:
```bash
docker cp rust.tar.xz    aio-app-1:/tmp/
docker cp vendored.tar.xz aio-app-1:/tmp/
docker exec aio-app-1 bash -lc '
  cd /tmp && mkdir rt && cd rt && tar -xJf /tmp/rust.tar.xz
  cd rust-*/ && ./install.sh --prefix=/home/gem/.rust --without=rust-docs
  export PATH=/home/gem/.rust/bin:$PATH
  cd /tmp && tar -xJf /tmp/vendored.tar.xz && cd <repo>-<ver>
  cargo build --offline --release
  cp target/release/<bin> /home/gem/.local/bin/'
```

**坑**:`cargo vendor --quiet` 吞 config 成 0 字节 → 离线构建联网失败,**不加 `--quiet`**。工具链用 standalone `install.sh`(离线),不是 `rustup`(见方法 E)。

## 4.5 方法 E:rust 工具链(rustup 管理)

**属性**:需要 rustup 作为工具链管理器(对照方法 D 的 install.sh)。**装哪**:`~/.cargo`+`~/.rustup`+`~/.rust-toolchain`(共享卷)。

**① 联网机**:
```bash
docker run --rm rust:1-bookworm cat /usr/local/cargo/bin/rustup > rustup-init    # rustup 二进制(本地无下载)
curl -L -o rust.tar.xz https://static.rust-lang.org/dist/<date>/rust-<ver>-x86_64-unknown-linux-gnu.tar.xz
```

**② 传输 + ③ 离线机安装**(rustup 1.29 真·频道装未通,用 `toolchain link`):
```bash
docker cp rustup-init aio-app-1:/home/gem/.local/bin/rustup-init
docker cp rust.tar.xz aio-app-1:/tmp/
docker exec aio-app-1 bash -lc '
  cd /tmp && mkdir rt && cd rt && tar -xJf /tmp/rust.tar.xz
  cd rust-*/ && ./install.sh --prefix=/home/gem/.rust-toolchain --without=rust-docs
  export RUSTUP_HOME=/home/gem/.rustup CARGO_HOME=/home/gem/.cargo
  mkdir -p /home/gem/.cargo/bin && cp /home/gem/.local/bin/rustup-init /home/gem/.cargo/bin/rustup
  /home/gem/.cargo/bin/rustup toolchain link mytool /home/gem/.rust-toolchain
  /home/gem/.cargo/bin/rustup default mytool
  rustc --version; cargo --version; rustup show'
```

**坑**:rustup 1.29 的 `file://` 频道装(`RUSTUP_DIST_SERVER=file://... + rustup-init --default-toolchain stable`)报 `no release found for 'stable'`,manifest/组件/无验签都对也不行 → 用 `toolchain link` 替代(rustup 官方的自定义工具链接口)。`which rustc` 应指向 `~/.cargo/bin/rustc`(rustup 代理 shim)。

## 4.6 方法 F:python + uv

**属性**:需 python 运行时(sandbox-base 已有 python3/pip/venv),用 uv 管 venv。**装哪**:uv 进 `~/.local/bin`,venv 进项目目录 `~/<proj>/.venv`。

**① 联网机**:
```bash
# 1) uv 二进制
curl -L -o uv.tgz https://github.com/astral-sh/uv/releases/latest/download/uv-x86_64-unknown-linux-gnu.tar.gz
tar -xzf uv.tgz
# 2) wheelhouse:用与离线目标同基的 python 容器建,保证 ABI 匹配
#    base python 现在是 L1 可选(默认 3.12.7;可选 3.11/3.12/3.13),按所选版本选镜像
docker run --rm -v "$PWD/wheels":/out python:3.12-slim-bookworm sh -c '
  pip download -d /out --only-binary :all: --no-cache-dir <pkg1> <pkg2> ...'
```

**② 传输 + ③ 离线机安装**:
```bash
docker cp uv aio-app-1:/home/gem/.local/bin/uv
docker exec -u root aio-app-1 chown 1000:1000 /home/gem/.local/bin/uv
docker cp wheels aio-app-1:/tmp/wheels
docker exec aio-app-1 bash -lc '
  export PATH=/home/gem/.local/bin:$PATH
  uv venv --python /usr/bin/python3 /home/gem/<proj>/.venv        # 系统 python,无网络;别用 --seed
  uv pip install --python /home/gem/<proj>/.venv/bin/python \
      --no-index --find-links /tmp/wheels --offline <pkg1> <pkg2> ...'
```

**坑**:native wheel 要匹配 ABI(cp312 + manylinux + glibc;按所选 base python 版本,L1 默认 3.12.7,可选 3.11/3.12/3.13),用**同镜像基** python 容器建 wheelhouse(`python:3.12-slim-bookworm` 对 sandbox-base 默认 3.12.7)最稳;`--only-binary :all:` 强制预编译 wheel(离线免编译)。`uv venv` 默认不 seed pip(无网络),**别加 `--seed`**;建 venv 要 `--python /usr/bin/python3` 显式指定系统解释器。wheelhouse root 属主,清理用 `-u root`/`sudo`(§3.3)。

## 4.7 方法 G:单文件脚本 / 数据资源

**属性**:shell/python 脚本、配置、数据集。**装哪**:`~/.local/bin`(可执行)或项目目录(资源)。

```bash
docker cp myscript.sh aio-app-1:/home/gem/.local/bin/myscript
docker exec -u root aio-app-1 chown 1000:1000 /home/gem/.local/bin/myscript
docker exec -u root aio-app-1 chmod +x /home/gem/.local/bin/myscript
docker cp data.csv aio-app-1:/home/gem/<proj>/data.csv
```
脚本 `#!/bin/sh` / `#!/usr/bin/env python3`,sh/python 在 base PATH,可直接跑。

---

## 第三部分:收尾

## 5. 部署前 checklist

- [ ] 联网机与离线机**同架构**(x86_64)、**同 debian suite**(bookworm)、语言运行时**同 ABI**(如 cp311)。
- [ ] 制品**自包含**(单二进制 / 预编译 wheel / 完整 tarball),离线机不依赖联网拉取。
- [ ] 传输通道就绪(同 daemon `docker cp`,或气隙 U 盘 + host `docker cp`)。
- [ ] 目标安装位选对(自包含 → `~/.local/bin`;apt 持久 → 镜像)。
- [ ] `~/.bashrc` 的 `AIO_LOCAL_BIN_BLOCK` 在(非 login 终端才能见工具,§3.2.1)。
- [ ] 验证脚本准备好(login `bash -lc` + 非 login `bash -ic` 两路)。
- [ ] 清理脚本准备好(含 root 属主 `-u root` / `sudo`,§3.3)。
- [ ] apt 服务:确认有手动启二进制方案(无 systemd)。
- [ ] 要持久:走镜像固化(`docker save/load` + 重建),不是容器层。

## 6. 通用坑速查

1. `docker cp`/bind mount 文件常 root 属主 → `-u root`/`sudo` 清(§3.3)。
2. 架构/glibc/ABI 不匹配 → 制品不兼容,按新目标在联网机重准备(§3.1)。
3. 能拿预编译就别现场编译;必须编译的,联网机(同基)编译好再 tar(§1.3)。
4. apt:同 suite + 同已装状态;容器无 systemd,服务手启;conffiles 用 noninteractive;持久进镜像(§4.3)。
5. rustup 1.29 真·频道装未通,用 `toolchain link`(§4.5)。
6. `cargo vendor --quiet` 吞 config,**别加**(§4.4)。
7. code-server 非 login 终端不见 `~/.local/bin` → `~/.bashrc` 兜底(§3.2.1)。
8. `down -v` 毁卷,装了工具后别误用(§3.4)。

## 附:快速对应表

| 要装 | 方法 | 联网机关键 | 离线机关键 |
|---|---|---|---|
| 静态/单二进制 | A | `curl`+`tar` | `docker cp`→`~/.local/bin` |
| npm 全局包 | B | `npm i -g --prefix`+`tar bin lib` | `tar -x -C ~/.local` |
| apt deb | C | `apt-get install --download-only`+`tar` | `dpkg -i`(易失)/进镜像 |
| cargo crate | D | `cargo vendor`+下 rust install.sh | `install.sh`+`cargo build --offline` |
| rust 工具链 | E | 取 rustup 二进制+rust host bundle | `rustup toolchain link`+`default` |
| python + uv | F | 下 uv+`pip download` 建 wheelhouse | `uv venv`+`uv pip install --no-index --find-links --offline` |
| 脚本/资源 | G | — | `docker cp`→`~/.local/bin`/项目目录 |
