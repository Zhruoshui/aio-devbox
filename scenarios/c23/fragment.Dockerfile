# >>> scenario: c23 >>>
# C23 开发环境:clang-22 主工具链(完整 C23)+ gcc-12(复用 build-essential)+ C 配套工具。
#
# 为什么这样搭:
#   - bookworm 的 apt 只有 gcc-12(C23 部分支持,-std=c2x);backports 无 gcc-13/14,
#     跨源(如 trixie)拉新 gcc 会连带升级 libc6,搞崩整个镜像。故完整 C23 由 clang
#     承担:clang-22 来自官方 apt.llvm.org 的 bookworm 专属源(依赖 libc6>=2.34,
#     与 bookworm 的 2.36 兼容)。gcc-12 已在 head 的 build-essential 中,这里不重复装。
#   - 所有工具装系统路径(/usr/bin、/usr/local/bin),不被共享卷 aio_workspace 遮盖;
#     无版本后缀命令(clang/clang++/clang-format/clang-tidy/clangd/lld)软链到
#     /usr/local/bin,保证 login shell(bash -l,WebUI 终端面板)可见。
#   - 只使用 HTTPS(apt.llvm.org 已验证网络可达,head 已强制 apt HTTPS)。
#   - 版本经 ARG(LLVM_REPO)固定可复现;构建期冒烟用 clang -std=c23 编译运行 C23
#     程序,失败即尽早暴露。
# apt lists 在 head 末尾被 `rm -rf /var/lib/apt/lists/*` 清空,这里需重新 update。

ARG LLVM_REPO=llvm-toolchain-bookworm-22
RUN install -d -m 0755 /etc/apt/keyrings \
 && curl -fsSL https://apt.llvm.org/llvm-snapshot.gpg.key | gpg --dearmor -o /etc/apt/keyrings/llvm-snapshot.gpg \
 && echo "deb [signed-by=/etc/apt/keyrings/llvm-snapshot.gpg] https://apt.llvm.org/bookworm/ ${LLVM_REPO} main" \
      > /etc/apt/sources.list.d/llvm-toolchain.list \
 && apt-get update \
 && apt-get install -y --no-install-recommends \
        clang-22 clang-format-22 clang-tidy-22 clangd-22 lld-22 libclang-rt-22-dev \
        gdb cmake ninja-build ccache valgrind cppcheck strace \
 && rm -rf /var/lib/apt/lists/* \
# 软链无版本后缀命令到 /usr/local/bin(全 shell PATH 通用,login/非 login 都可见;
# 同 rust/go 场景的做法)。clang-22 是主 C23 编译器,gcc-12 保持默认不动。
 && for b in clang clang++ clang-format clang-tidy clangd lld; do \
      ln -sf "/usr/bin/${b}-22" "/usr/local/bin/${b}"; done \
 && clang --version \
 && gcc --version && gdb --version | head -n1 \
 && cmake --version | head -n1 && ninja --version \
 && ccache --version | head -n1 && valgrind --version \
 && cppcheck --version && strace -V | head -n1

# 构建期冒烟:用 clang -std=c23 编译运行一个用到 C23 特性的最小程序
# (bool/true 关键字、typeof、二进制字面量、数字分隔符),提前暴露工具链问题。
RUN cat > /tmp/c23_smoke.c <<'EOF'
#include <stdbool.h>
#include <stdio.h>
int main(void) {
    bool ok = true;          /* C23: bool/true(关键字;stdbool.h 提供兼容) */
    typeof(ok) t = ok;       /* C23: typeof */
    int b = 0b1010;          /* C23: 二进制字面量 */
    unsigned long n = 1'000; /* C23: 数字分隔符 */
    printf("ok=%d b=%d n=%lu\n", ok && t, b, n);
    return 0;
}
EOF
RUN clang -std=c23 -Wall -Wextra /tmp/c23_smoke.c -o /tmp/c23_smoke \
 && /tmp/c23_smoke \
 && rm -f /tmp/c23_smoke.c /tmp/c23_smoke
# <<< scenario: c23 <<<
