# 离线软件包仓库(npm/python 镜像服务 + cargo vendor)

## Goal

让离线机(`docker load` 部署的那台)上的开发流程能继续 `npm install` / `uv pip install` /
`cargo build`,不依赖外网 —— 通过**可插拔的 `mirror` profile 外挂容器**(与 code-server/vnc
同模式)提供 npm/python 包的本地镜像服务;cargo 用 vendor 目录方案(不建服务)。

背景:本仓库已具备离线整机分发(`make save`/`make load`:镜像 + .env + hash)。镜像覆盖
"工具",共享卷覆盖"用户数据",但**项目依赖**(npm/pypi/crates)在离线机上无来源。本任务
补上这最后一环。

## Requirements

### R1 npm 镜像(Verdaccio)

- `mirror` profile 内跑 Verdaccio,作为 npm registry 的**回源代理**(pull-through cache)。
- 联网机:日常 `npm install` 走它 = 透明代理,自动 warm 缓存。
- 离线机:同一服务出缓存内容;未命中包报错(不挂起、不静默装错)。
- 缓存数据在独立命名卷 `aio_pkgs`(不进镜像 —— 镜像装服务,卷装数据,同 aio_workspace 哲学)。

### R2 python 镜像(devpi-server,按需回源)

- `mirror` profile 内跑 **devpi-server**(自建 ~6 行 Dockerfile,无官方镜像)作为
  PyPI 的 on-demand mirror(root/pypi 索引)—— 与 Verdaccio 同模型:联网时日常
  安装即自动 warm,离线时出缓存,未命中明确报错。
- 另提供**锁文件驱动的主动预热**(防"装过才缓存"的遗漏):按项目 `uv.lock`/requirements
  闭包批量过一遍 devpi 使其入缓存。
- 离线机:uv/pip 以 `http://devpi:3141/root/pypi/+simple/` 为 index 正常安装。
- (研究结论:pypiserver 官方镜像更省事但**无回源**,warm 全靠脚本纪律,降为备选;
  bandersnatch 全量镜像 TB 级,out of scope。)

### R3 cargo(vendor,不建服务)

- 固化 `cargo vendor` 流程为脚本 + 文档(offline-install-guide 方法 D 的产品化):
  联网机 vendor 打包 → 传目录 → 离线机 `--offline` 编译。
- 不做 crates.io sparse 索引镜像(panamax 失修、全量过重,明确 out of scope)。

### R4 可插拔(用户点名的要求)

- 形态必须与 code-server/vnc 一致:compose `profiles: [mirror]`,`make up PROFILES="... mirror"`
  即挂载;不启用 = 完全不存在,零 footprint。
- WebUI 侧边栏按钮随 profile 有无自动显隐(TCP 探测,同 codeServer/vnc)。

### R5 客户端接线(带降级)

- 终端面板里能方便地把 npm/uv 指到镜像(目标地址用 sandbox-net 服务名,如
  `http://verdaccio:4873`)。
- **镜像服务不在时不得破坏正常联网使用**(不能无条件 export registry 指向不存在的 DNS 名)。

### R6 分发闭环

- `make save` 的镜像集可含 mirror 服务镜像;`aio_pkgs` 卷提供独立的 tar 打包/恢复目标
  (`mirror-save` / `mirror-load`),并写进 offline-install-guide 的整机分发章节。

## Constraints

- 禁止全量 registry 镜像(npm/PyPI 均 TB 级);只做回源缓存 + 锁文件预热。
- 不新增对外暴露端口:一切经 sandbox-net;用户浏览器访问经 caddy 子路径。
- mirror 镜像须可离线 `docker load`(加入 SAVE_IMAGES 或文档说明)。
- 遵守现有分层规则:服务容器不 FROM sandbox-base(纯服务面,同 vnc 哲学);客户端接线脚本
  除外(需烘进 base,见 design)。

## Acceptance Criteria

- [ ] AC1 `make up PROFILES="code-server,vnc,mirror"` 后,verdaccio + pypi 容器 Up,
      侧边栏出现镜像面板按钮;不带 mirror profile 的 `make up` 按钮不出现。
- [ ] AC2 联网:app 容器终端里以镜像为 registry 执行一次真实 `npm install`(小包),
      Verdaccio web 面板可见该包;`aio_pkgs` 卷里出现缓存文件。
- [ ] AC3 联网:预热脚本对一个含 `uv.lock` 的示例项目走 devpi 完成闭包下载,
      devpi 缓存/web 面板可见这些包。
- [ ] AC4 模拟离线:阻断上游可达性(或验证缓存命中路径)后,`npm install <已缓存包>` 与
      `uv pip install --index http://devpi:3141/root/pypi/+simple/ <已缓存包>` 均成功;
      未缓存包得到明确报错。
- [ ] AC5 cargo vendor 脚本:联网机生成 vendor 目录,离线 `cargo build --offline` 通过
      (用 offline-install-guide 既有方法验证)。
- [ ] AC6 镜像 profile 未启用时,mirror 不在 PATH/DNS 上,`npm install` 走默认 registry 正常。
- [ ] AC7 `make save`(含 mirror 镜像)+ `mirror-save`(卷 tar)产物清单与恢复步骤写入
      docs/offline-install-guide.md;`mirror-load` 恢复后 AC2/AC4 复测通过。

## Out of scope

- apt 镜像(apt-cacher-ng);crates.io 全量/索引镜像;npm/PyPI 全量镜像;
  mirror 数据的加密与多环境同步策略。
