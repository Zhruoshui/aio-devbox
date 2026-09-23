# Implement：VNC netns 共享重构

前置：design.md §5 变更清单为唯一实现范围。顺序执行，每步带验证。

## Step 1 compose 拓扑改造

- [ ] `docker-compose.yml`：code-server 服务删 `expose: "8200"` 与 `networks:
      [sandbox-net]`，加 `network_mode: "service:app"`；vnc 服务同样处理（6080），
      另加 `tmpfs: /tmp`；两个服务的注释块更新（跨容器预览 out-of-scope 说明作废、
      探测名改 app:PORT、netns 语义一句、VSCODE_PROXY_URI 受益说明）。
- 验证：`docker compose -f docker-compose.yml config` 通过；无 ports/networks 残留。

## Step 2 网关与探测改名

- [ ] `gateway/Caddyfile`：两处 `reverse_proxy` 上游改 `app:8200` / `app:6080`，
      头部注释同步。
- [ ] `app/services.toml`：`target` 改 `app:8200` / `app:6080`；pi-web 注释更新为
      localhost 写法。
- [ ] `app/src/config.rs`：grep 确认无硬编码 `vnc:6080`/`code-server:8200` 残留
      （预期零改动）。
- 验证：`grep -rn 'vnc:6080\|code-server:8200' --exclude-dir={node_modules,target,dist,.git}`
      仅剩 README/docs/skill 参考中的描述性引用（Step 5 处理）。

## Step 3 构建与起栈（AC1）

- [ ] `make build`（含 app 镜像重建 —— services.toml 是 include_str! 编译期嵌入）。
- [ ] `make up PROFILES=vnc,code-server`。
- 验证：`docker ps` 三容器 Up；UI code-server/Chromium 按钮出现、iframe 可用；
  `docker compose exec gateway wget -qO- http://app:6080` 与 `http://app:8200`
  探活成功。

## Step 4 localhost 直通验收（AC2/AC3 + design §3 矩阵）

- [ ] 工作台终端 `python3 -m http.server 9999`，VNC Chromium 开
      `http://localhost:9999` → 目录列表。
- [ ] code-server 集成终端 `python3 -m http.server 9998`，VNC 开 localhost:9998。
- [ ] `docker restart aio-app-1` 后重试上述两条，记录 sidecar 网络行为到本文件
      （design §3 矩阵的实测栏）。

## Step 5 flapping 修复验收（AC4）

- [ ] `docker restart aio-vnc-1` 连续 3 次，每次后 `docker ps` 确认 Up、
      RestartCount 不增长；`docker exec aio-vnc-1 ls /tmp/.X99-lock` 在启动完成
      后存在（运行时正常锁）但 restart 重建后无 stale 残留报错、无 flapping。
- [ ] （若仍复现 flapping：entrypoint rm 逻辑保留未动，属新增问题回 design。）

## Step 6 场景回归（AC5）

- [ ] pi-web 按钮启动，VNC Chromium 开 `http://localhost:30141` 可用；
      顺带确认 pi-web 按钮经 `command_exists` 显隐未受影响。

## Step 7 文档与 skill 参考

- [ ] `README.md` / `README.zh-CN.md` 服务表：探测名、netns 共享、保留端口清单
      （8088/8200/6080/5900）。
- [ ] `docs/offline-tool-install.md:86` 跨容器预览写法改 localhost。
- [ ] `.claude/skills/aio-env-config/references/compose-registry.md` + `recipes.md`
      相关段：新服务接入参考改 netns 模式。

## Step 8 质量与收尾

- [ ] 最后一轮全量检查（trellis-check 语义：lint 无适用项、`docker compose config`
      、AC1–AC5 全绿、AC6 回滚演练记录）。
- [ ] spec 更新（Phase 3.3）：backend spec 无涉；aio-env-config skill 参考已在
      Step 7 覆盖；如 trellis-update-spec 判定有新契约（netns 接入模式）另记。
- [ ] 提交（Phase 3.4，单 commit，revert 即回滚）。

## 风险文件与回滚点

- 高风险：`docker-compose.yml`（拓扑开关本体）。回滚 = git revert 单 commit；
  `make down && make up PROFILES=...` 全量重建即恢复。
- 中风险：`app/services.toml`（按钮显隐数据源）——改名错误表现为 UI 按钮消失，
  AC1 即时暴露。
- 低风险：Caddyfile、文档。

## task.py start 前检查

- [ ] prd/design/implement 三件套齐且用户已审。
- [ ] implement.jsonl / check.jsonl 已填真实条目（非 _example）。
