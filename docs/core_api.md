# Core API

`writer_core` Rust 库的跨平台暴露边界已经拆成四层：

- Core domain：`workspace`、`project`、`volume`、`chapter`、`settings`、`sync`、`starmap`、`writing_stats` 等模块负责真实业务和文件 I/O。`mind_map` 为 legacy 模块（仅迁移兼容）。
- Facade：`facade::WriterCore` 聚合内部业务能力，是 **Core 内部协调层**；它不承诺作为平台稳定 DTO 边界，也不应该被外部直接调用。
- Core API：`api::WriterCoreApi`、`api::types`、`api::error`、`api::envelope` 是**平台稳定入口**，面向 UniFFI、Linux binding、Android JNI 和未来前端。Android JNI 只能调用 `WriterCoreApi`。
- UniFFI adapter：`app_service::WriterAppService` 只保留 Android 兼容的对象名和方法签名，负责接收 UniFFI 参数并委托 `WriterCoreApi`。

> `sync` 是唯一正式同步模块（已合并原 `sync_service`）。同步配置、密钥、状态、诊断和实际同步均在此模块中实现。

`api.udl` 只是 UniFFI 绑定声明，用于生成 Kotlin/外部语言桥接代码；业务 API 的事实来源是 Rust `api/` 模块及其文档边界，不是 UDL 文件本身。图谱写入类 UniFFI 方法必须接收 DTO，不允许在 `WriterAppService` 或 `WriterCoreApi` 接收 JSON 字符串后再解析。

## Core API 模块

- `core/writer_core/src/api/types.rs`：平台稳定 DTO，如 `ProjectDto`、`VolumeDto`、`ChapterMetaDto`、`ChapterContentDto`、`ChapterSaveReceiptDto`、`RecentEditDto`、`LocalSettingsDto`、`SyncConfigDto`、`SyncStateDto`、`SyncResultDto` 等。
- `core/writer_core/src/api/error.rs`：平台稳定错误 `WriterError`，集中从 `crate::error::Error` 映射，保留 `Io`、`Json`、`InvalidWorkspace`、`ProjectNotFound`、`VolumeNotFound`、`ChapterNotFound`、`EmptyOverwriteBlocked`、`NotImplemented`、`RefuseToDeleteWorkspaceRoot`、`InvalidDeleteTarget`、`Other` 语义。
- `core/writer_core/src/api/envelope.rs`：跨平台标准 `ResultEnvelope<T>`，统一序列化 `success`、`data`、`errorCode`、`userMessage`、`rawError`、`warnings`、`changedPaths`、`changedEntities`。平台端只能根据 `success` / `errorCode` / `userMessage` 分支，不能解析 `rawError` 猜错误。
- `core/writer_core/src/api/service.rs`：`WriterCoreApi` 持有 workspace path，统一封装 `facade::WriterCore` 调用，返回 API DTO / `WriterError`，不依赖 Android、Linux、QML 或 UniFFI。

## Editor Core

`core/writer_core/src/editor` 是平台无关的编辑器语义层。当前已提供：

- `EditorCursor { index }`：UTF-8 byte offset 光标位置，会夹到字符边界。
- `EditorSelection { anchor, head }`：选区锚点与活动端。
- `EditorChange::Insert { index, text }` / `EditorChange::Delete { index, text }`：正文变更。
- `EditorTransaction { old_text, new_text, changes, old_selection, new_selection, cause, should_animate }`：统一编辑事务。
- `EditorAnimationEvent { id, kind, range_start, range_len, text, old_cursor, new_cursor, duration_ms }`：平台 renderer 可消费的动画事件描述。
- `EditorEngine::create_transaction(...)`：从旧文本、新文本、选区和原因生成事务。
- `EditorEngine::animation_events(...)`：从事务生成插入、删除和光标动画事件。

该层不绘制、不处理输入法、不依赖 Qt/Android；Desktop 和 Android 后续必须消费该语义，不能各自猜 diff。

`writer_core` 当前稳定能力包括：

- `create_workspace(path: &Path) -> Result<()>`
- `validate_workspace(path: &Path) -> Result<bool>`
- `get_workspace_diagnostics(path: &Path, has_workspace: bool, tree_count: u64) -> Result<WorkspaceDiagnosticsDto>`：工作区结构、可写性、最近工作区与新建作品可用性诊断由 Core 计算；Linux/QML 只展示 `ResultEnvelope<WorkspaceDiagnosticsDto>`。
- `list_projects(path: &Path) -> Result<Vec<Project>>`
- `create_project(path: &Path, title: &str) -> Result<Project>`
- `rename_project(path: &Path, project_id: &str, new_title: &str) -> Result<bool>`
- `delete_project(path: &Path, project_id: &str) -> Result<bool>`
- `reorder_projects(path: &Path, ordered_project_ids: &[String]) -> Result<bool>`
- `get_project_stats(path: &Path, project_id: &str) -> Result<ProjectStats>`
- `list_volumes(path: &Path, project_id: &str) -> Result<Vec<Volume>>`
- `create_volume(path: &Path, project_id: &str, title: &str) -> Result<Volume>`
- `rename_volume(path: &Path, project_id: &str, volume_id: &str, new_title: &str) -> Result<bool>`
- `delete_volume(path: &Path, project_id: &str, volume_id: &str) -> Result<bool>`
- `reorder_volumes(path: &Path, project_id: &str, ordered_volume_ids: &[String]) -> Result<bool>`
- `list_chapters(path: &Path, project_id: &str, volume_id: &str) -> Result<Vec<Chapter>>`
- `create_chapter(path: &Path, project_id: &str, volume_id: &str, title: &str) -> Result<Chapter>`
- `rename_chapter(path: &Path, project_id: &str, volume_id: &str, chapter_id: &str, new_title: &str) -> Result<bool>`
- `delete_chapter(path: &Path, project_id: &str, volume_id: &str, chapter_id: &str) -> Result<bool>`
- `reorder_chapters(path: &Path, project_id: &str, volume_id: &str, ordered_chapter_ids: &[String]) -> Result<bool>`
- `read_chapter(path: &Path, project_id: &str, volume_id: &str, chapter_id: &str) -> Result<ChapterContent>`
- `open_chapter(path: &Path, project_id: &str, volume_id: &str, chapter_id: &str) -> Result<ChapterOpenResult>`
- `save_chapter(path: &Path, project_id: &str, volume_id: &str, chapter_id: &str, content: &str) -> Result<()>`
- `save_chapter_verified(path: &Path, project_id: &str, volume_id: &str, chapter_id: &str, content: &str) -> Result<ChapterSaveReceipt>`
- `save_chapter_verified_with_allow_empty_overwrite(path: &Path, project_id: &str, volume_id: &str, chapter_id: &str, content: &str, allow_empty_overwrite: bool) -> Result<ChapterSaveReceipt>`
- `clear_chapter_content(path: &Path, project_id: &str, volume_id: &str, chapter_id: &str) -> Result<()>`
- `clear_chapter_content_verified(path: &Path, project_id: &str, volume_id: &str, chapter_id: &str) -> Result<ChapterSaveReceipt>`
- `update_chapter_note(path: &Path, project_id: &str, volume_id: &str, chapter_id: &str, note: &str) -> Result<bool>`
- `load_local_settings(path: &Path) -> Result<LocalSettings>`
- `save_local_settings(path: &Path, settings: &LocalSettings) -> Result<()>`
- `load_syncable_settings(workspace_path: &Path) -> Result<SyncableSettings>`
- `save_syncable_settings(workspace_path: &Path, settings: &SyncableSettings) -> Result<()>`
- `backup_project(path: &Path, project_id: &str) -> Result<()>`
- `move_chapter_to_trash(path: &Path, chapter_id: &str) -> Result<()>`
- `process_writing_event(...) -> Result<bool, WriterError>`
- `record_writing_event(...) -> Result<bool, WriterError>`
- `record_writing_event_for_platform(...) -> Result<bool, WriterError>`：Linux/其他平台显式传入 `platform`，Android 兼容入口仍由 `record_writing_event` 固定为 `android`。
- `flush_writing_stats() -> Result<bool, WriterError>`
- `flush_recent_edits() -> Result<bool, WriterError>`
- `save_mindmap_graph(project_id: &str, graph: MindMapGraphDto) -> Result<bool, WriterError>`：**LEGACY** MindMap 整图保存接收强类型 DTO，Core API 不再提供 `graph_json` 写入入口。正式图谱路线为 starmap。
- `add_starmap_node(starmap_id: &str, node: StarMapNodeDto, x: f32, y: f32) -> Result<StarMapNodeDto, WriterError>`：StarMap 节点写入接收强类型 DTO。
- `save_starmap_layout(starmap_id: &str, layout: &StarMapLayoutDto) -> Result<bool, WriterError>`：StarMap 布局保存接收强类型 DTO。
- `get_starmap_viewport(starmap_id: &str) -> Result<StarMapViewportDto, WriterError>` / `save_starmap_viewport(starmap_id: &str, viewport: StarMapViewportDto) -> Result<bool, WriterError>`：StarMap 顶层画布视口（缩放、平移、视口尺寸）由 Core 持久化到星图目录，客户端不能只保存在 View 内存里。
- `compute_starmap_edge_renders(graph: StarMapGraphDto, layout: StarMapLayoutDto) -> Result<Vec<StarMapEdgeRenderDto>, WriterError>`：StarMap 连线几何由 Core 统一计算，Android/Desktop 不应各自实现边偏移、箭头和双向边规则。
- `hit_test_starmap_node(layout: StarMapLayoutDto, x: f32, y: f32) -> Result<Option<String>, WriterError>`：StarMap 节点命中测试由 Core 统一按布局和 `z_index` 判定，客户端只传入画布坐标。

当前 StarMap 渲染统一的边界是连线几何、节点命中和顶层 viewport 状态。节点样式、文字排版、颜色和完整边标签背景仍未统一进 Core 渲染层，客户端不得把现阶段称为完整渲染层统一。
- `list_starmaps_for_project(project_id: &str) -> Result<Vec<StarMapMetaDto>, WriterError>`
- `get_starmap(starmap_id: &str) -> Result<StarMapMetaDto, WriterError>`
- `create_child_starmap(parent_id: &str, title: &str, desc: &str, accent_color: Option<&str>) -> Result<StarMapMetaDto, WriterError>`：创建子星图的正式入口；旧 `create_child_starmap_legacy` 仅保留为兼容转发。
- `add_starmap_embed(starmap_id: &str, embed: StarMapEmbedDto) -> Result<StarMapEmbedDto, WriterError>`
- `update_starmap_embed(starmap_id: &str, instance_id: &str, patch: StarMapEmbedPatchDto) -> Result<StarMapEmbedDto, WriterError>`
- `delete_starmap_embed(...) -> Result<()>`
- `add_starmap_link(starmap_id: &str, link: StarMapLinkDto) -> Result<StarMapLinkDto, WriterError>`
- `update_starmap_link(starmap_id: &str, link_id: &str, patch: StarMapLinkPatchDto) -> Result<StarMapLinkDto, WriterError>`
- `delete_starmap_link(...) -> Result<()>`
- `find_starmap_references(...) -> Result<Vec<StarMapReference>>`
### ResultEnvelope JSON API (Legacy / 废弃中)

> [!WARNING]
> 所有 `envelope_json` API（包括下述接口）目前均已被标记为 **Legacy（已废弃）**。
>
> 新增功能和接口**绝对禁止**使用此模式。UI 与 Repository 层应完全使用强类型的 **typed DTO API** 进行交互。旧的 `envelope_json` 仅作为遗留兼容封闭在 Bridge 内部使用，并在后续逐步重构清理。

旧的稳定 envelope 入口包括：

- Project：`create_project_envelope_json`、`rename_project_envelope_json`、`reorder_projects_envelope_json`、`delete_project_envelope_json`。成功时写入 `ProjectCreated`、`ProjectRenamed`、`ProjectsReordered`、`ProjectDeleted`。
- Volume：`create_volume_envelope_json`、`rename_volume_envelope_json`、`reorder_volumes_envelope_json`、`delete_volume_envelope_json`。成功时写入 `VolumeCreated`、`VolumeRenamed`、`VolumesReordered`、`VolumeDeleted`。
- Chapter：`create_chapter_envelope_json`、`rename_chapter_envelope_json`、`reorder_chapters_envelope_json`、`save_chapter_content_envelope_json`、`clear_chapter_content_envelope_json`、`update_chapter_note_envelope_json`、`delete_chapter_envelope_json`。成功时写入 `ChapterCreated`、`ChapterRenamed`、`ChaptersReordered`、`ChapterSaved`、`ChapterCleared`、`ChapterNoteUpdated`、`ChapterDeleted`。
- Settings：`save_local_settings_envelope_json`、`save_syncable_settings_envelope_json`。成功时写入 `SettingsSaved`。
- Sync：`save_sync_config_envelope_json`、`save_sync_secrets_envelope_json`、`perform_sync_envelope_json`、`perform_sync_dry_run_envelope_json`、`perform_sync_diagnostics_envelope_json`。成功时写入 `SyncConfigSaved`、`SyncSecretsSaved`、`SyncCompleted` 等同步状态标记；失败时写入稳定同步错误码。

只读列表、打开章节和统计类 API 均要求直接返回 typed DTO。未来的写入和删除操作也必须统一转换为返回 typed DTO。

### 文件操作
所有写操作（`save_chapter`、`save_*_settings`）必须使用 core 的原子替换写入路径：写入临时文件、flush、`fsync` 临时文件，然后 `rename` 替换目标文件。该机制避免目标文件半写入；目录项持久化仍受平台和文件系统语义影响，不宣称跨设备断电的绝对耐久性。

章节写入有防止静默数据丢失的保护机制。在写入章节之前，Core 会读取现有的 `chapter.md`。如果现有内容非空，而普通保存尝试写入空或仅空白的内容，Core 会以 `Error::EmptyOverwriteBlocked` / `EMPTY_OVERWRITE_BLOCKED` 拒绝写入，并保持原文件不变。如需 intentional 清空，必须使用 `clear_chapter_content`、`clear_chapter_content_verified`，或在平台层已确认“用户主动清空”时调用 `save_chapter_content_with_options(..., allow_empty_overwrite=true)` / `save_chapter_verified_with_allow_empty_overwrite(..., true)`；自动保存和普通写入路径不得在未确认用户主动清空时打开该开关。

在用不同内容覆盖现有非空章节文本之前，Core 会在 `backups/chapters/` 下写入一个轻量级恢复副本。备份文件名包含 `project_id`、`volume_id`、`chapter_id` 和时间戳。Core 仅保留每个章节最近的备份，以避免无限增长。

### 错误处理
Core domain 定义统一 `Error` 枚举（如 `writer_core::error::Error`）来处理内部失败模式。跨平台 `api::error::WriterError` 是平台暴露层稳定错误类型，由 `api/error.rs` 集中映射。跨端 Bridge 必须传播稳定错误码和错误语义，不得吞错或只依赖字符串匹配。当前稳定错误码包括 `IO_ERROR`、`JSON_ERROR`、`INVALID_WORKSPACE`、`PROJECT_NOT_FOUND`、`VOLUME_NOT_FOUND`、`CHAPTER_NOT_FOUND`、`EMPTY_OVERWRITE_BLOCKED`、`NOT_IMPLEMENTED`、`REFUSE_DELETE_WORKSPACE_ROOT`、`INVALID_DELETE_TARGET`、`SYNC_CONFLICT`、`SYNC_FAILED`、`OTHER`。

跨端 JSON 入口必须返回标准 `ResultEnvelope`。JNI fallback 已通过 `ResultEnvelope::from_api_result` 序列化，Android legacy parser 兼容旧字段但优先读取 `errorCode` / `userMessage`。Project / Volume / Chapter 写操作、Settings 保存、Sync 保存与执行类 envelope 都必须写入 `changedEntities`；Android 主写路径已改为消费这些 Core 标记，不再通过平台端临时 bus 或裸字符串自行发业务事件。

Linux 工作区诊断入口已迁移到 `WriterCoreApi::get_workspace_diagnostics_envelope_json`。平台层只传递当前是否已加载工作区和缓存树节点数，不再自行探测 `workspace_manifest.json`、`projects/`、`app-meta/` 或创建 `.writer_write_test` 判断可写性。

写入、保存、同步和写作统计事件类 API 不允许用裸 `false` 代替错误。`bool` 只能表示业务成功值；Core API 失败必须返回 `WriterError`，Android Bridge 必须转换为 `BridgeResult.Error`。

### 跨端 DTO

- `ChapterOpenResult { meta, content }`：打开章节的唯一权威返回，客户端不应再自行用列表结果拼接标题、备注或正文。
- `ChapterSaveReceipt { chapter_relative_path, content_len, content_hash, meta_hash, updated_at, word_count }`：保存或明确清空后的回执，供客户端确认 Core 已完成写入和校验。
- `ProjectStats { total_word_count, volume_count, chapter_count }`：作品统计由 Core 计算，Android 通过 UniFFI `ProjectStatsDto` 获取，不在客户端自行遍历汇总。

### 同步 API

- 当前运行期同步后端仅暴露 `git` 和 `github_api`。WebDAV、S3、本地文件夹等后端不在 `BackendType` 配置枚举中，也不是客户端可选择的同步路径。

- **`BackendType` 默认值为 `GithubApi`**。GitHub REST API 是默认主链路（default），不是 Git 失败后的 fallback。`backend_type = "github_api"` 使用 Rust Core 的 GitHub REST API 同步路径，直接调用 GitHub Contents API 读写文件，不进入 libgit2/Git 工作树 reset 流程。`backend_type = "git"` 使用传统 Git 后端（libgit2），仅作为保留能力用于桌面高级场景。**不存在 Git 失败后自动 fallback 到 GitHub API 的机制**，两者互斥，由用户配置或代码显式选择。
- GitHub API 同步采用 `app-meta/sync/manifest.sync.json` 作为 LWW 清单。清单文件由 Core 管理，不作为普通用户文件计入 `downloaded_files` / `uploaded_files`。
- `ManifestFileRecord` 字段包括 `path`、`content_hash`、`updated_at_ms`、`deleted_at_ms`、`device_id`、`op`、`schema_version`。`deleted_at_ms` 为可选 tombstone 时间戳；旧清单缺少该字段时按 `updated_at_ms` 兼容读取。
- GitHub API 写入使用 Contents API 串行执行：更新文件时带当前远端 blob `sha`，删除文件时带当前远端 blob `sha`；遇到 `409` 会重新读取 `sha` 并重试一次。此路径不调用 Git Database API 写提交，也不调用本地 `fetch/reset`。
- `SyncResultDto` 的 `uploaded_files`、`downloaded_files`、`local_deletes`、`remote_deletes`、`overwritten_files`、`ignored_files` 为 UI 展示的权威计数来源；`error_category` 为同步错误分类的权威来源，客户端仅在该字段为空时退回消息文本分类；`error` 非空时客户端不得显示同步成功，即使状态字符串异常地表示成功。
- `perform_sync_diagnostics` 在 `github_api` 后端返回 `backend_type = "github_api"`，避免 UI 将 GitHub API 模式误判为传统 Git。
- 同步写入/执行类平台入口必须使用 `save_sync_config_envelope_json`、`save_sync_secrets_envelope_json`、`perform_sync_envelope_json`、`perform_sync_dry_run_envelope_json`、`perform_sync_diagnostics_envelope_json`。Core envelope 统一映射 `SYNC_CONFLICT`、`SYNC_AUTH_REQUIRED`、`SYNC_NETWORK_ERROR`、`SYNC_REMOTE_ERROR`、`SYNC_CONFIG_ERROR`、`SYNC_NOT_CONFIGURED`、`SYNC_FAILED` 等错误码，平台端不得再靠 raw string 推断冲突或认证失败。

### Android Bridge 入口

Android 主业务入口是 `AppServiceBridge + UniFFI`，UniFFI 暴露的 `WriterAppService` 只适配到 `WriterCoreApi`。上层必须使用领域 Bridge：`WorkspaceBridge`、`WritingBridge`、`StatsBridge`、`StarMapBridge`、`SettingsBridge`、`SyncBridge`。`MindMapBridge` 为 legacy 兼容桥接（正式图谱路线为 `StarMapBridge`）。`NativeCoreBridge` 仅作为 legacy JSON/JNI fallback 和 native 状态/旧动作路径；Repository/UI/ViewModel/Controller 不应直接处理 `NativeResult` 或 `NativeCoreBridge`。


## 平台边界原则

Android 和 Linux backend 都必须通过 `WriterCoreApi` 作为主入口，不能直接以 `facade::WriterCore` 作为平台稳定边界。

## UniFFI 生成物说明

### writer_core.kt

`apps/android/app/src/main/kotlin/com/xiwei/sujian/uniffi/uniffi/writer_core/writer_core.kt` 是 UniFFI 自动生成的 Kotlin 绑定文件。

> **禁止手改此文件。** 任何对 UniFFI 接口的修改必须通过以下流程重新生成。

**生成来源：**
- UDL 定义文件：`core/writer_core/src/api.udl`
- Rust scaffolding：`core/writer_core/build.rs` 中的 `uniffi::generate_scaffolding("./src/api.udl")`
- 绑定生成工具：`core/writer_core/src/bin/uniffi-bindgen.rs`

**重新生成命令：**

```bash
# 1. 先确保 Rust Core 编译通过（会自动生成 scaffolding）
cd core/writer_core && cargo build

# 2. 使用 uniffi-bindgen 生成 Kotlin 绑定
cargo run --bin uniffi-bindgen -- generate \
  --language kotlin \
  --out-dir apps/android/app/src/main/kotlin/com/xiwei/sujian/uniffi \
  core/writer_core/src/api.udl
```

**何时需要重新生成：**
- 修改了 `core/writer_core/src/api.udl` 中的类型或接口定义
- 新增或删除了 UniFFI 暴露的方法
- 修改了 `core/writer_core/src/app_service.rs` 中的 `WriterAppService` 方法签名
- 升级了 `uniffi` crate 版本
