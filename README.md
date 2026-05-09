# 写作软件 (Writer App)

这是一个极度追求稳定、本地优先、跨平台的长文写作软件。专为长期的生产力、数据安全和未来的扩展性而设计。

## 核心理念

1.  **绝对不丢稿：** 保护用户的创作是最高优先级。底层存储机制严格防止并发写入导致的损坏，并使用操作系统级别的原子重命名（Atomic Rename）。
2.  **本地优先与开放格式：** 唯一的数据源（Source of Truth）是本地文件系统中的开放格式文件（正文使用 Markdown，元数据使用 JSON）。SQLite 仅仅作为瞬时的索引和缓存使用，它可以随时从文件系统中完全重建。
3.  **整洁架构 (Clean Architecture)：** 严格的关注点分离。UI 层对文件系统和数据库一无所知。外部系统（如 AI 分析、Git 同步）被完全抽象隔离。

---

## 代码架构与文件指南

本项目严格遵守 **整洁架构 (Clean Architecture)** 原则。以下是 MVP（最小可行性产品）阶段引入的基础文件指南。这将帮助你一眼看懂代码结构，而无需通读整个仓库。

### 1. 领域层 (`lib/domain/`)
应用程序的绝对核心。包含纯粹的业务逻辑、实体对象和抽象接口。**该层对其他任何层或外部库（如 Flutter UI 框架或文件系统 API）没有任何依赖。**

*   **数据模型 (Models):**
    *   `lib/domain/models/project.dart`: `Project` 实体，代表一本小说或大型作品。使用不可变状态（`copyWith`）。
    *   `lib/domain/models/chapter.dart`: `Chapter` 实体，代表单个章节。包含 `contentHash` 字段，用于后续的同步追踪和冲突检测。
*   **存储库接口 (Repository Interfaces):**
    *   `lib/domain/repositories/chapter_repository.dart`: `IChapterRepository` 抽象接口，定义了如何保存章节以及如何重建缓存索引。它向更上层隐藏了与底层存储和缓存交互的复杂性。
*   **服务接口 (Service Interfaces):**
    *   `lib/domain/services_interfaces/storage_service.dart`: `IStorageService` 接口，定义了底层的文件写入操作。
    *   `lib/domain/services_interfaces/sync_service.dart`: `ISyncService` 和 `IGitClient` 接口，定义了非破坏性的同步流程，特别强调了冲突处理机制。
    *   `lib/domain/services_interfaces/ai_provider.dart` & `ai_tool_executor.dart`: `IAIProvider` 接口，用于接入 AI 服务（如 DeepSeek）。包含 Function Calling / Tool Call 的严格权限分级底层。AI 的写操作被严格限制在 `ai/` 或草稿区，**绝对不允许自动修改用户正文**。
    *   `lib/domain/services_interfaces/correction_engine.dart`: `ICorrectionEngine` 接口，用于本地文本纠错功能。

### 2. 应用层 (`lib/application/`)
负责协调领域对象以执行实际的业务用例 (Use Cases)。通过接口将 UI 层和底层基础设施连接起来。

*   **后台任务 (Background Tasks):**
    *   `lib/application/background_tasks/file_write_queue.dart`: **极其关键的安全文件。** 实现了 `FileWriteQueue`。它通过防抖（debounce）和队列机制，强制所有的文件保存操作串行执行。这保证了在用户快速输入引发的频繁自动保存中，绝对不会发生并发写入导致的文件损坏。

### 3. 基础设施层 (`lib/infrastructure/`)
最底层。包含数据库、文件系统、网络请求等具体的技术实现。

*   **存储 (Storage):**
    *   `lib/infrastructure/storage/atomic_writer.dart`: **极其关键的安全文件。** 实现了 `IStorageService`。它首先将数据写入一个临时文件（`.tmp`），写入成功后，执行操作系统级别的原子 `rename()` 操作来替换原文件。如果在写入中途发生软件崩溃或断电，原文件依然安然无恙。
*   **数据库缓存 (Database Cache):**
    *   `lib/infrastructure/database/database_helper.dart`: SQLite 数据库连接的桩代码。勾勒了所需的表结构（`projects_cache`, `chapters_cache`），并明确了最关键的要求：整个数据库必须能随时被清空，并根据 `workspacePath` 里的文件原貌重新构建出来。

### 4. 表现/UI 层 (`lib/presentation/` & `lib/main.dart`)
目前包含了一个最小可行性（MVP）的三栏写作界面。它严格遵循整洁架构：
*   **不接触文件系统**：所有的数据读写、删除、重命名全部委托给 `ChapterRepositoryImpl` 和 `WorkspaceService`。
*   **不直接依赖 SQLite**：UI 通过 `DatabaseHelper.getChapters` 获取章节列表，并且在任何文件变动（保存、删除、新建）后，都会指令底层重新从文件系统重建 SQLite 缓存（`rebuildCacheFromWorkspace`），以保证界面的数据永远和硬盘文件状态一致。
*   **包含基本操作**：新建章节、修改标题、未保存防误触提示、一键备份当前项目、安全删除（移入回收站并自动备份）。

---

## 快速开始

1. 确保你已安装 Flutter 开发环境（配置好 Linux 和 Android 目标平台）。
2. 运行 `flutter pub get` 获取依赖。
3. 运行 `flutter test` 验证核心领域逻辑。
4. 运行 `flutter run` （目前显示的是默认的 Flutter 计数器脚手架）。
