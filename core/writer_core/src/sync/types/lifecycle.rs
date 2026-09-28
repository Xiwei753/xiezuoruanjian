//! 目标生命周期与全量同步结果类型。
//!
//! Issue #624eb6a33「星图 Core 最终收口」之后同步域做了一轮同类的类型瘦身，本文件
//! 把 target lifecycle / full-sync 结果相关的数据结构从 `types.rs` 里独立出来，
//! 让 `types.rs` 只保留配置、结果、状态三类基础类型。
//!
//! 这些类型仍然描述"一次全量同步中单个 target 的执行结果与生命周期决策"，
//! 与 `crate::sync::target_lifecycle`（负责读写远端 catalog 的服务层）是不同的关注点。

use super::*;

/// 单个 target 的同步结果 — `perform_full_sync` 中一个本地根 → 远端前缀目标的输出。
///
/// `target_kind` 为 `"app"` 或 `"project"`；`project_id` 仅在 Project target 时有值。
/// `result` 为该 target 的 `SyncResult`；`error` 为该 target 执行失败时的错误描述。
///
///   `deleted_resolution` 仅 deleted_project target 有值，
/// `cleanup_completed_deleted_targets` 按此精确确认是否移除本地 `PendingDeletedTarget`，
/// 不再按 `SyncStatus` 猜。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetSyncResult {
    pub target_kind: String,
    pub project_id: Option<String>,
    pub remote_prefix: String,
    pub result: SyncResult,
    ///   deleted target 的 LWW 决策结果。
    ///
    /// 仅 `target_kind == "deleted_project"` 时有值。`cleanup_completed_deleted_targets`
    /// 按此精确确认：`LocalDeleteWins` 且远端删除+catalog tombstone 写入成功才移除 pending；
    /// `RemoteTargetWins` 且本地恢复成功才移除 pending；`Retry` 保留。
    #[serde(default)]
    pub deleted_resolution: Option<DeletedTargetResolution>,
    ///   本地 lifecycle commit action —
    /// Transfer 阶段产出，Commit 阶段执行。
    ///
    /// - `None`：无 lifecycle action，走普通 staging commit；
    /// - `DeleteProject { project_id }`：远端 delete 胜出，Commit 阶段执行完整
    ///   Project 本地删除事务（move worktree / unbind starmaps / history），
    ///   **不**生成 `PendingDeletedTarget`（不反向要求删远端，远端已删）；
    /// - `RestoreProject { project_id }`：预留，当前 RestoreProject 在 Transfer 直接下载。
    #[serde(default)]
    pub local_lifecycle_action: LocalLifecycleCommitAction,
}

///   本地 lifecycle commit action —
/// Transfer 阶段产出，Commit 阶段执行。
///
/// 把"删除本地 project"从 Transfer 阶段（裸 `remove_dir_all`）移到 Commit 阶段
/// （完整业务删除事务），避免 staging commit 把刚删掉的旧作品重新写回来。
///
///   新增 `ReplaceProject` 变体 —
/// DeleteLocalProject → remote 又 Upsert 时，本地已有 Project 需要整树替换，
/// 不再用<空 staging + 普通三方 commit>冒充 replace。`ReplaceProject` 的 Commit
/// 语义：对 live ∪ staging 做整树替换（staging 有 → Apply；live 有但 staging 没有 → Delete）。
///
///  C `DeleteProject` / `ReplaceProject` 携带
/// `expected_local_lww` guard，Commit 时再确认本地没有前进。
///
/// `expected_local_lww` 是**非 Option** 的
/// `LiveTargetLwwSerde`。破坏性 lifecycle action（DeleteProject / ReplaceProject）
/// 必须携带 guard — 不允许"无 guard 也允许删/替换"。Transfer 阶段生成这些 action
/// 时必须先成功获取当前 local LWW（snapshot 失败 → Retry，不生成 action）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LocalLifecycleCommitAction {
    /// 无 lifecycle action，走普通 staging commit。
    #[default]
    None,
    /// 远端 delete 胜出，Commit 阶段执行完整 Project 本地删除事务。
    DeleteProject {
        /// 被删除的 project id。
        project_id: String,
        /// Commit 时再确认本地没有前进的 guard。
        /// 非 Option — 破坏性 action 必须携带 guard，Commit 严格比较
        /// `current_local == expected_local_lww`，其他任何情况都拒绝执行。
        expected_local_lww: LiveTargetLwwSerde,
    },
    /// 预留：RestoreProject 在 Commit 阶段恢复本地 project。
    RestoreProject {
        /// 被恢复的 project id。
        project_id: String,
    },
    ///   问题&2：远端 Upsert 胜出且本地已有 Project → 整树替换。
    ///
    /// Commit 语义：对 live ∪ staging 做整树替换：
    /// 1. Commit 前再次确认本地没有比 guard 更新的编辑；
    /// 2. staging 有 → Apply；live 有但 staging 没有 → Delete；
    /// 3. 原子提交整棵 Project。
    ReplaceProject {
        /// 被替换的 project id。
        project_id: String,
        /// Commit 时再确认本地没有前进的 guard。
        /// 非 Option — 破坏性 action 必须携带 guard。
        expected_local_lww: LiveTargetLwwSerde,
    },
}

///   `LiveTargetLww` 的 serde 友好包装。
///
/// `LiveTargetLww` 定义在 `full_sync.rs`（非 Serialize），lifecycle action 需要
/// Serialize。此结构提供 serde 桥接，Commit 时转回 `LiveTargetLww` 做 guard 比较。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LiveTargetLwwSerde {
    pub lww_time_ms: i64,
    pub device_id: String,
}

impl LiveTargetLwwSerde {
    /// 从 `LiveTargetLww` 构造。
    pub fn from_lww(lww: &crate::sync::full_sync::LiveTargetLww) -> Self {
        Self {
            lww_time_ms: lww.lww_time_ms,
            device_id: lww.device_id.clone(),
        }
    }
}

/// 本地 lifecycle commit 的完整 receipt。
///
/// `commit_full_sync` 返回 `Vec<LocalLifecycleCommitReceipt>`，每个元素对应一个
/// RemoteLifecycle 删除事务。API 层用 `change_set` 调 `record_workspace_change_set`
/// 记本地 history，成功后调 `ack_project_delete_history` 推进 journal 到
/// `HistoryRecorded` → `Completed`（RemoteLifecycle origin 跳过 `RemoteDeleteQueued`）。
/// history 失败时 journal 保留在 `StarMapsUnbound`，下次启动 recover 补记。
#[derive(Debug, Clone)]
pub struct LocalLifecycleCommitReceipt {
    /// 本次删除的 journal token（用于 ack 推进 journal）。
    pub journal_token: String,
    /// 删除产生的 workspace 变更集（DeleteTree + 解绑 starmap 的 Upsert）。
    pub change_set: crate::storage::workspace_git::WorkspaceChangeSet,
    /// 本次被解绑的 starmap ids（供调用方刷搜索索引等）。
    pub unbound_starmap_ids: Vec<String>,
    /// 删除发起来源（User/RemoteLifecycle）。
    pub origin: crate::project::ProjectDeleteOrigin,
}

/// 单个 target 的 dry-run 计划。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetSyncPlan {
    pub target_kind: String,
    pub project_id: Option<String>,
    pub remote_prefix: String,
    pub plan: SyncPlan,
}

/// 全量同步聚合结果 — 一次 `perform_full_sync` 的完整输出。
///
/// `overall_status` 为总体状态（Success/PartialConflict/Error 等）：
/// - 所有 target 成功 → Success
/// - 部分 target 冲突 → PartialConflict
/// - 部分 target 错误 → Error
///
/// `targets` 为每个 target 的结果列表，顺序为 App target 在前、Project targets 在后。
/// `total_*` 为上传/下载/删除/冲突的聚合统计。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullSyncResult {
    pub overall_status: SyncStatus,
    pub targets: Vec<TargetSyncResult>,
    pub total_uploaded: u32,
    pub total_downloaded: u32,
    pub total_local_deletes: u32,
    pub total_remote_deletes: u32,
    pub total_overwritten: u32,
    pub total_ignored: u32,
    pub total_conflicts: u32,
    pub error: Option<String>,
    pub error_category: Option<String>,
    pub message_key: Option<String>,
}

/// 全量同步 dry-run 聚合结果。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullSyncDryRunResult {
    pub targets: Vec<TargetSyncPlan>,
    pub total_to_upload: u32,
    pub total_to_download: u32,
    pub total_to_delete_local: u32,
    pub total_to_delete_remote: u32,
    pub total_ignored: u32,
    pub total_conflicts: u32,
}

/// 全量同步诊断结果 — 只测一次仓库、分支、token。
///
/// `diagnostics` 为单次诊断结果；`error` 为诊断失败时的错误描述。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullSyncDiagnosticsResult {
    pub diagnostics: SyncDiagnosticsResult,
}

/// 待删除的同步 target — provider-neutral 持久状态。
///
/// 整部作品删除后，远端 `projects/<project_id>/` 下的对象需要被清理。
/// 但 `prepare_full_sync` 只通过 `list_projects()` 枚举现存作品生成 target，
/// 已删除作品不在列表里，远端前缀没有任何 target 会去清理。
///
/// `PendingDeletedTarget` 是 sync engine 自己的 provider-neutral 持久状态
/// （**不是** project tombstone），记录"这个 SyncTarget 已删除，下次同步时
/// 需要走 target-delete 计划清理远端前缀下所有对象"。
///
/// ## 生命周期
///
/// 1. `delete_project_with_changes` 时记录：把 `PendingDeletedTarget` 持久化到
///    `<app_data_root>/app-meta/sync/pending_deleted_targets.json`；
/// 2. `prepare_full_sync` 加载 pending deleted targets，加入 `FullSyncPlan.targets`，
///    `target_kind = "deleted_project"`；
/// 3. `run_transfer` 对 `deleted_project` target 走 target-delete 计划：
///    `provider.list(remote_prefix)` 枚举远端对象 → 逐个 `provider.delete(...)`；
/// 4. 全部远端删除成功后从 pending 列表移除该条目。
///
/// ## provider-neutral
///
/// 只使用 `SyncProvider::list/delete` 和 capabilities，不写 GitHub 专用逻辑。
/// GitHub 的 SHA/branch/API 细节由 `GitHubProvider` 自己处理。
///
///   `deleted_at_ms` / `device_id` 参与 LWW 决策。
/// `run_deleted_target_sync` 先读远端 manifest，用 `deleted_at_ms` 与远端
/// manifest 的 `max(lww_record_time)` 比较，本地 tombstone 胜出才删远端，
/// 远端更晚则不删（远端有更新，下次正常 sync 会下载恢复）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingDeletedTarget {
    /// 已删除的同步目标 — `SyncTarget::project(project_id)`。
    pub target: SyncTarget,
    /// 删除时间戳（Unix 毫秒）。
    ///
    ///   参与 LWW 决策，不再只用于排序和日志。
    /// 与远端 manifest 的 `max(lww_record_time)` 比较，本地 tombstone 胜出才删远端。
    pub deleted_at_ms: i64,
    /// 关联的 delete journal token，用于 ack 推进 journal。
    pub journal_token: String,
    /// 发起删除的设备 ID，用于 LWW 平局决胜。
    ///
    ///   时间戳相同时，字典序大的 device_id 获胜
    /// （与 `sync/lww/compare.rs::resolve_lww_path` 的 tie-break 规则一致）。
    /// `#[serde(default)]` 保持向后兼容：旧文件反序列化得到空字符串。
    #[serde(default)]
    pub device_id: String,
    /// 需要在远端删除的相对路径列表（相对于 `target.remote_prefix`）。
    ///
    /// `None` 表示删除整个 `remote_prefix` 下所有远端对象（由 `provider.list` 枚举）；
    /// `Some(paths)` 表示只删除指定路径（精确删除，不枚举远端）。
    /// 当前实现统一用 `None`（枚举远端前缀下所有对象逐个删除），
    /// `Some` 留作未来精确删除优化的扩展点。
    #[serde(default)]
    pub paths: Option<Vec<String>>,
}

impl PendingDeletedTarget {
    /// 为已删除的 project 构造 `PendingDeletedTarget`。
    ///
    /// `paths` 为 `None`：删除整个 `projects/<project_id>/` 前缀下所有远端对象。
    ///
    ///   `device_id` 参与 LWW 决策，必传。
    pub fn for_project(
        project_id: &str,
        deleted_at_ms: i64,
        journal_token: &str,
        device_id: &str,
    ) -> Self {
        Self {
            target: SyncTarget::project(project_id),
            deleted_at_ms,
            journal_token: journal_token.to_string(),
            device_id: device_id.to_string(),
            paths: None,
        }
    }
}

// ──   Target 生命周期 catalog（远端持久、provider-neutral） ──

/// target 生命周期操作类型 — provider-neutral，持久化到远端 catalog。
///
///   `TargetLifecycleRecord` 记录单个 sync target
/// （如 `projects/<project_id>`）的生命周期操作，让离线旧设备上线时能读到
/// target 的 delete tombstone，不会把本地旧 project 重新上传（P 被复活）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetOp {
    Upsert,
    Delete,
}

impl TargetOp {
    /// 线格式字符串。
    pub fn as_str(&self) -> &'static str {
        match self {
            TargetOp::Upsert => "upsert",
            TargetOp::Delete => "delete",
        }
    }
}

/// target 生命周期记录 — 远端持久、provider-neutral。
///
/// 持久化到远端 `app/app-meta/sync/targets.sync.json`（app target 的 remote_prefix 下，
/// 不会随 `projects/<id>/` 一起被删除）。catalog 只通过 `SyncProvider::read/write` 操作，
/// GitHub SHA / WebDAV ETag 等留在 Provider 里。
///
/// ## 与 `PendingDeletedTarget` 的职责区分
///
/// - 本地 `pending_deleted_targets.json`（`PendingDeletedTarget`）：负责
///   "本机删除事务还没同步完成"，本机状态。
/// - 远端 `targets.sync.json`（`TargetLifecycleRecord`）：负责
///   "跨设备都必须知道这个 target 的生命周期"，跨设备共识。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetLifecycleRecord {
    /// target 标识，如 `"projects/<project_id>"`，与 `SyncTarget.remote_prefix` 对齐。
    pub target_id: String,
    /// 远端前缀，如 `"projects/<project_id>"`。
    pub remote_prefix: String,
    /// 操作类型：upsert（target 存在）/ delete（target 已删除）。
    pub op: TargetOp,
    /// 记录更新时间（Unix 毫秒）。
    pub updated_at_ms: i64,
    /// 删除时间（仅 `op == Delete` 时有值），优先于 `updated_at_ms` 作为 LWW 时间。
    #[serde(default)]
    pub deleted_at_ms: Option<i64>,
    /// 发起操作的设备 ID，用于 LWW tie-break（字典序大的胜出，与 `resolve_lww_path` 同规则）。
    #[serde(default)]
    pub device_id: String,
    ///   generation 原子发布 — Upsert 记录指向的当前可见
    /// generation ID。
    ///
    /// LiveProject 先把完整 Project 上传到不可见 generation prefix
    /// （`projects/P/__generations__/G/`），全部成功后 CAS `targets.sync.json`
    /// 写 `active_generation = G`。CAS 成功后 G 才成为可见版本；CAS 输给 Delete
    /// 则 G 是未引用 generation，后续 GC。
    ///
    /// - `Some(G)`：Upsert 的当前可见 generation，RestoreProject 从
    ///   `projects/P/__generations__/G/` 下载；Delete cleanup 不碰此 generation prefix。
    /// - `None`：legacy（无 generation）或 Delete 记录，RestoreProject 从 legacy
    ///   `projects/P/` 下载。
    #[serde(default)]
    pub active_generation: Option<String>,
    /// schema 版本。
    #[serde(default = "default_target_catalog_schema_version")]
    pub schema_version: u32,
}

fn default_target_catalog_schema_version() -> u32 {
    1
}

impl TargetLifecycleRecord {
    /// 构造一条 upsert 记录。
    pub fn upsert(
        target_id: &str,
        remote_prefix: &str,
        updated_at_ms: i64,
        device_id: &str,
    ) -> Self {
        Self {
            target_id: target_id.to_string(),
            remote_prefix: remote_prefix.to_string(),
            op: TargetOp::Upsert,
            updated_at_ms,
            deleted_at_ms: None,
            device_id: device_id.to_string(),
            active_generation: None,
            schema_version: 1,
        }
    }

    /// 构造一条 delete tombstone 记录。
    pub fn delete(
        target_id: &str,
        remote_prefix: &str,
        deleted_at_ms: i64,
        device_id: &str,
    ) -> Self {
        Self {
            target_id: target_id.to_string(),
            remote_prefix: remote_prefix.to_string(),
            op: TargetOp::Delete,
            updated_at_ms: deleted_at_ms,
            deleted_at_ms: Some(deleted_at_ms),
            device_id: device_id.to_string(),
            active_generation: None,
            schema_version: 1,
        }
    }

    ///   builder 方法 — 给 Upsert 记录设置 active_generation。
    ///
    /// 用于 LiveProject generation 原子发布：先上传到 generation prefix，成功后
    /// 构造 `upsert(...).with_active_generation(G)` 作为 CAS candidate。
    /// Delete 记录不应设置 active_generation（调用方应只在 Upsert 上调用）。
    pub fn with_active_generation(mut self, generation: impl Into<String>) -> Self {
        self.active_generation = Some(generation.into());
        self
    }
}

/// target 生命周期 catalog 容器 — 持久化到远端 `targets.sync.json`。
///
/// `records` 按 `target_id` 唯一（合并/upsert 后）。LWW 合并规则：
/// 按 `target_id` 分组，取 `(lww_time, device_id)` 最大的记录
/// （与 `resolve_lww_path` 同规则）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct TargetLifecycleCatalog {
    #[serde(default)]
    pub records: Vec<TargetLifecycleRecord>,
}

///   带远端版本的 catalog 快照。
///
/// `load_remote_catalog` 返回此结构，携带远端当前版本。
/// `write_remote_catalog` 用 `version` 做 CAS 写入（`IfMatch`），
/// 防止多设备并发覆盖彼此的 lifecycle record。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTargetCatalogSnapshot {
    pub catalog: TargetLifecycleCatalog,
    /// 远端 `targets.sync.json` 的当前版本标识。
    /// `None` 表示文件不存在（首次写应用 `CreateNew`）。
    pub version: crate::sync::provider::model::RemoteVersion,
}

// ──   DeletedTargetResolution ──

/// deleted target 的 LWW 决策结果 — provider-neutral typed resolution。
///
///   `run_deleted_target_sync` 做完 target-level LWW
/// 后返回这个 typed resolution（而非 `SyncResult`），`cleanup_completed_deleted_targets`
/// 按此精确确认是否移除本地 `PendingDeletedTarget`，不再按 `SyncStatus` 猜。
///
/// - `LocalDeleteWins` → 执行远端删除 + catalog 写 delete tombstone → 才移除 pending；
/// - `RemoteTargetWins` → 下载远端到 staging → commit 恢复本地 project → 才移除 pending；
/// - `Retry` → 什么都不删/恢复 → pending 保留（manifest/读取失败时）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeletedTargetResolution {
    /// 本地 delete 胜出：执行远端删除 + catalog 写 delete tombstone。
    LocalDeleteWins,
    /// 远端 target 胜出：下载远端内容到 staging，commit 恢复本地 project。
    RemoteTargetWins,
    /// 无法确定（manifest/读取失败）：不删不恢复，pending 保留。
    Retry,
}

// ──   PlannedTargetKind ──

/// 计划阶段 target 的明确类型 — 替代字符串 `target_kind`，强类型决策结果。
///
///   `build_full_sync_target_plan` 按 `target_id` 合并
/// local live project / local pending delete / remote lifecycle record，
/// 生成此明确类型，`run_transfer` 按此走对应执行路径。
///
/// ## 决策语义
///
/// - `App`：app target，正常 LWW 同步。
/// - `LiveProject`：本地 live project，远端无 delete tombstone 或本地更新 → 正常 upsert 同步。
/// - `DeleteRemoteProject`：本地 pending delete，本地 tombstone 胜出 → 删远端对象 + 写 delete tombstone。
/// - `DeleteLocalProject`：本地 live project，远端 delete tombstone 更新 → 不上传，本地 project 应删除。
/// - `RestoreProject`：本地 pending delete，远端 upsert 更新 → 下载远端恢复本地 project。
/// - `Retry`：无法决策（catalog/manifest 读取失败）→ 不删不恢复，pending 保留。
/// - `RemoteCleanupProject`：   — 远端残留清理重试。
///   上一轮 authoritative Delete 清 prefix 失败时记录 `PendingRemoteTargetCleanup`，
///   下一轮 Prepare 加载 pending 生成此 target，重试 `delete_all_remote_objects`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlannedTargetKind {
    App,
    LiveProject,
    DeleteRemoteProject,
    DeleteLocalProject,
    RestoreProject,
    Retry,
    /// 远端残留清理重试。
    RemoteCleanupProject,
}

impl PlannedTargetKind {
    /// 转为 `TargetSyncResult.target_kind` 的字符串标识（API 契约，不可随意更改）。
    ///
    /// - `App` → `"app"`
    /// - `LiveProject` / `DeleteLocalProject` → `"project"`（都是 live project target）
    /// - `DeleteRemoteProject` / `RestoreProject` / `Retry` / `RemoteCleanupProject`
    ///   → `"deleted_project"`（都是 pending delete / cleanup target）
    pub fn as_target_kind_str(&self) -> &'static str {
        match self {
            PlannedTargetKind::App => "app",
            PlannedTargetKind::LiveProject | PlannedTargetKind::DeleteLocalProject => "project",
            PlannedTargetKind::DeleteRemoteProject
            | PlannedTargetKind::RestoreProject
            | PlannedTargetKind::Retry
            | PlannedTargetKind::RemoteCleanupProject => "deleted_project",
        }
    }

    /// 是否为待删除 target（pending delete target）。
    ///
    /// `DeleteRemoteProject` / `RestoreProject` / `Retry` / `RemoteCleanupProject`
    /// 为 true（都是 pending delete / cleanup target）。
    pub fn is_pending_deleted(&self) -> bool {
        matches!(
            self,
            PlannedTargetKind::DeleteRemoteProject
                | PlannedTargetKind::RestoreProject
                | PlannedTargetKind::Retry
                | PlannedTargetKind::RemoteCleanupProject
        )
    }
}

// ──   TargetLifecycleApplyResult ──

/// provider-neutral 原子决策接口的返回值 — catalog CAS 写入后的决策结果。
///
/// 原 `LostToRemote(snapshot)` 让调用方猜 op 反转，
/// 导致 LWW 相等时被误判为"远端 delete 赢"或"远端 upsert 赢"。新枚举明确四种结果：
///
/// - `Applied(snapshot)` → candidate 严格赢，merge + IfMatch 写成功，携带持久化后的完整 snapshot；
/// - `AlreadyCurrent(snapshot)` → candidate 与远端 record 完全相等（同 op/同时间/同 device_id），
///   不需要再写 catalog，调用方按 candidate.op 继续后续动作（不删本地/不恢复）；
/// - `RemoteWinner { snapshot, record }` → 远端严格赢，携带远端最新 snapshot 和**真实**赢的 record，
///   调用方按 `record.op` 决策（Delete → 删本地；Upsert → 恢复本地），不再猜 op 反转；
/// - `Retry(err)` → 无法写入（重试耗尽/网络错误等），不删任何远端文件，pending 保留。
#[derive(Debug)]
pub enum TargetLifecycleApplyResult {
    /// candidate 严格赢，CAS 写成功，携带持久化后的完整 snapshot。
    Applied(RemoteTargetCatalogSnapshot),
    /// candidate 与远端 record 完全相等，不需要再写 catalog。
    AlreadyCurrent(RemoteTargetCatalogSnapshot),
    /// 远端严格赢，携带远端最新 snapshot 和真实赢的 record（含 op）。
    RemoteWinner {
        snapshot: RemoteTargetCatalogSnapshot,
        record: TargetLifecycleRecord,
    },
    /// 无法写入（重试耗尽/网络错误等），不删任何远端文件，pending 保留。
    Retry(crate::Error),
}

impl TargetLifecycleApplyResult {
    /// 取出内嵌的 snapshot（Applied/AlreadyCurrent/RemoteWinner 共用）。
    ///
    /// `Retry` 返回 `None`（无 snapshot）。供调用方统一更新本地 catalog 缓存。
    pub fn snapshot(&self) -> Option<&RemoteTargetCatalogSnapshot> {
        match self {
            TargetLifecycleApplyResult::Applied(s)
            | TargetLifecycleApplyResult::AlreadyCurrent(s) => Some(s),
            TargetLifecycleApplyResult::RemoteWinner { snapshot, .. } => Some(snapshot),
            TargetLifecycleApplyResult::Retry(_) => None,
        }
    }
}
