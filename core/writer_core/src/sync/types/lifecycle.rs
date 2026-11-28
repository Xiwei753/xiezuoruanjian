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
    /// App 数据组的稳定 id；Project targets 为 `None`。
    #[serde(default)]
    pub group_id: Option<String>,
    /// 失败路径，相对本地数据根；成功时为空。
    #[serde(default)]
    pub failure_path: Option<String>,
    /// 失败阶段，例如 `merge`、`upload`、`delete`、`manifest_publish`。
    #[serde(default)]
    pub failure_phase: Option<String>,
    pub result: SyncResult,
    /// 本地 lifecycle commit action —
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
/// 旧 target 生命周期协议：DeleteLocalProject → remote 又 Upsert 时整树替换，
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

// ──   PlannedTargetKind ──

/// 计划阶段 target 的明确类型 — 替代字符串 `target_kind`，强类型决策结果。
///
///   `build_full_sync_target_plan` 按 `target_id` 合并
/// local live project / local pending delete / remote lifecycle record，
/// 生成此明确类型，`run_transfer` 按此走对应执行路径。
///
/// ## 决策语义
///
/// - `App`：App 数据组 target。
/// - `LiveProject`：本机存在的作品，按对象版本比较。
/// - `DeleteRemoteProject`：本机已删除作品，远端对象逐个发布删除。
/// - `RestoreProject`：远端还有本机没有的作品，把远端对象下载回来。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlannedTargetKind {
    App,
    LiveProject,
    /// 用户已删除作品：远端对象逐个发布删除。
    DeleteRemoteProject,
    /// 远端还有本机没有的作品：把远端对象下载回来。
    RestoreProject,
}

impl PlannedTargetKind {
    /// 转为 `TargetSyncResult.target_kind` 的字符串标识（API 契约，不可随意更改）。
    ///
    /// - `App` → `"app"`
    /// - `LiveProject` → `"project"`
    /// - `DeleteRemoteProject` / `RestoreProject` → `"deleted_project"`
    pub fn as_target_kind_str(&self) -> &'static str {
        match self {
            PlannedTargetKind::App => "app",
            PlannedTargetKind::LiveProject => "project",
            PlannedTargetKind::DeleteRemoteProject | PlannedTargetKind::RestoreProject => {
                "deleted_project"
            }
        }
    }

    /// 是否为待删除 target（pending delete target）。
    ///
    /// `DeleteRemoteProject` / `RestoreProject` 为 true。
    pub fn is_pending_deleted(&self) -> bool {
        matches!(
            self,
            PlannedTargetKind::DeleteRemoteProject | PlannedTargetKind::RestoreProject
        )
    }
}

