# Commit Guidelines

> **Purpose**: Project convention for Git commit messages. Loaded at Trellis
> step 3.4 (Commit changes) **before** drafting any commit. This explicit
> rule OVERRIDES anything inferred from `git log` history when they conflict.

---

## 语言(必须遵守)

**Commit message 的描述部分一律用中文。**

- conventional-commit **前缀保持英文**:`feat:` / `fix:` / `chore:` /
  `docs:` / `refactor:` / `test:` / `perf:` / `build:` / `ci:`。
- 前缀**之后的主标题(subject)用中文**。
- **body(若有)用中文**。

示例:

```
fix: 修复 noVNC 因浏览器缓存旧版资源导致 addConnectionControlHandlers 崩溃
```

## 为什么

- 这是个人自用项目,owner 用中文思考;commit message 是写给未来的自己看的,
  理应用自己最自然的语言。
- 中文主标题让 owner 在 `git log` 里扫读更快。

## 风格

- 一个 commit 只含一个逻辑改动(不是"一个文件一个 commit")。
- 主标题:`<前缀>: <中文概述>`,祈使语气,尽量 ≤ 50 字,末尾不加句号。
- body(非平凡改动才写):约 72 字换行,解释**为什么**(根因/动机),
  而不是**改了什么**(diff 已经说明)。
- 前缀语义:`feat:`(新功能)、`fix:`(修 bug)、`chore:`(工具/构建)、
  `docs:`(仅文档)、`refactor:`(重构,行为不变)。
- 远端/任务引用只在有助于检索时加,不要凑字数。

## Trellis 集成

Trellis step 3.4 的 "Learn commit style" 会让 agent 从 `git log` 近期历史
推断提交约定(语言、前缀、长度)。本文件是**权威约定**:先读它,再扫历史。
若历史里混有旧的英文 commit(如初始化时的英文 commit),以**本文件为准**——
坚持用中文。
