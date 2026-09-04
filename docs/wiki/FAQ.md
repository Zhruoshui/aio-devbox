# 常见问题 / FAQ

[← 返回首页](Home)

收录使用过程中的高频问题与排错入口。背景知识见 [架构总览](Architecture) 与
[场景配置](Scenarios),离线相关问题另见 [离线分发](Offline-Bundle)。

## 安装与启动

**首次启动报 hash / .env 缺失?**

```sh
cp .env.example .env      # 网关凭据等
make hash                 # 生成网关密码(默认用户 admin)
make up
```

**code-server / Chromium 面板不见了?**

它们是 `profiles` 门控的可插拔容器,没起就没按钮(探测使然,非 bug):

```sh
make up PROFILES="code-server vnc"
```

**哪些端口要发布到宿主?**(在 `sbx` 类沙箱环境里跑时)

- 宿主只需 `8080`(网关)——code-server / vnc 都走网关子路径;
- `30141`(pi-web)是例外,端口直发不经网关;宿主侧换端口用
  `PI_WEB_HOST_PORT=30142 make up`,注意 `sbx ports --publish` 两端都要配成
  同一个端口号。

## 场景重建

**场景重新勾选后为什么不生效?**

`make config` 只改了 `.aio/enabled.toml`(清单),工具链要烘进镜像才存在:

```sh
make build                                    # gen + 重烘 sandbox-base
docker compose up -d --force-recreate app code-server   # 让运行中的容器换新镜像
```

⚠️ 经典坑:`docker compose up --build` 对**运行中**的服务会跳过重建——必须
先 `make build` 再显式 `--force-recreate`,缺一步都白做。

**场景里的工具版本怎么升?**

- L1 node / python:`make config` 里方向键切版本;
- mise 场景(rust/go/uv/ruff/opencode):改 `scenarios/mise/fragment.Dockerfile`
  顶部 ARG 块,重新 `make build`;
- 其余场景:改各自 fragment 的 ARG 后重建。

**运行时自装的工具,重建后没了?**

- 装进 `~/.local/bin` 的自包含二进制:在卷上,**扛重建**;
- `mise use <tool>`:落容器可写层,**recreate 即丢**(已知取舍);要留就写进
  `scenarios/mise/fragment.Dockerfile` 的 `[tools]` 重新 build,或用离线
  整目录搬迁(`docs/offline-tool-install.md` §14);
- 容器里 `apt install`:可写层,临时验证用,别依赖。

## 按钮 / 端口预览

**agent 按钮(opencode / pi)不出现?**

按钮只在命令真实存在于 login shell PATH 时显示(`command_exists` 探测)。
pi 是 `always_on` 场景、恒定烘入,正常不会缺席;opencode 由 mise 场景附带
烘焙——勾了 mise 场景但没 `make build` 重建镜像,按钮就不会出现。排查:
`docker exec aio-app-1 bash -lc 'command -v opencode'`。

**pi Web 面板不出现 / 502?**

pi-web 由 app entrypoint 自启在 `:30141`(TCP 探活决定按钮)。看日志:
`tail ~/.aio/pi-web.log`。宿主换端口发布时,URL 与端口映射由
`PI_WEB_HOST_PORT` 统一驱动,两端要配对。

**在终端里起的 dev server,怎么在浏览器预览?**

三方共享 loopback(见 [架构总览](Architecture)),所以:

- code-server 内置端口转发:`/proxy/<port>/`;
- 侧边栏底部 `+` 注册 **web 型自定义按钮**指向该端口:有监听时按钮出现,
  点击经 app 的 `/preview/<port>/` 反代在 iframe 打开(WS / SSE 友好);
- VNC 里的 Chromium 直接开 `http://localhost:<port>`(loopback 豁免
  HTTPS-first,不会强制跳 https)。

## 网关鉴权

**改密码?** `make hash` 重新生成(默认用户 `admin`),然后
`docker compose up -d --force-recreate gateway`。

**`make up` 报 hash 校验失败?** Caddyfile 哈希与容器内不一致,重跑
`make hash` 即可(`ensure-hash` 会兜,但离线 bundle 场景先 `make load`)。

## 已知坑速查

| 症状 | 原因 / 处理 |
|---|---|
| 单独 `docker restart aio-app-1` 后 code-server/vnc 全断 | app 的 netns 被重建,侧车指向死 netns;一起重启侧车恢复。日常走 `make restart` |
| vnc 容器反复 flapping(Xvnc "Server is already active") | 旧版遗留 stale `/tmp/.X99-lock`;现 vnc 的 `/tmp` 是 tmpfs,重启即清 |
| code-server 里 cpptools 弹许可证窗后 LSP 退出 | MS 许可证检查拒绝非微软宿主,**不要装 cpptools**;C/C++ 用 clangd(Open VSX 扩展 + c23 场景烘的 clangd-22) |
| Chromium 打开 `http://服务名:port` 强制跳 https | HTTPS-first 只豁免 localhost/IP 字面量;内部服务一律用 `http://localhost:port`(netns 共享下与容器内一致) |
| `make gen` 报场景 id 找不到 | 本地 `.aio/enabled.toml`(gitignored)还勾着已删场景;`make config` 重选或手工移除 |
| login shell 找不到某工具,非 login 找得到 | 该工具只配了 ENV PATH 或只配了 profile.d 之一;场景编写规则见 [场景配置](Scenarios) 铁律 2 |
