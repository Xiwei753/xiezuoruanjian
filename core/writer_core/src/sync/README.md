# Sync 同步接口模块

本模块定义了外部调用同步功能的通用接口与核心抽象。实际的底层同步引擎和服务实现在 [sync_service](file:///home/xiwei/xiezuoruanjian/core/writer_core/src/sync_service/) 中。

## 模块内文件说明

- **[mod.rs](file:///home/xiwei/xiezuoruanjian/core/writer_core/src/sync/mod.rs)**：提供同步功能的统一入口点（如 [sync_workspace](file:///home/xiwei/xiezuoruanjian/core/writer_core/src/sync/mod.rs#L31) 函数）。目前该入口为占位 Stub 实现（返回 `NotImplemented` 错误）。
- **[engine.rs](file:///home/xiwei/xiezuoruanjian/core/writer_core/src/sync/engine.rs)**：定义了同步引擎的核心抽象接口，包含以下主要 trait 与数据结构：
  - [SyncEngine](file:///home/xiwei/xiezuoruanjian/core/writer_core/src/sync/engine.rs#L56)：定义 Git 操作的标准接口（初始化、设置远端、自动提交、推送、拉取）。
  - [TimeMachine](file:///home/xiwei/xiezuoruanjian/core/writer_core/src/sync/engine.rs#L64)：定义版本历史管理接口（获取提交历史、恢复及预览历史版本）。
  - [CommitRecord](file:///home/xiwei/xiezuoruanjian/core/writer_core/src/sync/engine.rs#L49) & [SyncError](file:///home/xiwei/xiezuoruanjian/core/writer_core/src/sync/engine.rs#L36)：定义同步操作的提交记录格式与错误类型类型。

## 同步状态与配置文件路径

所有同步状态和配置数据都保存在工作区的 `app-meta/` 目录下：

- **同步配置**：`app-meta/settings/settings.sync.json`（需要同步至云端）。
- **同步密钥**：`app-meta/sync/sync_secrets.local.json`（本地私有，不同步）。
- **本地同步状态**：`app-meta/sync/state.local.json`（存储本地设备 ID、上次同步时间、已知的本地文件 Hash 及时间戳等，本地私有，不同步）。

## 冲突处理策略与路径

冲突的判定与合并逻辑实现在 **[conflict.rs](file:///home/xiwei/xiezuoruanjian/core/writer_core/src/sync_service/conflict.rs)** 及 **[lww.rs](file:///home/xiwei/xiezuoruanjian/core/writer_core/src/sync_service/lww.rs)** 中：

- **活动冲突记录**：所有检测到且尚未解决的冲突将记录在 `app-meta/sync/conflicts.json` 中。
- **文件隔离策略**：
  - 对非正文的 JSON 元数据（如项目、分卷、章节元信息等），采用 **LWW（Last-Write-Wins，以时间戳+设备ID排序进行 Tie-breaker）** 或**三路语义合并**（三路合并逻辑在 [conflict.rs](file:///home/xiwei/xiezuoruanjian/core/writer_core/src/sync_service/conflict.rs) 中的 `semantic_merge_json`）。
  - 对用户创作的正文文本（如 `chapters/` 下的 `.md` 文件），一旦检测到双端修改冲突（`BothChanged`），系统**绝对不会覆盖**本地文件，而是将远端冲突版本另存为副本文件：
    - Git 引擎：另存为 `<original_path>.conflict.<timestamp>`。
    - GitHub API 引擎：另存为 `<original_path>.remote-conflict-<timestamp>`。
    - 并将冲突信息写入 `conflicts.json` 中，由用户手动处理。

## GitHub API 同步链路

当在移动端或受阻网络环境下不便使用原生 Git 时，系统会使用轻量级的 GitHub REST API 引擎进行同步：

- **链路核心实现**：
  - **[github_backend.rs](file:///home/xiwei/xiezuoruanjian/core/writer_core/src/sync_service/github_backend.rs)**：GitHub 远端仓库的 Backend 包装器。
  - **[github_api_client.rs](file:///home/xiwei/xiezuoruanjian/core/writer_core/src/sync_service/github_api_client.rs)**：基于 HTTP REST API 封装的低阶 API 请求客户端。
  - **[lww.rs](file:///home/xiwei/xiezuoruanjian/core/writer_core/src/sync_service/lww.rs)**：核心的 LWW + 三路正文冲突判定与同步动作编排算法。通过 `perform_lww_sync` 执行，拉取远端 `manifest.sync.json` 清单并与本地对比，自动下载/上传差异文件，安全处理冲突文件并回写新的清单。