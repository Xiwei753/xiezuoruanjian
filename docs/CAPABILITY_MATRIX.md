# 跨平台能力矩阵

> 当前 Android 主业务入口已切换为 `AppServiceBridge + UniFFI`。本矩阵中的历史 `NativeCoreBridge` / JNI 列仅用于识别 legacy fallback 和待收敛能力，不得作为新业务实现路线。

本文档是迁移现有代码到 Core-first Capability Contract 的盘点表。
本文档不定义新路线，只记录当前事实和待收敛项。
后续每次新增业务能力，都必须更新本文档。

## 1. WorkspaceCapability

| Capability | Core 当前入口 | Android JNI 入口 | Android Kotlin 入口 | Linux backend 入口 | Linux QML 触发点 | 当前是否对齐 | 风险级别 | 问题描述 | 收敛建议 |
|---|---|---|---|---|---|---|---|---|---|
| 创建/打开工作区 | `facade::WriterCore::new`, `create_workspace`, `validate_workspace` | `createWorkspace`, `validateWorkspace` | `NativeCoreBridge.createWorkspace`, `validateWorkspace` | `create_new_workspace`, `open_existing_workspace`, `validate_workspace` | `EmptyWorkspace.qml`, `main.qml` | 否 | P1 | Linux backend 存在直接判断和写入文件行为，而不是完全依赖 Core | 去除 Linux backend 中的文件系统判断，改为全权调用 Core API，并封装 ResultEnvelope |

**小结**：
- 当前 Core **并非**唯一业务来源，Linux backend 在打开/创建工作区时依然有部分直接的文件目录校验和状态管理。
- Android 目前仅仅是 adapter，但没有封装统一的 ResultEnvelope。
- Linux 存在 UI 假成功情况，其 backend 封装了类似 `refresh_app_state_json` 的状态机。
- 需要统一使用 ResultEnvelope 返回验证和创建结果。

## 2. ProjectCapability

| Capability | Core 当前入口 | Android JNI 入口 | Android Kotlin 入口 | Linux backend 入口 | Linux QML 触发点 | 当前是否对齐 | 风险级别 | 问题描述 | 收敛建议 |
|---|---|---|---|---|---|---|---|---|---|
| 创建作品 | `create_project` | `createProject` | `NativeCoreBridge.createProject` | `create_new_project`, `create_project_json` | `main.qml` -> `onCreateProject` | 否 | P0 | Linux backend `create_new_project` 中直接操作了 `projects` 目录，手动写了 `.writer_write_test` 并拼凑假 JSON，导致新建失败或行为分叉 | 废除 Linux backend 的文件写入逻辑，统一调用 Core `create_project`。成功后依靠 changedEntities reload tree，使用标准 ResultEnvelope |
| 获取作品列表 | `list_projects` | `listProjects` | `NativeCoreBridge.listProjects` | `refresh_app_state_json` (间接) | `main.qml` | 否 | P1 | Android JNI 单独导出，Linux backend 通过拼装大 JSON 返回 | 统一 List 接口，支持按需获取 |
| 修改/删除 | `rename_project`, `delete_project`, `reorder_projects` | `renameProjectNative`, `deleteProjectNative`, `reorderProjectsNative` | `NativeCoreBridge.renameProject`... | `rename_project`, `delete_project`, `delete_project_json` | `WorkspaceTree.qml` Context Menu | 否 | P1 | 两边 JSON 序列化不一致，Linux 返回了 UI 直接消费的自定义 JSON | 统一返回 ResultEnvelope |

**小结**：
- Linux 新建作品（createProject）严重分叉，甚至在 backend 进行了写盘测试和目录创建，存在直接写 workspace 行为。
- Linux 成功后没有根据 Core changedEntities 机制刷新，而是手动重建整树（`build_tree_model_json`）再传给 UI。
- Android 的操作也只是简单包装，并未利用信封机制。
- 必须优先将 `createProject` 彻底收敛进 Core 并统一 ResultEnvelope。

## 3. VolumeCapability

| Capability | Core 当前入口 | Android JNI 入口 | Android Kotlin 入口 | Linux backend 入口 | Linux QML 触发点 | 当前是否对齐 | 风险级别 | 问题描述 | 收敛建议 |
|---|---|---|---|---|---|---|---|---|---|
| 增删改查 | `create_volume`, `list_volumes`, `rename_volume`, `delete_volume`, `reorder_volumes` | `createVolume`, `listVolumes`, `renameVolumeNative`, `deleteVolumeNative`, `reorderVolumesNative` | `NativeCoreBridge.createVolume`... | `create_new_volume`, `rename_volume`, `delete_volume`, `reorder_volumes` | `WorkspaceTree.qml` Context Menu | 否 | P1 | Linux 包装了 `*_json` 后缀方法向 QML 传状态，错误用 raw string | 统一返回标准 ResultEnvelope，废除 `*_json` 特供接口 |

**小结**：
- 增删改查基本都存在于 Core，但平台层各自用不同方式消化了返回数据。
- Linux 返回中带有特化的 UI 假成功拼装（例如 `create_volume_json` 中补齐 `state`）。
- 存在 raw string error 抛给 UI 层的情况。

## 4. ChapterCapability

| Capability | Core 当前入口 | Android JNI 入口 | Android Kotlin 入口 | Linux backend 入口 | Linux QML 触发点 | 当前是否对齐 | 风险级别 | 问题描述 | 收敛建议 |
|---|---|---|---|---|---|---|---|---|---|
| 创建与编辑 | `create_chapter`, `read_chapter`, `write_chapter` | `createChapter`, `readChapter`, `writeChapter` | `NativeCoreBridge.createChapter`... | `create_new_chapter`, `get_chapter_content`, `save_current_chapter` | `EditorPage.qml` | 否 | P1 | Linux 侧 `save_current_chapter` 自身维护了 `current_save_status` | 状态需从 Core 事件获得，而不是 UI/Backend 自行维护 |
| 属性与删除 | `list_chapters`, `rename_chapter`, `delete_chapter`, `reorder_chapters` | `listChapters`, `renameChapterNative`... | `NativeCoreBridge` 相关接口 | `rename_chapter`, `delete_chapter`... | `WorkspaceTree.qml` | 否 | P1 | 平台端可能绕过事件机制，依赖本地状态机 | 统一定义 |

**小结**：
- Android 和 Linux 皆为 Adapter，但 Linux Backend 介入了较多业务状态管理。
- 不存在直接写文件，但存在 UI 假成功和独立的状态维护（如 `current_save_status`）。

## 5. SettingsCapability

| Capability | Core 当前入口 | Android JNI 入口 | Android Kotlin 入口 | Linux backend 入口 | Linux QML 触发点 | 当前是否对齐 | 风险级别 | 问题描述 | 收敛建议 |
|---|---|---|---|---|---|---|---|---|---|
| 本地设置 | `load_local_settings`, `save_local_settings` | `loadLocalSettings`, `saveLocalSettings` | `SettingsRepository.saveLocalSettings` | `load_local_settings`, `save_local_settings` | `SettingsDialog.qml` | 否 | P0 | Android 保存后靠 `SettingsChangeBus` 自行补事件，未走 Core `SettingsSaved` 事件 | 废弃 `SettingsChangeBus`，由 Core 派发事件，平台端被动监听生效 |
| 同步设置 | `load_syncable_settings`, `save_syncable_settings` | `loadSyncableSettings`, `saveSyncableSettings` | `SettingsRepository.saveSyncableSettings` | 待确认 (无明显暴露) | 缺失 | 否 | P1 | Linux backend 疑似未实现 syncable settings 编辑 | 补齐并统一事件 |

**小结**：
- 设置生效机制严重分叉，Android 端直接使用了 `SettingsChangeBus` 这个内存临时总线补足了保存事件，未达到 "SettingsSaved 必须由 Core 广播" 的契约。
- 必然导致未来扩展设置项时，多端行为不一致，Android 设置不生效问题频发。
- 必须统一事件模型与 ResultEnvelope。

## 6. SyncCapability

| Capability | Core 当前入口 | Android JNI 入口 | Android Kotlin 入口 | Linux backend 入口 | Linux QML 触发点 | 当前是否对齐 | 风险级别 | 问题描述 | 收敛建议 |
|---|---|---|---|---|---|---|---|---|---|
| 同步执行 | `sync_workspace`, `perform_sync`, `perform_sync_dry_run` | `performSync`, `performSyncDryRun` | `SettingsRepository.performSync` | `perform_sync`, `perform_sync_dry_run` | `SyncPage.qml` | 否 | P0 | Linux backend 手动判断 conflict，封装了 `SyncTaskOutcome` | 在 Core 映射 libgit2 checkout conflict；双端依赖 Core 返回统一的 `ResultEnvelope` 和 `status: "conflict"` |
| 同步配置 | `load_sync_config`, `save_sync_config` | `loadSyncConfig`, `saveSyncConfig` | `SettingsRepository.saveSyncConfig` | `load_sync_config`, `save_sync_config` | `SettingsDialog.qml` | 否 | P1 | Linux QML 和 Android UI 各自做校验 | 配置读取应完全一致 |

**小结**：
- Linux GitHub 同步 Conflict 时，Linux backend 自己通过硬编码或者抓字符串映射了 conflict，且封装了 `SyncTaskOutcome` 而非直接消费 Core 给定的结构。
- Android 同步状态的 Error 经常因为解析失败或缺少底层结构而吞错。
- 存在大量 raw string error 打印在 UI 上。
- 必须立刻统一 SyncCapability 的 ResultEnvelope 及 conflict 状态收敛。

## 7. MindMapCapability

| Capability | Core 当前入口 | Android JNI 入口 | Android Kotlin 入口 | Linux backend 入口 | Linux QML 触发点 | 当前是否对齐 | 风险级别 | 问题描述 | 收敛建议 |
|---|---|---|---|---|---|---|---|---|---|
| 图谱与节点编辑 | `create_mind_map_graph`, `list_mind_map_graphs`, `create_mind_map_node`, `update_mind_map_node` 等 | `createMindMapGraphJson`, `createMindMapNodeJson` 等 | `NativeCoreBridge` 等，由 `MindMapActivity` 消费 | 缺失 | 缺失 | 否 | P1 | Android 已经通过 Core edit API 写入并消费 snapshot，Linux 完全没有对应入口 | Linux backend 需要补齐同等的 MindMap Capability 入口 |
| 渲染视图 | `get_mind_map_snapshot` | `getMindMapSnapshotJsonNative` | `NativeCoreBridge`, 最终给 `MindMapRenderView` | 缺失 | 缺失 | 否 | P1 | 同上 | 补齐 |

**小结**：
- 能力只在 Android 端存在，Linux 端存在严重缺失。
- Android 端确实做到了通过 Core API 写入，但由于 ResultEnvelope 尚未全局统一定义，仍然使用了强依赖 `RustResponse<T>` 类似结构。

## 8. EditorModelCapability

| Capability | Core 当前入口 | Android JNI 入口 | Android Kotlin 入口 | Linux backend 入口 | Linux QML 触发点 | 当前是否对齐 | 风险级别 | 问题描述 | 收敛建议 |
|---|---|---|---|---|---|---|---|---|---|
| 统计/模型 | `record_recent_edit`, `get_recent_edits` | `recordRecentEdit`, `getRecentEdits` | `NativeCoreBridge.recordRecentEdit` | `calculate_word_count` (疑似单独特化) | `EditorPage.qml` | 否 | P2 | 两端字数统计或历史记录获取机制不一致 | 统一走 EditorModelCapability 规范 |
| 动画与排版 | 缺失 (属于 UI 层) | 缺失 | `TypingAnimationController`, `SmoothCursorRenderer` 等 | 缺失 | 无 | 否 | P2 | 这属于平台渲染逻辑，但开关来源必须来自 Core SettingsCapability | Android 设置来源必须对接到 Core |

**小结**：
- Editor 动画属于平台渲染逻辑，但其启用状态未有效挂载到 Core 的 Settings 统一入口。
- Linux 特化了 `calculate_word_count`。

## 9. Trash/DeleteGuardCapability

| Capability | Core 当前入口 | Android JNI 入口 | Android Kotlin 入口 | Linux backend 入口 | Linux QML 触发点 | 当前是否对齐 | 风险级别 | 问题描述 | 收敛建议 |
|---|---|---|---|---|---|---|---|---|---|
| 软删除 | `move_chapter_to_trash` | 待确认 | 待确认 | 待确认 (`delete_chapter` 是真删还是移入回收站？) | `main.qml` | 否 | P1 | 暂不明确两端 `delete_chapter` 是否都对齐了回收站能力 | 彻底盘点并强制应用 Delete Guard |

**小结**：
- `delete_chapter` 在平台端可能直接执行了物理删除逻辑，绕过了安全防线。

## 10. SearchCapability

| Capability | Core 当前入口 | Android JNI 入口 | Android Kotlin 入口 | Linux backend 入口 | Linux QML 触发点 | 当前是否对齐 | 风险级别 | 问题描述 | 收敛建议 |
|---|---|---|---|---|---|---|---|---|---|
| 全文/章节搜索 | 缺失 | 缺失 | 缺失 (`EditorActivity.kt` 存在本地视图搜索) | 缺失 | 缺失 | 否 | P1 | Android 自己实现了 Editor 视图级别的替换和搜索 | 将搜索算法收拢到 Core 层 |

**小结**：
- 搜索能力缺失，Android 自行通过视图实现了局部搜索，不符合 Core-first。

## 11. ExportCapability

缺失

## 12. AICapability

| Capability | Core 当前入口 | Android JNI 入口 | Android Kotlin 入口 | Linux backend 入口 | Linux QML 触发点 | 当前是否对齐 | 风险级别 | 问题描述 | 收敛建议 |
|---|---|---|---|---|---|---|---|---|---|
| AI 功能上下文 | `build_ai_context`, `get_ai_request_payload` | 缺失 | 缺失 | 缺失 | 缺失 | 否 | P1 | Core 有底层桩，但平台未对接 | 统一设计 AI Event/Command |

**小结**：
- 未实现完整的端到端能力。

---

## 下一步重构顺序 (Refactoring Roadmap)

1. **P0-1：ProjectCapability.createProject 收敛，修 Linux 新建作品**
   - 移除 Linux backend 中 `create_new_project` 的直接 `projects` 目录创建与可写测试。
   - 彻底依赖 Core 层 `create_project` 方法。
   - Linux backend 成功后，必须根据 Core 返回的 `changedEntities` 事件或标志去 reload tree，而不是拼凑假 JSON `create_project_json`。
2. **P0-2：SyncCapability ResultEnvelope + conflict 状态收敛**
   - 统一所有的 Core Sync API 响应为标准 `ResultEnvelope`。
   - 明确在 Core 中映射 libgit2 的 checkout conflict。
   - Android 同步和 Linux 同步统一消费 `SyncStatus.conflict`，停止使用裸字符串 (rawError) 或自己封装的 `SyncTaskOutcome`。
3. **P0-3：SettingsCapability 保存/事件收敛，替代平台临时 bus**
   - 废除 Android 端 `SettingsChangeBus.notifyChanged()`。
   - 实现 Core 的 `SettingsSaved` 事件广播机制。
   - 双端保存设置统一经由 `saveLocalSettings` 等接口，UI 被动响应 Core 事件完成字号和动画开关刷新。
4. **P1-1：MindMapCapability 双端入口补齐**
   - Linux backend 需要按 Android `NativeCoreBridge` 的已有模式，对接并暴露完整的 MindMap Edit 和 Snapshot API，确保双端能力对称。
5. **P1-2：统一 ResultEnvelope 类型**
   - 在 JNI 与 Backend 的所有接口中，彻底用标准化的 JSON 结构体 `ResultEnvelope` 替代现有各种散落的特化 JSON 拼接。
6. **P1-3：Android JNI / Linux backend 薄适配器化**
   - 清除所有的业务判断（如 `workspace_path.is_empty()` 判断、权限判定提示），全权由 Core 拒绝并返回错误码。
