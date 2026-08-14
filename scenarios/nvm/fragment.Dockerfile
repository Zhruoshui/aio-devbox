# >>> scenario: nvm >>>
# L3 可选:nvm(Node 版本管理器)。nvm.sh 单脚本烘到 /opt/nvm(系统路径,躲过
# 共享卷 aio_workspace 对 /home/gem 的遮盖;root 只读)。运行时 NVM_DIR=$HOME/.nvm
# (卷上),profile.d 把 nvm.sh 软链进 ~/.nvm/nvm.sh 再 source -> nvm 在
# $NVM_DIR/nvm.sh 找到自己(软链指向 /opt/nvm/nvm.sh),versions 装到
# ~/.nvm/versions/node(卷上,抗 container recreate)。
#
# 风险点:nvm.sh 期望自己在 $NVM_DIR/nvm.sh;软链方案解决自定位(实现期实测)。
# 覆盖:仅 login shell source /etc/profile.d(AIO 终端面板 = /bin/bash -l);
# code-server 非login 终端不 source profile.d,需手动 source 或后续加
# /etc/bash.bashrc 钩子(同 shell-utils 别名曾遇到的问题)。
ARG NVM_VERSION=v0.40.1
RUN mkdir -p /opt/nvm \
 && curl -fsSL "https://raw.githubusercontent.com/nvm-sh/nvm/${NVM_VERSION}/nvm.sh" \
        -o /opt/nvm/nvm.sh \
 && chmod 0644 /opt/nvm/nvm.sh \
 && grep -q 'nvm_echo' /opt/nvm/nvm.sh

RUN cat > /etc/profile.d/aio-nvm.sh <<'EOF'
# nvm (scenario: nvm). nvm.sh is baked at /opt/nvm (system path, survives the
# workspace volume); NVM_DIR points at the shared volume so `nvm install`
# versions survive container recreate. Symlink nvm.sh into NVM_DIR so nvm finds
# itself at $NVM_DIR/nvm.sh (its expected location), then source it.
export NVM_DIR="$HOME/.nvm"
mkdir -p "$NVM_DIR"
[ -e "$NVM_DIR/nvm.sh" ] || ln -sf /opt/nvm/nvm.sh "$NVM_DIR/nvm.sh"
[ -s "$NVM_DIR/nvm.sh" ] && . "$NVM_DIR/nvm.sh"
EOF
# <<< scenario: nvm <<<
