# HarmonyOS Core Action Map

Status: draft
Last verified: 2026-06-14
Truth source: docs/TECHNICAL_ROUTE.md

---

## 概述

本文档将鸿蒙端页面需要的能力逐条映射到 WriterCoreApi / ResultEnvelope，确保鸿蒙端与 Android/Desktop 端使用相同的 Core API。

---

## 页面能力映射

### 1. Index 页面（首页）

| UI 操作 | Bridge 方法 | Core API | ResultEnvelope 类型 |
|---------|------------|----------|-------------------|
| 加载工作区 | `openWorkspace(path)` | `WorkspaceCapability.openWorkspace` | `ResultEnvelope<WorkspaceState>` |
| 显示最近编辑 | `getRecentEdits()` | `ProjectCapability.getRecentEdits` | `ResultEnvelope<RecentEdit[]>` |
| 显示作品列表 | `listProjects()` | `ProjectCapability.listProjects` | `ResultEnvelope<Project[]>` |
| 创建新作品 | `createProject(name)` | `ProjectCapability.createProject` | `ResultEnvelope<Project>` |
| 删除作品 | `deleteProject(id)` | `ProjectCapability.deleteProject` | `ResultEnvelope<boolean>` |
| 获取写作统计 | `getWritingStats()` | `StatsCapability.getWritingStats` | `ResultEnvelope<WritingStats>` |

### 2. WorkspacePage（工作区页面）

| UI 操作 | Bridge 方法 | Core API | ResultEnvelope 类型 |
|---------|------------|----------|-------------------|
| 加载项目树 | `getProjectTree(id)` | `ProjectCapability.getProjectTree` | `ResultEnvelope<ProjectTree>` |
| 创建卷 | `createVolume(projId, name)` | `ChapterCapability.createVolume` | `ResultEnvelope<Volume>` |
| 删除卷 | `deleteVolume(projId, volId)` | `ChapterCapability.deleteVolume` | `ResultEnvelope<boolean>` |
| 重命名卷 | `renameVolume(projId, volId, name)` | `ChapterCapability.renameVolume` | `ResultEnvelope<boolean>` |
| 创建章节 | `createChapter(volId, name)` | `ChapterCapability.createChapter` | `ResultEnvelope<Chapter>` |
| 删除章节 | `deleteChapter(chId)` | `ChapterCapability.deleteChapter` | `ResultEnvelope<boolean>` |
| 重命名章节 | `renameChapter(chId, name)` | `ChapterCapability.renameChapter` | `ResultEnvelope<boolean>` |
| 重排序卷 | `reorderVolumes(projId, ids)` | `ChapterCapability.reorderVolumes` | `ResultEnvelope<boolean>` |
| 重排序章节 | `reorderChapters(volId, ids)` | `ChapterCapability.reorderChapters` | `ResultEnvelope<boolean>` |

### 3. WritingPage（写作页面）

| UI 操作 | Bridge 方法 | Core API | ResultEnvelope 类型 |
|---------|------------|----------|-------------------|
| 加载章节内容 | `loadChapter(chId)` | `ChapterCapability.loadChapter` | `ResultEnvelope<ChapterData>` |
| 保存章节内容 | `saveChapter(chId, text)` | `ChapterCapability.saveChapter` | `ResultEnvelope<SaveReceipt>` |
| 清空章节 | `clearChapter(chId)` | `ChapterCapability.clearChapter` | `ResultEnvelope<SaveReceipt>` |
| 计算字数 | `calculateWordCount(text)` | `EditorModelCapability.computeWordStats` | `number` |
| 记录写作事件 | `processWritingEvent(chars, dur)` | `EditorModelCapability.trackSessionStats` | `ResultEnvelope<boolean>` |
| 获取设置 | `getLocalSettings()` | `SettingsCapability.getLocalSettings` | `ResultEnvelope<LocalSettings>` |
| 保存设置 | `saveLocalSettings(settings)` | `SettingsCapability.saveLocalSettings` | `ResultEnvelope<boolean>` |
| 获取有效设置 | `getEffectiveSettings()` | `SettingsCapability.getEffectiveSettings` | `ResultEnvelope<EffectiveSettings>` |

### 4. StarMapPage（星图页面）

| UI 操作 | Bridge 方法 | Core API | ResultEnvelope 类型 |
|---------|------------|----------|-------------------|
| 列出星图 | `listStarMaps()` | `StarMapCapability.listStarMaps` | `ResultEnvelope<StarMapMeta[]>` |
| 按项目列出 | `listStarMapsForProject(id)` | `StarMapCapability.listStarMapsForProject` | `ResultEnvelope<StarMapMeta[]>` |
| 获取星图元数据 | `getStarMap(id)` | `StarMapCapability.getStarMap` | `ResultEnvelope<StarMapMeta>` |
| 获取图数据 | `getStarMapGraph(id)` | `StarMapCapability.getStarMapGraph` | `ResultEnvelope<StarMapGraph>` |
| 创建星图 | `createStarMap(title, desc)` | `StarMapCapability.createStarMap` | `ResultEnvelope<StarMapMeta>` |
| 删除星图 | `deleteStarMap(id)` | `StarMapCapability.deleteStarMap` | `ResultEnvelope<boolean>` |
| 重命名星图 | `renameStarMap(id, title)` | `StarMapCapability.renameStarMap` | `ResultEnvelope<boolean>` |

### 5. SettingsPage（设置页面）

| UI 操作 | Bridge 方法 | Core API | ResultEnvelope 类型 |
|---------|------------|----------|-------------------|
| 获取本地设置 | `getLocalSettings()` | `SettingsCapability.getLocalSettings` | `ResultEnvelope<LocalSettings>` |
| 保存本地设置 | `saveLocalSettings(settings)` | `SettingsCapability.saveLocalSettings` | `ResultEnvelope<boolean>` |
| 获取同步设置 | `getSyncableSettings()` | `SettingsCapability.getSyncableSettings` | `ResultEnvelope<SyncableSettings>` |
| 保存同步设置 | `saveSyncableSettings(settings)` | `SettingsCapability.saveSyncableSettings` | `ResultEnvelope<boolean>` |
| 加载同步配置 | `loadSyncConfig()` | `SyncCapability.loadSyncConfig` | `ResultEnvelope<SyncConfig>` |
| 保存同步配置 | `saveSyncConfig(config)` | `SyncCapability.saveSyncConfig` | `ResultEnvelope<boolean>` |
| 同步演练 | `syncDryRun()` | `SyncCapability.dryRun` | `ResultEnvelope<SyncReport>` |
| 同步诊断 | `syncDiagnostics()` | `SyncCapability.diagnostics` | `ResultEnvelope<SyncDiagnostics>` |
| 执行同步 | `performSync()` | `SyncCapability.sync` | `ResultEnvelope<SyncResult>` |

---

## Capability 模块对应关系

### WorkspaceCapability
- 工作区打开、验证、状态查询
- 鸿蒙端通过 `HarmonyFileAccess` 接口实现文件系统访问

### ProjectCapability
- 作品 CRUD、项目树查询
- 与 Android `ProjectBridge` / Desktop `ProjectBackend` 对齐

### ChapterCapability
- 卷、章节 CRUD、重排序
- 与 Android `WorkspaceBridge` / Desktop `ProjectBackend` 对齐

### EditorModelCapability
- 章节文本加载/保存、字数统计、写作事件
- 鸿蒙端通过 `WritingPage` 直接调用

### SettingsCapability
- 本地设置、同步设置、有效设置
- 鸿蒙端通过 `SettingsPage` 调用

### SyncCapability
- 同步配置、诊断、执行
- 鸿蒙端通过 `SettingsPage` 调用

### StarMapCapability
- 星图元数据、图数据 CRUD
- 鸿蒙端通过 `StarMapPage` 调用

---

## 错误处理映射

所有 Bridge 方法返回 `ResultEnvelope<T>`，包含：

- `success: boolean` - 操作是否成功
- `data?: T` - 业务数据
- `errorCode?: ErrorCode` - 标准错误码
- `userMessage?: string` - 用户友好提示
- `rawError?: string` - 原始错误（用于日志）
- `warnings?: string[]` - 警告信息
- `changedPaths?: string[]` - 变更路径
- `changedEntities?: string[]` - 变更实体

鸿蒙端 UI 根据 `success` 和 `errorCode` 进行相应处理，与 Android/Desktop 端保持一致。

---

## 线程模型

| 操作类型 | 执行线程 | 说明 |
|---------|---------|------|
| 读取操作 | UI 线程或异步 | 列表查询、加载内容 |
| 写入操作 | 后台线程 | 保存章节、创建项目 |
| 同步操作 | 后台线程 | Git 同步、网络请求 |
| 统计计算 | 后台线程 | 字数统计、写作速度 |

鸿蒙端使用 `async/await` 处理异步操作，UI 线程只负责渲染和用户交互。

---

## 数据流

```
用户操作 → ArkUI 页面 → IWriterCoreBridge → MockWriterCoreBridge (当前)
                                              ↓
                                         NativeWriterCoreBridge (未来)
                                              ↓
                                         NAPI / C-ABI
                                              ↓
                                         Rust Core (writer_core)
                                              ↓
                                         文件系统
```

---

## 参考文档
- [API_CONTRACTS.md](archive/API_CONTRACTS.md) - 接口边界与交互契约（已归档）

- [CROSS_PLATFORM_CAPABILITY_CONTRACT.md](archive/CROSS_PLATFORM_CAPABILITY_CONTRACT.md) - 跨平台能力契约（已归档）
- [harmony_bridge_contract.md](harmony_bridge_contract.md) - 鸿蒙桥接契约
- [harmony_route.md](harmony_route.md) - 鸿蒙技术路线
