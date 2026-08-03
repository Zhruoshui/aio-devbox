# >>> scenario: python-dev >>>
# uv + ruff as single static-ish binaries to /usr/local/bin (system path, NOT
# /home/gem -> not masked by the workspace volume; on PATH for all users/shells
# without a ~/.bashrc block). base already ships python3/pip/venv; this only
# adds the uv/ruff tooling on top. Versions pinned via ARG for reproducible
# builds; bump when updating.

ARG UV_VERSION=0.5.11
ARG RUFF_VERSION=0.8.4
RUN curl -fsSL "https://github.com/astral-sh/uv/releases/download/${UV_VERSION}/uv-x86_64-unknown-linux-gnu.tar.gz" -o /tmp/uv.tgz \
 && tar -xzf /tmp/uv.tgz -C /tmp \
 && install -m 0755 /tmp/uv-x86_64-unknown-linux-gnu/uv /usr/local/bin/uv \
 && curl -fsSL "https://github.com/astral-sh/ruff/releases/download/${RUFF_VERSION}/ruff-x86_64-unknown-linux-gnu.tar.gz" -o /tmp/ruff.tgz \
 && tar -xzf /tmp/ruff.tgz -C /tmp \
 && install -m 0755 /tmp/ruff-x86_64-unknown-linux-gnu/ruff /usr/local/bin/ruff \
 && rm -rf /tmp/uv.tgz /tmp/uv-x86_64-unknown-linux-gnu /tmp/ruff.tgz /tmp/ruff-x86_64-unknown-linux-gnu \
 && uv --version && ruff --version
# <<< scenario: python-dev <<<
