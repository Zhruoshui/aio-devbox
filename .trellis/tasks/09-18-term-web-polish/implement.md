# implement — term-web-polish (issue #21)

执行清单;技术细节以 prd.md 为准(帧格式/字体解析坑都写在 Requirements 里)。
单一实现者顺序执行,无并行分支,不需要 review gate 拆分。

## 前置

- [x] 依赖:`mgr-web` 安装 `@xterm/addon-webgl` `@xterm/addon-clipboard`
      `@xterm/addon-search` `@xterm/addon-web-links`(xterm 5.5 配套版)
- [x] GitHub issue:#21

## 后端(app/)

- [ ] `pty.rs::spawn_pty` 加 `cwd: Option<&str>` 参数:存在且为目录 →
      `CommandBuilder.cwd()`,否则(或 None)回退 `/root`(`std::fs::metadata`
      + `is_dir`)
- [ ] `terminal.rs::TermQuery` 加 `cwd: Option<String>`;`run_pty_session`
      透传给 `spawn_pty`
- [ ] `terminal.rs` teardown:取 exit code(`child.wait()` 返回值,u32 LE),
      断开前发 5 字节 Binary 帧 `[0x02, code_le_u32]`(正常退出 0 也发)
- [ ] `config.rs` Service/ManifestEntry 加可选 `cwd`;manifest.rs 透传
- [ ] `services.toml` agent 条目文档注释补 `cwd` 字段说明(暂不设默认值)
- [ ] `cd app && cargo check && cargo test`

## 前端(mgr-web/)

- [ ] `types.ts::ServiceEntry` 加 `cwd?: string`
- [ ] `paneUrl.ts::termWsUrl` 加可选 `cwd` 参数(encodeURIComponent)
- [ ] `XtermPane.tsx`:
  - [ ] `scrollback: 10000`(R1)
  - [ ] mount 时 `getComputedStyle` 解析 `--font-mono` 为具体字体串(R3 前置)
  - [ ] WebGL addon:open 后 load,`onContextLoss` → dispose 回退 DOM(R3)
  - [ ] clipboard addon(R2)
  - [ ] web-links addon(R4)
  - [ ] search addon + Ctrl+F 最小搜索条 DOM(输入框 + 上/下一个 + Esc)(R5)
  - [ ] onmessage 处理 0x02 帧:写 `● 进程已退出 (code N)` notice(R6)
- [ ] `cd mgr-web && npx tsc --noEmit && npm run build`

## 收尾

- [ ] spec `frontend/xterm-pane.md`:WS 协议段补 0x02 帧 + cwd 参数 +
      WebGL/字体解析契约
- [ ] 冒烟:`make mgr-up` 后 terminal / opencode pane 开用关重开
- [ ] commit + push + PR(link #21)
