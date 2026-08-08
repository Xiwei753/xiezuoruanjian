# 同步规则

Status: active
Last verified: 2026-08-08
Truth source: product decision / protocol / code
Supersedes: None

本文档概述了共享核心的同步和冲突解决规则。
- 所有变更同步到私有 Git 仓库。
- 冲突存储为单独的冲突文件，绝不自动覆盖本地数据。
- 保存 ≠ 同步。连续保存不会产生连续网络请求。

## 持续集成（CI）
- GitHub Actions 工作流覆盖 Android debug APK、Linux 和 Windows 构建。
- 详见 `.github/workflows/` 目录。

## 同步规则
- 系统存在两种明确的同步目标（Issue #600 评论 #3 问题四）：
  - **应用级 Git**：同步根 = `app_data_root`，同步设置、全局星图、主题调色板。
  - **作品级 Git**：同步根 = `project_root`（`projects_root/<project_id>`），同步单部作品正文、元数据、作品自己的同步状态。
- 两种目标各自独立的 Git 仓库、白名单/黑名单和同步配置，在 API/数据模型里明确区分为 `AppSyncProfile` 与 `ProjectSyncProfile(projectId)`，不混成同一个 `SyncProfile`。
- 冲突存储为单独的冲突文件，绝不自动覆盖本地数据。
- 保存 ≠ 同步。连续保存不会产生连续网络请求。

## 应用级同步白名单/黑名单

同步根 = `app_data_root`，路径相对应用数据根：

- **白名单（参与同步）**：`settings.sync.json`、`starmaps/**`、`themes/palettes/**`。
- **黑名单（绝不参与）**：`作品/`（projects_root）、`日志/`（log_dir）、`导出/`、`备份/`、`settings.local.json`、`recent_edits.json`、`app-meta/sync/config.local.json`、`app-meta/sync/conflicts.json`、含 `secret` 的路径（`app-meta/sync/*secret*`）、`device/`、`app-meta/stats/`、`app-meta/transactions/`、`.git/`、`.tmp`/`.lock` 后缀、含 `cache`/`tmp`/`backups`/`sqlite_cache` 的路径。

## 作品级同步白名单/黑名单

同步根 = 作品目录（`project_root`），路径相对作品目录：

- **白名单（参与同步）**：`project.json`、`volumes/**/volume.json`、`volumes/**/chapters/**/chapter.md`、`volumes/**/chapters/**/chapter.meta.json`、`characters/**`、`app-meta/sync/manifest.sync.json`。
- **黑名单（绝不参与）**：`app-meta/sync/config.local.json`、`app-meta/sync/state.local.json`（及旧格式 `sync_state.json`）、`app-meta/sync/conflicts.json`、含 `secret` 的路径、`.git/`、`.tmp`/`.lock` 后缀、`app-meta/logs`、含 `cache`/`tmp`/`backups`/`sqlite_cache` 的路径。

## 内容分类（ContentClass）

同步使用 `classify_content_path(path)` 对文件分类，决定冲突处理策略：

| 分类 | 匹配规则 | 同步策略 |
|------|---------|---------|
| `UserTextDocument` | `/chapters/*.md`、`note.md`、`outline.md`、`scene.md`、`character_notes.md`、`timeline_notes.md`、`draft.md` | 三路合并，绝不 LWW |
| `Metadata` | `project.json`、`volume.json`、`chapter.meta.json`、`starmap.json`、`writing_stats.json` | LWW |
| `LocalOnly` | `app-meta/*`（回收站/备份等本地内容） | 不同步 |
| `GeneratedCache` | 其他所有文件 | LWW |

判断函数：`lww::classify_content_path(path)` → `ContentClass`
兼容函数：`lww::is_document_content_path(path)` → `classify_content_path(path) == ContentClass::UserTextDocument`

## 正文文件冲突保护

### 保护范围

`ContentClass::UserTextDocument` 类文件享受三路合并保护，绝不静默覆盖。

### 三路合并策略

对正文文件，使用 `base_hash / local_hash / remote_hash` 三路比较：

| base_hash | local_hash | remote_hash | 结果 |
|-----------|-----------|-------------|------|
| == local | == local | == local | 无变更 |
| == local | == local | != base | 仅远端改了，下载 |
| == local | != base | == local | 仅本地改了，上传 |
| != local | != base | != base | **双端都改了，冲突** |

其中 `base_hash` 来自 `state.known_files`（上次成功同步后记录的 hash）。

### 冲突处理

当检测到双端修改冲突时：

1. **不覆盖本地文件**：本地原文件保持不动
2. **生成远端冲突副本**：`chapter.remote-conflict-YYYYMMDD-HHMMSS.md`
3. **记录冲突信息**：写入 `app-meta/sync/conflicts.json`
4. **返回 `PartialConflict` 状态**：表示部分同步完成，但有正文冲突需手动处理
5. **不污染远端 manifest**：冲突时保留远端原 manifest record（指向远端 hash），不插入本地 hash
6. **known_files 存远端 hash**：下次同步仍能检测到 BothChanged，冲突持续到手动解决

### Manifest 一致性

正文冲突时，`merged_manifest_files` 保留 `remote_rec`（远端 hash），不插入 `local_rec`。
这保证远端 manifest 与远端实际文件内容一致，不会出现 manifest 说 hash=A 但文件实际是 hash=B 的情况。

### 非正文文件

配置文件、元数据文件（`project.json`、`volume.json` 等）继续使用 LWW（Last Writer Wins）策略，按时间戳比较。

> 说明：旧模型中的设置文件语义合并（`settings.sync.json` 逐键三路合并）已随 workspace 概念删除（Issue #600）。设置是应用级数据，位于 `app_data_root`，不进入作品仓库。

### 冲突状态

| 状态 | 含义 |
|------|------|
| `Conflict` | Git 合并冲突 |
| `PartialConflict` | 正文文件双端修改冲突，部分同步完成 |
| `LatestWinsApplied` | LWW 策略已应用 |
| `NoChanges` | 无变更 |

### 删除保护

- 本地修改 + 远端删除 = 冲突，不自动删除本地
- 本地删除 + 远端修改 = 冲突，不自动删除本地

## 同步防抖

### 设计原则

- **保存 ≠ 同步**：章节保存只写本地磁盘，不触发网络同步
- **自动同步仅在特定时机**：数据根打开、应用回到前台
- **Core 层防抖**：`perform_lww_sync` 内置最小间隔检查（`sync_interval_seconds`，最小 60 秒）

### 防抖层级

| 层级 | 机制 | 间隔 |
|------|------|------|
| Core | `perform_lww_sync` 入口检查 `last_sync_time` | `max(sync_interval_seconds, 60)` |
| Linux_qt Rust | `can_start_auto_sync(reason, 60)` | 同 reason 60 秒 |
| Linux_qt QML | `workspaceOpenAutoSyncTimer` / `foregroundAutoSyncTimer` | 1.5s / 1.2s 延迟 |
| Android | `AutoSyncWorker` 遍历所有作品，每个作品独立检查 interval/elapsed + WorkManager 周期 | 每作品 `syncIntervalSeconds`（默认 300s） |

`AutoSyncWorker` 遍历 `ProjectRepository.getProjects()`，逐个读取该作品的 `snapshotSyncProfile(projectId)`，只处理 `enabled && autoSync` 的作品，分别调用 `runSync(..., projectId, snapshot)`，每个作品独立检查 interval/elapsed，不再依赖单一 `ActiveProjectGate`。
