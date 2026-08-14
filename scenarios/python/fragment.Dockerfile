# >>> scenario: python >>>
# L1 always_on:CPython。python-build-standalone 预编译 tarball,解压到 /usr/local
# (系统路径,躲过共享卷;python3/pip3 在 PATH 上)。版本由 TUI 下拉选,gen 注入
# {{version}}+{{tag}}。
#
# 风险点(实现期已标记):python-build-standalone 的 release tag 与 python 版本
# 耦合,asset 命名 cpython-<ver>+<tag>-x86_64-unknown-linux-gnu-install_only.tar.gz。
# scenario.toml 每个 [[versions]] 带 version+tag,须与上游 release 对齐;tag 错则
# 404。仓库已从 indygreg 迁到 astral-sh(301)。见
# https://github.com/astral-sh/python-build-standalone/releases。
ARG PY_VERSION={{version}}
ARG PS_TAG={{tag}}
RUN curl -fsSL "https://github.com/astral-sh/python-build-standalone/releases/download/${PS_TAG}/cpython-${PY_VERSION}+${PS_TAG}-x86_64-unknown-linux-gnu-install_only.tar.gz" \
        -o /tmp/py.tgz \
 && mkdir -p /tmp/py \
 && tar -xzf /tmp/py.tgz -C /tmp/py \
 && cp -a /tmp/py/python/. /usr/local/ \
 && rm -rf /tmp/py /tmp/py.tgz \
 && python3 --version && pip3 --version
# <<< scenario: python <<<
