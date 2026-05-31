# writer_core/src — Rust 核心源码目录

本目录包含 Rust 核心业务的全部模块实现，它是跨平台系统的核心逻辑引擎。

## 核心子模块介绍

- [workspace/](file:///home/xiwei/xiezuoruanjian/core/writer_core/src/workspace/)：管理工作区的逻辑（目录规整、安全性检查、`workspace_manifest.json` 维护）。
- [project/](file:///home/xiwei/xiezuoruanjian/core/writer_core/src/project/)：维护项目、卷和章节的数据层。处理章节文件的创建、防空覆盖安全保存（防止网络同步或极端意外覆盖用户辛勤产出的稿件）。
- [sync/](file:///home/xiwei/xiezuoruanjian/core/writer_core/src/sync/)：包含底层 Git 和基于 GitHub REST API 的网络同步机制，并制定冲突文件的非覆盖存储策略。
- [settings/](file:///home/xiwei/xiezuoruanjian/core/writer_core/src/settings/)：应用设置和用户偏好的持久化 Schema 管理。
- [formatting/](file:///home/xiwei/xiezuoruanjian/core/writer_core/src/formatting/)：提供不破坏正文纯文本的一键排版和格式化算法。
- [api/](file:///home/xiwei/xiezuoruanjian/core/writer_core/src/api/)：提供给 UniFFI (Android 侧) 和 CXX/Qt (Linux 侧) 消费的统一聚合 API。

## 开发原则

- 每一部分改动都需要对齐相应模块的技术路线及单元测试（`cargo test`）。
- 始终以高内聚、低耦合的接口隔离方式设计各个子模块。