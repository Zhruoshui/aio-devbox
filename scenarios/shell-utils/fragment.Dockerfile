# >>> scenario: shell-utils >>>
# L2 示例:便利 shell 工具(纯二进制,不含别名机制)。
#   - 工具装系统路径(/usr/bin,/usr/local/bin),全 shell PATH 通用,不被共享卷
#     aio_workspace 遮盖。
#   - 不提供 shell 别名/函数:别名靠 source 加载、仅特定 shell 形态生效
#     (login vs 非login 交互),覆盖不一致;故本场景只装二进制工具。
# apt lists 在 head 末尾被 `rm -rf /var/lib/apt/lists/*` 清空,这里需重新 update。

RUN apt-get update \
 && apt-get install -y --no-install-recommends fzf ripgrep bat fd-find \
 && rm -rf /var/lib/apt/lists/* \
# Debian renames fd->fdfind and bat->batcat to avoid conflicts; symlink the
# conventional names into /usr/local/bin so scripts and shells can use fd/bat.
 && ln -sf "$(command -v fdfind)" /usr/local/bin/fd \
 && ln -sf "$(command -v batcat)" /usr/local/bin/bat \
 && fzf --version && rg --version && bat --version && fd --version
# <<< scenario: shell-utils <<<
