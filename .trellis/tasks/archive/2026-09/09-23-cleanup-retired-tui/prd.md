# 清理已退役 TUI 配置器及相关死代码

## 背景

`aio-config` crate 的 `tui` 子命令（ratatui 交互式场景勾选器）已被 web 管理界面（mgr-web EnvPicker）完全取代，不再使用。但 `aio-config` crate 的 lib 部分（scenario / manifest / gen）仍被 mgr 依赖，用于 Dockerfile.base 组装，**不可删除**。

## 范围

### 1. 删除 TUI 子命令实现
- `config/src/tui.rs` — 整个文件删除
- `config/src/main.rs` — 移除 `Cmd::Tui` 变体、`mod tui`、`tui::run()` 调用
- `config/Cargo.toml` — 移除 `ratatui`、`crossterm` 依赖
- `config/src/lib.rs` — 移除关于 tui 的注释
- `config/Dockerfile` — 移除关于 tui 的注释

### 2. 移除 Makefile 中的 `make config` 目标
- 删除 `config:` target
- 更新头部注释
- 保留 `gen` / `build-config`（build-base 依赖 gen）

### 3. 更新 scenario.toml 头部注释
- 23 个 scenario.toml 文件中的 `# Scenario metadata consumed by aio-config tui (listing) and gen` 改为去掉 tui 引用

### 4. 更新文档
- `README.md` — 移除 TUI 相关描述，改为说明场景选择通过 mgr-web 界面完成
- `README.zh-CN.md` — 同上
- `.trellis/spec/backend/directory-structure.md` — 更新 config/ 描述
- `.trellis/spec/frontend/component-guidelines.md` — 更新 EnvPicker 与 scenario.rs 的契约描述（去掉 TUI 引用）
- `.trellis/spec/backend/sandbox-mgr-ops.md` — 来源任务名更新

### 5. mise-poc/ 目录
保留（内含 FINDINGS.md 被 docs/offline-tool-install.md 引用，且是历史调研记录）。

## 不做

- 不删除 `config/` crate（lib 部分被 mgr 依赖）
- 不删除 `.aio/enabled.toml` 机制（gen 仍需要读取）
- 不删除 docs/Web-Prototype/ 和 docs/open-design/（设计参考文档）

## 验收

- [ ] `cargo check -p aio-config` 通过
- [ ] `cargo check -p aio-mgr` 通过
- [ ] `make gen` 正常工作（gen 不依赖 tui）
- [ ] grep 全仓无 `ratatui` / `crossterm` 残留
- [ ] grep 全仓无 `Cmd::Tui` / `mod tui` / `tui.rs` 残留
- [ ] README/Makefile 中无 `make config` 目标引用
