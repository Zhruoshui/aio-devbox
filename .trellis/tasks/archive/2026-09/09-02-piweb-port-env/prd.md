# piWeb iframe URL 端口可配置:PI_WEB_HOST_PORT 环境变量打通

> GitHub issue: Zhruoshui/aio-devbox#3(已认领,assignee: taosi222)

## Goal

消除 piWeb iframe URL 中硬编码的宿主机端口 30141:当宿主发布端口不同
(`sbx ports --publish 30142:30141/tcp`、多实例部署)时,workbench iframe
仍指向本实例的 pi-web,而非宿主机上另一个实例。

## Background

- `app/services.toml:91`:`url = "http://{host}:30141/"` 端口写死。
- `web/src/panes/IframePane.tsx:21` 只替换 `{host}`,端口原样透传。
- `{host}` 是前端渲染期替换,本 issue 不动它;只把**端口**从静态配置变成
  跟环境走的 env 配置缝(与 `AIO_BUTTONS_FILE` / `AIO_MODELS_FILE` 同模式)。
- pi-web 侧无需改动(target 是 sandbox-net 内部探活;Host 校验信任
  IP 字面量;静态资源根绝对路径,origin 整体切换即可)。

## Requirements

1. **services.toml**:piWeb 的 `url` 改为
   `url = "http://{host}:{env:PI_WEB_HOST_PORT:30141}/"`,
   并补注释说明 `{env:VAR:default}` 语义。
2. **Rust(config.rs)**:新增通用 `expand_placeholders()`(展开
   `{env:VAR:default}`,env 缺失时用 default = 现行为,零破坏);
   在 `load_services()` 后对 builtin 列表展开一次(env 运行期不变,
   不做每请求展开);附单元测试(含缺失 env 走默认值、多处占位符)。
3. **main.rs**:在 `load_services()` 调用点应用展开(1 行)。
4. **docker-compose.yml**:app service 增加
   `environment: PI_WEB_HOST_PORT: ${PI_WEB_HOST_PORT:-30141}`,
   ports 映射改为 `"${PI_WEB_HOST_PORT:-30141}:30141"`。
5. **README**:补充两种用法说明(sbx 场景写 `.env`、裸 docker 场景
   `PI_WEB_HOST_PORT=30142 make up`)。

## Out of Scope

- `web/` 前端不改动(`{host}` 替换逻辑保持)。
- pi-web 容器侧/entrypoint/profile.d 不改动。
- gateway 子路径方案(已在 issue 中排除)。

## Acceptance Criteria

- [ ] `PI_WEB_HOST_PORT` 未设置时,manifest 中 piWeb 的 `url` 仍为
      `http://{host}:30141/`(零破坏)。
- [ ] 设 `PI_WEB_HOST_PORT=30142` 后,`GET /api/manifest` 返回的 piWeb
      `url` 为 `http://{host}:30142/`。
- [ ] `cargo test`(app crate)全部通过,含新增 expand_placeholders 单测。
- [ ] docker-compose 配置语法有效(`docker compose config -q` 或等效)。
- [ ] README 有新用法说明。

## Notes

- 已在 issue 中验证过:issue 描述与仓库现状一致(services.toml:91、
  IframePane.tsx 只换 host、compose 37 行固定映射)。
- 展开只对 builtin(services.toml)做;用户按钮(buttons.toml)不含
  该占位符,不在本次范围。
