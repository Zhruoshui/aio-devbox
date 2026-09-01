# Statusbar 页脚可用性功能增强

## Goal

给 web 工作台页脚(`web/src/Statusbar.tsx`)增加实用功能,提升日常使用的信息密度与操作效率。
MVP 范围(用户已选定):**A1 布局持久化 + B1 系统资源监控 + A4 连接状态真实化 + A3 地址一键复制**。

## Background(代码勘察确认的事实)

- 页脚组件:`web/src/Statusbar.tsx`(57 行,纯展示,数据全走 props,无 fetch)。
- 现状左侧:静态绿色圆点 + "{n} 个服务可用"(i18n `statusAvail`)+ enabled 服务 id 串(mono);
  右侧:`window.location.host`(mono)+ "工作区卷已挂载" + 主题/语言切换按钮。
- 圆点是硬编码恒绿(`.dot` CSS),不反映任何真实健康状态。
- 后端(app/,axum)现有 API 仅:`GET /api/manifest`、`GET /api/term/ws`、`POST/DELETE /api/buttons`。
  无资源/版本/uptime API;Cargo.toml 无 sysinfo 类依赖。
- manifest 的 `enabled` 是 fetch 瞬间快照;前端仅在窗口 focus(2s 防抖)或手动刷新时重新拉取。
- golden-layout 布局不持久化:每次 reload 重建默认单 terminal 布局(App.tsx 注释明确此行为);
  golden-layout 自带 `saveLayout()`(ResolvedLayoutConfig 可 minify 成 JSON)。
- UI 偏好存 localStorage 是仓库既有惯例(`aio.theme` / `aio.lang` / `aio.sidebar.collapsed`)。
- i18n 为平铺字符串表(`web/src/i18n.ts`),新增文案需同步 zh-CN/en。
- docker-compose.yml **未设置任何资源限制**(无 mem_limit/cpus);app 容器 cgroup 为 **v2**(cgroup2fs)。
  即:容器内 /proc/meminfo = 宿主机视角;cgroup v2 `memory.current` = 容器自身用量视角。

## Requirements

### R1 布局持久化(A1)

- R1.1 工作区布局(分屏/标签/弹出窗内容)自动保存,刷新页面后自动恢复。
- R1.2 提供"重置布局"入口,一键回到默认单 terminal 布局。
- R1.3 存储位置:localStorage(遵循仓库 UI 偏好惯例,跨浏览器漫游不做)。

### R2 系统资源监控(B1)

- R2.1 新增后端 `GET /api/stats`,返回 CPU / 内存 / 磁盘用量数据。
- R2.2 数据视角:**仅容器自身**(cgroup v2:CPU/MEM;磁盘=工作区卷 `/home/gem` 的 statvfs)。
  用户决策:宿主机可能是 Windows/Linux,容器视角语义统一且跨宿主可移植。
- R2.3 页脚常驻显示三项资源的**纯文本紧凑读数**(无图表/详情面板),纯前端轮询,不新建 WS 通道。
- R2.4 后端异常/不可达时页脚资源区静默降级(整段隐藏,不遮挡其他页脚功能)。

### R3 连接状态真实化(A4)

- R3.1 页脚圆点反映后端真实可达性(取代恒绿):可达=绿,不可达=红/灰,文案联动。
- R3.2 状态判定复用既有 manifest fetch 通道 + 周期轮询,不新增探测端点。

### R4 地址一键复制(A3)

- R4.1 页脚 host 区块可点击,复制完整网关 URL 到剪贴板,带复制成功反馈。

### 通用约束

- G1 所有新增文案双语(zh-CN / en)。
- G2 页脚保持纯展示轻量组件形态;新增数据获取逻辑上移到 App.tsx 或独立 hook,不往 Statusbar 里塞 fetch(遵循现有"Pure presentation"注释契约)。
- G3 弹出子窗口(popout)不渲染页脚,维持现状。

## Acceptance Criteria

- [x] AC1(R1)分屏摆放多个 pane → 刷新页面 → 布局(含各 pane 内容与位置)完整恢复;点"重置布局"回到默认单 terminal。
- [x] AC2(R2)页脚显示 CPU/内存/磁盘三项实时读数,数值随轮询刷新且与容器内实际用量一致(抽查对比 `docker stats` / cgroup 文件)。
- [x] AC3(R2)停掉 app 后端或断开网络,页脚资源区优雅降级,圆点变红,恢复后自动回绿、数据恢复刷新。
- [x] AC4(R3)`window.location.host` 点击后剪贴板内容为完整 URL,有可见成功反馈。
- [x] AC5(G1)中英文语言切换后,所有新增页脚文案正确切换。
- [x] AC6 现有功能(主题/语言切换、服务清单、注册按钮、popout 窗口)无回归;`web/smoke-test.cjs` 通过。

## Out of Scope

- C 档大件:命令面板 Ctrl+K、通知中心/toast 历史、网络流量监控。
- A2(页脚服务 id 快捷启动)、A5(时钟/会话时长)、A6(全屏按钮)、A7(快捷键帮助面板)。
- B2(沙箱信息/版本/uptime 端点)、B3(逐服务健康周期探测)。
- 布局跨浏览器/跨设备漫游(后端存储)。
