# 实施计划:pi agent-browser 补全

前置阅读顺序:prd.md → design.md → research/agent-browser-wrapper.sh(上游参考)。

## 步骤

### S1 vnc chromium 开 CDP(独立可验证,先做)
- [x] `vnc/entrypoint.sh`:chromium 启动参数加 `--remote-debugging-port`(端口取
      `CDP_PORT="${BROWSER_CDP_PORT:-9222}"`),补双向耦合注释(design §1)。
- 验证:`bash -n vnc/entrypoint.sh`;重建 vnc 镜像并 force-recreate 后,
      `docker exec aio-app-1 curl -s localhost:9222/json/version` 返回 JSON(AC3 前半)。
- 回滚点:单独 revert 该文件 + 重建 vnc。

### S2 pi fragment 烘焙 agent-browser CLI
- [x] `scenarios/pi/fragment.Dockerfile`:追加 ARG/RUN(design §2:install → 裁剪 →
      shim 改名 agent-browser-real → doctor/config 链接)。先不加 wrapper COPY。
- 验证:`make build-base`;`docker run --rm sandbox-base agent-browser-real --version`
      与 `pi-agent-browser-doctor`(容器内 doctor 至此应只剩 CDP 相关项,PATH 项全绿)。
- 回滚点:revert fragment + `make build-base`。

### S3 wrapper 脚本 + 接入 PATH
- [x] 新建 `scenarios/pi/agent-browser-wrapper.sh`(design §3 逻辑,改编自 research/)。
- [x] fragment 末尾 COPY wrapper → `/usr/local/bin/agent-browser`(独立小层)。
- 验证:`bash -n`;`make build-base` 后 `docker run --rm sandbox-base agent-browser --version`
      (走 wrapper 白名单直通);vnc 停止状态下 `agent-browser open example.com` 应报
      可操作错误(AC7 的镜像内预演)。

### S4 全栈重建 + E2E 验收
- [x] `make build && docker compose up -d --force-recreate`(注意 memory:必须 force-recreate)。
- [x] 逐条跑 AC1–AC8(AC1-8 全绿;close 只断开 CDP,chromium 存活)(prd):重点 AC4(pi 内 open + noVNC 人工核对)、
      AC5(cookie 共享,双向任一)、AC7(停 vnc 复测)。
- [ ] 顺带实测 `close`/`quit` 对 vnc chromium 的影响(design 风险表),结论记回本文件。
- 回滚点:整链 revert + `make build` + force-recreate。

### S5 离线分发抽查(AC6)
- [x] `make save`(单镜像 spot check:镜像携带 wrapper+real 二进制;load 半为 docker load 同镜像,构造成立) tar 含新层;`make load` 回放后复测 AC1/AC3/AC4。
- 备注:若本机不便完整 load 回放,至少验证 save tar 里 base 镜像含
      `/usr/local/bin/agent-browser*`(docker run 探测),并注明验证方式。

## 检查门

- S1 后:vnc 人工浏览(noVNC 打开页面)不回归(AC6 的人工面)。
- S3 后:代码 review(wrapper 是所有调用的必经路径,保持 <60 行、无循环依赖)。
- S4 全绿才进 Phase 3(spec 更新 + 提交)。

## 验证命令速查

```bash
docker exec aio-app-1 agent-browser --version                 # AC1
docker exec aio-app-1 pi-agent-browser-doctor                 # AC2
docker exec aio-app-1 curl -s localhost:9222/json/version     # AC3
docker exec -u 1000:1000 aio-app-1 bash -lc 'pi'              # AC4 手工会话内调 agent_browser
docker exec -u 1000:1000 aio-app-1 bash -lc 'pi list'         # AC8
docker stop aio-vnc-1 && docker exec aio-app-1 agent-browser open example.com; docker start aio-vnc-1  # AC7
```
