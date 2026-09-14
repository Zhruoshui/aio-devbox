# S3 镜像页增强：组合说明 + 删除 + 清理

父任务：09-10-mgr-web-ux-batch2（决策 D3；**依赖 S1**：组合清单含服务开关，
须 S1 的 envhash/构建改动先落地）。

## Goal

镜像页从只读列表升级为可管理：每镜像显示组合清单与体积，支持删除未引用
镜像与一键清理（含构建缓存）。

## Requirements

- R1 images 表增组合清单列（构建时写入：场景+版本+服务开关的可读描述）；
  旧镜像行回退显示 env_hash
- R2 镜像体积：list_images 响应附带 docker inspect 实时体积（查询失败
  显示 —）
- R3 单删：refcount=0 才可用（禁用态 title 显示原因），确认后 mgr 调
  docker rmi 连带 base/app/cs 镜像组；运行中构建任务引用时也禁用
- R4 一键清理：删除全部 refcount=0 镜像 + 构建缓存清理（builder prune），
  显示回收空间预估与结果
- R5 删除走既有 job 机制（异步 + 进度可见），失败不半删（组内顺序删除，
  失败中止并报告已删项）

## Acceptance Criteria

- [ ] AC1 镜像列表显示组合清单 + 体积；空数据回退正常
- [ ] AC2 refcount>0 的镜像删除按钮禁用且显示原因
- [ ] AC3 删除未引用镜像组成功且镜像页刷新；docker images 验证真删
- [ ] AC4 一键清理报告回收空间（镜像 + 缓存分列）
- [ ] AC5 cargo test + tsc 全绿

## Notes

- design.md + implement.md 在本任务 task.py start 前补全
