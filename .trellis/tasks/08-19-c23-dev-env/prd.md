# 新增 C23 开发环境场景(gcc + clang-22 工具链 + C 配套工具)

## Goal

在 AIO sandbox 中新增一个可选场景 `c23`,提供符合 C23 标准(ISO/IEC 9899:2024)
的 C 语言开发环境:gcc + clang 双工具链 + C 配套开发工具(调试/构建/静态分析),
装进 `sandbox-base` 镜像,使 WebUI 终端里能直接以 `-std=c23` 编写、编译、调试 C 程序。

## Requirements

- **场景 `c23`(category = `lang`,L3 语言工具链),可在 TUI 中按需开关**:
  目录 `scenarios/c23/`,`scenario.toml` 的 `id == "c23"`。
- **clang 侧(clang-22,来自官方 apt.llvm.org,完整 C23)**:
  - 安装 `clang-22` 及配套 `clang-format-22`、`clang-tidy-22`、`clangd-22`、`lld-22`。
  - 在 `/usr/local/bin` 提供无版本后缀软链(`clang`/`clang++`/`clang-format`/
    `clang-tidy`/`clangd`/`lld` → `*-22`),确保 **login shell**(终端面板是 `bash -l`)
    能找到。clang 作为主 C23 编译器。
  - 版本以 `ARG` 固定(可复现构建)。
- **gcc 侧(保留 bookworm 默认 gcc-12,不改动)**:
  - `build-essential` 已在 `Dockerfile.base.head` 安装,gcc-12/g++/make 本来就存在,
    不重复安装、不额外拉新源。
  - 文档说明:gcc-12 对 C23 是部分支持(用 `-std=c2x`),完整 C23 用 clang-22。
- **C 配套开发工具(合并进同一场景,随场景一起开关)**:
  - 调试:gdb、valgrind
  - 构建:cmake、ninja-build、ccache
  - 静态分析/辅助:cppcheck、strace
- **场景规则约束**(见 skill `scenario-authoring.md`):
  - 安装到系统路径(`/usr/local`、`/usr/local/bin`、`/opt`),**绝不装到 `/home/gem`**(被共享卷遮盖)。
  - 只用 HTTPS URL(apt.llvm.org 已验证可达,head 已强制 apt HTTPS)。
  - fragment 内 `apt-get install` 必须先 `apt-get update`(head 末尾清空了 apt lists)。
  - 版本固定可复现;`gen` 生成的 `Dockerfile.base` 不得残留 `{{` 占位符。
- **不引入 L5 服务、不新增 compose 服务、不改 Caddyfile/前端/按钮**——纯场景。

## Acceptance Criteria

- [x] `scenarios/c23/scenario.toml`(`id == "c23"`)与 `scenarios/c23/fragment.Dockerfile` 存在,
       `id` 与目录名一致。(已验证 + trellis-check 复核)
- [x] `make build-base` 成功(gen 组装 + docker build 通过,EXIT=0),生成的 `Dockerfile.base`
       无 `{{` 残留。(2026-08-19 实跑验证)
- [x] 容器内 **login shell** 验证通过(aio-app-1,2026-08-19 实跑):
      - `clang --version` → **Debian clang version 22.1.8**
      - 16 个工具 `command -v` 全部命中(clang 工具链 → /usr/local/bin,apt 工具 → /usr/bin)
      - `gcc --version` → 12.2.0(gcc-12 未被动过)
- [x] **C23 冒烟编译**(2026-08-19 实跑):
      - 构建期 + 容器内 `clang -std=c23` 均通过 → `ok=1 b=10 n=1000`
        (`bool`/`true`、`typeof`、`0b1010`、`1'000`)
      - `gcc -std=c2x` 二进制字面量子集通过 → `b=10`;实测 gcc-12 strict c2x **仅**支持
        二进制字面量(typeof/constexpr/nullptr 均不支持,详见 research 笔记);完整 C23 用 clang-22
      - 附加:ASan(`-fsanitize=address`)经 libclang-rt-22-dev 实测可用;code-server 容器
        同样已换新镜像(clang 22.1.8 可用)
- [x] 场景在 `.aio/enabled.toml` 中被勾选(tick),`make config` 后确认存在。

## Notes

- 诚实限制(非本任务范围):C23 标准库新增(如 `<stdbit.h>`)需要 glibc ≥ 2.39,
  bookworm 是 2.36。编译器层面的 C23 语言特性(clang-22)完整可用;库层面完整
  C23 需整体换 trixie 基座,作为独立后续任务,不在本任务实现。
- 用户已决策:保留 gcc-12 不源码编译;clang 选 clang-22(apt.llvm.org);
  配套工具合并进同一 `c23` 场景(不做独立 `c-devtools` 场景)。
- `llvm-toolchain-bookworm-22` 仓库 clang-22 版本经实测为 `22.1.8`(2026-07 快照)。
