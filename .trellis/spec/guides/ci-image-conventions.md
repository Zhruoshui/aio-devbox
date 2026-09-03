# CI Image Pipeline Conventions

> **Purpose**: Hard rules for anything touching `.github/workflows/images.yml`,
> `Dockerfile.base` 生成链路,派生镜像 Dockerfile,或镜像标签/消费方式。
> 来源任务: 08-31-ci-image-pipelines(2026-08-31)。约定冲突时以本文 +
> `design.md` 为准,不以 `git log` 推断为准。

---

## 约定 1: gen 命令漂移锚定(最高风险耦合)

workflow 里的 gen 调用**逐字复刻 `Makefile` gen 目标**(当前 `Makefile:56-58`,
workflow `images.yml` 内有行号锚定注释)。CI 不走 `make gen` 是有意的——
其 `build-config` 前置依赖在 runner 上无缓存必重编 Rust。

**改 Makefile gen 行 → 必须同步 workflow**;改前先 grep:

```bash
grep -n "与 Makefile" .github/workflows/images.yml   # 找到锚定注释
```

## 约定 2: 派生镜像禁止硬编码 `FROM sandbox-base`

凡从 base 派生的镜像(app/code-server/未来新增)一律:

```dockerfile
ARG BASE_IMAGE=sandbox-base        # 全局 ARG,放第一个 FROM 之前
FROM ${BASE_IMAGE}
```

**Why**: CI 用 buildx docker-container 驱动(为了 `type=gha` 层缓存),该驱动
**看不见 daemon 本地镜像**——硬编码 `FROM sandbox-base` 在 CI 必失败。默认值
保证本地零回归;CI 注入 GHCR 全限定名。

配套排序约束: push 路径必须**先推 base 的 `:<variant>-<ref>` 固定标签**,
再构建派生镜像(它们的 `BASE_IMAGE` 指向该标签)。

## 约定 3: 场景知识不得泄漏进 CI

full 变体 = 预设 `scenarios = ["*"]` 经 gen 展开,**CI 脚本零场景枚举**。
always_on 排除规则唯一归属 `config/src/manifest.rs::expand`。

- 新增场景目录 → full 流水线自动纳入(零配置);
- 唯一手动跟进: 代表性 CLI 加进 workflow 的 full 探针清单(见
  scenario-authoring.md 的 checklist 末项)。

## 约定 4: 探针语义

镜像内工具抽查一律 `docker run --rm --entrypoint bash <img> -lc 'command -v …'`:

- `bash -lc` = 登录 shell 语义,顺带守护场景的 PATH 铁律(Rule 2);
- `command -v` 而非 `--version`(对无标准 version 旗标的工具稳健);
- mise 场景额外补一路非 login 抽查(`bash -c` 无 -l),守护 ENV + shims
  通道(code-server 非 login 终端、非交互子进程的可见性)。

## 约定 5: 标签方案(三处对齐)

| 镜像 | 浮动标签 | ref 标签 | v* 追加 |
|------|---------|----------|---------|
| sandbox-base/-app/-code-server | `:minimal` `:full` | `:<variant>-<ref>` | full 家族 `:latest` |
| sandbox-vnc | `:latest` | `:<ref>` | — |

- `make pull VARIANT=<v>` 消费**浮动标签**;ref 钉定靠手动 `docker tag`。
- 改标签方案须同步: workflow `gen_tags()` ↔ `Makefile pull` ↔ 双语 README。

## 约定 6: 权限与车道

- workflow 权限顶级别 `contents: read + packages: write`,不加别的;
- PR 车道全程零 push(所有 push/login 步骤 `if: push == 'true'`,prepare 对
  PR 恒置 false)——PR 引入 push 步骤时必须保持该门;
- `.env`/`gateway/secrets/`/`Dockerfile.base`(生成物)不入库;`.dockerignore`
  排除 `.env` + `gateway/secrets`。

## 遗留占位(已解决)

~~`REGISTRY_PREFIX ?= ghcr.io/<OWNER>` 占位~~ → 2026-08-31 仓库定为
`Zhruoshui/aio-devbox`,已替换为 `ghcr.io/zhruoshui`(Makefile + 双语 README
+ 技能 paths-and-offline 四处)。workflow 侧运行时从
`github.repository_owner` 小写化计算,无占位。
