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
*   **配置模型 (Settings):**
    *   `lib/domain/models/settings.dart`: 区分 `LocalSettings` (本机不同步设置) 和 `SyncableSettings` (随仓库同步的统一配置)。API 密钥等允许以明文存入同步配置以方便多端流转。
*   **服务接口 (Service Interfaces):**
    *   `lib/domain/services_interfaces/storage_service.dart`: `IStorageService` 接口，定义了底层的文件写入操作。
    *   `lib/domain/services_interfaces/sync_service.dart`: `ISyncService` 和 `IGitClient` 接口，定义了非破坏性的同步流程，特别强调了冲突处理机制。
    *   `lib/domain/services_interfaces/settings_service.dart`: `ISettingsService` 接口，定义了配置加载、保存及容灾处理。
    *   `lib/domain/services_interfaces/ai_provider.dart` & `ai_tool_executor.dart`: `IAIProvider` 接口，用于接入 AI 服务（如 DeepSeek）。包含 Function Calling / Tool Call 的严格权限分级底层。AI 的写操作被严格限制在 `ai/` 或草稿区，**绝对不允许自动修改用户正文**。
    *   `lib/domain/services_interfaces/correction_engine.dart`: `ICorrectionEngine` 接口，用于本地文本纠错功能。

### 2. 应用层 (`lib/application/`)
负责协调领域对象以执行实际的业务用例 (Use Cases)。通过接口将 UI 层和底层基础设施连接起来。

*   **状态与控制器 (Controllers):**
    *   `lib/application/controllers/settings_controller.dart`: 统一的设置状态管理（ChangeNotifier），供 UI 订阅并安全保存修改。
*   **后台任务 (Background Tasks):**
    *   `lib/application/background_tasks/file_write_queue.dart`: **极其关键的安全文件。** 实现了 `FileWriteQueue`。它通过防抖（debounce）和队列机制，强制所有的文件保存操作串行执行。这保证了在用户快速输入引发的频繁自动保存中，绝对不会发生并发写入导致的文件损坏。

### 3. 基础设施层 (`lib/infrastructure/`)
最底层。包含数据库、文件系统、网络请求等具体的技术实现。

*   **存储 (Storage):**
    *   `lib/infrastructure/storage/atomic_writer.dart`: **极其关键的安全文件。** 实现了 `IStorageService`。它首先将数据写入一个临时文件（`.tmp`），写入成功后，执行操作系统级别的原子 `rename()` 操作来替换原文件。如果在写入中途发生软件崩溃或断电，原文件依然安然无恙。
*   **数据库缓存 (Database Cache):**
    *   `lib/infrastructure/database/database_helper.dart`: SQLite 数据库连接的桩代码。勾勒了所需的表结构（`projects_cache`, `chapters_cache`），并明确了最关键的要求：整个数据库必须能随时被清空，并根据 `workspacePath` 里的文件原貌重新构建出来。

### 4. 表现/UI 层 (`lib/presentation/` & `lib/main.dart`)
目前包含了一个最小可行性（MVP）的三栏写作界面与设置面板。它严格遵循整洁架构：
*   **不接触文件系统**：所有的数据读写、删除、重命名全部委托给 `ChapterRepositoryImpl`、`WorkspaceService` 和 `SettingsController`。
*   **不直接依赖 SQLite**：UI 通过 `DatabaseHelper.getChapters` 获取章节列表，并且在任何文件变动（保存、删除、新建）后，都会指令底层重新从文件系统重建 SQLite 缓存（`rebuildCacheFromWorkspace`），以保证界面的数据永远和硬盘文件状态一致。
*   **包含基本操作**：新建章节、修改标题、未保存防误触提示、一键备份当前项目、安全删除（移入回收站并自动备份）。
*   **设置系统与免责提示**：在设置界面修改可同步设置时，使用专门的 Draft 状态处理。对于 API Key 这种明文存储的字段，UI 会固定展示免责声明而不采用阻断式的强制弹窗。

---

## 开发运行不会污染 Git 仓库

本项目的设计严格保障**代码（Code）与用户数据（Data）物理隔离**。

*   **默认保存路径：** 你的所有写作数据（正文、备份、回收站、SQLite 缓存）默认保存在操作系统分配的应用程序文档目录下（如 Linux/Windows/macOS 的 `Documents/writer_app_workspace`），**绝对不会**自动保存在你 `clone` 下来的 Git 源码目录中。
*   **为何这样做：** 将用户珍贵的文稿混入开源/闭源的软件源码仓库是极其危险的。未来用户的文稿应当通过专门的同步机制（如私有 GitHub Repo 挂载到工作区目录）来备份，而不是直接污染本源码仓库。
*   **被忽略的文件：** 项目中产生的所有构建缓存、环境变量配置 `.env`、API 密钥文件 `secrets.json` 均已在 `.gitignore` 中被全面拦截。
*   **如何验证：** 在开发过程中，你可以随时运行仓库内置的脚本来测试和检查当前环境：
    ```bash
    ./tool/check_clean_worktree.sh
    ```
    它会自动跑完格式化、静态分析、单元测试，并确保 `git status` 依然保持干净。

---

## 一键更新

你可以直接使用内置的一键更新脚本来安全地获取最新代码：

```bash
./tool/update_project.sh
```

**更新流程说明：**
- 脚本会自动使用 `git stash` 暂存本地改动，避免因为这些文件导致 `git pull` 被挡住而失败。
- 脚本拉取最新代码后，**不会自动恢复 stash 的内容**，以避免潜在的自动合并冲突。
- 你可以使用 `git stash list` 查看被暂存的内容。
- 如需恢复，请使用 `git stash pop` 手动恢复。
- 如果你没有修改源码，正常更新后最后输出的 `git status` 应该是“工作区干净”的状态。

---

## 快速开始

1. 确保你已安装 Flutter 开发环境（配置好 Linux 和 Android 目标平台）。
2. 运行 `flutter pub get` 获取依赖。
3. 运行 `flutter test` 验证核心领域逻辑。
4. 运行 `flutter run` （目前显示的是默认的 Flutter 计数器脚手架）。

### 更新日志
- 完成了一次稳定性审计和小修复。
- 对 `WorkspaceScreen` 进行了整理，抽取了 `_restoreEditorState` 和 `_debounceSaveLocalSettings` 函数。
- 增强了输入动画（`EditorInputAnimationOverlay`）的安全边界，例如：composing 状态时跳过动画，粘贴大段文本时不逐字动画，防止越界。
- 增强了光标定位的健壮性。
- 修复了设置保存防抖，分离了 `saveLocal` 和 `save` 方法。
- 修复了从作品页进入写作页的导航稳定性。
- 更新了 `.gitignore` 以防止临时文件或缓存进入代码仓库。
