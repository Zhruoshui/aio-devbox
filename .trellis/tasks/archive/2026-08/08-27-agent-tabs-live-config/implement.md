# Implement — 08-27-agent-tabs-live-config

> 顺序:后端(库→渲染→路由)→ 前端 → 容器实测。每步末尾的验证命令必须绿
> 才进下一步。回滚点 = 每个 commit 前的 git checkpoint(本任务整体单 commit,
> 中间以 cargo test / npm build 为门)。

## 后端

- [x] 1. `store.rs`:重构 `import_from_pi` 抽出 `map_pi_provider(key, value)`
      (sanitize_id + name 回填,行为不变,现有 28 测试须保持绿)。
- [x] 2. `store.rs`:新增 `import_from_opencode(path, current) -> ImportResult`
      + fragment→ProviderEntry 适配(design §3:baseURL 缺失入 skipped、npm
      反推 api、models name 映射、headers/apiKey 直映射)。
- [x] 3. `store.rs` tests:import_from_opencode 全矩阵(正常/anthropic 反推/
      baseURL 缺失/幂等 skip/NotFound/Corrupt/json5 注释 fixture)。
      验证:`cargo test -p app models::store` 绿。
- [x] 4. `render/common.rs`:`ProviderPatch {name?, baseUrl?, apiKey?, api?}`
      (camelCase,全 Option)。
- [x] 5. `render/pi.rs`:`edit_pi_provider` / `delete_pi_provider`(Value 容错
      读 + 键级操作 + backup_write_verify_json;delete 级联清 settings
      defaultProvider/defaultModel)。tests:字段合并保留兄弟/自身 models/
      未知键;delete 两种 settings 分支。
- [x] 6. `render/opencode.rs`:`edit_opencode_provider` / `delete_opencode_provider`
      (json5 读 + pretty JSON 写 + delete 前缀命中清顶层 model)。tests:json5
      注释 fixture、其他键保留、前缀两分支。
      验证:`cargo test -p app models::render` 绿。
- [x] 7. `mod.rs`:`read_live` 扩展(pi providers 摘要 Value 容错提取 +
      opencode provider 对象摘要 + api 反推;两文件独立,损坏降级不报错)。
      新增 mod.rs 首个 tests mod:read_live 全矩阵(design §5)。live 摘要结构
      `LiveProviderSummary`(serde camelCase)或 json! 构造,二选一以实现简洁
      为准,但字段名对齐前端 `LiveProviderSummary`。
- [x] 8. `mod.rs` + `main.rs`:三条新路由(PUT/DELETE
      `/api/models/agents/:agent/provider/:id`、POST
      `/api/models/agents/:agent/sync`;Agent 解析复用,非 incremental → 400;
      sync 持 models_lock;edit/delete 走 ApplyResult 通道)。
      验证:`cargo test -p app models` 全绿(原 173 + 新增)。
- [x] 9. `cargo clippy -p app` 无新告警;`cargo fmt` 过。

## 前端(`web/src`)

- [x] 10. `types.ts`:`LiveProviderSummary` + `AgentLive.providers` +
       `decodeAgents` 容错透传 + `liveMatchState(agent, live, assignment)`。
- [x] 11. `i18n.ts`:新增键(zh+en 成对):maLiveSection/maLiveEmpty/maLiveNotInstalled/
       maLiveCurrent/maModelsCount/maSyncToLib/maSyncOk/maEditLive/maDeleteLive/
       maConfirmDeleteLive/maPickModel/maNoModelsInProvider/maApiKeyKeepHint 等,
       命名对齐现有 ma* 前缀。
- [x] 12. `LiveProviderList.tsx`(新):折叠/展开行 + 当前默认徽标 + 同步/编辑/
       删除(design §4.2);`styles.css` 增 `.ml-live-*`(对齐 preset 卡片语言,
       徽标复用 .ml-badge*)。
- [x] 13. `AgentTabs.tsx`:readback 单行 → live 匹配徽标 + LiveProviderList;
       模型 select → ModelPicker 触发器 + 展开面板(design §4.3;未安装隐藏
       live 区,预写行为不变)。
- [x] 14. `ModelsPane.tsx`:syncLiveProvider / deleteLiveProvider /
       editLiveProvider handlers(结果进 applyResult 面板 / agentSaveMsg;
       操作后 fetchAgents + fetchConfig)。
       验证:`npm run build` 干净(tsc strict 门)。

## 容器实测(AC 对账)

- [x] 15. 重建 + force-recreate **app + vnc + code-server**(netns 联动,
       见 journal R1 教训),确认侧车恢复。
- [x] 16. API 实测(curl localhost:8088):
       - GET /api/models/agents 的 pi/opencode providers 与容器内真实
         `~/.pi/agent/models.json` / `~/.config/opencode/opencode.jsonc` 抽样
         一致(AC1/AC2 前半);
       - 牺牲 provider(先 .aio-bak 备份)走 edit → 键级保留验证 → delete →
         悬空默认清理验证 → 复原;sync 单个/幂等重放;
       - 分配流:选供应商 → ModelPicker 选模型 → 保存 → apply → live 回读
         反映新默认(AC3)。
- [x] 17. CDP 截图(vnc 容器 chromium :9222,R1 同款 stdlib WebSocket 客户端):
       pi / opencode 页签 live 列表、编辑展开、ModelPicker 展开、未安装态
       (opencode 若未装)各一张。
- [x] 18. `npm run build` + `cargo test -p app models` 终态绿(AC5)。

## 收尾

- [x] 19. spec:`model-config-guide.md` 路由表 +3、live 回读段、import_opencode
       适配;frontend spec(component-guidelines 或对应文件)同步
       LiveProviderList/ModelPicker 接线。
- [x] 20. journal 追加;父任务 prd.md 勾 R2/R3;task.py finish;单 commit
       `feat: pi/opencode 页签 live 配置管理 + 供应商模型列表复用(R2+R3)`。
