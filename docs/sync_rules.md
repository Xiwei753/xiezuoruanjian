# 同步规则

本文档概述了共享核心的同步和冲突解决规则。
- 所有变更同步到私有 Git 仓库。
- 冲突存储为单独的冲突文件，绝不自动覆盖本地数据。

## 持续集成（CI）
- 默认的 GitHub Actions 工作流严格用于构建 Android debug APK。
- Linux 和桌面构建应由用户在本地机器上手动执行。

## 同步规则
- 数据同步遵循工作区中定义的严格白名单/黑名单配置。
- `app-meta/settings/settings.local.json` 和 `app-meta/sync/sync_secrets.local.json` 被列入黑名单，仅保留在本地。
