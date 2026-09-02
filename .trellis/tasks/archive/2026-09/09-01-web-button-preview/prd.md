# 支持用户注册 web 类型按钮(自定义 dev server 端口预览)

来源:issue #1(Zhruoshui/aio-devbox),分支约定 `feat/andes`(已满足),完成后向 `main` 提 PR(`Closes #1`)。

## Goal

用户可在侧边栏 `+` 表单注册 **web 类型**的自定义按钮,指向自己在工作区终端里起的任意 dev server(如 vite dev :5173);点击后在 tab 中以 iframe 形式打开预览,显隐语义与内置 code-server / vnc 按钮一致。

## Requirements

### R1 后端注册 API 扩展

- `POST /api/buttons` 接受 `type`(`"agent"` 默认 / `"web"`)与 `port`(web 必填):
  - `type=agent`:必须提供非空 `cmd`,`port` 忽略(维持现状)。
  - `type=web`:必须提供合法 `port`(1–65535),`cmd` 非必填;非法端口返回 400。
- 持久化到 `buttons.toml`(workspace 卷,存活于 recreate、跨浏览器共享),旧文件(无 port 字段)必须向后兼容读取。
- `GET /api/manifest` 正常下发 web 按钮:`type=web`、`url=/preview/<port>/`、`deletable=true`。
- `DELETE /api/buttons/:id` 对 web 按钮同样生效(现有逻辑应天然覆盖,需验证)。

### R2 动态端口预览反代

- axum(app)侧新增 `/preview/<port>`、`/preview/<port>/`、`/preview/<port>/<path>` 动态反代到 `127.0.0.1:<port>`(dev server 与 app 共享 netns,终端内起的 server 天然可达)。
- HTTP 全方法转发;SSE / 流式响应不被缓冲断裂。
- WebSocket 升级请求原样转发(vite HMR 等),双向通路打通。
- 上游不可达时返回 502(与 reserved seam 同语义);端口非法(非数字/0/8088 自环)返回 404 级别的快速失败,不反代。
- gateway(Caddy)零改动:catch-all 已把 `/preview/*` 交给 axum。

### R3 前端注册表单

- `+` 注册表单支持选择按钮类型(Terminal 命令 / Web 端口预览):
  - agent 类型:现有 label + cmd 表单不变。
  - web 类型:label + 端口输入(替换 cmd 字段),客户端校验端口数字范围,8088 提示拒绝。
- 注册成功后刷新 manifest,按钮按 TCP 探活结果显隐(有监听出现、无监听隐藏,与内置 web 按钮同语义)。
- 中英文文案齐全(zh-CN / en)。

### R4 回归约束

- 既有 agent 类型注册行为不回归(label/cmd 校验、slug/id 去重、原子写、删除)。
- 内置按钮(code-server / vnc / pi-web)manifest 行为不回归。
- 既有 Rust 单测全绿,新增路径有单测覆盖。

## Constraints

- 不改 gateway/Caddyfile、docker-compose 拓扑(方案 2 的前提)。
- buttons.toml 是用户可直接编辑的文件:字段增删必须向后兼容(缺 port 的 agent 按钮照常工作)。
- 信任模型不变:注册按钮的用户已有完整 terminal,无 allowlist 需求;仅做形状校验与自环防护(8088)。
- 已知限制需写进 README:root-absolute 资源的 app(vite 默认配置)在子路径下需上游配合(`base` + `server.hmr.path` 指向 `/preview/<port>/`);反代层只保证 WS/SSE 传输不断裂。

## Acceptance Criteria

- [ ] `POST /api/buttons` 接受 `type: "web"` + `port`,持久化到 buttons.toml 并经 `GET /api/manifest` 下发(url=/preview/<port>/,TCP 探活);
- [ ] 前端 `+` 注册表单支持选择按钮类型(agent / web),web 类型填端口而非命令;
- [ ] 端口有服务监听时按钮出现、无监听时隐藏(TCP 探活,与内置 web 按钮同语义);
- [ ] 点击按钮在 tab 中打开预览;WS / SSE 页面不因反代断裂(用 WS echo / SSE 服务实测);
- [ ] `DELETE /api/buttons/:id` 对 web 按钮同样生效;
- [ ] 既有 agent 类型注册行为不回归(含单测);`cargo test`、前端 build 全绿;
- [ ] README(中英)更新:移除 "user-registered buttons are terminal+command only" 缺口描述,补充 `/preview/<port>/` 用法与 vite 子路径配置说明。

## Notes

- Issue 明确的两个候选方向中选 **方案 2(axum 自建反代)**:dev server 在 app netns 内,axum 直达 127.0.0.1:<port>;gateway 侧方案需 Caddyfile 正则捕获端口做动态 upstream,复杂且收益为零。见 design.md §2。
