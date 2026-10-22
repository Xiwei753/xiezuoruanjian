# 同步规则

Status: active
Last verified: 2026-10-18
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
- 系统只有一个全局 `SyncProfile`、一个远端仓库、一次全量同步（Issue #630）。
- Core 内部仍区分 `SyncScope::App / Project` 两个 target，但只用于路径过滤和远端前缀路由，不再暴露成两套用户配置。
- App target 同步根 = `app_data_root`，远端前缀 `app/`，同步设置、全局星图、主题调色板。
- Project target 同步根 = `project_root`（`projects_root/<project_id>`），远端前缀 `projects/<project_id>/`，同步单部作品正文、元数据、作品自己的同步状态。
- 全局同步配置只存在一份：`<app_data_root>/app-meta/sync/config.local.json`。
- 冲突存储为单独的冲突文件，绝不自动覆盖本地数据。
- 保存 ≠ 同步。连续保存不会产生连续网络请求。

## 应用级同步白名单/黑名单

同步根 = `app_data_root`，路径相对应用数据根：

- **白名单（参与同步）**：`settings.sync.json`、`starmaps/**`、`themes/palettes/**`、写作统计事件 `stats/events/<device_id>/YYYY-MM-DD.events.jsonl`（仅通过对应设备的统计数据组同步，见下文「统计设备组」）。
- **黑名单（绝不参与）**：`作品/`（projects_root）、`日志/`（log_dir）、`导出/`、`备份/`、`settings.local.json`、`recent_edits.json`、`app-meta/sync/config.local.json`、`app-meta/sync/conflicts.json`、含 `secret` 的路径（`app-meta/sync/*secret*`）、`device/`、`app-meta/stats/`（旧版单机事件库、迁移标记与冲突诊断文件，只作为一次性迁移输入）、`app-meta/transactions/`、`.git/`、`.tmp`/`.lock` 后缀、含 `cache`/`tmp`/`backups`/`sqlite_cache` 的路径。

### App 数据组与独立发布

App target 内部按数据组独立扫描、合并、记录状态并发布不可变 generation：`global` 包含跨设备设置和调色板；每张星图是一个组，包含该图的 meta、graph 和对象文件。`starmaps/index.json` 当前仍是跨图聚合记录，因此作为依赖组，在本轮所有星图组成功后才发布。

远端 `app/app-meta/sync/app_groups.sync.json` 选择每个数据组的当前 generation，与 project 生命周期清单 `targets.sync.json` 分离。没有该目录清单时，从旧 `app/app-meta/sync/manifest.sync.json` 推导数据组；各组在迁移完成前继续从旧 `app/` 读取。成功发布一组 generation 后才通过版本条件更新目录清单切换该组，尚未迁移的组和旧文件继续保留。

**客户端升级是一次协调切换，不支持新旧客户端同时写同一 App 数据。** 启用 v2 catalog 前，先在所有旧客户端暂停同步并升级到支持 App 数据组的版本，再恢复同步。首个新客户端从旧 manifest 推导组并逐组迁移：catalog 未登记的组继续以旧 `app/` 为只读迁移源；catalog 选中某组 generation 后，该 generation 成为唯一有效源，旧 `app/` 中的后续写入不会回流。迁移保留旧文件用于恢复，不把它们作为并行写入副本。若 v2 catalog 已创建，恢复同步前必须确保所有活跃客户端都已升级；不要让旧客户端在 flat `app/` 上继续写。

每个 App 数据组使用独立的本地 `manifest.sync.json`、`state.local.json` 与 `conflicts.json`。缺少本地文件且没有对应删除 tombstone 时，只将该组报告为失败，不把缺文件推断成删除，也不阻断其他独立组。星图索引依赖检查失败时保留旧索引 generation，避免它引用尚未完成的数据组。

### 统计设备组

写作统计的原始事件以「来源设备」为粒度独立同步：本机事件写在 `stats/events/<device_id>/YYYY-MM-DD.events.jsonl`，每台设备独占写入自己的分区，其它设备下载后只读，永不重写同分区；同步组 id 为 `stats_device:<device_id>`（远端 storage key `stats-device-<hex>`），与 `global`、星图组一样参与远端目录清单、独立 generation 发布和按组失败隔离。一次数据组传输只上传本机新增/变化的事件文件，下载不会触发自动重传，也不重复写事件。

聚合永远从原始事件投影派生：Core 读取全部已到达的来源分区，按 `event_id` 去重（本机分区优先保留）。跨分区意外重复的 `event_id` 只计数一次，结构化冲突信息追加到 `app-meta/stats/event_conflicts.local.jsonl` 供诊断，绝不静默覆盖原始事实。不维护可被 LWW 覆写的每日汇总账本，也不把合计数直接相加。

旧版单机事件库 `app-meta/stats/events.local/` 作为一次性迁移输入：首次升级时把旧事件按原 `event_id`、`timestamp_ms`、`local_date`、`source`、`inserted_chars` 原样迁入本机来源分区，迁移完成标记 `events.device_partitions.v1` 写入后旧目录不再参与读写。迁移不把同步下载的正文、正文当前字数或缺失事件制造成人工输入。`historyScope` 如实返回 `localEvents`（仅本机）、`syncedDevices`（已收到其它设备事件且统计组无失败）或 `partialSync`（存在未完成的统计设备组同步），平台页面按此标示范围，不得仅因打开同步开关就宣称历史全量可达。

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

> 写作统计原始事件 `stats/events/<device_id>/*.events.jsonl` 落在 `GeneratedCache`：每个来源分区由该设备独占写入、其它设备只读，LWW 不会合并事件内容；跨设备历史由 Core 读取全部分区并按 `event_id` 去重派生，不依赖 LWW 合并单体统计文件。

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
- **Linux Qt 自动同步由应用内调度器驱动**：QML 只负责定时唤醒，到期判定、工作区身份校验和单任务互斥都在后端完成
- **自动触发不冒充手动请求**：自动同步使用 `trigger=auto`（非强制同步），不写 `manual_sync_request` 记录；手动同步仍走 `manual` 与既有优先队列

### Linux Qt 调度路径（Issue #983）

| 层级 | 机制 | 间隔 / 条件 |
|------|------|-------------|
| `main.qml` | 长期存在的 `autoSyncWakeTimer` 调 `syncBackend.auto_sync_due()`；工作区打开、配置变化后立即补一次检查 | 每 30 秒唤醒 |
| `sync_operations.rs` | `auto_sync_gate` 读取 Core 持久化配置并检查工作区、layout、单任务互斥、远端与 Token | `enabled && auto_sync && sync_interval_seconds > 0` 且工作区有效 |
| 到期基准 | `max(FullSyncState.last_attempt_time, last_actual_sync_started_at, last_auto_sync_dispatch_at, last_auto_sync_failure_at)` | `max(sync_interval_seconds, 60)`；不按 `last_success_time` 反复重试 |
| 保存屏障 | QML 到期后先执行既有 `preSyncBarrier()`：刷新当前正文、`flush_recent_edits`、`flush_pending_settings_save` | 保存失败只记录 `auto_sync_preflight_failed`，不启动、不上传旧正文 |
| 执行 | 复用 `perform_sync_internal("auto", true, ...)` | 进入同一个 Core 全量/分组增量同步、进度回调与报告落盘路径 |
| 互斥与取消 | 现有 single-flight、`manual_sync_pending` 优先、`SyncCancellationToken`、workspace generation、data_root、operation_id 校验 | 自动到期遇到现有任务跳过本次；切/关工作区取消旧任务 |

QML 定时器只做唤醒，不计算 LWW、分组或网络请求；`sync_runtime.json` 记录当前构建的已保存开关、间隔、最近调度判定、跳过原因、最近自动 operation_id 与失败原因。

### Android

| 层级 | 机制 | 间隔 |
|------|------|------|
| Android | `AutoSyncWorker` 读取全局 profile，检查一次 interval/elapsed + WorkManager 周期 | `syncIntervalSeconds`（默认 300s） |

`AutoSyncWorker` 只读取一份全局 `snapshotSyncProfile()`，判断一次 `enabled && autoSync`，调用一次 `runFullSync(...)`。作品枚举由 Core 的 full-sync 完成，不再逐作品读取不同 profile、不再依赖单一 `ActiveProjectGate`。
