# 技术设计:沙箱服务开关依赖展示与工作区按钮联动修复

对应 prd.md 的 Req 1–4。四个修复点相互独立可验证,合为一个任务交付。

## D1: ServicesPicker 级联关闭与依赖提示(Req 1)

现状 `set()` 对 "pi_web 依赖被关" 是静默 `return`(prd 已否决)。

改法(`mgr-web/src/pages/ServicesPicker.tsx`):

```ts
const set = (key, v) => {
  const next = { ...services, [key]: v };
  if (key === "pi_web" && v) {
    next.pi = true; next.vnc = true;          // 开 piWeb 拉起依赖(保留)
  } else if (key === "pi" && !v && next.pi_web) {
    next.pi_web = false;                       // 级联关闭
  } else if (key === "vnc" && !v && next.pi_web) {
    next.pi_web = false;                      // 级联关闭
  }
  onChange(next);
};
```

展示:pi Web 开启(`services.pi_web`)时,pi / VNC 行的 name 列追加
`svcDependedBy` 标签(新 i18n key,zh:"pi Web 依赖此服务" / en:"required
by pi Web"),复用现有 `.lock` 样式类。readonly(镜像详情)模式不渲染标签。

后端 `normalize_services` 的 400 保留:级联后客户端不会送出矛盾组合,
旧客户端(无级联)仍被后端拦截。

## D2: 工作区按钮按 installed_services 过滤(Req 2)

数据已在手:`Sandbox.installed_services`(`mgr-web/src/types.ts:86`,
mgr 列表 API 每个沙箱都带,含 pre-S1 全开读回)。当前 `buttonsOf()`
(`SandboxTree.tsx:107`)只看 manifest 的 `enabled` + ON_DEMAND 特例。

改动链(单一数据流:安装集过滤在树这一层做,manifest 依旧原样):

1. `SandboxTree.tsx` `buttonsOf(services, installed)` 新增第二参数
   `installed: Sandbox["installed_services"] | undefined`:
   ```ts
   const INSTALLED_GATE: Record<string, keyof InstalledServices> = {
     codeServer: "code_server", vnc: "vnc", pi: "pi", piWeb: "pi_web",
   };
   // undefined(老后端未返回)不过滤 —— 兼容降级
   visible = services.filter(s =>
     (installed === undefined || s.deletable ||
      !INSTALLED_GATE[s.id] || installed[INSTALLED_GATE[s.id]])
     && (s.enabled || ON_DEMAND_SERVICE_IDS.has(s.id)) && s.type !== "page");
   ```
   - Terminal 不在 gate 表 → 恒显示;用户自注册(deletable)恒显示;
   - ON_DEMAND 特例(codeServer)保留但被安装门控覆盖:未安装时连
     manifest 特例都进不去 → 未安装的 code-server 按钮消失(AC2 核心)。
2. 调用点传入 `sb.installed_services`(树内每沙箱节点)。
3. `NodeMenu` / 恢复布局的 `readPaneState` 路径不动:保存过的布局恢复
   出已卸载服务的 pane 是存量布局数据,不在本任务范围(关闭即消失的
   按钮不会再新开这种 pane)。

风险:manifest 里未安装服务的 `enabled` 误报在 D3 修复后已不出现,
D2 是**双保险**+同时解决"code-server 未安装仍显示"(ON_DEMAND 特例
不看 enabled 的问题 manifest 侧修不了,必须安装门控)。

## D3: manifest 探测目标改 localhost(Req 3)

`app/services.toml`:

```toml
[[service]]
id = "codeServer"
target = "localhost:8200"   # 原 app:8200
[[service]]
id = "vnc"
target = "localhost:6080"    # 原 app:6080
[[service]]
id = "piWeb"
target = "localhost:30141"   # 原 app:30141
```

依据:code-server / vnc 是 `network_mode: service:app` 的 sidecar,
与 app 共享网络命名空间 → app 内 `localhost:8200` 即本沙箱的
code-server;pi-web 由 `app/entrypoint.sh` 在 app 容器内自起监听
`0.0.0.0:30141`。localhost 探测**只反映本沙箱**,别名歧义天然消除。

注释同步更新(toml 头注释 + 各 target 行注释说明为何 localhost)。
探测实现 `is_web_reachable` 不动。

注意:services.toml 经 `include_str!` 进 app 镜像,mgr 侧按 env-hash
复用镜像 → 同 hash 沙箱需手动触发 app 镜像重建(见 implement.md)。

## D4: 沙箱网关唯一别名 + 路由按安装集裁剪(Req 4)

`mgr/src/composegen.rs`:

1. app 服务在 sandbox-net 增加唯一别名:
   ```yaml
   networks:
     sandbox-net:
       aliases:
         - sbx-{name}-app
   ```
   (aio-mgr-net 上已有 `sbx-{name}-piweb` 别名,同模式。)
2. `render_caddyfile(services)` 从无参改为接收 `db::Services`:
   - `services.code_server` 为 false → 不生成 `handle_path /code-server/*`;
   - `services.vnc` 为 false → 不生成 `handle_path /vnc/*`;
   - 保留的块 `reverse_proxy sbx-{name}-app:<port>`(原 `app:<port>`)。
   Caddyfile 从 `&'static str` 变 `format!` 字符串,`generate()` 传参。
3. 单测:全关 → 无 `/code-server/`、无 `/vnc/`、反代带唯一别名;
   全开 → 两路由齐、别名正确;cs-only → 只有 code-server 路由。
4. 旧实例兼容:已存在的沙箱目录不回写;沙箱重建/删除重建后自然生效。
   (mgr 重启沙箱不会重新生成 compose——这是既有语义,不在本任务扩大。)

为什么别名而不是改网关端口探测:网关与 app 同在 sandbox-net,别名
是 compose 原生能力,零新增机制;探测侧已由 D3 用 localhost 解决,
D4 只处理网关反代这一个残留消费点。

## 数据流总览(修复后)

```
创建页 ServicesPicker(级联) ──POST──▶ mgr normalize_services(400 防线)
                                            │ env.scenarios + services_json
                                            ▼
                            jobs.rs 构建(composegen: 别名/路由裁剪)
                                            │
              ┌─────────────────────────────┼──────────────────────────┐
              ▼                             ▼                          ▼
    沙箱 app services.toml         沙箱 gateway Caddyfile        mgr 列表 API
  (localhost 探测,只看本机)   (sbx-<name>-app 反代,      installed_services
              │                  未安装无路由)                │
              ▼                                                  ▼
        manifest.enabled                              SandboxTree buttonsOf
              └──────────────► 双保险合并 ◄─────────── installed 门控
                                     ▼
                     最小沙箱 = Terminal + 注册按钮
```

## 权衡记录

- **级联关闭 vs 锁定开关**:用户选定级联(一键到位,不留 400 死路)。
- **过滤放树层 vs manifest 层**:manifest 是 app 侧契约(还有沙箱直连
  消费者),installed_services 是 mgr 侧概念,树层合并是自然边界。
- **localhost vs 唯一别名探测**:两者都改——localhost 修探测源头,别名
  修网关反代;只做其一分别留下另一处跨沙箱歧义。
