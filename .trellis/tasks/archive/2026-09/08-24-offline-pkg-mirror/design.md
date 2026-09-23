# Design: 离线软件包仓库(mirror profile)

> 选型经开源调研定稿(2026-08-24):npm = **Verdaccio**(官方镜像,事实标准);
> python = **devpi-server**(on-demand mirror,与 Verdaccio 同模型;自建 6 行 Dockerfile,
> 因无官方镜像);cargo = **vendor 脚本**(调研未找到"活跃维护 + 官方镜像 + 按需回源"
> 三全的 cargo 镜像:panamax 2024-06 停更且全量索引,ktra 无官方镜像且文档薄,
> ByteDance rsproxy 服务端未开源)。备选与证据见 prd R2 与任务 research 记录。

## 总体形态

沿用 L5 外挂模式(同 code-server / vnc):`mirror` compose profile = 2 个服务容器 +
1 个数据卷,**不 FROM sandbox-base**(纯服务面)。镜像 = 服务,数据 = 卷,和
`aio_workspace` 的分工哲学一致。

```
                       sandbox-net
  ┌────────────┐   ┌─────────────────────┐
  │ app / cs   │──▶│ verdaccio  :4873    │──(联网时)──▶ registry.npmjs.org
  │ 终端面板    │   │  storage /mirror/npm│   回源代理,miss 即缓存
  │            │──▶│ devpi      :3141    │──(联网时)──▶ pypi.org
  └────────────┘   │  serverdir           │   root/pypi on-demand mirror
       │           └──────────┬──────────┘
       │ source aio-mirror-env│
       ▼                      ▼
   NPM_CONFIG_REGISTRY     aio_pkgs 卷(独立 tar 分发:mirror-save/load)
   UV_DEFAULT_INDEX
```

caddy 只服务**浏览器按钮**(web 面板);包管理器客户端一律走 sandbox-net 直连
(`http://verdaccio:4873`、`http://devpi:3141/root/pypi/+simple/`),与 pi-web `app:30141` 同理。

## 1. docker-compose.yml(`mirror` profile)

```yaml
  mirror-init:                      # 一次性:建目录 + 属主,跑完退出
    image: caddy:2                  # 复用已有镜像(alpine 系,有 sh),不引新依赖
    profiles: [mirror]
    volumes: [pkgs:/mirror]
    user: "0:0"
    command: sh -c "mkdir -p /mirror/npm /mirror/devpi && chown -R 1000:1000 /mirror"

  verdaccio:
    image: verdaccio/verdaccio:v6     # pin 具体 minor,实施时定
    profiles: [mirror]
    user: "1000:1000"                 # 与共享卷属主对齐(默认 10001 会写不进)
    expose: ["4873"]
    volumes:
      - pkgs:/mirror
      - ./mirror/verdaccio-config.yaml:/verdaccio/conf/config.yaml:ro
    depends_on:
      mirror-init: { condition: service_completed_successfully }
    networks: [sandbox-net]

  devpi:
    build: { context: ., dockerfile: mirror/devpi.Dockerfile }   # 自建,~6 行
    image: sandbox-devpi
    profiles: [mirror]
    user: "1000:1000"
    expose: ["3141"]
    volumes:
      - pkgs:/mirror
    environment:
      DEVPI_SERVERDIR: /mirror/devpi
      DEVPI_PORT: "3141"
    depends_on:
      mirror-init: { condition: service_completed_successfully }
    networks: [sandbox-net]

volumes:
  workspace: ...
  pkgs:                               # 名字 aio_pkgs(compose 前缀)
```

`mirror/devpi.Dockerfile`(自建,不 FROM base,遵守服务面约束):

```dockerfile
FROM python:3.13-slim
RUN pip install --no-cache-dir devpi-server==6.20.3 devpi-web
# 首启初始化(卷为空时 devpi-init),随后常驻;root/pypi 即 on-demand mirror 索引
COPY mirror/devpi-entrypoint.sh /entrypoint.sh
ENTRYPOINT ["/entrypoint.sh"]
```

entrypoint 逻辑:`[ -d "$SERVERDIR/.serverversion" ] || devpi-init --serverdir "$SERVERDIR"`,
然后 `exec devpi-server --serverdir "$SERVERDIR" --host 0.0.0.0 --port "$DEVPI_PORT"`。
客户端 index:`http://devpi:3141/root/pypi/+simple/`(匿名读默认放行)。

要点:
- **单卷双目录**:`pkgs:/mirror` 两个容器同挂,verdaccio 写 `/mirror/npm`,
  devpi 写 `/mirror/devpi` —— 分发时一个 tar 搞定。
- **mirror-init**:共享卷属主问题的确定性解(官方镜像默认 uid 各不相同)。
- `expose` 而非 `ports`,不对外。

## 2. mirror/verdaccio-config.yaml

```yaml
storage: /mirror/npm
web: { enable: true }
publish: { allow_offline: true }
url_prefix: /npm          # caddy 子路径后面板资源 URL 才正确
uplinks:
  npmjs:
    url: https://registry.npmjs.org/
    cache: true
packages:
  '@*/*':
    access: $all, proxy: npmjs
  '**':
    access: $all, proxy: npmjs
  # 本地 publish 留空(离线补包的进阶用法,先不做)
```

离线语义:uplink 不可达 → **缓存命中正常出包;未命中直接报错**(Verdaccio 原生行为,
满足 R1 "不挂起")。

## 3. 客户端接线(R5):`/usr/local/bin/aio-mirror-env`

烘进 **Dockerfile.base.tail**(基础设施工件,永远在场但不自激;不走 scenario —— 它不
值得一个 TUI 开关,且 tail 在 USER gem 之前仍可写系统路径)。

```bash
# 用法: source aio-mirror-env   (探针 1s,可达才 export)
probe() { timeout 1 bash -c "echo >/dev/tcp/$1/$2" 2>/dev/null; }
if probe verdaccio 4873; then
  export NPM_CONFIG_REGISTRY=http://verdaccio:4873
fi
if probe devpi 3141; then
  export UV_DEFAULT_INDEX=http://devpi:3141/root/pypi/+simple/
  export PIP_INDEX_URL=http://devpi:3141/root/pypi/+simple/
fi
```

**不做自动 source**(设计决策):每 login 探测网络 = 延迟 + 抖动;显式 `source` 语义
清晰、天然满足 AC6(mirror 不在 → 什么都不改,走默认 registry)。cargo 不接(vendor 模式
与 registry 无关)。

## 4. WebUI 按钮 + caddy

- Caddyfile(catch-all 之前):
  ```caddyfile
  handle_path /npm/*  { reverse_proxy verdaccio:4873 }
  handle_path /pypi/* { reverse_proxy devpi:3141 }
  ```
- services.toml 两个 `type=web`(target `verdaccio:4873` / `devpi:3141`;url `/npm/`、
  `/pypi/`;label "npm 镜像" / "PyPI 镜像")→ TCP 探测自动随 profile 显隐(AC1)。
  services.toml 是 include_str!,**需重建 app 镜像**。devpi 面板由 devpi-web 提供,
  需验证其子路径行为(+simple 安装端点客户端直连,不经 caddy,不受影响)。

## 5. warm(预热)

- `make mirror-warm-npm`:在 sandbox-net 上跑一次性 sandbox-base 容器(挂 workspace 卷),
  遍历 `/home/gem` 下(限深)的 `package-lock.json`,逐项目
  `npm ci --registry http://verdaccio:4873 --ignore-scripts`(ci 严格按锁文件拉全闭包)。
- `make mirror-warm-pypi`:同一容器(无需挂 pkgs —— 缓存在 devpi 侧),遍历
  `uv.lock` / `requirements*.txt`:`uv export --frozen --no-dev -o req.txt` →
  `pip download --index-url http://devpi:3141/root/pypi/+simple/ -r req.txt -d /tmp/warm`
  (下载会自然穿透 devpi 入缓存;落盘目录用完即弃,真正的产物是 devpi 的缓存)。
- 两个 target 只应联网机使用;输出统计(项目数/包数/体积)。

## 6. cargo vendor(R3):`/usr/local/bin/aio-cargo-vendor`

同样烘 tail(~20 行包装):联网机在项目目录运行 → `cargo vendor vendor/` + 打印
`.cargo/config.toml` 的 source-replacement 片段 + 提示 tar 分发;离线机
`cargo build --offline`。本质是 offline-install-guide 方法 D 的产品化,不建服务。

## 7. 分发(R6)

- `make save`:SAVE_IMAGES 保持可覆盖变量,文档给
  `make save SAVE_IMAGES="sandbox-base sandbox-app sandbox-code-server sandbox-vnc caddy:2 verdaccio/verdaccio:vX sandbox-devpi"`;
  默认集不变(mirror 未用时不至于 save 失败)。
- `make mirror-save`:`docker run --rm -v aio_pkgs:/mirror -v $(PWD):/out caddy:2
  tar cf /out/aio_pkgs.tar -C /mirror .`(复用 caddy:2,免引 alpine)。
- `make mirror-load`:`docker volume create aio_pkgs` + 反向 tar。
- .gitignore:`aio_pkgs.tar`。
- offline-install-guide 整机分发章节补镜像服务段。

## 8. 离线验证(AC4)手段

- Verdaccio 侧(严格断网验证):临时 compose override(`compose.offline-test.yml`,
  测试资产)把 uplink 换成 `http://127.0.0.1:9`(必死地址)→ 复测缓存命中/未命中行为。
- devpi 侧:root/pypi 的上游 URL 不可配 env,严格断网用 docker `internal: true`
  附加网络成本高 —— 验证降级为**行为验证**:装过的包再装时 devpi 日志无上游请求
  (缓存命中路径),未命中报错文案记录进 guide。离线真机的最终行为与 Verdaccio 同构
  (都是"上游不可达 → miss 即报错"),由 devpi 文档语义背书。

## 风险与对策

| 风险 | 对策 |
|---|---|
| Verdaccio 子路径资源错位 | url_prefix: /npm;实施第一步就验证 web 面板 |
| devpi 自建镜像(devpi-init 语义/权限) | Step 1 单独验证首启初始化与匿名读;失败回退 pypiserver + wheelhouse 方案(prd 备选) |
| devpi-web 子路径资源错位 | 面板仅辅助;安装端点(+simple)客户端直连不经 caddy,不受影响 |
| 共享卷 uid 冲突 | mirror-init 一次性 chown + 两服务统一 user 1000:1000 |
| warm 时 npm ci 触发生命周期脚本 | 统一 --ignore-scripts |
| uv 的 index 环境变量名版本差异 | 同时验证 UV_DEFAULT_INDEX / UV_INDEX_URL,取生效者写文档 |
| aio_pkgs 无限增长 | 文档给 du + 清空命令;verdaccio storage 按包可删 |

## 兼容与回滚

- 纯增量:不动现有服务;`make up` 不带 mirror 时零影响(AC6)。
- 回滚 = `docker compose --profile mirror down` + `docker rmi <两镜像>` + (可选)
  `docker volume rm aio_pkgs`;tail 烘焙物无害可留。
