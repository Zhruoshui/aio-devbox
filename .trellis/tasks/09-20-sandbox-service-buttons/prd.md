# 沙箱服务开关依赖展示与工作区按钮联动修复

## Goal

创建沙箱时的四个服务开关(code-server / VNC 桌面 / pi / pi Web)需要:

1. 在创建页以合理的展示状态呈现服务间依赖关系,方便开关;
2. 关闭的服务**真正不在工作区出现**——最小沙箱只保留 Terminal 按钮与注册按钮;
3. 顺带修复排查中发现的两个底层缺陷(manifest 探测误报、跨沙箱代理泄漏)。

## 背景与根因(2026-09-20 实测验证)

用户在创建页关闭服务后,构建成功的工作区里 code-server / Chromium / pi /
pi Web 按钮依然存在(可点击但无法运行)。用最小沙箱 `min1`(四服务全关)实测:

- **R1 manifest 探测误报**:每个沙箱的 app 容器以 compose 服务别名 `app` 加入
  共享外部网络 `aio-mgr-net`,所有沙箱的 app 在该网络上同名。`app/services.toml`
  的按钮可见性探测(`app:8200` / `app:6080` / `app:30141`)从本沙箱 app 内发起,
  DNS 却可解析到**其他沙箱**的 app → 未安装的服务误报 `enabled: true`
  (实测 min1 的 manifest 中 codeServer / vnc / piWeb 均 enabled,来源是 dev1)。
- **R2 跨沙箱代理泄漏**:mgr 生成的沙箱 Caddyfile `reverse_proxy app:8200`
  同样命中别名歧义。实测 min1 的 `/code-server/`、`/vnc/` 返回 200,实际代理
  的是 **dev1** 的 code-server / Chromium(跨沙箱数据泄漏,编辑的是 dev1 的卷)。
- **R3 ON_DEMAND 特例不看安装集**:`SandboxTree` 的 `ON_DEMAND_SERVICE_IDS`
  让 codeServer 按钮无视 `enabled` 恒显示,且从不检查 mgr 列表 API 已返回的
  `installed_services.code_server` → 未安装也显示,点击后启动 pane 才失败。
- pi 按钮(agent 类型)本体逻辑正确(`command_exists("pi")` 未烘焙即隐藏),
  用户看到的 pi 按钮残留来自 R1 的 piWeb 误报链路与旧沙箱。

## Requirements

### Req 1: 创建页依赖展示与级联开关(ServicesPicker)

- pi Web 行明确展示"依赖 pi + VNC"(已有 lock 标签,保留);
- 当 pi Web 开启时,pi / VNC 行显示"pi Web 依赖此服务"提示;
- **级联关闭**:pi Web 开启时关闭 pi 或 VNC → pi Web 一并自动关闭
  (替换当前"点了没反应"的静默拒绝;用户已确认选此行为);
- 开启 pi Web 仍自动开启 pi + VNC(现状保留);
- 后端 `normalize_services` 的 400 防线保留不动(级联保证客户端不会送出
  矛盾组合,后端防线继续拦截旧客户端)。

### Req 2: 工作区按钮按安装集过滤

- mgr-web 工作区树的四个内置服务按钮按该沙箱 `installed_services` 过滤:
  `codeServer→code_server`、`vnc→vnc`、`pi→pi`、`piWeb→pi_web`,
  未安装即不渲染(不是置灰);
- Terminal 按钮、用户自注册按钮(deletable)、注册按钮入口不受影响;
- 最小沙箱(四服务全关、env 无相关场景)在工作区只显示 Terminal(+注册)。

### Req 3: manifest 探测改为本沙箱本地探测

- `app/services.toml` 三个 web 探测目标改为 `localhost:端口`
  (code-server / vnc sidecar 共享 app 网络命名空间,pi-web 由 app
  entrypoint 在容器内自启,localhost 探测天然只反映本沙箱);
- 探测语义不变(400ms TCP 连接),仅目标地址变更。

### Req 4: 沙箱网关反代消除别名歧义(修跨沙箱泄漏)

- mgr 生成的 compose 给 app 在 sandbox-net 上增加唯一别名
  `sbx-<name>-app`;生成的 Caddyfile 反代目标由 `app:<port>` 改为
  `sbx-<name>-app:<port>`;
- 未安装 code-server / vnc 的沙箱,Caddyfile 不再生成对应
  `/code-server/*`、`/vnc/*` 路由块(未安装即无路由,而非 502 路由);
- 兼容性:已有沙箱实例目录里的旧 compose/Caddyfile 不回写,重建(重建
  沙箱/env 变更)时自然换代。

## Constraints

- 不改 `normalize_services` / `installed_services_of` 的存储与读回契约
  (S1 语义:pi/pi_web 属 env.scenarios,code_server/vnc 属 services_json)。
- 不改 `ON_DEMAND_SERVICES` 后端白名单与 service-start 路由语义。
- 老沙箱(pre-S1, services_json NULL → 读回全开)必须继续显示全部按钮。
- adopted(纳管)沙箱保持全开读回,行为不变。
- `app/services.toml` 是 `include_str!` 编进 app 镜像的:改后 app 镜像必须
  重建才生效;mgr 按 env-hash 复用镜像,同 hash 旧镜像不会自动重建——
  构建命令需明确列出(见 implement.md 验证节)。

## Acceptance Criteria

- [ ] AC1 创建页:pi Web 开启时关闭 VNC(或 pi),pi Web 自动一并关闭,
      且 pi / VNC 行显示依赖提示;重新开启 pi Web 时 pi + VNC 自动开启。
- [ ] AC2 新建最小沙箱(四服务全关)构建成功后,工作区树中该沙箱只显示
      Terminal 按钮与注册按钮入口;无 code-server / Chromium / pi / pi Web。
- [ ] AC3 最小沙箱 manifest 中 codeServer / vnc / piWeb 的
      `enabled` 为 false(localhost 探测不再误报);全开沙箱四按钮正常显示。
- [ ] AC4 最小沙箱网关 `GET /code-server/` 与 `/vnc/vnc.html` 不再返回
      其他沙箱的内容(路由未生成或 404/502,而非 200 代理 dev1)。
- [ ] AC5 存量全开沙箱(dev1)行为不回退:四按钮可见、code-server 可按需
      启动、Chromium / pi Web 可打开。
- [ ] AC6 `cargo fmt/clippy/test`(mgr、app)与 `tsc --noEmit && vite build`
      (mgr-web)全绿;composegen 单测覆盖路由按安装集裁剪与唯一别名。
