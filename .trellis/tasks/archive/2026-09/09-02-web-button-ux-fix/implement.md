# Implement — 09-02-web-button-ux-fix

## Step 1 后端：probe 端点

- [ ] `app/src/routes/buttons.rs`：新增 `GET /api/buttons/probe?port=<N>`
  - 参数校验：1-65535 整数且 ≠8088，否则 400（与 POST 校验同一套规则，抽共用函数或复用既有校验）
  - 探活：复用 `config.rs` 里 manifest 用的 TCP-dial 语义（同一超时常量），listening → `{"listening":true}`，否则 `{"listening":false}`
  - 挂路由（`routes/mod.rs` 如需）
- [ ] 单测：活端口 / 死端口 / port=0 / port=8088 / 非数字 各一例
- 验证:`cargo test -p aio-app --manifest-path app/Cargo.toml`

## Step 2 前端：侧栏分组去重

- [ ] `web/src/Sidebar.tsx` web 组过滤器:`type === "web" && !s.deletable`
  - 注释说明「deletable 归 custom 组，避免双份」
- 验证:`cd web && npm run build`

## Step 3 前端：注册表单探活提示

- [ ] `web/src/RegisterDialog.tsx`:
  - web 态、端口格式合法（1-65535 且 ≠8088）时，防抖 500ms 调 `/api/buttons/probe?port=N`
  - 结果态:`unknown`(初始/格式错/请求中) / `listening` / `dead`
  - 端口字段下新增提示行:`dead` → 警告文案(黄),`listening` → 正向文案(绿);格式错误时不渲染探活提示(错误提示优先)
  - 请求竞态防护:只采纳最后一次请求的结果(useEffect cleanup 或 AbortController)
  - agent 态不受影响
- [ ] `web/src/i18n.ts` 中英文案:`probeListening` / `probeDead`(+ 变量插值端口号)
- [ ] `web/src/styles.css` 提示行样式(复用/扩展 hint 或 field-error 类)
- 验证:`cd web && npm run build`

## Step 4 集成验证(本地 compose 栈)

- [ ] 重建 app 镜像 + `make up`(或 `docker compose up -d --build app`)
- [ ] API 层:
  - `GET /api/buttons/probe?port=8000`(活)→ `{"listening":true}`
  - `GET /api/buttons/probe?port=9123`(死)→ `{"listening":false}`
  - `?port=0` / `?port=8088` / `?port=abc` → 400
  - 死端口 POST 注册仍 201
- [ ] manifest 回归:注册→manifest→删除链路无回归
- [ ] UI 层(提示用户浏览器验证):分组只出现一次;输入 8080 出警告、8000 出正向提示

## Step 5 收尾

- [ ] 验证记录追加到本文件末尾
- [ ] spec 更新(`.trellis/spec/backend/api-contracts.md` 补 probe 端点)
- [ ] commit

## 关键验证命令

```bash
cargo test -p aio-app --manifest-path app/Cargo.toml
cargo clippy --manifest-path app/Cargo.toml
cd web && npm run build
docker compose up -d --build app
curl -s -u admin:admin "http://localhost:8080/api/buttons/probe?port=8000"
```

## 回滚点

- 全部为增量改动(新端点 + 前端过滤/提示),单 commit 可整体 revert。

---

## 验证记录(2026-09-02)

- `cargo test`(rust:1-bookworm 容器):**237 passed, 0 failed**(新增 probe 3 例:活端口/死端口/非法矩阵)。
- `cargo clippy --all-targets`:21 条警告,与改动前基线完全一致(git stash 对比),改动文件零新增。
- `npm run build`(node:20, tsc --noEmit + vite):零错误。
- 部署:`docker compose up -d --build app` 成功;新静态资源确认含 probe-hint CSS + probeDead 文案。
- API 集成(经 gateway basic-auth):
  - `GET /api/buttons/probe?port=8000`(demo server 在跑)→ 200 `{"listening":true}`
  - `?port=9123`(无服务)→ 200 `{"listening":false}`
  - `port=0/8088/abc/-1/65536` → 全部 400
  - 死端口 POST 注册仍 201(AC3),manifest `enabled=false`,删除 204
- AC1 数据层验证:manifest 的 `deletable` 字段驱动前端分组,内置 web 服务 deletable=false(归 Web 工具),用户按钮 deletable=true(归自定义)。
- UI 层(AC1 分组渲染 / AC2 提示三态)待用户浏览器确认。
