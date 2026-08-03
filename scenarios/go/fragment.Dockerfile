# >>> scenario: go >>>
# Go toolchain from the official tarball (build machine online; offline via
# whole-image save/load). Extracted to /usr/local/go (system path, NOT /home/gem
# -> not volume-masked). go/gofmt live in /usr/local/go/bin (custom PATH); the
# ENV PATH below is inherited by non-login shells, but LOGIN shells (bash -l)
# source /etc/profile which RESETS PATH -> symlink go/gofmt into /usr/local/bin
# so every shell type finds them. Same pattern as the rust scenario.
#
# No chown needed (unlike rust): gem's `go install` writes GOPATH=$HOME/go (on
# the workspace volume, gem-writable by default); /usr/local/go stays root-owned
# and read-only for gem, which is fine.

ARG GO_VERSION=1.23.4
RUN curl -fsSL "https://go.dev/dl/go${GO_VERSION}.linux-amd64.tar.gz" -o /tmp/go.tgz \
 && tar -xzf /tmp/go.tgz -C /usr/local \
 && rm /tmp/go.tgz \
 && /usr/local/go/bin/go version \
 && ln -sf /usr/local/go/bin/go /usr/local/bin/go \
 && ln -sf /usr/local/go/bin/gofmt /usr/local/bin/gofmt
ENV PATH=/usr/local/go/bin:$PATH
# <<< scenario: go <<<
