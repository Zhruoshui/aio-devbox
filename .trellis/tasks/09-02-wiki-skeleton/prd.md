# Wiki 骨架:docs/ 为源 + GitHub Action 同步

## Goal

为本项目建立 GitHub Wiki,内容源放在仓库内 `docs/wiki/`,通过 GitHub Action 自动同步到 `Zhruoshui/aio-devbox.wiki`。本次只搭骨架(目录结构 + Home/_Sidebar + TODO 占位页),页面正文后续填充。

## Background

- 主仓库:`https://github.com/Zhruoshui/aio-devbox`(public,`has_wiki=true`)。
- Wiki 是独立 git 仓库 `<repo>.wiki.git`,当前不存在——需用户在网页上点一次 "Create the first page" 初始化。
- `GITHUB_TOKEN` 配合 `permissions: contents: write` 即可推送 wiki(经社区 action 验证),不需要 PAT。
- 选用 `Andrew-Chen-Wang/github-wiki-action`(活跃维护,支持自定义 path)。

## Requirements

- R1: 新建 `docs/wiki/` 目录,包含骨架页面(Markdown):
  - `Home.md` — Wiki 首页:项目简介 + 页面导航
  - `_Sidebar.md` — 侧边栏导航(GitHub wiki 自动识别该文件名)
  - 占位页(TODO 正文):`Architecture.md`(架构总览)、`Scenarios.md`(场景配置)、`Offline-Bundle.md`(离线分发)、`FAQ.md`(常见问题)
- R2: 新建 `.github/workflows/publish-wiki.yml`:
  - 触发:push 到 main,paths 过滤 `docs/wiki/**` 与 workflow 自身
  - 权限:`contents: write`;concurrency 防并发推送
  - 使用 `Andrew-Chen-Wang/github-wiki-action@v4`,`path: docs/wiki`
- R3: README(双语)的文档区域补充指向 wiki 的链接(一句话,不重写 README)。
- R4: 页面内链接使用 wiki 内部相对链接格式(页名,不带 .md)。

## Constraints

- 不移动/修改现有 `docs/` 其他文件(offline-install-guide.md 等保持原位)。
- workflow 首次推送会失败,直到用户在网页初始化 wiki——需在验收时明确告知用户此手动步骤。

## Acceptance Criteria

- [ ] `docs/wiki/` 包含 R1 列出的 6 个文件,链接互通,无死链
- [ ] `publish-wiki.yml` 语法有效(actionlint 或人工核对)
- [ ] README 双语各有 wiki 链接
- [ ] 用户执行网页初始化后,push 到 main 可让 workflow 成功同步(手动步骤,由用户配合验证;本次 PR 内至少完成 workflow lint 与内容自查)

## Notes

- 轻量任务,PRD-only。
- 后续内容页(架构/场景/离线 bundle)不在本任务范围。
