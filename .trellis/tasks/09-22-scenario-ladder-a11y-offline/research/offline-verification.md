# 离线验证实测记录（2026-09-22）

全部在真实 docker daemon + `--network none` 容器内执行,非推测。

## 1. `make save` / `make load` 端到端

```
$ make save
docker save sandbox-base sandbox-app sandbox-code-server sandbox-vnc caddy:2 -o aio-offline-bundle/images.tar
cp .env aio-offline-bundle/env ; cp .aio/enabled.toml aio-offline-bundle/enabled.toml
2.5G	aio-offline-bundle          real 0m22.7s

$ make load
Loaded image: sandbox-base:latest / sandbox-app:latest / sandbox-code-server:latest
             / sandbox-vnc:latest / caddy:2

$ make up NOBUILD=1
Container aio-app-1 Running
Container aio-gateway-1 Running
```

- bundle = **2.5G**(远小于按 `docker system df` 估算的 11GB —— 共享层在 tar 内去重更彻底)
- 落盘位置是**仓库目录**(宿主 fs,1TB 可用),不占 `/var/lib/docker`
- 结构完整性:manifest 列 5 镜像;引用 143 个 blob,**全部在位,0 缺失**

## 2. 场景工具离线可用性(`--network none`)

`.aio/enabled.toml` 的 21 个可选场景 + 3 个必装,逐条探针:

| 通道 | 结果 |
|---|---|
| login(`bash -l`,走 profile.d + 卷探测) | **35 / 35 通过,0 失败** |
| 非 login(`bash -c`,走镜像 ENV 通道) | **35 / 35 通过,0 失败** |

覆盖:L1(mise engine / node / python / 字体)、L2 全部 10 个 shell 工具、
L3 mise 派(rustc/cargo/clippy/rustfmt/go/uv/ruff)+ apt 派(clang/gdb/cmake/
ninja/valgrind/cppcheck/strace)、L4(opencode/claude/codex/pi/pi-web)。

- `pi-web` **无 `--version`**:它是 Next.js 服务,调用即启动。正确探针 = 起服务 +
  loopback 探活 → 离线 **HTTP 200**、`✓ Ready in 75ms`。首轮报 HANG 是探针命令错,
  不是离线失败。

## 3. 运行期自装边界(`mise use -g`)

| 用例 | 结果 |
|---|---|
| `mise use -g hyperfine`(新工具,离线) | ❌ `dns error: failed to lookup address information` —— **快速显式失败**,不挂起 |
| `mise use -g hyperfine@1.19.0`(钉版本,离线) | ❌ 同上,钉版本也要联网 |
| `MISE_OFFLINE=1` + 新工具 | ❌ 失败但信息误导:`no versions found for <tool> matching date filter` |
| `mise use -g bat`(**已烘焙**,离线) | ✅ **成功** —— 打 `Remote versions cannot be fetched` 警告后回落本地安装,`bat --version` 正常 |

**结论**:离线能力边界 = 镜像烘焙了什么。新工具离线装不上是 mise 固有属性。
加新工具的合规路径是在联网机改 `enabled.toml` → `make gen && make build-base` → 重新 `make save`。

缓存不构成兜底:`~/.cache/mise` / `/opt/mise/downloads` 只含已烘焙工具的制品,
新工具从未下载过。`~/.cache/mise` 位于 `/root`,新卷首次挂载时由 Docker 从镜像播种
(实测新卷下有 21 个条目),故运行期可见 —— 但只放宽"已烘焙"边界,不改变结论。
