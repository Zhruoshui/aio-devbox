# Implement: 离线软件包仓库(mirror profile)

> 执行顺序即依赖顺序;每步带验证命令与回滚点。构建/重启类命令在本机
> (沙箱内 docker)由 AI 执行,与本次 pi 任务同流程。

## Step 1 — compose 服务骨架 + 卷

- [ ] `docker-compose.yml`:新增 `mirror-init` / `verdaccio` / `devpi` 三个服务
      (`profiles: [mirror]`)+ `pkgs` 卷,按 design §1(verdaccio tag 先查 Docker Hub pin)。
- [ ] `mirror/verdaccio-config.yaml`(design §2)、`mirror/devpi.Dockerfile` +
      `mirror/devpi-entrypoint.sh`(design §1,首启 init + 常驻 serve)。
- 验证:`docker compose --profile mirror config >/dev/null` 语法通过;
  `make up PROFILES="mirror"` 后两容器 Up、mirror-init Exited(0)、
  `docker run --rm -v aio_pkgs:/m caddy:2 ls -la /m` 显示 npm/devpi 属 1000;
  devpi 首启完成 init:`curl http://localhost:3141/root/pypi/+simple/` 返回索引页。
- 回滚:删除三个 service + 卷定义 + mirror/ 目录。

## Step 2 — 客户端接线脚本烘 base

- [ ] `Dockerfile.base.tail`:root 段新增 `/usr/local/bin/aio-mirror-env`
      (design §3,含探针降级)与 `/usr/local/bin/aio-cargo-vendor`(design §6)。
- [ ] `make build-base`;验证:`docker run --rm sandbox-base bash -n /usr/local/bin/aio-mirror-env`
      与 `... aio-cargo-vendor`;空环境跑 `aio-mirror-env`(不 source)无副作用。
- 回滚:tail 还原 + rebuild。

## Step 3 — caddy 路由 + WebUI 按钮

- [ ] `gateway/Caddyfile`:`/npm/*`、`/pypi/*` handle_path(catch-all 之前)。
- [ ] `app/services.toml`:两个 `type=web` 条目(target `verdaccio:4873` / `devpi:3141`)。
- [ ] 重建:`make up PROFILES="code-server,vnc,mirror"`(app 含 services.toml 必须 rebuild)。
- 验证:`curl -u admin:admin http://localhost:8080/npm/` 出 Verdaccio 面板(资源不 404);
  `/pypi/` 出 devpi-web 面板;`/api/manifest` 两按钮 enabled=true;去掉 mirror profile
  重启 → enabled=false(AC1)。
- 回滚:Caddyfile/services.toml 还原 + gateway/app recreate。

## Step 4 — 联网回源验证(AC2)

- [ ] app 容器内:`source /usr/local/bin/aio-mirror-env && npm config get registry`
      → `http://verdaccio:4873`;`cd /tmp && npm install cowsay --registry ...` 成功。
- [ ] 面板可见 cowsay;`docker run --rm -v aio_pkgs:/m caddy:2 find /m/npm -name "*cowsay*"`
      命中缓存。
- [ ] uv 侧:`uv pip install --index http://devpi:3141/root/pypi/+simple/ <小包>` 成功且
      devpi 面板可见。
- [ ] 反向(AC6):无 mirror profile 的栈里 `npm install` 走默认 registry 正常。

## Step 5 — warm 目标(AC3)

- [ ] Makefile:`mirror-warm-npm` / `mirror-warm-pypi`(design §5,一次性
      sandbox-base 容器,sandbox-net)。
- [ ] 造样例:workspace 放一个含 package-lock.json 的小项目 + 一个 uv.lock 项目,
      跑两 target。
- 验证:verdaccio 面板出现样例闭包;devpi 面板出现 python 闭包
  (重复安装同一包,devpi 日志无上游请求 = 真入缓存)。
- 回滚:删 Makefile 目标;卷内容可按目录清。

## Step 6 — 离线模拟(AC4)

- [ ] `compose.offline-test.yml`:verdaccio 换 uplink→死地址配置;`make up` 带此 override。
- [ ] 复测:已缓存包 npm 安装成功;未缓存包得到明确报错(记录报错文案进 guide)。
- [ ] devpi 侧行为验证(design §8:缓存命中日志无上游请求 + miss 报错观察)。
- 回滚:不带 override 重启即恢复。

## Step 7 — cargo vendor(AC5)

- [ ] 用 aio-cargo-vendor 对样例 rust 项目生成 vendor 目录;`cargo build --offline` 通过。
- 验证记录写入 offline-install-guide(方法 D 更新为脚本用法)。

## Step 8 — 分发闭环(AC7)

- [ ] Makefile:`mirror-save` / `mirror-load`(design §7);`.gitignore` + `aio_pkgs.tar`。
- [ ] `make save SAVE_IMAGES="...verdaccio... pypiserver..."` 实测;
      `mirror-save` → 删卷 → `mirror-load` → AC2 复测。
- [ ] docs/offline-install-guide.md:整机分发章节补"镜像服务"小节
      (SAVE_IMAGES 覆盖、mirror-save/load、warm 时机:迁移前联网机跑一次)。
- [ ] README profile 列表补 mirror。

## Step 9 — 收尾

- [ ] 全 AC 勾验;无 `{{` 残留;`bash -n` 全部烘焙脚本。
- [ ] spec 更新(3.3):mirror profile 模式(L5 外挂 + 单卷双目录 + init 属主)进
      aio-env-config skill 的 compose-registry 参考。
- [ ] 提交拆分建议:①compose+config ②tail 脚本 ③caddy+services.toml
      ④Makefile+docs;commit message 前缀 feat:。

## 验证总命令(收尾一把梭)

```bash
make up PROFILES="code-server,vnc,mirror"
docker exec aio-app-1 bash -lc 'source /usr/local/bin/aio-mirror-env && npm config get registry'
curl -sS -u admin:admin http://localhost:8080/api/manifest | grep -E 'verdaccio|pypi'
docker compose --profile mirror ps
grep -n '{{' Dockerfile.base   # 空
```
