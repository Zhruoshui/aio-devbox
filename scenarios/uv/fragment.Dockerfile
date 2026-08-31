# >>> scenario: uv >>>
# L3 可选:uv(astral-sh 的 Python 版本/包管理器)。单二进制装 /usr/local/bin
# (系统路径,躲过共享卷 aio_workspace 对 /root 的遮盖;在 PATH 上)。
# 运行时 `uv python install <ver>` 装别的 CPython 到 ~/.local/share/uv/python
# (卷上,抗 recreate);`uv venv`/`uv pip` 管理环境/包。与 L1 python 场景互补:
# L1 提供默认 CPython(版本可选),uv 管额外版本/venv。
ARG UV_VERSION=0.5.11
RUN curl -fsSL "https://github.com/astral-sh/uv/releases/download/${UV_VERSION}/uv-x86_64-unknown-linux-gnu.tar.gz" \
        -o /tmp/uv.tar.gz \
 && tar -xzf /tmp/uv.tar.gz -C /tmp \
 && install -m 0755 /tmp/uv-x86_64-unknown-linux-gnu/uv /usr/local/bin/uv \
 && rm -rf /tmp/uv* \
 && uv --version
# <<< scenario: uv <<<
