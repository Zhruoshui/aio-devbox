# C23 工具链在 Debian bookworm 的可用性(2026-08 实测)

调研结论,供后续「在 AIO sandbox 加编译/语言环境」类任务直接复用。所有数据来自
直接查询 Debian / apt.llvm.org 的 Packages 索引与网络可达性测试。

## 核心事实

| 需求 | bookworm 现状 | 结论 |
|---|---|---|
| gcc C23 | 主仓库只有 gcc-12(12.2.0);backports **没有** gcc-13/14(已 grep Packages 确认) | **apt 拿不到完整 C23 的 gcc** |
| 跨源拉新 gcc | 从 trixie/sid 拉 gcc-14 会连带升级 libc6(2.36→2.41),搞崩镜像 | 不可行 |
| clang C23 | **bookworm main 里有 clang-19.1.7**(`1:19.1.7-3~deb12u1`,Debian 回移植进主源) | 零额外源即可拿到近完整 C23 |
| clang 最新 | apt.llvm.org `llvm-toolchain-bookworm-21` / `-22` 仓库,clang-22 = 22.1.8 | 需加 apt 源,网络已验证可达(HTTP 200) |
| C 配套工具 | gdb 13.1、cmake 3.25.1、ninja 1.11.1、ccache 4.7.5、valgrind 3.19、cppcheck 2.10、strace 6.1 | 全在 bookworm 主仓库 |

## 关键 URL / 源

- apt.llvm.org 源行:
  `deb [signed-by=/etc/apt/keyrings/llvm-snapshot.gpg] https://apt.llvm.org/bookworm/ llvm-toolchain-bookworm-22 main`
- GPG 密钥:`https://apt.llvm.org/llvm-snapshot.gpg.key`(ASCII PGP,3167B,需 `gpg --dearmor`)
- clang-22 依赖:`libc6 (>= 2.34)`(bookworm 2.36 满足)、`libstdc++6 (>= 11)`、`libstdc++-12-dev libgcc-12-dev` 等(bookworm 都有)
- clang-22 的版本串带 `~exp1~<日期>` 后缀(apt.llvm.org 的构建风格),属正常

## 配套包(apt.llvm.org -22 仓库内,均 22.1.8)

- `clang-22`(内含 `/usr/bin/clang-22` + `/usr/bin/clang++-22` 软链)
- `clang-format-22`、`clang-tidy-22`、`clangd-22`、`lld-22`
- `libclang-rt-22-dev`(提供 ASan/UBSan 运行时;依赖 `libc6-i386`、`lib32stdc++6`,均无需 dpkg --add-architecture)

## C23 语言层 vs 标准库层

- **编译器层面**(语言特性):clang-19/22 基本完整;gcc-14+ 才是正式 `-std=c23`
  (gcc-12 只有 `-std=c2x` 部分支持,`bool`/`true` 不是关键字,需 `#include <stdbool.h>`)。
- **标准库层面**:C23 新增(如 `<stdbit.h>` 的部分函数)需要 **glibc ≥ 2.39**;
  bookworm 是 2.36 → 语言特性完整,库层面不完整。库层面完整只能等整体换
  trixie 基座(独立任务)。

## 验证过的 C23 冒烟源(clang -std=c23)

```c
#include <stdbool.h>
#include <stdio.h>
int main(void) {
    bool ok = true;          /* C23: bool/true */
    typeof(ok) t = ok;       /* C23: typeof */
    int b = 0b1010;          /* C23: 二进制字面量 */
    unsigned long n = 1'000; /* C23: 数字分隔符 */
    printf("ok=%d b=%d n=%lu\n", ok && t, b, n);
    return 0;
}
```
gcc-12 只能编子集,`bool` 关键字需 stdbool.h 宏路径。

## 实测补充(gcc-12 `-std=c2x` 严格模式特性探测,2026-08-19 容器内)

| C23 特性 | gcc-12 -std=c2x | clang-22 -std=c23 |
|---|---|---|
| 二进制字面量 `0b1010` | ✅ | ✅ |
| `typeof` | ❌(strict 模式禁用 GNU 扩展) | ✅ |
| `constexpr` | ❌ | ✅ |
| `nullptr` | ❌ | ✅ |
| `static_assert`(块内) | ❌ | ✅ |
| `bool`/`true` 关键字 | ❌(需 `<stdbool.h>` 宏) | ✅ |

→ gcc-12 严格 `-std=c2x` 基本只支持二进制字面量这类早期 C23 项;要更全的 C23 特性:
用 `-std=gnu2x`(走 GNU 扩展)或直接用 clang-22。ASan(`-fsanitize=address`)经
libclang-rt-22-dev 实测可用。
