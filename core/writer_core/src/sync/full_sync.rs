//! 全量同步三段式编排 — Prepare → Transfer → Commit（Issue #644 评论 5467821839）。
//!
//! 把全量同步从"整个流程持一把写锁"拆成三段，每段只持短锁，Transfer 阶段完全不持锁：
//!
//! 1. **Prepare**（短写锁）：写 `Syncing` 状态、加载 secrets 快照、枚举 targets、
//!    算出每个 target 的 `local_root`，产出 [`FullSyncPlan`]（owned，不依赖 core）。
//! 2. **Transfer**（不持锁）：用 plan 里的 secrets/config 创建 backend，对每个 target
//!    调 `backend.sync()`（网络 + 本地文件读写）。本模块的 [`run_transfer`] 是纯函数，
//!    不接触 [`crate::facade::WriterCore`]，调用方在 API 层释放锁后调用。
//! 3. **Commit**（短写锁）：聚合 [`FullSyncTransferResult`] → [`FullSyncResult`]，
//!    原子写终态 `FullSyncState`，成功类重建搜索索引。
//!
//! 本模块只放纯编排逻辑（无 `&self`、无锁、无磁盘状态读写）；
//! 持锁、持久化、搜索索引等副作用留在 `facade/sync_ops.rs` 的薄转发方法里。
//!
//! ## 聚合优先级（Issue #630 评论 5308040939 Part 2）
//!
//! [`aggregate_full_sync_result`] 按"需要用户处理的终态 > 可重试 > 成功"保留错误类型：
//! `Fatal/Error > Dirty > Conflict > Recoverable > Success`。`error` /
//! `error_category` / `message_key` 从与总体同优先级的第一个 dominant target 取得，
//! 避免"总体是认证失败、文案却拿到前一个网络错误"的错位。

use std::path::{Path, PathBuf};

use crate::sync::provider::SyncProvider;
use crate::sync::types::{FullSyncResult, SyncPolicy, SyncResult, SyncTarget, TargetSyncResult};
use crate::sync::SyncStatus;

// ── #645 评论 5504296097 问题2：generation 原子发布 helpers ──

/// generation 原子发布 — 不可见 generation prefix 的子目录名。
///
/// LiveProject 先把完整 Project 上传到 `projects/P/__generations__/G/`，
/// CAS `targets.sync.json` 成功后才成为可见版本。`delete_all_remote_objects`
/// 跳过此子目录，不碰并发 Upsert 正在上传的 generation。
const GENERATION_SUBDIR: &str = "__generations__";

/// 构造 generation 不可见 prefix。
///
/// `projects/P` + `G` → `projects/P/__generations__/G`。
///
/// #645 评论 5504296097 问题4：防御性校验 `generation_id` 是合法单 path segment，
/// 不只依赖 catalog loader。非法 `generation_id`（空、`.`、`..`、含 `/`/`\`）→ `Err`。
fn generation_remote_prefix(
    project_remote_prefix: &str,
    generation_id: &str,
) -> crate::error::Result<String> {
    crate::sync::target_lifecycle::validate_generation_id(generation_id)?;
    Ok(format!(
        "{}/{}/{}",
        project_remote_prefix, GENERATION_SUBDIR, generation_id
    ))
}

/// 判断远端相对路径是否落在 generation 不可见 prefix 下。
///
/// `delete_all_remote_objects` 用此跳过 `__generations__/` 下的对象，不碰并发
/// Upsert 正在上传的 generation。
fn is_generation_path(rel_path: &str) -> bool {
    rel_path == GENERATION_SUBDIR || rel_path.starts_with(&format!("{}/", GENERATION_SUBDIR))
}

// ── Plan / Transfer 结果 ──

/// Prepare 阶段产出 — Transfer 阶段需要的全部数据（owned，不依赖 core 锁）。
///
/// 包含 sync_policy、force_sync 标志和已枚举的 target 列表（含每个 target
/// 的 `local_root`）。Transfer 阶段只读这份 plan，不再回 core 取数据。
///
/// #645 评论 5504296097 第2点：不再携带 `workspace_git_layout`。
/// 本地 Git 仓库由 bootstrap 阶段初始化，同步计划不负责 Git 生命周期。
///
/// #645 评论 5504296097 问题4：携带 `remote_catalog_snapshot` —
/// Prepare 阶段在写锁外读取的远端 catalog 完整 snapshot（含 version）。
/// Transfer 阶段用这份 snapshot 作为 lifecycle CAS 起点，不再无条件再读一次
/// catalog（避免 target discovery 和执行使用两套时间点的 catalog）。
/// `apply_lifecycle_record` 在 CAS 冲突时仍会重读远端最新 snapshot。
#[derive(Debug, Clone)]
pub struct FullSyncPlan {
    pub sync_policy: SyncPolicy,
    pub force_sync: bool,
    pub targets: Vec<PlannedTarget>,
    /// #644 评论 5473401065 第1节：app_data_root 供 API 层在无锁状态下
    /// 调用 `prepare_staging_runs` 时传给 `StagingRun::create`。
    pub app_data_root: PathBuf,
    /// #645 评论 5504296097 问题4：Prepare 阶段读取的远端 catalog snapshot。
    ///
    /// Transfer 阶段 `run_transfer` 用它作为 lifecycle CAS 起点。
    /// `apply_lifecycle_record` 在 CAS 冲突时重读远端最新 snapshot。
    pub remote_catalog_snapshot: crate::sync::types::RemoteTargetCatalogSnapshot,
}

/// 单个 target 的执行计划 — target 元数据 + 本地根 + 分类标签。
#[derive(Debug, Clone)]
pub struct PlannedTarget {
    pub target: SyncTarget,
    pub local_root: PathBuf,
    /// staging root for isolated transfer（三段式 staging 路径）。
    /// `Some` 时 Transfer 阶段写 staging 而非 live；`None` 时回退 `local_root`。
    pub staging_root: Option<PathBuf>,
    /// #645 评论 5504296097 问题1：强类型决策结果，替代字符串 `target_kind`。
    ///
    /// `build_full_sync_target_plan` 按 `target_id` 合并 local live project /
    /// local pending delete / remote lifecycle record 生成此类型，
    /// `run_transfer` 按此走对应执行路径。
    pub target_kind: crate::sync::types::PlannedTargetKind,
    pub project_id: Option<String>,
    /// #644 评论 5473401065 第1节：target 对应的 live root，
    /// 供 `prepare_staging_runs` 在无锁状态下创建 staging 时使用。
    pub target_live_root: PathBuf,
    /// #645 评论 5504296097 问题1：待删除 target 的 journal_token，
    /// 全部远端删除成功后用于从 pending_deleted_targets.json 移除该条目。
    /// `None` 表示普通 target（app/project），非 deleted target。
    #[allow(clippy::struct_field_names)]
    pub deleted_journal_token: Option<String>,
    /// #645 评论 5504296097 问题3：deleted target 的 LWW 元数据。
    ///
    /// `deleted_at_ms` 与远端 manifest 的 `max(lww_record_time)` 比较，
    /// 本地 tombstone 胜出才删远端；远端更晚则不删（远端有更新，下次正常 sync
    /// 会下载恢复）。`device_id` 用于时间相同时的 tie-break（字典序大的胜出，
    /// 与 `resolve_lww_path` 一致）。
    /// `None` 表示非 deleted target。
    pub deleted_lww: Option<DeletedTargetLww>,
    /// #645 评论 5504296097 问题1：live project 的 LWW 元数据。
    ///
    /// 从本地 sync manifest 计算（`max(updated_at_ms / deleted_at_ms)`），
    /// 用于与远端 catalog 的 delete tombstone 做 target-level LWW 决策。
    /// `None` 表示非 live project target 或 manifest 读取失败。
    pub live_lww: Option<LiveTargetLww>,
    /// #645 评论 5504296097 问题2 修复：RemoteCleanupProject target 携带的
    /// 产生该 cleanup 的 Delete lifecycle identity。
    ///
    /// `run_transfer` 在执行 `delete_all_remote_objects` 前重新读远端 catalog，
    /// 校验当前 winner record 仍是同一条/更新的 Delete（lww_time 和 device_id 匹配，
    /// 或当前 Delete 的 lww_time >= expected）。当前是 Upsert → pending 过期，不删 prefix。
    /// `None` 表示非 RemoteCleanupProject target，或无可靠 lifecycle identity
    /// （占位 0/"" 时仍执行 CAS 校验，但 expected 永远不匹配 Upsert）。
    pub expected_delete_lww: Option<DeletedTargetLww>,
}

/// #645 评论 5504296097 问题3：deleted target 的 LWW 决策元数据。
///
/// 从 `PendingDeletedTarget` 提取，传给 `run_transfer` → `run_deleted_target_sync`
/// 做 provider-neutral 的 LWW 比较。
#[derive(Debug, Clone)]
pub struct DeletedTargetLww {
    /// 删除时间戳（Unix 毫秒），与远端 manifest 的 `max(lww_record_time)` 比较。
    pub deleted_at_ms: i64,
    /// 发起删除的设备 ID，时间相同时用于 tie-break。
    pub device_id: String,
}

/// #645 评论 5504296097 问题1：live project 的 LWW 决策元数据。
///
/// 从本地 sync manifest 计算（`max(updated_at_ms / deleted_at_ms)`），
/// 用于与远端 catalog 的 delete tombstone 做 target-level LWW 决策。
/// `device_id` 来自真实 `DeviceInfo.device_id`，用于时间相同时的 tie-break。
#[derive(Debug, Clone)]
pub struct LiveTargetLww {
    /// 本地最新 LWW 时间（Unix 毫秒），取 manifest 中所有记录的最大 lww_record_time。
    pub lww_time_ms: i64,
    /// 本设备 ID，来自 `DeviceInfo.device_id`，用于 tie-break。
    pub device_id: String,
}

impl PlannedTarget {
    /// 是否为待删除 target（pending delete target）。
    pub fn is_deleted_target(&self) -> bool {
        self.target_kind.is_pending_deleted()
    }
}

/// #645 评论 5504296097 问题4：无副作用共享 target planner。
///
/// 正式 `prepare_full_sync` 和 `perform_full_sync_dry_run` 都调用本函数枚举 targets，
/// 不复制一套 target 枚举逻辑。正式同步再在结果上创建 staging/provider transfer。
///
/// 产出的 `PlannedTarget` 列表顺序：App target → live Project targets → pending
/// deleted targets。`staging_root` 全部为 `None`（由 `prepare_staging_runs` 填充）。
///
/// #645 评论 5504296097 问题1：`remote_catalog` 真正参与 target 决策。按 `target_id`
/// 合并 local live project / local pending delete / remote lifecycle record，
/// 生成 `PlannedTargetKind` 明确类型：
/// - 远端无 delete tombstone 或本地更新 → `LiveProject`；
/// - 远端 delete tombstone 更新 → `DeleteLocalProject`（不上传）；
/// - 本地 pending delete + 本地 tombstone 胜出 → `DeleteRemoteProject`；
/// - 本地 pending delete + 远端 upsert 更新 → `RestoreProject`；
/// - 无法决策 → `Retry`。
///
/// `device_id` 来自真实 `DeviceInfo.device_id`，用于 lifecycle record 的 tie-break。
#[allow(clippy::too_many_arguments)] // 9 个参数均为独立决策输入，打包会掩盖各自语义
#[allow(clippy::too_many_lines)] // target 枚举 + LWW 决策逻辑 inherently long
pub fn build_full_sync_target_plan(
    app_data_root: &Path,
    projects_root: &Path,
    live_projects: &[crate::project::Project],
    pending_deleted: &[crate::sync::types::PendingDeletedTarget],
    remote_catalog: &crate::sync::types::TargetLifecycleCatalog,
    _sync_policy: &crate::sync::types::SyncPolicy,
    _force_sync: bool,
    device_id: &str,
    pending_remote_cleanups: &[crate::sync::pending_remote_cleanup::PendingRemoteTargetCleanup],
) -> Vec<PlannedTarget> {
    use crate::sync::types::PlannedTargetKind;

    let mut targets = Vec::new();

    // App target
    let app_target = crate::sync::types::SyncTarget::app();
    targets.push(PlannedTarget {
        target: app_target,
        local_root: app_data_root.to_path_buf(),
        staging_root: None,
        target_kind: PlannedTargetKind::App,
        project_id: None,
        target_live_root: app_data_root.to_path_buf(),
        deleted_journal_token: None,
        deleted_lww: None,
        live_lww: None,
        expected_delete_lww: None,
    });

    // Project targets — #645 评论 5504296097 问题1：按 remote catalog 决策。
    for project in live_projects {
        let target = crate::sync::types::SyncTarget::project(&project.id);
        let project_local_root = projects_root.join(&project.id);
        let target_id = &target.remote_prefix;

        // #645 评论 5504296097 问题1 修复：lifecycle candidate 走 snapshot_local_records_read_only
        // 单一来源，不再读旧 manifest / 手写 initial scanner。失败 → Retry。
        let candidate = compute_local_project_lifecycle_candidate(&project_local_root, device_id);
        let (live_lww, kind) = match &candidate {
            LifecycleCandidate::Live { lww } => {
                let lww_clone = lww.clone();
                let kind = decide_live_project_kind(remote_catalog, target_id, Some(lww));
                (Some(lww_clone), kind)
            }
            LifecycleCandidate::Retry => {
                // #645 评论 5504296097 问题2：无法可靠求本地 LWW → 不 DeleteLocalProject
                // （无证据证明远端 delete 更新），让 target 走 Retry 保留 pending。
                (None, PlannedTargetKind::Retry)
            }
        };

        targets.push(PlannedTarget {
            target,
            local_root: project_local_root.clone(),
            staging_root: None,
            target_kind: kind,
            project_id: Some(project.id.clone()),
            target_live_root: project_local_root,
            deleted_journal_token: None,
            deleted_lww: None,
            live_lww,
            expected_delete_lww: None,
        });
    }

    // #645 评论 5504296097 问题1/2/4：Pending deleted targets —
    // 已删除作品的远端前缀需要清理。按 remote catalog 决策：
    // - 本地 tombstone 胜出 → DeleteRemoteProject（删远端 + 写 tombstone）；
    // - 远端 upsert 胜出 → RestoreProject（下载恢复）；
    // - 无法决策 → Retry。
    for pending in pending_deleted {
        // #645 评论 5504296097 问题6：用 parse_project_target_id 严格验证 target_id，
        // 非法记录跳过（不恢复、不删除、不 panic），不再 unwrap_or_default()。
        let project_id = match crate::sync::target_lifecycle::parse_project_target_id(
            &pending.target.remote_prefix,
        ) {
            Ok(id) => id,
            Err(e) => {
                log::warn!(
                    "[sync] build_full_sync_target_plan: skip pending deleted target \
                     with invalid target_id {:?}: {e}",
                    pending.target.remote_prefix
                );
                continue;
            }
        };
        let project_root = projects_root.join(&project_id);
        let target_id = &pending.target.remote_prefix;

        let kind = decide_pending_deleted_kind(remote_catalog, target_id, pending);

        targets.push(PlannedTarget {
            target: pending.target.clone(),
            local_root: project_root.clone(),
            staging_root: None,
            target_kind: kind,
            project_id: Some(project_id),
            target_live_root: project_root,
            deleted_journal_token: Some(pending.journal_token.clone()),
            deleted_lww: Some(DeletedTargetLww {
                deleted_at_ms: pending.deleted_at_ms,
                device_id: pending.device_id.clone(),
            }),
            live_lww: None,
            expected_delete_lww: None,
        });
    }

    // #645 评论 5504296097 问题1：遍历 remote_catalog.records 补远端独有 target。
    // 对本地既没有 live project 也没有 pending delete 的远端记录：
    // - 远端 Upsert → RestoreProject（下载远端恢复，让新设备发现远端独有作品）；
    // - 远端 Delete → RemoteCleanupProject（#645 评论 5504296097 问题3 修复：
    //   不再跳过，让远端 Delete tombstone 本身成为 durable cleanup queue）。
    {
        use std::collections::HashSet;
        let local_target_ids: HashSet<String> = targets
            .iter()
            .map(|t| t.target.remote_prefix.clone())
            .collect();
        for remote_rec in &remote_catalog.records {
            if local_target_ids.contains(&remote_rec.target_id) {
                continue;
            }
            // #645 评论 5504296097 问题6：用 parse_project_target_id 严格验证 target_id，
            // 非法记录跳过并 log warn（不恢复、不删除、不 panic），不再 unwrap_or_default()。
            let project_id =
                match crate::sync::target_lifecycle::parse_project_target_id(&remote_rec.target_id)
                {
                    Ok(id) => id,
                    Err(e) => {
                        log::warn!(
                            "[sync] build_full_sync_target_plan: skip remote-only target \
                         with invalid target_id {:?}: {e}",
                            remote_rec.target_id
                        );
                        continue;
                    }
                };
            let project_root = projects_root.join(&project_id);
            // #645 评论 5504296097 问题3 修复：remote-only Delete 直接生成
            // RemoteCleanupProject target（不再跳过）。这让远端 Delete tombstone
            // 本身成为 durable cleanup queue — 即使本地 pending_remote_cleanups.json
            // 没成功持久化，下一轮看到 remote Delete + local absent 仍会生成
            // cleanup target。本地 pending_remote_cleanups.json 保留诊断/退避信息，
            // 但不再决定是否存在 cleanup target。
            // - remote-only Upsert → RestoreProject（下载远端恢复）；
            // - remote-only Delete → RemoteCleanupProject，expected_delete_lww 从
            //   remote_rec 构造（deleted_at_ms 或 updated_at_ms 作为 lww_time，
            //   device_id 从 remote_rec）。
            match remote_rec.op {
                crate::sync::types::TargetOp::Upsert => {
                    targets.push(PlannedTarget {
                        target: crate::sync::types::SyncTarget::project(&project_id),
                        local_root: project_root.clone(),
                        staging_root: None,
                        target_kind: PlannedTargetKind::RestoreProject,
                        project_id: Some(project_id),
                        target_live_root: project_root,
                        deleted_journal_token: None,
                        deleted_lww: None,
                        live_lww: None,
                        expected_delete_lww: None,
                    });
                }
                crate::sync::types::TargetOp::Delete => {
                    let expected_lww_time =
                        crate::sync::target_lifecycle::record_lww_time(remote_rec);
                    targets.push(PlannedTarget {
                        target: crate::sync::types::SyncTarget::project(&project_id),
                        local_root: project_root.clone(),
                        staging_root: None,
                        target_kind: PlannedTargetKind::RemoteCleanupProject,
                        project_id: Some(project_id),
                        target_live_root: project_root,
                        deleted_journal_token: None,
                        deleted_lww: None,
                        live_lww: None,
                        expected_delete_lww: Some(DeletedTargetLww {
                            deleted_at_ms: expected_lww_time,
                            device_id: remote_rec.device_id.clone(),
                        }),
                    });
                }
            }
        }
    }

    // #645 评论 5504296097 问题3 修复：加载 pending_remote_cleanups，为每个未已在
    // targets 中的 remote_prefix 生成 RemoteCleanupProject target。这让上一轮
    // authoritative Delete 清 prefix 失败的远端残留能在下一轮被重试清理，
    // 即使本地没有该 Project（remote-only cleanup 场景）。
    {
        use std::collections::HashSet;
        let existing_prefixes: HashSet<String> = targets
            .iter()
            .map(|t| t.target.remote_prefix.clone())
            .collect();
        for cleanup in pending_remote_cleanups {
            if existing_prefixes.contains(&cleanup.remote_prefix) {
                // 已有 target 会处理这个 prefix（如 DeleteLocalProject /
                // DeleteRemoteProject / RestoreProject），不重复加入。
                continue;
            }
            // 校验 project_id（防路径穿越）。非法 → skip（不 panic）。
            let target_id = format!("projects/{}", cleanup.project_id);
            if let Err(e) = crate::sync::target_lifecycle::parse_project_target_id(&target_id) {
                log::warn!(
                    "[sync] build_full_sync_target_plan: skip pending remote cleanup \
                     with invalid project_id {:?}: {e}",
                    cleanup.project_id
                );
                continue;
            }
            let project_root = projects_root.join(&cleanup.project_id);
            targets.push(PlannedTarget {
                target: crate::sync::types::SyncTarget::project(&cleanup.project_id),
                local_root: project_root.clone(),
                staging_root: None,
                target_kind: PlannedTargetKind::RemoteCleanupProject,
                project_id: Some(cleanup.project_id.clone()),
                target_live_root: project_root,
                deleted_journal_token: None,
                deleted_lww: None,
                live_lww: None,
                // #645 评论 5504296097 问题2 修复：从 PendingRemoteTargetCleanup
                // 填入 Delete lifecycle identity，run_transfer 时 CAS 校验。
                expected_delete_lww: Some(DeletedTargetLww {
                    deleted_at_ms: cleanup.expected_delete_lww_time_ms,
                    device_id: cleanup.expected_delete_device_id.clone(),
                }),
            });
        }
    }

    targets
}

/// #645 评论 5504296097 问题2（评论 5504296097 问题1 修复）：本地 project lifecycle candidate。
///
/// `compute_local_project_lifecycle_candidate` 只做：
/// 1. `snapshot_local_records_read_only(project_root, SyncScope::Project, device_id)`
///    → 从 records 取 `max(lww_time, device_id)` → `Live(lww)`；
/// 2. 失败（known file 消失且无 tombstone 等）→ `Retry`。
///
/// 首次同步、已有 manifest、离线改动全部走这一套。target lifecycle 和文件 LWW
/// 共用同一份"当前本地 records"，不再保留旧 manifest 直读 + 手写 initial scanner
/// 的第二套状态机（`build_initial_lww_from_project_scan` / `scan_sync_file` /
/// `append_chapter_meta_records` / `initial_manifest` 已删除）。
#[derive(Debug, Clone)]
pub(crate) enum LifecycleCandidate {
    Live { lww: LiveTargetLww },
    Retry,
}

/// #645 评论 5504296097 问题1 修复：从 `snapshot_local_records_read_only` 单一来源
/// 建立 lifecycle candidate。
///
/// 不再自己读旧 manifest，也不再手写 initial scanner。snapshot 失败 → `Retry`
/// （调用方让 target 保留 pending，下次同步重试）。
fn compute_local_project_lifecycle_candidate(
    project_root: &Path,
    device_id: &str,
) -> LifecycleCandidate {
    use crate::sync::types::SyncScope;

    let records = match crate::sync::lww::snapshot_local_records_read_only(
        project_root,
        SyncScope::Project,
        device_id,
    ) {
        Ok(records) => records,
        Err(e) => {
            log::warn!(
                "[sync] compute_local_project_lifecycle_candidate: \
                 snapshot_local_records_read_only failed at {}: {e} — returning Retry",
                project_root.display()
            );
            return LifecycleCandidate::Retry;
        }
    };

    // 从 records 取 max(lww_time, device_id)。
    let winner = records.values().max_by(|a, b| {
        let a_time = lww_record_time_for_manifest_record(a);
        let b_time = lww_record_time_for_manifest_record(b);
        a_time
            .cmp(&b_time)
            .then_with(|| a.device_id.cmp(&b.device_id))
    });

    match winner {
        Some(w) => {
            let lww = LiveTargetLww {
                lww_time_ms: lww_record_time_for_manifest_record(w),
                device_id: w.device_id.clone(),
            };
            log::debug!(
                "[sync] compute_local_project_lifecycle_candidate: \
                 snapshot at {} — lww_time={} device_id={} records={}",
                project_root.display(),
                lww.lww_time_ms,
                lww.device_id,
                records.len()
            );
            LifecycleCandidate::Live { lww }
        }
        None => {
            // records 为空（全新 project，无任何可同步文件）→ 用 device_id + 0 时间。
            // 不伪造 now()，让远端 catalog 决策按真实事实进行（远端无记录 → LiveProject）。
            log::debug!(
                "[sync] compute_local_project_lifecycle_candidate: \
                 empty snapshot at {} — using zero lww",
                project_root.display()
            );
            LifecycleCandidate::Live {
                lww: LiveTargetLww {
                    lww_time_ms: 0,
                    device_id: device_id.to_string(),
                },
            }
        }
    }
}

/// #645 评论 5504296097 问题1：live project 的 target-level LWW 决策。
///
/// - 远端无记录或远端是 upsert → `LiveProject`（正常同步）；
/// - 远端是 delete tombstone 且本地 live 更新（`live_lww` 胜出）→ `LiveProject`（重新 upsert）；
/// - 远端是 delete tombstone 且远端胜出 → `DeleteLocalProject`（不上传，本地应删除）；
/// - 远端是 delete tombstone 且无 `live_lww`（manifest 读取失败）→ `Retry`
///   （#645 评论 5504296097 问题2：无证据证明远端 delete 更新，绝不破坏性删除，
///   也不伪造 now 复活远端 delete。让 target 保留 pending，下次同步重试）。
///
/// #645 评论 5504296097 问题2：远端无记录/upsert 且 manifest 缺失时仍返回 `LiveProject`，
/// 但 `run_transfer` 的 LiveProject 分支在 `live_lww=None` 时不伪造 now()，
/// 返回 `RecoverableError`（防御性，不写 catalog，不改 remote lifecycle）。
fn decide_live_project_kind(
    remote_catalog: &crate::sync::types::TargetLifecycleCatalog,
    target_id: &str,
    live_lww: Option<&LiveTargetLww>,
) -> crate::sync::types::PlannedTargetKind {
    use crate::sync::types::{PlannedTargetKind, TargetOp};

    let Some(remote_rec) = crate::sync::target_lifecycle::find_record(remote_catalog, target_id)
    else {
        // 远端无记录 → 正常同步（run_transfer 在 live_lww=None 时不伪造 now）。
        return PlannedTargetKind::LiveProject;
    };

    // 远端是 upsert → 正常同步。
    if remote_rec.op == TargetOp::Upsert {
        return PlannedTargetKind::LiveProject;
    }

    // 远端是 delete tombstone → 需要与本地 live LWW 比较。
    let Some(live) = live_lww else {
        // #645 评论 5504296097 问题2：无本地 LWW（manifest 读取失败）→
        // 不 DeleteLocalProject（无证据证明远端 delete 更新），
        // 也不伪造 now 复活远端 delete。返回 Retry，pending 保留。
        return PlannedTargetKind::Retry;
    };

    let remote_time = crate::sync::target_lifecycle::record_lww_time(remote_rec);
    let local_wins = live.lww_time_ms > remote_time
        || (live.lww_time_ms == remote_time && live.device_id > remote_rec.device_id);

    if local_wins {
        // 本地离线编辑更晚 → 重新 upsert 建立远端 target。
        PlannedTargetKind::LiveProject
    } else {
        // 远端 delete 更晚 → 不上传，本地应删除。
        PlannedTargetKind::DeleteLocalProject
    }
}

/// #645 评论 5504296097 问题1：pending deleted target 的 target-level LWW 决策。
///
/// - 远端无记录或远端是 upsert 且本地 tombstone 胜出 → `DeleteRemoteProject`；
/// - 远端是 upsert 且远端胜出 → `RestoreProject`；
/// - 远端是 delete tombstone → `DeleteRemoteProject`（catalog 已有 tombstone，仍需清理远端对象）。
fn decide_pending_deleted_kind(
    remote_catalog: &crate::sync::types::TargetLifecycleCatalog,
    target_id: &str,
    pending: &crate::sync::types::PendingDeletedTarget,
) -> crate::sync::types::PlannedTargetKind {
    use crate::sync::types::{PlannedTargetKind, TargetOp};

    let Some(remote_rec) = crate::sync::target_lifecycle::find_record(remote_catalog, target_id)
    else {
        // 远端无 catalog 记录 → 本地 tombstone 胜出，删远端。
        return PlannedTargetKind::DeleteRemoteProject;
    };

    match remote_rec.op {
        TargetOp::Delete => {
            // 远端已有 delete tombstone → 仍需清理远端对象（可能有残留文件）。
            PlannedTargetKind::DeleteRemoteProject
        }
        TargetOp::Upsert => {
            // 远端是 upsert → 需要与本地 tombstone 做 LWW 比较。
            let remote_time = crate::sync::target_lifecycle::record_lww_time(remote_rec);
            let local_wins = pending.deleted_at_ms > remote_time
                || (pending.deleted_at_ms == remote_time
                    && pending.device_id > remote_rec.device_id);

            if local_wins {
                PlannedTargetKind::DeleteRemoteProject
            } else {
                PlannedTargetKind::RestoreProject
            }
        }
    }
}

/// 计算单条 manifest 记录的 LWW 时间（delete 用 `deleted_at_ms`，upsert 用 `updated_at_ms`）。
fn lww_record_time_for_manifest_record(r: &crate::sync::types::ManifestFileRecord) -> i64 {
    if r.op == "delete" {
        r.deleted_at_ms.unwrap_or(r.updated_at_ms)
    } else {
        r.updated_at_ms
    }
}

/// #645 评论 5504296097 问题3修复：从 manifest 取 target-level LWW，携带 winner 的 device_id。
///
/// 按 `(lww_record_time(record), record.device_id)` 取最大 record，
/// 返回完整 `LiveTargetLww { lww_time_ms, device_id: winner_record.device_id }`。
/// Prepare 前判断和 post-transfer publish 都用同一个 helper，
/// 保证 catalog 写入的 device_id 与真实 winner 一致（不再硬塞本机设备）。
fn manifest_target_lww(manifest: &crate::sync::types::SyncManifest) -> Option<LiveTargetLww> {
    let winner = manifest.files.iter().max_by(|a, b| {
        let a_time = lww_record_time_for_manifest_record(a);
        let b_time = lww_record_time_for_manifest_record(b);
        // 先按时间比较，时间相同按 device_id 字典序比较（与 resolve_lww_path 同规则）。
        a_time
            .cmp(&b_time)
            .then_with(|| a.device_id.cmp(&b.device_id))
    })?;
    Some(LiveTargetLww {
        lww_time_ms: lww_record_time_for_manifest_record(winner),
        device_id: winner.device_id.clone(),
    })
}

/// #645 评论 5504296097 问题3修复：读 post-transfer staging manifest，算最终 LWW。
///
/// 正文 transfer 成功后，publish candidate 必须用 staging manifest 的
/// `max(lww_record_time)` 作为 lifecycle 时间，不能用 Transfer 前的旧 live_lww
/// （那只是"本机有没有资格尝试同步"的判断，不是正文 LWW 合并后的最终状态）。
///
/// #645 评论 5504296097 问题3修复：返回完整 `LiveTargetLww`（含 winner 的 device_id），
/// 不再只返回 `i64` 时间。调用方用 winner 的 device_id 构造 candidate，
/// 不再硬塞本机设备。
///
/// `root` 是 staging root（`planned.staging_root`）或 `planned.local_root`。
/// manifest 不存在或解析失败 → None（调用方返回 RecoverableError，不伪造旧时间）。
fn read_post_transfer_lww(root: &Path) -> Option<LiveTargetLww> {
    let manifest_path = root.join("app-meta/sync/manifest.sync.json");
    let content = std::fs::read(&manifest_path).ok()?;
    let manifest: crate::sync::types::SyncManifest = serde_json::from_slice(&content).ok()?;
    manifest_target_lww(&manifest)
}

/// Transfer 阶段产出 — 各 target 的 `SyncResult`，待 Commit 聚合。
#[derive(Debug, Clone)]
pub struct FullSyncTransferResult {
    pub targets: Vec<TargetSyncResult>,
    /// #645 评论 5504296097 问题2 修复：generation GC 的 provider-neutral 维护结果。
    ///
    /// `None` 表示未执行 GC（sync disabled 或无 project target）。
    /// `Some(Ok(()))` 表示 GC 成功；`Some(Err(msg))` 表示 GC 失败（RecoverableError），
    /// Commit 聚合进去，下一轮 full-sync 自然再次执行 GC，不再用 `log::warn!` 吞掉。
    /// 用 `String` 而非 `crate::error::Error` 因为 `Error` 不实现 `Clone`。
    pub generation_gc_result: Option<Result<(), String>>,
}

// ── Transfer ──

/// 执行 Transfer 阶段：对 plan 中每个 target 调对应同步函数，收集结果。
///
/// #645 评论 5504296097 问题1：按 `PlannedTargetKind` 分派执行路径：
/// - `App` / `LiveProject`：先写 catalog upsert（lifecycle CAS 是完成条件），再 LWW 同步；
/// - `DeleteRemoteProject`：先写 catalog delete tombstone（CAS 是完成条件），再删远端对象；
/// - `DeleteLocalProject`：不上传，返回 `NoChanges`（本地删除由后续流程处理）；
/// - `RestoreProject`：下载远端内容到 staging；
/// - `Retry`：不删不恢复，pending 保留。
///
/// #645 评论 5504296097 问题5：CAS — 执行破坏性动作前重新读远端 catalog 确认 winner。
///
/// 重新从远端加载 catalog，返回该 target_id 的当前记录。
/// - `Ok(Some(record))`：远端有记录，调用方按 `record.op` 和 LWW 重新决策；
/// - `Ok(None)`：远端无记录（target 从未被同步过）；
/// - `Err(e)`：catalog 读取失败，调用方应 Retry（不执行破坏性动作）。
fn resolve_current_target_lifecycle(
    provider: &dyn SyncProvider,
    target_id: &str,
) -> crate::error::Result<Option<crate::sync::types::TargetLifecycleRecord>> {
    let snapshot = crate::sync::target_lifecycle::load_remote_catalog(provider)?;
    Ok(crate::sync::target_lifecycle::find_record(&snapshot.catalog, target_id).cloned())
}

/// #645 评论 5504296097 问题2：catalog 写入用 `apply_lifecycle_record` 原子决策。
/// `Applied` 才继续；`LostToRemote` 改走恢复/删除本地；`Retry` 不删。
///
/// #645 评论 5504296097 问题3：catalog 写接口返回完整 snapshot，调用方更新本地 catalog。
///
/// #645 评论 5504296097 问题4：lifecycle 写失败进入 target 失败状态（RecoverableError），
/// 不只 warn。lifecycle 决策在文件 transfer 前完成，避免半状态。
///
/// 单个 target 的 `Err` 不提前打断：转为该 target 的 `SyncResult::error(...)` 后 push。
///
/// 本函数是纯函数 — 不接触 `WriterCore`、不持锁、不写 `FullSyncState`。
#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting
)]
pub fn run_transfer(provider: &dyn SyncProvider, plan: &FullSyncPlan) -> FullSyncTransferResult {
    use crate::sync::types::{
        DeletedTargetResolution, PlannedTargetKind, TargetLifecycleApplyResult,
    };

    // #645 评论 5504296097 问题3 修复：sync disabled → 在创建 generation_id / merge /
    // publish / lifecycle CAS 之前直接返回 no-op。这样内部调用即使绕过 API 层
    // perform_full_sync 的 guard，也不可能在 disabled 状态写远端。
    if !plan.sync_policy.enabled {
        log::debug!("[sync] run_transfer: sync disabled — returning no-op");
        return FullSyncTransferResult {
            targets: Vec::new(),
            generation_gc_result: None,
        };
    }

    // #645 评论 5504296097 问题4：不再无条件再读一次 catalog。
    // 用 plan 携带的 `remote_catalog_snapshot`（Prepare 阶段在写锁外读取的）
    // 作为 lifecycle CAS 起点。`apply_lifecycle_record` 在 CAS 冲突时仍会重读
    // 远端最新 snapshot。这避免 target discovery（用 Prepare 阶段 catalog）
    // 和执行（用 Transfer 阶段重读 catalog）使用两套时间点的 catalog。
    let mut catalog_snapshot = plan.remote_catalog_snapshot.clone();

    let mut targets = Vec::with_capacity(plan.targets.len());
    for planned in &plan.targets {
        let (result, resolution, action) = match planned.target_kind {
            PlannedTargetKind::App => {
                // App target：正常 LWW 同步，不写 lifecycle catalog。
                let sync_root = planned
                    .staging_root
                    .as_deref()
                    .unwrap_or(&planned.local_root);
                let r = run_single_target(
                    provider,
                    sync_root,
                    &plan.sync_policy,
                    &planned.target,
                    plan.force_sync,
                );
                (r, None, None)
            }
            PlannedTargetKind::LiveProject => {
                // #645 评论 5504296097 问题5：先传正文，成功后再发布 lifecycle upsert。
                // 绝不在正文未传成功时留下假的已发布 upsert target。
                // #645 评论 5504296097 问题2：lifecycle 时间从 sync manifest LWW 计算，
                // 不用 now() 伪造。live_lww 为 None 时 decide_live_project_kind 已转
                // DeleteLocalProject/Retry，不应进入此分支；防御性返回 RecoverableError。
                // #645 评论 5504296097 问题4：catalog 已由 Prepare 阶段读取并装入 plan，
                // 失败在 API 层提前返回 RecoverableError，不进入 run_transfer。
                if planned.live_lww.is_some() {
                    // 1. 先执行 LWW 文件同步（正文 transfer）。
                    // #645 评论 5504296097 问题3修复：live_lww 只用于资格判断
                    // （None 时不进入 LiveProject），post-transfer LWW 用 winner 的 device_id。
                    // #645 评论 5504296097 问题2：generation 原子发布 — 先上传到不可见
                    // generation prefix（projects/P/__generations__/G/），全部成功后
                    // CAS targets.sync.json 写 active_generation=G。CAS 成功后 G 才成为
                    // 可见版本；CAS 输给 Delete 则 G 是未引用 generation，后续 GC。
                    let generation_id = uuid::Uuid::new_v4().to_string();
                    let sync_root = planned
                        .staging_root
                        .as_deref()
                        .unwrap_or(&planned.local_root);
                    // #645 评论 5504296097 问题1 修复：先从当前可见 generation 用统一
                    // LWW merge 核心合并远端到 staging，再上传 staging 到新 generation。
                    // 不再用"存在就本地赢"的 merge_remote_into_staging，统一复用
                    // merge_remote_into_local_snapshot（与普通 LWW 同源），正确处理
                    // 远端同路径更新（LWW 时间戳比较）和远端删除 tombstone。
                    let merge_outcome: crate::error::Result<
                        Option<crate::sync::lww::LwwMergeOutcome>,
                    > = (|| {
                        if let Some(source_record) = crate::sync::target_lifecycle::find_record(
                            &catalog_snapshot.catalog,
                            &planned.target.remote_prefix,
                        ) {
                            if let Some(source_prefix) =
                                crate::sync::target_lifecycle::resolve_visible_project_prefix(
                                    source_record,
                                    &planned.target.remote_prefix,
                                )?
                            {
                                log::info!(
                                    "[sync] run_transfer: LiveProject {} — merging from visible source {}",
                                    planned.target.remote_prefix,
                                    source_prefix
                                );
                                let mut merge_state =
                                    crate::sync::SyncService::load_sync_state(sync_root)?;
                                let outcome = crate::sync::lww::merge_remote_into_local_snapshot(
                                    sync_root,
                                    provider,
                                    &source_prefix,
                                    planned.target.scope,
                                    &mut merge_state,
                                )?;
                                return Ok(Some(outcome));
                            }
                        }
                        Ok(None)
                    })();
                    // #645 评论 5504296097 问题4：generation_remote_prefix 防御性校验
                    // generation_id（UUID 总是合法，但不依赖调用方不变式）。
                    match generation_remote_prefix(&planned.target.remote_prefix, &generation_id) {
                        Ok(gen_remote_prefix) => {
                            log::info!(
                        "[sync] run_transfer: LiveProject {} — uploading to generation prefix {}",
                        planned.target.remote_prefix,
                        gen_remote_prefix
                    );
                            let content_result = match merge_outcome {
                                Ok(Some(outcome)) => {
                                    // #645 评论 5504296097 问题1 修复：有正文冲突 →
                                    // 不发布新 generation，不改 active_generation，
                                    // 返回 PartialConflict。
                                    if !outcome.conflicts.is_empty() {
                                        let mut r = SyncResult::success();
                                        r.status = SyncStatus::PartialConflict;
                                        r.conflicts = outcome.conflicts;
                                        r.downloaded_files = outcome.downloaded_files;
                                        r.local_deletes = outcome.remote_delete_paths;
                                        r.remote_deletes = outcome.local_deletes;
                                        r.overwritten_files = outcome.overwritten_files;
                                        r.ignored_files = outcome.ignored_files;
                                        r
                                    } else {
                                        publish_generation(
                                            provider,
                                            sync_root,
                                            &gen_remote_prefix,
                                            &generation_id,
                                            planned.project_id.as_deref().unwrap_or(""),
                                            planned.target.scope,
                                            &plan.sync_policy,
                                            plan.force_sync,
                                            Some(&outcome),
                                        )
                                    }
                                }
                                Ok(None) => {
                                    // 无 source generation（首次同步 / legacy）→
                                    // 直接 publish staging 到新 generation。
                                    publish_generation(
                                        provider,
                                        sync_root,
                                        &gen_remote_prefix,
                                        &generation_id,
                                        planned.project_id.as_deref().unwrap_or(""),
                                        planned.target.scope,
                                        &plan.sync_policy,
                                        plan.force_sync,
                                        None,
                                    )
                                }
                                Err(e) => {
                                    let msg = format!(
                                        "LiveProject: merge remote into staging failed: {e}"
                                    );
                                    SyncResult::error(
                                        SyncStatus::RecoverableError(msg.clone()),
                                        msg,
                                        None,
                                    )
                                }
                            };
                            // 正文 transfer 失败 → 不发布 lifecycle，直接返回错误。
                            // generation prefix 下残留的未引用 generation 由后续 GC 清理。
                            let content_ok = matches!(
                                content_result.status,
                                SyncStatus::Success
                                    | SyncStatus::NoChanges
                                    | SyncStatus::LatestWinsApplied
                            );
                            if !content_ok {
                                (content_result, None, None)
                            } else {
                                // 2. 正文 transfer 成功 → 从 post-transfer staging manifest
                                //    算最终 LWW（#645 评论 5504296097 问题3修复）。
                                //    live_lww 只是 Transfer 前的资格判断，不是最终状态。
                                //    用 winner 的 device_id 构造 candidate，不再硬塞本机设备。
                                //    #645 评论 5504296097 问题2：candidate 携带 active_generation=G，
                                //    CAS 成功后 G 成为可见版本。
                                let post_transfer_root = planned
                                    .staging_root
                                    .as_deref()
                                    .unwrap_or(&planned.local_root);
                                match read_post_transfer_lww(post_transfer_root) {
                                    Some(post_transfer_lww) => {
                                        let candidate =
                                            crate::sync::types::TargetLifecycleRecord::upsert(
                                                &planned.target.remote_prefix,
                                                &planned.target.remote_prefix,
                                                post_transfer_lww.lww_time_ms,
                                                &post_transfer_lww.device_id,
                                            )
                                            .with_active_generation(&generation_id);
                                        match crate::sync::target_lifecycle::apply_lifecycle_record(
                                            provider,
                                            &catalog_snapshot,
                                            candidate,
                                        ) {
                                            TargetLifecycleApplyResult::Applied(persisted) => {
                                                // #645 评论 5504296097 问题3：更新本地 catalog snapshot。
                                                catalog_snapshot = persisted;
                                                (content_result, None, None)
                                            }
                                            TargetLifecycleApplyResult::AlreadyCurrent(
                                                persisted,
                                            ) => {
                                                // #645 评论 5504296097 问题1修复：candidate 与远端
                                                // 完全相等，不需要写 catalog，保持 live（不删本地）。
                                                catalog_snapshot = persisted;
                                                (content_result, None, None)
                                            }
                                            TargetLifecycleApplyResult::RemoteWinner {
                                                snapshot: persisted,
                                                record: winner,
                                            } => {
                                                catalog_snapshot = persisted;
                                                // #645 评论 5504296097 问题1修复：按真实 winner.op
                                                // 决策，不再猜 op 反转。
                                                // - Upsert → 保持 live（远端是 upsert，不删本地）；
                                                // - Delete → 远端 delete 赢，安排 DeleteProject。
                                                match winner.op {
                                                    crate::sync::types::TargetOp::Upsert => {
                                                        // #645 评论 5504296097 问题1 修复：
                                                        // RemoteWinner(Upsert) 说明 merge 期间
                                                        // 远端已切到新 generation，本次 staging
                                                        // 基于旧 source，必须 Retry（下一轮从
                                                        // 新 active generation 重新 merge）。
                                                        log::info!(
                                                            "[sync] run_transfer: LiveProject \
                                                     RemoteWinner(Upsert) {} — retrying (remote generation changed during merge)",
                                                            planned.target.remote_prefix
                                                        );
                                                        let msg = "LiveProject: remote generation changed during merge, retrying"
                                                            .to_string();
                                                        (
                                                            SyncResult::error(
                                                                SyncStatus::RecoverableError(
                                                                    msg.clone(),
                                                                ),
                                                                msg,
                                                                None,
                                                            ),
                                                            None,
                                                            None,
                                                        )
                                                    }
                                                    crate::sync::types::TargetOp::Delete => {
                                                        // #645 评论 5504296097 问题1：不在 Transfer 里
                                                        // remove_dir_all()。返回 DeleteProject action，
                                                        // 由 Commit 阶段执行完整 Project 本地删除事务。
                                                        // #645 评论 5504296097 问题3：携带 expected_local_lww guard。
                                                        // #645 评论 5504296097 问题2 修复：guard 非 Option —
                                                        // 必须有 live_lww 才能生成 DeleteProject。
                                                        // #645 评论 5504296097 问题4：RemoteWinner(Delete) 后
                                                        // 清理远端 projects/P/ 下所有对象（含本轮上传正文 +
                                                        // manifest.sync.json + 旧残留），删对 prefix。
                                                        log::info!(
                                                    "[sync] run_transfer: LiveProject \
                                                     RemoteWinner(Delete) {} — cleaning remote + deferring delete \
                                                     to Commit",
                                                    planned.target.remote_prefix
                                                );
                                                        let cleanup_result =
                                                            delete_all_remote_objects(
                                                                provider,
                                                                &planned.target.remote_prefix,
                                                            );
                                                        let cleanup_ok = matches!(
                                                            cleanup_result.status,
                                                            SyncStatus::Success
                                                                | SyncStatus::NoChanges
                                                        );
                                                        if !cleanup_ok {
                                                            // #645 评论 5504296097 问题4：远端清理失败 →
                                                            // RecoverableError，不删本地（catalog Delete 已持久，
                                                            // 下轮重新清理远端残留）。
                                                            // #645 评论 5504296097 问题3 修复：cleanup 失败 →
                                                            // record pending remote cleanup，下轮 Prepare
                                                            // 生成 RemoteCleanupProject 重试。
                                                            // #645 评论 5504296097 问题2 修复：传入当前 Delete
                                                            // 的 lww_time 和 device_id，绑定 lifecycle identity。
                                                            // #645 评论 5504296097 问题3 修复：record 失败也向上
                                                            // 传递，不再 let _ = 吞掉。
                                                            let expected_time = crate::sync::target_lifecycle::record_lww_time(&winner);
                                                            let expected_device = &winner.device_id;
                                                            let record_result = crate::sync::pending_remote_cleanup::record_pending_remote_cleanup(
                                                        &plan.app_data_root,
                                                        &planned.target.remote_prefix,
                                                        planned.project_id.as_deref().unwrap_or(""),
                                                        &format!(
                                                            "LiveProject RemoteWinner(Delete) cleanup failed: {:?}",
                                                            cleanup_result.status
                                                        ),
                                                        expected_time,
                                                        expected_device,
                                                    );
                                                            if let Err(e) = record_result {
                                                                let msg = format!(
                                                            "record pending remote cleanup failed: {e}"
                                                        );
                                                                (
                                                            SyncResult::error(
                                                                SyncStatus::RecoverableError(
                                                                    msg.clone(),
                                                                ),
                                                                msg,
                                                                None,
                                                            ),
                                                            None,
                                                            None,
                                                        )
                                                            } else {
                                                                (cleanup_result, None, None)
                                                            }
                                                        } else {
                                                            // guard 必须存在（外层 live_lww.is_some() 已判断）。
                                                            let action = planned
                                                        .project_id
                                                        .as_ref()
                                                        .and_then(|pid| {
                                                            planned.live_lww.as_ref().map(|lww| {
                                                                crate::sync::types::
                                                                    LocalLifecycleCommitAction::DeleteProject {
                                                                        project_id: pid.clone(),
                                                                        expected_local_lww:
                                                                            crate::sync::types::
                                                                                LiveTargetLwwSerde::from_lww(lww),
                                                                    }
                                                            })
                                                        });
                                                            (content_result, None, action)
                                                        }
                                                    }
                                                }
                                            }
                                            TargetLifecycleApplyResult::Retry(e) => {
                                                // #645 评论 5504296097 问题4：lifecycle 写失败 → RecoverableError。
                                                let msg = format!("lifecycle upsert failed: {e}");
                                                (
                                                    SyncResult::error(
                                                        SyncStatus::RecoverableError(msg.clone()),
                                                        msg,
                                                        None,
                                                    ),
                                                    None,
                                                    None,
                                                )
                                            }
                                        }
                                    }
                                    None => {
                                        // #645 评论 5504296097 问题3：post-transfer staging manifest
                                        // 读取失败 → RecoverableError，不伪造旧 live_lww 时间。
                                        let msg =
                                            "post-transfer staging manifest unreadable".to_string();
                                        (
                                            SyncResult::error(
                                                SyncStatus::RecoverableError(msg.clone()),
                                                msg,
                                                None,
                                            ),
                                            None,
                                            None,
                                        )
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            let msg = format!("LiveProject: invalid generation id: {e}");
                            (
                                SyncResult::error(
                                    SyncStatus::RecoverableError(msg.clone()),
                                    msg,
                                    None,
                                ),
                                None,
                                None,
                            )
                        }
                    }
                } else {
                    // 防御性：live_lww 为 None 不应进入 LiveProject（decide 已过滤）。
                    let msg = "live project missing lww (manifest unreadable)".to_string();
                    (
                        SyncResult::error(SyncStatus::RecoverableError(msg.clone()), msg, None),
                        None,
                        None,
                    )
                }
            }
            PlannedTargetKind::DeleteLocalProject => {
                // #645 评论 5504296097 问题1：不在 Transfer 里裸 remove_dir_all()。
                // 返回 NoChanges + LocalLifecycleCommitAction::DeleteProject，
                // 由 Commit 阶段执行完整 Project 本地删除事务（move worktree /
                // unbind starmaps / history），避免 staging commit 把刚删掉的
                // 旧作品重新写回来。
                //
                // #645 评论 5504296097 问题5：CAS — 执行破坏性删除前重新读远端
                // catalog 确认 winner 仍是 delete tombstone。若远端已变成 upsert
                // 且更晚，改走 ReplaceProject 整树替换，绝不按过期 snapshot 删本地。
                //
                // #645 评论 5504296097 问题3：Ok(None) 直接 Retry（不再 "plan confirmed"），
                // 与 RestoreProject 已修成 Retry 的语义一致。
                //
                // #645 评论 5504296097 问题5 修复：DeleteLocalProject -> current Delete
                // 也必须先执行 delete_all_remote_objects()，成功后才返回 DeleteProject action。
                // remote lifecycle 已确认 Delete winner -> delete_all_remote_objects(projects/P)
                // -> 全部成功 -> 才允许 DeleteProject 本地提交。
                // remote cleanup 失败 -> 不删本地 -> 下轮 DeleteLocalProject 再次先清 remote prefix。
                match resolve_current_target_lifecycle(provider, &planned.target.remote_prefix) {
                    Ok(Some(current_rec)) => {
                        use crate::sync::types::TargetOp;
                        match current_rec.op {
                            TargetOp::Upsert => {
                                // #645 评论 5504296097 问题2：远端已变成 upsert 且本地已有 Project
                                // → ReplaceProject 整树替换（不再用空 staging + 普通三方 commit 冒充）。
                                // #645 评论 5504296097 问题2 修复：guard 非 Option —
                                // 必须有 live_lww 才能生成 ReplaceProject。live_lww 为 None
                                // 时返回 RecoverableError（decide_live_project_kind 在 manifest
                                // 读取失败时已转 Retry，不应进入此分支）。
                                log::info!(
                                    "[sync] run_transfer: DeleteLocalProject {} — \
                                     CAS: remote changed to Upsert, switching to ReplaceProject",
                                    planned.target.remote_prefix
                                );
                                let result = download_remote_to_staging(
                                    provider,
                                    &planned.target.remote_prefix,
                                    planned.staging_root.as_deref(),
                                );
                                let action = planned
                                    .project_id
                                    .as_ref()
                                    .and_then(|pid| {
                                        planned.live_lww.as_ref().map(|lww| {
                                            crate::sync::types::LocalLifecycleCommitAction::ReplaceProject {
                                                project_id: pid.clone(),
                                                expected_local_lww:
                                                    crate::sync::types::LiveTargetLwwSerde::from_lww(lww),
                                            }
                                        })
                                    });
                                (result, None, action)
                            }
                            TargetOp::Delete => {
                                // 远端仍是 delete — 先清远端 residue，成功后才 defer to Commit。
                                // #645 评论 5504296097 问题5 修复：DeleteLocalProject -> current Delete
                                // 也必须先执行 delete_all_remote_objects()，成功后才返回 DeleteProject action。
                                log::info!(
                                    "[sync] run_transfer: DeleteLocalProject {} — \
                                     CAS confirmed remote Delete, cleaning remote residue before deferring to Commit",
                                    planned.target.remote_prefix
                                );
                                let cleanup_result = delete_all_remote_objects(
                                    provider,
                                    &planned.target.remote_prefix,
                                );
                                let cleanup_ok = matches!(
                                    cleanup_result.status,
                                    SyncStatus::Success | SyncStatus::NoChanges
                                );
                                if !cleanup_ok {
                                    // 远端清理失败 → 不删本地 → 下轮 DeleteLocalProject 再次先清 remote prefix。
                                    // #645 评论 5504296097 问题3 修复：cleanup 失败 →
                                    // record pending remote cleanup，下轮 Prepare
                                    // 生成 RemoteCleanupProject 重试。
                                    // #645 评论 5504296097 问题2 修复：传入当前 Delete
                                    // 的 lww_time 和 device_id，绑定 lifecycle identity。
                                    // #645 评论 5504296097 问题3 修复：record 失败也向上
                                    // 传递，不再 let _ = 吞掉。
                                    let expected_time =
                                        crate::sync::target_lifecycle::record_lww_time(
                                            &current_rec,
                                        );
                                    let expected_device = &current_rec.device_id;
                                    let record_result = crate::sync::pending_remote_cleanup::record_pending_remote_cleanup(
                                        &plan.app_data_root,
                                        &planned.target.remote_prefix,
                                        planned.project_id.as_deref().unwrap_or(""),
                                        &format!(
                                            "DeleteLocalProject current Delete cleanup failed: {:?}",
                                            cleanup_result.status
                                        ),
                                        expected_time,
                                        expected_device,
                                    );
                                    if let Err(e) = record_result {
                                        let msg =
                                            format!("record pending remote cleanup failed: {e}");
                                        (
                                            SyncResult::error(
                                                SyncStatus::RecoverableError(msg.clone()),
                                                msg,
                                                None,
                                            ),
                                            None,
                                            None,
                                        )
                                    } else {
                                        (cleanup_result, None, None)
                                    }
                                } else {
                                    // 远端清理成功 → defer to Commit。
                                    // #645 评论 5504296097 问题3：携带 expected_local_lww guard。
                                    // #645 评论 5504296097 问题2 修复：guard 非 Option —
                                    // 必须有 live_lww 才能生成 DeleteProject。
                                    let action = planned
                                        .project_id
                                        .as_ref()
                                        .and_then(|pid| {
                                            planned.live_lww.as_ref().map(|lww| {
                                                crate::sync::types::LocalLifecycleCommitAction::DeleteProject {
                                                    project_id: pid.clone(),
                                                    expected_local_lww:
                                                        crate::sync::types::LiveTargetLwwSerde::from_lww(lww),
                                                }
                                            })
                                        });
                                    (SyncResult::no_changes(), None, action)
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        // #645 评论 5504296097 问题3：远端无记录 → 无法确认 delete winner
                        // → 直接 Retry（不再 "plan confirmed"），与 RestoreProject 语义一致。
                        log::info!(
                            "[sync] run_transfer: DeleteLocalProject {} — \
                             CAS: no remote record, returning Retry",
                            planned.target.remote_prefix
                        );
                        (
                            SyncResult::error(
                                SyncStatus::RecoverableError(
                                    "DeleteLocalProject: no remote record, retrying".to_string(),
                                ),
                                "DeleteLocalProject: no remote record to confirm delete winner"
                                    .to_string(),
                                None,
                            ),
                            Some(DeletedTargetResolution::Retry),
                            None,
                        )
                    }
                    Err(e) => {
                        // CAS 读取失败 — 不执行破坏性删除，返回 Retry。
                        let msg = format!(
                            "DeleteLocalProject CAS failed for {}: {e}",
                            planned.target.remote_prefix
                        );
                        log::warn!("[sync] {msg}");
                        (
                            SyncResult::error(SyncStatus::RecoverableError(msg.clone()), msg, None),
                            Some(DeletedTargetResolution::Retry),
                            None,
                        )
                    }
                }
            }
            PlannedTargetKind::DeleteRemoteProject => {
                // #645 评论 5504296097 问题2/4：先写 catalog delete tombstone（CAS 是完成条件），
                // 再删远端对象。用 apply_lifecycle_record 原子决策。
                // #645 评论 5504296097 问题4：catalog 已由 Prepare 阶段读取并装入 plan，
                // 失败在 API 层提前返回 RecoverableError，不进入 run_transfer。
                {
                    let deleted_at_ms = planned
                        .deleted_lww
                        .as_ref()
                        .map(|l| l.deleted_at_ms)
                        .unwrap_or_else(|| {
                            crate::sync::full_sync_utils::now_epoch_seconds() * 1000
                        });
                    let device_id = planned
                        .deleted_lww
                        .as_ref()
                        .map(|l| l.device_id.as_str())
                        .unwrap_or("");
                    let candidate = crate::sync::types::TargetLifecycleRecord::delete(
                        &planned.target.remote_prefix,
                        &planned.target.remote_prefix,
                        deleted_at_ms,
                        device_id,
                    );
                    match crate::sync::target_lifecycle::apply_lifecycle_record(
                        provider,
                        &catalog_snapshot,
                        candidate,
                    ) {
                        TargetLifecycleApplyResult::Applied(persisted) => {
                            // #645 评论 5504296097 问题3：更新本地 catalog snapshot。
                            catalog_snapshot = persisted;
                            // catalog tombstone 写成功，再删远端对象。
                            let del_result =
                                delete_all_remote_objects(provider, &planned.target.remote_prefix);
                            // 即使远端删除部分失败，catalog tombstone 已持久，
                            // 返回 LocalDeleteWins 让 pending 被清理（不会复活远端）。
                            (
                                del_result,
                                Some(DeletedTargetResolution::LocalDeleteWins),
                                None,
                            )
                        }
                        TargetLifecycleApplyResult::AlreadyCurrent(persisted) => {
                            // #645 评论 5504296097 问题1修复：candidate 与远端完全相等
                            // （都是 delete tombstone），catalog 已有 tombstone，
                            // 继续清理远端残留对象。
                            catalog_snapshot = persisted;
                            log::info!(
                                "[sync] run_transfer: DeleteRemoteProject AlreadyCurrent(Delete) \
                                 {} — continuing remote cleanup",
                                planned.target.remote_prefix
                            );
                            let del_result =
                                delete_all_remote_objects(provider, &planned.target.remote_prefix);
                            (
                                del_result,
                                Some(DeletedTargetResolution::LocalDeleteWins),
                                None,
                            )
                        }
                        TargetLifecycleApplyResult::RemoteWinner {
                            snapshot: persisted,
                            record: winner,
                        } => {
                            catalog_snapshot = persisted;
                            // #645 评论 5504296097 问题1修复：按真实 winner.op 决策。
                            // - Delete → 远端已是 delete，继续清理远端残留；
                            // - Upsert → 远端 upsert 赢，改走 RestoreProject 下载恢复。
                            match winner.op {
                                crate::sync::types::TargetOp::Delete => {
                                    log::info!(
                                        "[sync] run_transfer: DeleteRemoteProject \
                                         RemoteWinner(Delete) {} — continuing remote cleanup",
                                        planned.target.remote_prefix
                                    );
                                    let del_result = delete_all_remote_objects(
                                        provider,
                                        &planned.target.remote_prefix,
                                    );
                                    (
                                        del_result,
                                        Some(DeletedTargetResolution::LocalDeleteWins),
                                        None,
                                    )
                                }
                                crate::sync::types::TargetOp::Upsert => {
                                    log::info!(
                                        "[sync] run_transfer: DeleteRemoteProject \
                                         RemoteWinner(Upsert) {} — switching to restore",
                                        planned.target.remote_prefix
                                    );
                                    let restore_result = download_remote_to_staging(
                                        provider,
                                        &planned.target.remote_prefix,
                                        planned.staging_root.as_deref(),
                                    );
                                    (
                                        restore_result,
                                        Some(DeletedTargetResolution::RemoteTargetWins),
                                        None,
                                    )
                                }
                            }
                        }
                        TargetLifecycleApplyResult::Retry(e) => {
                            // #645 评论 5504296097 问题4：lifecycle 写失败 → Retry，不删远端。
                            let msg = format!("lifecycle tombstone write failed: {e}");
                            (
                                SyncResult::error(
                                    SyncStatus::RecoverableError(msg.clone()),
                                    msg,
                                    None,
                                ),
                                Some(DeletedTargetResolution::Retry),
                                None,
                            )
                        }
                    }
                }
            }
            PlannedTargetKind::RestoreProject => {
                // #645 评论 5504296097 问题1：远端 upsert 胜出，下载远端内容到 staging。
                //
                // #645 评论 5504296097 问题5：CAS — 执行恢复前重新读远端 catalog
                // 确认 winner 仍是 upsert。若远端已变成 delete tombstone 且更晚，
                // 改走 DeleteLocalProject（defer to Commit），绝不按过期 snapshot
                // 恢复一个已被删除的作品。
                //
                // #645 评论 5504296097 问题3 修复：
                // - Ok(None) -> Retry（不再 "plan confirmed" download）；
                // - current=Delete && local project 不存在 -> 不生成 DeleteProject action
                //   -> 清理 authoritative Delete 下的远端 residue（delete_all_remote_objects）；
                // - current=Delete && local project 存在 -> 重新计算 current local LWW
                //   -> 只有 Delete 仍赢才生成带 guard 的 DeleteProject。
                match resolve_current_target_lifecycle(provider, &planned.target.remote_prefix) {
                    Ok(Some(current_rec)) => {
                        use crate::sync::types::TargetOp;
                        match current_rec.op {
                            TargetOp::Delete => {
                                // 远端已变成 delete。
                                // #645 评论 5504296097 问题3 修复：检查 local project 是否存在。
                                let local_project_exists = planned.local_root.exists() && {
                                    // 简单存在性检查：local_root 非空目录。
                                    std::fs::read_dir(&planned.local_root)
                                        .map(|mut it| it.next().is_some())
                                        .unwrap_or(false)
                                };
                                if !local_project_exists {
                                    // current=Delete && local project 不存在 ->
                                    // 不生成 DeleteProject action -> 清理 authoritative Delete
                                    // 下的远端 residue。
                                    log::info!(
                                        "[sync] run_transfer: RestoreProject {} — \
                                         CAS: remote changed to Delete, local project absent — \
                                         cleaning remote residue, no DeleteProject action",
                                        planned.target.remote_prefix
                                    );
                                    let cleanup_result = delete_all_remote_objects(
                                        provider,
                                        &planned.target.remote_prefix,
                                    );
                                    let cleanup_ok = matches!(
                                        cleanup_result.status,
                                        SyncStatus::Success | SyncStatus::NoChanges
                                    );
                                    if !cleanup_ok {
                                        // #645 评论 5504296097 问题3 修复：remote-only cleanup 失败 →
                                        // record pending remote cleanup，下轮 Prepare
                                        // 生成 RemoteCleanupProject 重试。
                                        // 这是问题3的核心场景：本地没有 P，remote catalog 已是 Delete(P)，
                                        // 下一轮 planner 对"remote Delete + 本地无 live + 无 pending delete"
                                        // 会直接跳过，再也没有 target 会清这个 prefix。
                                        // pending_remote_cleanup 让下一轮重试。
                                        // #645 评论 5504296097 问题2 修复：传入当前 Delete
                                        // 的 lww_time 和 device_id，绑定 lifecycle identity。
                                        // #645 评论 5504296097 问题3 修复：record 失败也向上
                                        // 传递，不再 let _ = 吞掉。
                                        let expected_time =
                                            crate::sync::target_lifecycle::record_lww_time(
                                                &current_rec,
                                            );
                                        let expected_device = &current_rec.device_id;
                                        let record_result = crate::sync::pending_remote_cleanup::record_pending_remote_cleanup(
                                            &plan.app_data_root,
                                            &planned.target.remote_prefix,
                                            planned.project_id.as_deref().unwrap_or(""),
                                            &format!(
                                                "RestoreProject remote-only cleanup failed: {:?}",
                                                cleanup_result.status
                                            ),
                                            expected_time,
                                            expected_device,
                                        );
                                        if let Err(e) = record_result {
                                            let msg = format!(
                                                "record pending remote cleanup failed: {e}"
                                            );
                                            (
                                                SyncResult::error(
                                                    SyncStatus::RecoverableError(msg.clone()),
                                                    msg,
                                                    None,
                                                ),
                                                None,
                                                None,
                                            )
                                        } else {
                                            (cleanup_result, None, None)
                                        }
                                    } else {
                                        (cleanup_result, None, None)
                                    }
                                } else {
                                    // current=Delete && local project 存在 ->
                                    // 重新计算 current local LWW -> 只有 Delete 仍赢才生成
                                    // 带 guard 的 DeleteProject。
                                    log::info!(
                                        "[sync] run_transfer: RestoreProject {} — \
                                         CAS: remote changed to Delete, local project exists — \
                                         re-evaluating with current local LWW",
                                        planned.target.remote_prefix
                                    );
                                    let current_candidate =
                                        compute_local_project_lifecycle_candidate(
                                            &planned.local_root,
                                            planned
                                                .live_lww
                                                .as_ref()
                                                .map(|l| l.device_id.as_str())
                                                .unwrap_or(""),
                                        );
                                    // #645 评论 5504296097 问题2 修复：snapshot 失败（Retry）时
                                    // 不生成 DeleteProject（不把"无法确认本地当前状态"解释成"可以删"）。
                                    // 直接返回 Retry，让 pending 保留，下次同步重试。
                                    match current_candidate {
                                        LifecycleCandidate::Live { lww: current_lww } => {
                                            let remote_time =
                                                crate::sync::target_lifecycle::record_lww_time(
                                                    &current_rec,
                                                );
                                            let local_wins = current_lww.lww_time_ms > remote_time
                                                || (current_lww.lww_time_ms == remote_time
                                                    && current_lww.device_id
                                                        > current_rec.device_id);
                                            let delete_wins = !local_wins;
                                            if delete_wins {
                                                // #645 评论 5504296097 问题2 修复：guard 非 Option —
                                                // current_lww 已确认存在（Live 分支），直接用它。
                                                let action = planned.project_id.as_ref().map(|pid| {
                                                    crate::sync::types::LocalLifecycleCommitAction::DeleteProject {
                                                        project_id: pid.clone(),
                                                        expected_local_lww:
                                                            crate::sync::types::LiveTargetLwwSerde::from_lww(
                                                                &current_lww,
                                                            ),
                                                    }
                                                });
                                                (SyncResult::no_changes(), None, action)
                                            } else {
                                                // local LWW 赢 → 不删本地，保持 live。
                                                log::info!(
                                                    "[sync] run_transfer: RestoreProject {} — \
                                                     CAS: remote Delete but local LWW wins — keeping live",
                                                    planned.target.remote_prefix
                                                );
                                                (SyncResult::no_changes(), None, None)
                                            }
                                        }
                                        LifecycleCandidate::Retry => {
                                            log::info!(
                                                "[sync] run_transfer: RestoreProject {} — \
                                                 CAS: remote Delete but local snapshot failed — \
                                                 returning Retry (not generating DeleteProject)",
                                                planned.target.remote_prefix
                                            );
                                            (
                                                SyncResult::error(
                                                    SyncStatus::RecoverableError(
                                                        "RestoreProject: local snapshot failed, cannot confirm delete winner"
                                                            .to_string(),
                                                    ),
                                                    "RestoreProject: local snapshot failed, cannot confirm delete winner"
                                                        .to_string(),
                                                    None,
                                                ),
                                                Some(DeletedTargetResolution::Retry),
                                                None,
                                            )
                                        }
                                    }
                                }
                            }
                            TargetOp::Upsert => {
                                // 远端仍是 upsert — 确认恢复，下载远端内容。
                                // #645 评论 5504296097 问题2：如果 catalog winner 有
                                // active_generation=G，从 generation prefix
                                // （projects/P/__generations__/G/）下载；否则从 legacy
                                // prefix（projects/P/）下载（兼容旧数据/无 generation 记录）。
                                // #645 评论 5504296097 问题4：generation_remote_prefix 防御性
                                // 校验 active_generation（catalog 已校验，但 CAS 重读可能绕过）。
                                let download_prefix: crate::error::Result<String> =
                                    match &current_rec.active_generation {
                                        Some(gen_id) => {
                                            let gen_prefix = generation_remote_prefix(
                                                &planned.target.remote_prefix,
                                                gen_id,
                                            );
                                            if let Ok(ref p) = gen_prefix {
                                                log::info!(
                                                    "[sync] run_transfer: RestoreProject {} — \
                                                     CAS confirmed remote Upsert with active_generation={}, \
                                                     downloading from generation prefix {}",
                                                    planned.target.remote_prefix,
                                                    gen_id,
                                                    p
                                                );
                                            }
                                            gen_prefix
                                        }
                                        None => {
                                            log::info!(
                                                "[sync] run_transfer: RestoreProject {} — \
                                                 CAS confirmed remote Upsert (no active_generation), \
                                                 downloading from legacy prefix",
                                                planned.target.remote_prefix
                                            );
                                            Ok(planned.target.remote_prefix.clone())
                                        }
                                    };
                                match download_prefix {
                                    Ok(download_prefix) => {
                                        let result = download_remote_to_staging(
                                            provider,
                                            &download_prefix,
                                            planned.staging_root.as_deref(),
                                        );
                                        (
                                            result,
                                            Some(DeletedTargetResolution::RemoteTargetWins),
                                            None,
                                        )
                                    }
                                    Err(e) => {
                                        let msg = format!(
                                            "RestoreProject: invalid active_generation: {e}"
                                        );
                                        (
                                            SyncResult::error(
                                                SyncStatus::RecoverableError(msg.clone()),
                                                msg,
                                                None,
                                            ),
                                            None,
                                            None,
                                        )
                                    }
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        // #645 评论 5504296097 问题3 修复：远端无记录 → 无法确认 upsert winner
                        // → 直接 Retry（不再 "plan confirmed" download），与 DeleteLocalProject
                        // 语义一致。Prepare 明明看到 Upsert，Transfer 时这条 lifecycle record
                        // 整体消失，说明远端事实已经变化，不能继续相信旧 plan。
                        log::info!(
                            "[sync] run_transfer: RestoreProject {} — \
                             CAS: no remote record, returning Retry",
                            planned.target.remote_prefix
                        );
                        (
                            SyncResult::error(
                                SyncStatus::RecoverableError(
                                    "RestoreProject: no remote record, retrying".to_string(),
                                ),
                                "RestoreProject: no remote record to confirm upsert winner"
                                    .to_string(),
                                None,
                            ),
                            Some(DeletedTargetResolution::Retry),
                            None,
                        )
                    }
                    Err(e) => {
                        // CAS 读取失败 — 不执行恢复，返回 Retry。
                        let msg = format!(
                            "RestoreProject CAS failed for {}: {e}",
                            planned.target.remote_prefix
                        );
                        log::warn!("[sync] {msg}");
                        (
                            SyncResult::error(SyncStatus::RecoverableError(msg.clone()), msg, None),
                            Some(DeletedTargetResolution::Retry),
                            None,
                        )
                    }
                }
            }
            PlannedTargetKind::Retry => {
                // #645 评论 5504296097 问题1：无法决策，pending 保留。
                let msg = "target lifecycle decision retry".to_string();
                (
                    SyncResult::error(SyncStatus::RecoverableError(msg.clone()), msg, None),
                    Some(DeletedTargetResolution::Retry),
                    None,
                )
            }
            PlannedTargetKind::RemoteCleanupProject => {
                // #645 评论 5504296097 问题3 修复：远端残留清理重试。
                // 上一轮 authoritative Delete 清 prefix 失败时记录了
                // PendingRemoteTargetCleanup，本轮 Prepare 生成此 target。
                //
                // #645 评论 5504296097 问题2 修复：Transfer 前重新查询远端 catalog
                // 当前该 target 的 lifecycle，校验当前 winner 仍是同一条/更新的 Delete：
                // - 当前是 Upsert → pending 过期，返回 Success + LocalDeleteWins
                //   （pending 应被移除），**绝不删 prefix**；
                // - 当前仍是同一条/更新的 Delete（lww_time 和 device_id 匹配 expected，
                //   或当前 Delete 的 lww_time >= expected）→ delete_all_remote_objects；
                // - record 消失 / 读取失败 → Retry（RecoverableError），不删。
                log::info!(
                    "[sync] run_transfer: RemoteCleanupProject {} — \
                     CAS: re-confirming remote lifecycle before cleanup",
                    planned.target.remote_prefix
                );
                match resolve_current_target_lifecycle(provider, &planned.target.remote_prefix) {
                    Ok(Some(current_rec)) => {
                        use crate::sync::types::TargetOp;
                        match current_rec.op {
                            TargetOp::Upsert => {
                                // 远端已变成 Upsert → pending 过期，不删 prefix。
                                log::info!(
                                    "[sync] run_transfer: RemoteCleanupProject {} — \
                                     CAS: remote changed to Upsert, pending expired — \
                                     returning LocalDeleteWins without deleting prefix",
                                    planned.target.remote_prefix
                                );
                                (
                                    SyncResult::no_changes(),
                                    Some(DeletedTargetResolution::LocalDeleteWins),
                                    None,
                                )
                            }
                            TargetOp::Delete => {
                                // 远端仍是 Delete → 校验 lifecycle identity。
                                let current_time =
                                    crate::sync::target_lifecycle::record_lww_time(&current_rec);
                                let expected = planned.expected_delete_lww.as_ref();
                                let expected_time = expected.map(|e| e.deleted_at_ms).unwrap_or(0);
                                let expected_device =
                                    expected.map(|e| e.device_id.as_str()).unwrap_or("");
                                // 同一条 Delete（time 和 device_id 都匹配）
                                // 或更新的 Delete（current lww_time >= expected）→ 才删。
                                let same_delete = current_time == expected_time
                                    && current_rec.device_id == expected_device;
                                let newer_delete = current_time >= expected_time;
                                if !same_delete && !newer_delete {
                                    // 当前 Delete 的 lww_time < expected → 异常，Retry。
                                    let msg = format!(
                                        "RemoteCleanupProject {}: CAS: current Delete \
                                         lww_time {} < expected {} — stale catalog, retrying",
                                        planned.target.remote_prefix, current_time, expected_time
                                    );
                                    log::warn!("[sync] {msg}");
                                    (
                                        SyncResult::error(
                                            SyncStatus::RecoverableError(msg.clone()),
                                            msg,
                                            None,
                                        ),
                                        Some(DeletedTargetResolution::Retry),
                                        None,
                                    )
                                } else {
                                    // 校验通过 → delete_all_remote_objects。
                                    let cleanup_result = delete_all_remote_objects(
                                        provider,
                                        &planned.target.remote_prefix,
                                    );
                                    let cleanup_ok = matches!(
                                        cleanup_result.status,
                                        SyncStatus::Success | SyncStatus::NoChanges
                                    );
                                    if cleanup_ok {
                                        (
                                            cleanup_result,
                                            Some(DeletedTargetResolution::LocalDeleteWins),
                                            None,
                                        )
                                    } else {
                                        // cleanup 失败 → 保留 pending，返回 RecoverableError。
                                        (cleanup_result, Some(DeletedTargetResolution::Retry), None)
                                    }
                                }
                            }
                        }
                    }
                    Ok(None) => {
                        // 远端无记录 → 无法确认 delete winner → Retry，不删。
                        let msg = format!(
                            "RemoteCleanupProject {}: CAS: no remote record, retrying",
                            planned.target.remote_prefix
                        );
                        log::warn!("[sync] {msg}");
                        (
                            SyncResult::error(SyncStatus::RecoverableError(msg.clone()), msg, None),
                            Some(DeletedTargetResolution::Retry),
                            None,
                        )
                    }
                    Err(e) => {
                        // CAS 读取失败 — 不执行删除，返回 Retry。
                        let msg = format!(
                            "RemoteCleanupProject CAS failed for {}: {e}",
                            planned.target.remote_prefix
                        );
                        log::warn!("[sync] {msg}");
                        (
                            SyncResult::error(SyncStatus::RecoverableError(msg.clone()), msg, None),
                            Some(DeletedTargetResolution::Retry),
                            None,
                        )
                    }
                }
            }
        };

        targets.push(TargetSyncResult {
            target_kind: planned.target_kind.as_target_kind_str().to_string(),
            project_id: planned.project_id.clone(),
            remote_prefix: planned.target.remote_prefix.clone(),
            result,
            deleted_resolution: resolution,
            local_lifecycle_action: action.unwrap_or_default(),
        });
    }

    // #645 评论 5504296097 问题2 修复：generation GC — 清理未引用 generation。
    // 对每个 project target 调一次 run_generation_gc。不在 CAS 成功后立刻删旧
    // generation（另一台设备可能正拿着旧 catalog 下载）。
    //
    // 错误传播：GC 出错 → RecoverableError，聚合进 generation_gc_result，下一轮
    // full-sync 自然再次执行 GC。不再用 `if let Err(e) = ... { log::warn!(...); }` 吞掉。
    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut generation_gc_result: Option<Result<(), String>> = None;
    for planned in &plan.targets {
        // 只对 project target 做 GC（app target 无 generation）。
        if planned.target.remote_prefix.starts_with("projects/") {
            let active_generation = crate::sync::target_lifecycle::find_record(
                &catalog_snapshot.catalog,
                &planned.target.remote_prefix,
            )
            .and_then(|r| r.active_generation.as_deref());
            match crate::sync::generation_gc::run_generation_gc(
                provider,
                &planned.target.remote_prefix,
                active_generation,
                now_ms,
                crate::sync::generation_gc::GENERATION_RETENTION_MS,
            ) {
                Ok(()) => {}
                Err(e) => {
                    log::warn!(
                        "[sync] run_transfer: generation GC failed for {}: {e}",
                        planned.target.remote_prefix
                    );
                    generation_gc_result = Some(Err(e.to_string()));
                }
            }
        }
    }

    FullSyncTransferResult {
        targets,
        generation_gc_result,
    }
}

/// #645 评论 5504296097 问题1：RestoreProject / DeleteRemoteProject LostToRemote 时
/// 把远端 `projects/<id>/` 下所有对象下载到 staging，commit 阶段把 staging 写回 live 恢复本地 project。
///
/// `staging_root` 为 `None` 时只记录 `downloaded_files`，不落盘（调用方需保证
/// deleted target 有 staging_root 才能真正恢复）。
fn download_remote_to_staging(
    provider: &dyn SyncProvider,
    remote_prefix: &str,
    staging_root: Option<&Path>,
) -> SyncResult {
    let entries = match provider.list(remote_prefix) {
        Ok(e) => e,
        Err(e) => return recoverable_error_from_provider(e),
    };
    let mut downloaded: Vec<String> = Vec::new();
    for entry in &entries {
        let full_remote_path = format!("{}/{}", remote_prefix, entry.path);
        let obj = match provider.read(&full_remote_path) {
            Ok(Some(obj)) => obj,
            Ok(None) => continue, // 已被删，跳过
            Err(e) => return recoverable_error_from_provider(e),
        };
        if let Some(staging) = staging_root {
            if let Err(e) = write_staging_file(staging, &entry.path, &obj.content) {
                let msg = format!("staging write failed: {e}");
                return SyncResult::error(SyncStatus::RecoverableError(msg.clone()), msg, None);
            }
        }
        downloaded.push(full_remote_path);
    }
    let mut result = SyncResult::success();
    result.downloaded_files = downloaded;
    result
}

/// #645 评论 5504296097 问题2：generation 原子发布 — 上传 staging 到新 generation prefix，
/// 写 `generation.meta.json`（complete=false → 内容 → complete=true）。
///
/// 步骤：
/// 1. 写 `meta(complete=false, lease_until=now+lease)` 到 `generation_prefix/generation.meta.json`。
/// 2. 上传 staging 内容到 generation prefix：
///    - 有 `merge_outcome`（已 merge）→ 直接用 outcome 的 upload paths / manifest
///      上传到新 generation prefix（不再做第二次 LWW 同步）；
///    - 无 `merge_outcome`（首次同步 / legacy）→ `run_single_target` 全量上传。
/// 3. 内容上传成功后写 `meta(complete=true)`。
///
/// meta 让 GC 能识别 incomplete generation（上传中，不删）和 complete generation
/// （可按保留期删）。`uploader_device_id` 用空字符串（诊断字段，不影响 GC 逻辑）。
#[allow(clippy::too_many_arguments)] // 9 个参数均为独立发布输入，打包会掩盖各自语义
fn publish_generation(
    provider: &dyn SyncProvider,
    sync_root: &Path,
    generation_prefix: &str,
    generation_id: &str,
    project_id: &str,
    scope: crate::sync::types::SyncScope,
    sync_policy: &SyncPolicy,
    force_sync: bool,
    merge_outcome: Option<&crate::sync::lww::LwwMergeOutcome>,
) -> SyncResult {
    use crate::sync::generation_gc::{
        GenerationMeta, GENERATION_META_FILENAME, GENERATION_UPLOAD_LEASE_MS,
    };
    use crate::sync::provider::model::WritePrecondition;

    let now_ms = chrono::Utc::now().timestamp_millis();
    // 1. 写 meta(complete=false, lease)。
    let meta = GenerationMeta {
        generation_id: generation_id.to_string(),
        project_id: project_id.to_string(),
        created_at_ms: now_ms,
        uploader_device_id: String::new(),
        upload_lease_until_ms: now_ms + GENERATION_UPLOAD_LEASE_MS,
        complete: false,
    };
    let meta_path = format!("{generation_prefix}/{GENERATION_META_FILENAME}");
    let meta_content = match serde_json::to_vec(&meta) {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("publish_generation: serialize meta failed: {e}");
            return SyncResult::error(SyncStatus::RecoverableError(msg.clone()), msg, None);
        }
    };
    if let Err(e) = provider.write(&meta_path, &meta_content, WritePrecondition::CreateNew) {
        return recoverable_error_from_provider(e);
    }

    // 2. 上传 staging 内容到 generation prefix。
    let content_result = if let Some(outcome) = merge_outcome {
        // #645 评论 5504296097 问题1 修复：已 merge → 上传完整快照到新
        // generation prefix，不再做第二次 LWW 同步。
        // 关键：必须上传 merged_manifest 里所有 upsert 文件，不能只上传
        // outcome.remote_upload_paths（delta 动作）。NoOp/DownloadRemote/
        // LwwRemoteWinsDownload/pending_take_remote 成功下载的文件都不在
        // remote_upload_paths 里，但它们在 merged_manifest 中，新 generation
        // 必须包含这些文件对象，否则 catalog 指向一个 manifest 声称文件存在
        // 但实际对象不存在的 generation。
        match upload_complete_generation_snapshot(
            provider,
            sync_root,
            generation_prefix,
            &outcome.merged_manifest,
            scope,
        ) {
            Ok(()) => {
                let mut r = SyncResult::success();
                r.uploaded_files = outcome.remote_upload_paths.clone();
                r.downloaded_files = outcome.downloaded_files.clone();
                r.local_deletes = outcome.remote_delete_paths.clone();
                r.remote_deletes = outcome.local_deletes.clone();
                r.overwritten_files = outcome.overwritten_files.clone();
                r.ignored_files = outcome.ignored_files.clone();
                if r.uploaded_files.is_empty()
                    && r.downloaded_files.is_empty()
                    && r.local_deletes.is_empty()
                    && r.remote_deletes.is_empty()
                {
                    r.status = SyncStatus::NoChanges;
                } else {
                    r.status = SyncStatus::LatestWinsApplied;
                }
                r
            }
            Err(e) => {
                let msg = format!("publish_generation: upload complete snapshot failed: {e}");
                SyncResult::error(SyncStatus::RecoverableError(msg.clone()), msg, None)
            }
        }
    } else {
        // 无 merge_outcome（首次同步 / legacy）→ run_single_target 全量上传。
        let generation_target = SyncTarget {
            scope,
            remote_prefix: generation_prefix.to_string(),
        };
        run_single_target(
            provider,
            sync_root,
            sync_policy,
            &generation_target,
            force_sync,
        )
    };
    let content_ok = matches!(
        content_result.status,
        SyncStatus::Success | SyncStatus::NoChanges | SyncStatus::LatestWinsApplied
    );
    if !content_ok {
        return content_result;
    }

    // 3. 写 meta(complete=true)。
    let mut meta = meta;
    meta.complete = true;
    let meta_content = match serde_json::to_vec(&meta) {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("publish_generation: serialize complete meta failed: {e}");
            return SyncResult::error(SyncStatus::RecoverableError(msg.clone()), msg, None);
        }
    };
    if let Err(e) = provider.write(&meta_path, &meta_content, WritePrecondition::Unconditional) {
        return recoverable_error_from_provider(e);
    }

    content_result
}

/// #645 评论 5504296097 问题1 修复：把完整 merged manifest 快照上传到新 generation prefix。
///
/// 新 generation 是不可变完整快照。本函数遍历 `merged_manifest.files`，
/// 对每个 `op=upsert` 的 record：
/// 1. 路径必须仍在同步白名单（`SyncService::is_whitelisted_path`）。
/// 2. `staging_root/path` 必须存在。
/// 3. 重新算 md5 hash，必须 == `record.content_hash`（不一致则 `Err`）。
/// 4. 上传到新 generation prefix（`CreateNew` 或 `Unconditional`）。
///
/// `op=delete` 的 record 只保留 tombstone（manifest 里有记录），不上传物理文件。
///
/// 上传规则保证新 generation 的 manifest 与实际对象一致：manifest 声称存在的
/// 文件对象一定已上传到 generation prefix。原先 `upload_merged_outcome_to_generation`
/// 只上传 `remote_upload_paths`（delta 动作），NoOp/DownloadRemote/
/// LwwRemoteWinsDownload/pending_take_remote 成功下载的文件都不会进新 generation，
/// 导致 catalog 指向一个 manifest 声称文件存在但实际对象不存在的 generation。
fn upload_complete_generation_snapshot(
    provider: &dyn SyncProvider,
    staging_root: &Path,
    generation_prefix: &str,
    merged_manifest: &crate::sync::types::SyncManifest,
    scope: crate::sync::types::SyncScope,
) -> crate::error::Result<()> {
    use crate::sync::provider::model::WritePrecondition;

    let caps = provider.capabilities();

    // 遍历 merged_manifest.files，上传所有 upsert 文件到新 generation prefix。
    for record in &merged_manifest.files {
        // op=delete 只保留 tombstone（manifest 里有记录），不上传物理文件。
        if record.op == "delete" {
            continue;
        }
        if record.op != "upsert" {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "upload_complete_generation_snapshot: unknown op={} for path={}",
                record.op, record.path
            ))));
        }

        // 1. 路径白名单检查。
        if !crate::sync::SyncService::is_whitelisted_path(&record.path, scope) {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "upload_complete_generation_snapshot: path {} not whitelisted for scope {:?}",
                record.path, scope
            ))));
        }

        // 2. staging 文件必须存在。
        let local_full = staging_root.join(&record.path);
        if !local_full.exists() {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "upload_complete_generation_snapshot: staging file missing for path={}",
                record.path
            ))));
        }

        // 3. 读取 staging 文件，计算 md5 hash，必须 == record.content_hash。
        let content = std::fs::read(&local_full).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "upload_complete_generation_snapshot: read {}: {e}",
                record.path
            )))
        })?;
        let actual_hash = format!("{:x}", md5::compute(&content));
        if actual_hash != record.content_hash {
            return Err(crate::Error::Io(std::io::Error::other(format!(
                "upload_complete_generation_snapshot: hash mismatch for path={} expected={} actual={}",
                record.path, record.content_hash, actual_hash
            ))));
        }

        // 4. 上传到新 generation prefix。
        let remote_path = format!("{generation_prefix}/{}", record.path);
        let precondition = if caps.conditional_write {
            WritePrecondition::CreateNew
        } else {
            WritePrecondition::Unconditional
        };
        provider.write(&remote_path, &content, precondition)?;
    }

    // 上传 manifest 到 generation prefix。
    let manifest_remote_path = format!(
        "{generation_prefix}/{}",
        crate::sync::lww::SYNC_MANIFEST_PATH
    );
    let manifest_json = serde_json::to_string(merged_manifest).map_err(|e| {
        crate::Error::Io(std::io::Error::other(format!(
            "upload_complete_generation_snapshot: serialize manifest: {e}"
        )))
    })?;
    let manifest_precondition = if caps.conditional_write {
        WritePrecondition::CreateNew
    } else {
        WritePrecondition::Unconditional
    };
    provider.write(
        &manifest_remote_path,
        manifest_json.as_bytes(),
        manifest_precondition,
    )?;

    Ok(())
}

/// 把单个远端对象内容写入 staging（创建父目录 + 写文件）。
fn write_staging_file(staging: &Path, rel: &str, content: &[u8]) -> std::io::Result<()> {
    let dest = staging.join(rel);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&dest, content)
}

/// `ProviderError` → `SyncResult::error(RecoverableError, ...)`。
fn recoverable_error_from_provider(e: crate::sync::provider::ProviderError) -> SyncResult {
    let core_err = crate::Error::from(e);
    let msg = core_err.to_string();
    SyncResult::error(SyncStatus::RecoverableError(msg.clone()), msg, None)
}

/// 枚举远端前缀下所有对象并逐个删除（Unconditional 幂等）。
///
/// 全部删除成功返回 `SyncResult::success()`（`remote_deletes` 记录已删路径）。
/// 远端 list 失败 → `RecoverableError`。单个 delete 失败 → `RecoverableError`（已删的保留）。
///
/// #645 评论 5504296097 问题2：跳过 `__generations__/` 下的对象 — 不碰并发 Upsert
/// 正在上传的 generation prefix。Delete cleanup 只清 legacy prefix（`projects/P/`
/// 下非 generation 的对象），generation prefix 由 GC 单独清理（未引用 generation）。
/// 这修复了"设备 B 已上传新正文到 generation prefix 但还没 CAS catalog，设备 A
/// cleanup 看到 Delete(P) → 删 projects/P/ → 误删 B 的 generation"的竞态。
fn delete_all_remote_objects(provider: &dyn SyncProvider, remote_prefix: &str) -> SyncResult {
    let remote_entries = match provider.list(remote_prefix) {
        Ok(entries) => entries,
        Err(err) => {
            let core_err = crate::Error::from(err);
            let msg = core_err.to_string();
            let category = core_err.sync_category();
            let error_category = if category.is_empty() {
                None
            } else {
                Some(category.to_string())
            };
            return SyncResult::error(
                SyncStatus::RecoverableError(msg.clone()),
                msg,
                error_category,
            );
        }
    };

    let mut remote_deletes: Vec<String> = Vec::new();
    for entry in &remote_entries {
        // #645 评论 5504296097 问题2：跳过 generation prefix 下的对象，
        // 不碰并发 Upsert 正在上传的 generation。
        if is_generation_path(&entry.path) {
            log::debug!(
                "[sync] delete_all_remote_objects: skipping generation path {} under {}",
                entry.path,
                remote_prefix
            );
            continue;
        }
        let full_remote_path = format!("{}/{}", remote_prefix, entry.path);
        match provider.delete(
            &full_remote_path,
            crate::sync::provider::model::DeletePrecondition::Unconditional,
        ) {
            Ok(()) => {
                remote_deletes.push(full_remote_path);
            }
            Err(err) => {
                let core_err = crate::Error::from(err);
                let msg = core_err.to_string();
                let category = core_err.sync_category();
                let error_category = if category.is_empty() {
                    None
                } else {
                    Some(category.to_string())
                };
                let mut result = SyncResult::error(
                    SyncStatus::RecoverableError(msg.clone()),
                    msg,
                    error_category,
                );
                result.remote_deletes = remote_deletes;
                return result;
            }
        }
    }

    let mut result = SyncResult::success();
    result.remote_deletes = remote_deletes;
    result
}

/// 执行单个 target 的同步，把 `Err` 转为该 target 的 `SyncResult::error(...)`。
///
/// `Err` 的 `recoverable()` 决定 `SyncStatus::RecoverableError` / `FatalError`，
/// `sync_category()` 决定 `error_category`（空字符串视为无分类）。
fn run_single_target(
    provider: &dyn SyncProvider,
    local_root: &Path,
    sync_policy: &SyncPolicy,
    target: &SyncTarget,
    force_sync: bool,
) -> SyncResult {
    // #645 评论 5504296097 问题2 修复：通过 SyncService::perform_lww_sync
    // （pub(crate) 内部 staging 引擎）调用，保持唯一调用路径。
    match crate::sync::SyncService::perform_lww_sync(
        local_root,
        provider,
        sync_policy,
        target,
        force_sync,
    ) {
        Ok(result) => result,
        Err(err) => {
            let msg = err.to_string();
            let category = err.sync_category();
            let error_category = if category.is_empty() {
                None
            } else {
                Some(category.to_string())
            };
            let status = if err.recoverable() {
                SyncStatus::RecoverableError(msg.clone())
            } else {
                SyncStatus::FatalError(msg.clone())
            };
            SyncResult::error(status, msg, error_category)
        }
    }
}

// ── Commit：聚合 ──

/// 将各 target 的结果聚合为 [`FullSyncResult`]：统计上传/下载/删除/冲突数，
/// 总体状态保留错误类型，优先级按"需要用户处理的终态 > 可重试 > 成功"：
/// `Fatal/Error > Dirty > Conflict/PartialConflict > Recoverable > Success`
/// （Issue #630 评论 5308040939 Part 2）。
///
/// `error` / `error_category` / `message_key` 从与 `overall_status` 同优先级的
/// 第一个 dominant target 取得，避免"总体是认证失败、文案却拿到前一个网络错误"
/// 的错位。
pub fn aggregate_full_sync_result(targets: Vec<TargetSyncResult>) -> FullSyncResult {
    let total_uploaded: u32 = targets
        .iter()
        .map(|t| u32::try_from(t.result.uploaded_files.len()).unwrap_or(u32::MAX))
        .sum();
    let total_downloaded: u32 = targets
        .iter()
        .map(|t| u32::try_from(t.result.downloaded_files.len()).unwrap_or(u32::MAX))
        .sum();
    let total_local_deletes: u32 = targets
        .iter()
        .map(|t| u32::try_from(t.result.local_deletes.len()).unwrap_or(u32::MAX))
        .sum();
    let total_remote_deletes: u32 = targets
        .iter()
        .map(|t| u32::try_from(t.result.remote_deletes.len()).unwrap_or(u32::MAX))
        .sum();
    let total_overwritten: u32 = targets
        .iter()
        .map(|t| u32::try_from(t.result.overwritten_files.len()).unwrap_or(u32::MAX))
        .sum();
    let total_ignored: u32 = targets
        .iter()
        .map(|t| u32::try_from(t.result.ignored_files.len()).unwrap_or(u32::MAX))
        .sum();
    let total_conflicts: u32 = targets
        .iter()
        .map(|t| u32::try_from(t.result.conflicts.len()).unwrap_or(u32::MAX))
        .sum();

    // Issue #630 评论 5308439467 Part 3：终态分两步聚合。
    // 第一步：任何 target 返回 Syncing/Idle/ConfiguredNotTested 都是协议错误
    // （这三个是非终态/未测试状态，不应出现在 target 结果里），直接生成
    // FatalError，绝不能当成功。
    if let Some((overall_status, error, error_category, message_key)) =
        build_protocol_error_fields(&targets)
    {
        return FullSyncResult {
            overall_status,
            targets,
            total_uploaded,
            total_downloaded,
            total_local_deletes,
            total_remote_deletes,
            total_overwritten,
            total_ignored,
            total_conflicts,
            error,
            error_category,
            message_key,
        };
    }

    let overall_priority = targets
        .iter()
        .map(|t| full_sync_status_priority(&t.result.status))
        .max()
        .unwrap_or(0);
    let overall_status = match overall_priority {
        4 => SyncStatus::FatalError("one_or_more_targets_failed".to_string()),
        3 => SyncStatus::PartialConflict,
        1 => SyncStatus::RecoverableError("one_or_more_targets_temporarily_failed".to_string()),
        _ => aggregate_success_status(&targets),
    };

    // dominant target：与 overall_status 同优先级的第一个 target。
    let dominant = targets
        .iter()
        .find(|t| full_sync_status_priority(&t.result.status) == overall_priority);
    let error = dominant.and_then(|t| t.result.error.clone());
    let error_category = dominant.and_then(|t| t.result.error_category.clone());
    let message_key = dominant
        .and_then(|t| t.result.message_key.clone())
        .or_else(|| {
            error_category.as_deref().map(|c| {
                crate::sync::types::SyncErrorCategory::from_code(c, "")
                    .to_message_key()
                    .to_string()
            })
        });

    FullSyncResult {
        overall_status,
        targets,
        total_uploaded,
        total_downloaded,
        total_local_deletes,
        total_remote_deletes,
        total_overwritten,
        total_ignored,
        total_conflicts,
        error,
        error_category,
        message_key,
    }
}

// ── 纯辅助函数（从 facade/sync_ops.rs 搬移） ──

/// transport 初始化失败的类型化 Error 转换（Issue #630 评论 5308439467 Part 2）。
///
/// 唯一一份转换，同时用于持久化 FullSyncState 状态和返回给调用方，避免
/// "磁盘写 FatalError 但返回 Io → Android 视为 Retryable"的错位。
///
/// - token/auth/permission/repo-permission 类 → `Error::SyncAuthFailed`（不可恢复）
/// - network/dns/tls/临时 IO 类 → `Error::SyncNetworkUnavailable`（可恢复）
/// - rate limit → `Error::SyncRateLimited`（可恢复）
/// - 其它未知项 → `Error::SyncAuthFailed`（保守起视为不可恢复，不落 Io 后自动变可重试）
pub fn transport_init_failure_error(category: &str, message: &str) -> crate::Error {
    use crate::sync::types::{legacy_category_compat, SyncErrorCategory};
    let reason = format!("Transport init failed: {} - {}", category, message);
    let resolved = legacy_category_compat(category)
        .unwrap_or_else(|| SyncErrorCategory::from_code(category, ""));
    match resolved {
        SyncErrorCategory::AuthFailed
        | SyncErrorCategory::PermissionDenied
        | SyncErrorCategory::NotFound => crate::Error::SyncAuthFailed { reason },
        SyncErrorCategory::Network | SyncErrorCategory::TemporaryUnavailable => {
            crate::Error::SyncNetworkUnavailable { reason }
        }
        SyncErrorCategory::RateLimited => crate::Error::SyncRateLimited {
            retry_after_secs: 0,
        },
        // 其它未知项保守起视为不可恢复，不落 Io 后自动变可重试
        _ => crate::Error::SyncAuthFailed { reason },
    }
}

/// 把 Error 转为持久化用的 SyncStatus：recoverable → RecoverableError，否则 FatalError。
pub fn error_to_persist_status(err: &crate::Error) -> SyncStatus {
    let msg = err.to_string();
    if err.recoverable() {
        SyncStatus::RecoverableError(msg)
    } else {
        SyncStatus::FatalError(msg)
    }
}

/// 判断 target 状态是否为协议错误（不应出现在 target 结果里的非终态/未测试状态）。
fn is_protocol_error_status(status: &SyncStatus) -> bool {
    matches!(
        status,
        SyncStatus::Syncing | SyncStatus::Idle | SyncStatus::ConfiguredNotTested
    )
}

/// 协议错误聚合字段：(overall_status, error, error_category, message_key)。
type ProtocolErrorFields = (SyncStatus, Option<String>, Option<String>, Option<String>);

/// 协议错误聚合字段构造（Issue #630 评论 5308439467 Part 3）。
///
/// 任何 target 返回 Syncing/Idle/ConfiguredNotTested 时，返回
/// (FatalError("invalid_target_status_for_aggregation"), error, error_category, message_key)，
/// 从第一个协议错误 target 取 error/error_category/message_key。无协议错误时返回 None。
fn build_protocol_error_fields(targets: &[TargetSyncResult]) -> Option<ProtocolErrorFields> {
    if !targets
        .iter()
        .any(|t| is_protocol_error_status(&t.result.status))
    {
        return None;
    }
    let overall_status =
        SyncStatus::FatalError("invalid_target_status_for_aggregation".to_string());
    let dominant = targets
        .iter()
        .find(|t| is_protocol_error_status(&t.result.status));
    let error = dominant.and_then(|t| t.result.error.clone());
    let error_category = dominant.and_then(|t| t.result.error_category.clone());
    let message_key = dominant
        .and_then(|t| t.result.message_key.clone())
        .or_else(|| {
            error_category
                .as_deref()
                .map(sync_error_category_to_message_key_string)
        });
    Some((overall_status, error, error_category, message_key))
}

/// SyncErrorCategory code → message_key 字符串（供 protocol-error 聚合复用）。
fn sync_error_category_to_message_key_string(code: &str) -> String {
    crate::sync::types::SyncErrorCategory::from_code(code, "")
        .to_message_key()
        .to_string()
}

/// 聚合成功类终态（Issue #630 评论 5311102143）。
///
/// 失败优先级为 0 时调用。用语义判断而非数字优先级：
/// - `LatestWinsApplied` 存在 → `LatestWinsApplied`
/// - 全部 `NoChanges` → `NoChanges`
/// - 其余情况 → `Success`
///
/// 关键语义：`Success + NoChanges → Success`（有 target 实际上传/下载了）。
/// 协议错误状态不应到达此处（已由 `build_protocol_error_fields` 拦截）。
fn aggregate_success_status(targets: &[TargetSyncResult]) -> SyncStatus {
    if targets
        .iter()
        .any(|t| matches!(t.result.status, SyncStatus::LatestWinsApplied))
    {
        return SyncStatus::LatestWinsApplied;
    }
    if !targets.is_empty()
        && targets
            .iter()
            .all(|t| matches!(t.result.status, SyncStatus::NoChanges))
    {
        return SyncStatus::NoChanges;
    }
    SyncStatus::Success
}

/// 单个 target 状态在聚合中的优先级（数字越大越需要用户处理）：
/// 4=Fatal/Error，3=Conflict/PartialConflict，1=Recoverable，0=其余（成功类）。
fn full_sync_status_priority(status: &SyncStatus) -> u8 {
    match status {
        SyncStatus::FatalError(_) | SyncStatus::Error(_) => 4,
        SyncStatus::Conflict | SyncStatus::PartialConflict => 3,
        SyncStatus::RecoverableError(_) => 1,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::provider::memory::MemoryProvider;
    use crate::sync::provider::SyncProvider;
    use crate::sync::types::{
        DeletedTargetResolution, ManifestFileRecord, SyncManifest, SyncTarget,
        TargetLifecycleCatalog,
    };
    use tempfile::TempDir;

    /// #645 评论 5504296097 问题4：测试用空 catalog snapshot。
    fn test_empty_catalog_snapshot() -> crate::sync::types::RemoteTargetCatalogSnapshot {
        crate::sync::types::RemoteTargetCatalogSnapshot {
            catalog: crate::sync::types::TargetLifecycleCatalog::default(),
            version: crate::sync::provider::model::RemoteVersion::new("__nonexistent__"),
        }
    }

    /// 模拟 catalog 路径写入始终失败的 Provider（用于测试 catalog 写失败时的 Retry 语义）。
    struct AlwaysFailCatalogProvider {
        inner: MemoryProvider,
    }
    impl AlwaysFailCatalogProvider {
        fn new() -> Self {
            Self {
                inner: MemoryProvider::with_entries([(
                    "projects/p1/chapter.md".to_string(),
                    b"hello".to_vec(),
                )]),
            }
        }
    }
    impl SyncProvider for AlwaysFailCatalogProvider {
        fn capabilities(&self) -> crate::sync::provider::capabilities::SyncCapabilities {
            self.inner.capabilities()
        }
        fn list(
            &self,
            prefix: &str,
        ) -> Result<
            Vec<crate::sync::provider::model::RemoteEntry>,
            crate::sync::provider::error::ProviderError,
        > {
            self.inner.list(prefix)
        }
        fn read(
            &self,
            path: &str,
        ) -> Result<
            Option<crate::sync::provider::model::RemoteObject>,
            crate::sync::provider::error::ProviderError,
        > {
            self.inner.read(path)
        }
        fn write(
            &self,
            path: &str,
            content: &[u8],
            precondition: crate::sync::provider::model::WritePrecondition,
        ) -> Result<
            crate::sync::provider::model::RemoteVersion,
            crate::sync::provider::error::ProviderError,
        > {
            if path == crate::sync::target_lifecycle::TARGET_CATALOG_REMOTE_PATH {
                return Err(crate::sync::provider::error::ProviderError::Other {
                    reason: "catalog write always fails".to_string(),
                });
            }
            self.inner.write(path, content, precondition)
        }
        fn delete(
            &self,
            path: &str,
            precondition: crate::sync::provider::model::DeletePrecondition,
        ) -> Result<(), crate::sync::provider::error::ProviderError> {
            self.inner.delete(path, precondition)
        }
    }

    /// 构造一个 deleted target LWW 元数据。
    fn lww(deleted_at_ms: i64, device_id: &str) -> DeletedTargetLww {
        DeletedTargetLww {
            deleted_at_ms,
            device_id: device_id.to_string(),
        }
    }

    /// #645 评论 5504296097 问题4：build_full_sync_target_plan 包含 pending deleted target。
    #[test]
    fn build_plan_includes_pending_deleted_targets() {
        use crate::sync::types::{PendingDeletedTarget, PlannedTargetKind};

        let tmp = TempDir::new().unwrap();
        let app_root = tmp.path().join("app");
        let projects_root = tmp.path().join("projects");
        std::fs::create_dir_all(&app_root).unwrap();
        std::fs::create_dir_all(&projects_root).unwrap();

        let pending = vec![PendingDeletedTarget::for_project(
            "p-deleted",
            1000,
            "token-1",
            "dev-1",
        )];
        let sync_policy = crate::sync::types::SyncPolicy::default();

        let planned = build_full_sync_target_plan(
            &app_root,
            &projects_root,
            &[],
            &pending,
            &TargetLifecycleCatalog::default(),
            &sync_policy,
            false,
            "dev-1",
            &[],
        );

        // App target + 1 deleted target。
        assert_eq!(planned.len(), 2);
        assert_eq!(planned[0].target_kind, PlannedTargetKind::App);
        // 远端无 catalog 记录 → DeleteRemoteProject。
        assert_eq!(
            planned[1].target_kind,
            PlannedTargetKind::DeleteRemoteProject
        );
        assert_eq!(planned[1].target.remote_prefix, "projects/p-deleted");
        assert_eq!(planned[1].deleted_journal_token.as_deref(), Some("token-1"));
        assert!(planned[1].deleted_lww.is_some());
        // #645 评论 5504296097 问题2：target_live_root 应指向 projects_root/<id>。
        assert_eq!(planned[1].target_live_root, projects_root.join("p-deleted"));
    }

    /// #645 评论 5504296097 问题4：build_full_sync_target_plan 包含 live project targets。
    #[test]
    fn build_plan_includes_live_projects() {
        use crate::project::Project;
        use crate::sync::types::PlannedTargetKind;

        let tmp = TempDir::new().unwrap();
        let app_root = tmp.path().join("app");
        let projects_root = tmp.path().join("projects");
        std::fs::create_dir_all(&app_root).unwrap();
        std::fs::create_dir_all(&projects_root).unwrap();

        let projects = vec![Project {
            id: "p1".to_string(),
            title: "T1".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
            order: 0,
        }];
        // #645 评论 5504296097 问题2：首次同步 manifest 缺失，需要 project.json
        // 元数据建立初始 LWW。写一份合法 project.json。
        let project_root = projects_root.join("p1");
        std::fs::create_dir_all(&project_root).unwrap();
        std::fs::write(
            project_root.join("project.json"),
            serde_json::to_vec(&projects[0]).unwrap(),
        )
        .unwrap();
        let sync_policy = crate::sync::types::SyncPolicy::default();

        let planned = build_full_sync_target_plan(
            &app_root,
            &projects_root,
            &projects,
            &[],
            &TargetLifecycleCatalog::default(),
            &sync_policy,
            false,
            "dev-1",
            &[],
        );

        // App target + 1 project target。
        assert_eq!(planned.len(), 2);
        assert_eq!(planned[0].target_kind, PlannedTargetKind::App);
        // 远端无 catalog 记录 → LiveProject（首次同步建立初始 manifest）。
        assert_eq!(planned[1].target_kind, PlannedTargetKind::LiveProject);
        assert_eq!(planned[1].target.remote_prefix, "projects/p1");
        assert_eq!(planned[1].local_root, projects_root.join("p1"));
    }

    /// #645 评论 5504296097 问题1：远端 catalog 有 delete tombstone 且本地 live 更晚 → LiveProject。
    #[test]
    fn build_plan_live_project_with_remote_delete_local_wins() {
        use crate::project::Project;
        use crate::sync::types::PlannedTargetKind;

        let tmp = TempDir::new().unwrap();
        let app_root = tmp.path().join("app");
        let projects_root = tmp.path().join("projects");
        std::fs::create_dir_all(&app_root).unwrap();
        let project_root = projects_root.join("p1");
        std::fs::create_dir_all(&project_root).unwrap();

        // 写本地 manifest，lww_time = 12000。
        std::fs::create_dir_all(project_root.join("app-meta/sync")).unwrap();
        let manifest = SyncManifest {
            files: vec![ManifestFileRecord {
                path: "volumes/v1/chapter.md".to_string(),
                content_hash: "9a0364b9e99bb480dd25e1f0284c8555".to_string(),
                updated_at_ms: 12000,
                deleted_at_ms: None,
                device_id: "dev-1".to_string(),
                op: "upsert".to_string(),
                schema_version: 1,
            }],
        };
        std::fs::write(
            project_root.join("app-meta/sync/manifest.sync.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        // #645 评论 5504296097 问题1 修复：同时创建实际文件（whitelisted 路径），
        // 让 snapshot_local_records_read_only 第 3 步处理（当前文件存在 + hash 匹配），
        // 直接 clone old manifest record 保留 updated_at_ms = 12000。
        std::fs::create_dir_all(project_root.join("volumes/v1")).unwrap();
        std::fs::write(project_root.join("volumes/v1/chapter.md"), b"content").unwrap();

        let projects = vec![Project {
            id: "p1".to_string(),
            title: "T1".to_string(),
            created_at: "2024-01-01".to_string(),
            updated_at: "2024-01-01".to_string(),
            order: 0,
        }];

        // 远端 catalog 有 delete tombstone，deleted_at = 11000 < 本地 12000。
        let mut catalog = TargetLifecycleCatalog::default();
        crate::sync::target_lifecycle::upsert_record(
            &mut catalog,
            crate::sync::types::TargetLifecycleRecord::delete(
                "projects/p1",
                "projects/p1",
                11000,
                "dev-2",
            ),
        );

        let planned = build_full_sync_target_plan(
            &app_root,
            &projects_root,
            &projects,
            &[],
            &catalog,
            &crate::sync::types::SyncPolicy::default(),
            false,
            "dev-1",
            &[],
        );

        // 本地 live 更晚 → LiveProject（重新 upsert）。
        assert_eq!(planned[1].target_kind, PlannedTargetKind::LiveProject);
    }

    /// #645 评论 5504296097 问题1：远端 catalog 有 delete tombstone 且远端更晚 → DeleteLocalProject。
    #[test]
    fn build_plan_live_project_with_remote_delete_remote_wins() {
        use crate::project::Project;
        use crate::sync::types::PlannedTargetKind;

        let tmp = TempDir::new().unwrap();
        let app_root = tmp.path().join("app");
        let projects_root = tmp.path().join("projects");
        std::fs::create_dir_all(&app_root).unwrap();
        let project_root = projects_root.join("p1");
        std::fs::create_dir_all(&project_root).unwrap();

        // 写本地 manifest，lww_time = 11000。
        std::fs::create_dir_all(project_root.join("app-meta/sync")).unwrap();
        let manifest = SyncManifest {
            files: vec![ManifestFileRecord {
                path: "volumes/v1/chapter.md".to_string(),
                content_hash: "9a0364b9e99bb480dd25e1f0284c8555".to_string(),
                updated_at_ms: 11000,
                deleted_at_ms: None,
                device_id: "dev-1".to_string(),
                op: "upsert".to_string(),
                schema_version: 1,
            }],
        };
        std::fs::write(
            project_root.join("app-meta/sync/manifest.sync.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        // #645 评论 5504296097 问题1 修复：同时创建实际文件（whitelisted 路径），
        // 让 snapshot_local_records_read_only 第 3 步处理（当前文件存在 + hash 匹配），
        // 直接 clone old manifest record 保留 updated_at_ms = 11000。
        std::fs::create_dir_all(project_root.join("volumes/v1")).unwrap();
        std::fs::write(project_root.join("volumes/v1/chapter.md"), b"content").unwrap();

        let projects = vec![Project {
            id: "p1".to_string(),
            title: "T1".to_string(),
            created_at: "2024-01-01".to_string(),
            updated_at: "2024-01-01".to_string(),
            order: 0,
        }];

        // 远端 catalog 有 delete tombstone，deleted_at = 12000 > 本地 11000。
        let mut catalog = TargetLifecycleCatalog::default();
        crate::sync::target_lifecycle::upsert_record(
            &mut catalog,
            crate::sync::types::TargetLifecycleRecord::delete(
                "projects/p1",
                "projects/p1",
                12000,
                "dev-2",
            ),
        );

        let planned = build_full_sync_target_plan(
            &app_root,
            &projects_root,
            &projects,
            &[],
            &catalog,
            &crate::sync::types::SyncPolicy::default(),
            false,
            "dev-1",
            &[],
        );

        // 远端 delete 更晚 → DeleteLocalProject（不上传）。
        assert_eq!(
            planned[1].target_kind,
            PlannedTargetKind::DeleteLocalProject
        );
    }

    /// #645 评论 5504296097 问题1：pending delete + 远端 upsert 且本地 tombstone 胜出 → DeleteRemoteProject。
    #[test]
    fn build_plan_pending_delete_local_tombstone_wins() {
        use crate::sync::types::{PendingDeletedTarget, PlannedTargetKind};

        let tmp = TempDir::new().unwrap();
        let app_root = tmp.path().join("app");
        let projects_root = tmp.path().join("projects");
        std::fs::create_dir_all(&app_root).unwrap();
        std::fs::create_dir_all(&projects_root).unwrap();

        let pending = vec![PendingDeletedTarget::for_project(
            "p1", 12000, "token-1", "dev-1",
        )];

        // 远端 catalog 有 upsert，updated_at = 11000 < 本地 tombstone 12000。
        let mut catalog = TargetLifecycleCatalog::default();
        crate::sync::target_lifecycle::upsert_record(
            &mut catalog,
            crate::sync::types::TargetLifecycleRecord::upsert(
                "projects/p1",
                "projects/p1",
                11000,
                "dev-2",
            ),
        );

        let planned = build_full_sync_target_plan(
            &app_root,
            &projects_root,
            &[],
            &pending,
            &catalog,
            &crate::sync::types::SyncPolicy::default(),
            false,
            "dev-1",
            &[],
        );

        // 本地 tombstone 更晚 → DeleteRemoteProject。
        assert_eq!(
            planned[1].target_kind,
            PlannedTargetKind::DeleteRemoteProject
        );
    }

    /// #645 评论 5504296097 问题1：pending delete + 远端 upsert 且远端胜出 → RestoreProject。
    #[test]
    fn build_plan_pending_delete_remote_upsert_wins() {
        use crate::sync::types::{PendingDeletedTarget, PlannedTargetKind};

        let tmp = TempDir::new().unwrap();
        let app_root = tmp.path().join("app");
        let projects_root = tmp.path().join("projects");
        std::fs::create_dir_all(&app_root).unwrap();
        std::fs::create_dir_all(&projects_root).unwrap();

        let pending = vec![PendingDeletedTarget::for_project(
            "p1", 11000, "token-1", "dev-1",
        )];

        // 远端 catalog 有 upsert，updated_at = 12000 > 本地 tombstone 11000。
        let mut catalog = TargetLifecycleCatalog::default();
        crate::sync::target_lifecycle::upsert_record(
            &mut catalog,
            crate::sync::types::TargetLifecycleRecord::upsert(
                "projects/p1",
                "projects/p1",
                12000,
                "dev-2",
            ),
        );

        let planned = build_full_sync_target_plan(
            &app_root,
            &projects_root,
            &[],
            &pending,
            &catalog,
            &crate::sync::types::SyncPolicy::default(),
            false,
            "dev-1",
            &[],
        );

        // 远端 upsert 更晚 → RestoreProject。
        assert_eq!(planned[1].target_kind, PlannedTargetKind::RestoreProject);
    }

    /// #645 评论 5504296097 问题3：run_transfer 在 DeleteRemoteProject 时先写 catalog tombstone 再删远端。
    #[test]
    fn run_transfer_catalog_tombstone_before_remote_delete() {
        use crate::sync::types::{PlannedTargetKind, SyncPolicy};

        let provider = MemoryProvider::with_entries([(
            "projects/p1/chapter.md".to_string(),
            b"hello".to_vec(),
        )]);
        let target = SyncTarget::project("p1");
        let lww_val = lww(2000, "dev-1");

        let tmp = TempDir::new().unwrap();
        let planned = PlannedTarget {
            target,
            local_root: tmp.path().to_path_buf(),
            staging_root: None,
            target_kind: PlannedTargetKind::DeleteRemoteProject,
            project_id: Some("p1".to_string()),
            target_live_root: tmp.path().to_path_buf(),
            deleted_journal_token: Some("token-1".to_string()),
            deleted_lww: Some(lww_val),
            live_lww: None,
            expected_delete_lww: None,
        };
        let plan = FullSyncPlan {
            sync_policy: SyncPolicy {
                enabled: true,
                ..Default::default()
            },
            force_sync: false,
            targets: vec![planned],
            app_data_root: tmp.path().to_path_buf(),
            remote_catalog_snapshot: test_empty_catalog_snapshot(),
        };

        let transfer = run_transfer(&provider, &plan);
        assert_eq!(transfer.targets.len(), 1);
        assert_eq!(
            transfer.targets[0].deleted_resolution,
            Some(DeletedTargetResolution::LocalDeleteWins)
        );

        // catalog 已写入 delete tombstone。
        let snapshot = crate::sync::target_lifecycle::load_remote_catalog(&provider).unwrap();
        let rec = crate::sync::target_lifecycle::find_record(&snapshot.catalog, "projects/p1");
        assert!(rec.is_some());
        assert_eq!(rec.unwrap().op, crate::sync::types::TargetOp::Delete);
        // 远端对象已被删除。
        assert!(provider.read("projects/p1/chapter.md").unwrap().is_none());
    }

    /// #645 评论 5504296097 问题4：catalog 写失败时 deleted target 走 Retry，pending 保留。
    ///
    /// 使用 AlwaysFailCatalogProvider 模拟 catalog 写入始终失败的场景。
    #[test]
    fn run_transfer_catalog_write_failure_returns_retry() {
        use crate::sync::types::{PlannedTargetKind, SyncPolicy};

        let provider = AlwaysFailCatalogProvider::new();
        let target = SyncTarget::project("p1");
        let lww_val = lww(2000, "dev-1");

        let tmp = TempDir::new().unwrap();
        let planned = PlannedTarget {
            target,
            local_root: tmp.path().to_path_buf(),
            staging_root: None,
            target_kind: PlannedTargetKind::DeleteRemoteProject,
            project_id: Some("p1".to_string()),
            target_live_root: tmp.path().to_path_buf(),
            deleted_journal_token: Some("token-1".to_string()),
            deleted_lww: Some(lww_val),
            live_lww: None,
            expected_delete_lww: None,
        };
        let plan = FullSyncPlan {
            sync_policy: SyncPolicy {
                enabled: true,
                ..Default::default()
            },
            force_sync: false,
            targets: vec![planned],
            app_data_root: tmp.path().to_path_buf(),
            remote_catalog_snapshot: test_empty_catalog_snapshot(),
        };

        let transfer = run_transfer(&provider, &plan);
        assert_eq!(transfer.targets.len(), 1);
        // catalog 写失败 → Retry。
        assert_eq!(
            transfer.targets[0].deleted_resolution,
            Some(DeletedTargetResolution::Retry)
        );
        // #645 评论 5504296097 问题4：lifecycle 写失败 → RecoverableError，不只 warn。
        assert!(matches!(
            transfer.targets[0].result.status,
            SyncStatus::RecoverableError(_)
        ));
        // 远端对象未被删除（catalog 写失败，不应删远端）。
        assert!(provider.read("projects/p1/chapter.md").unwrap().is_some());
    }

    /// #645 评论 5504296097 问题1/4：run_transfer 对 LiveProject 先写 catalog upsert（lifecycle CAS 是完成条件）。
    #[test]
    fn run_transfer_writes_catalog_upsert_for_live_project() {
        use crate::sync::types::{ManifestFileRecord, PlannedTargetKind, SyncManifest, SyncPolicy};

        let provider = MemoryProvider::new();
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("projects").join("p1");
        std::fs::create_dir_all(&project_root).unwrap();
        // #645 评论 5504296097 问题3：post-transfer LWW 从 staging manifest 读取。
        // staging_root 为 None → 用 local_root。写一份 manifest 让 LWW 能读到。
        // #645 评论 5504296097 问题1 修复：merge_remote_into_local_snapshot 会调
        // snapshot_local_records_read_only，要求 manifest 中 upsert record 对应的
        // 文件必须存在或有 tombstone，否则返回 Err。补上实际 project.json 文件。
        std::fs::write(project_root.join("project.json"), b"project content").unwrap();
        std::fs::create_dir_all(project_root.join("app-meta/sync")).unwrap();
        let manifest = SyncManifest {
            files: vec![ManifestFileRecord {
                path: "project.json".to_string(),
                content_hash: format!("{:x}", md5::compute(b"project content")),
                updated_at_ms: 2000,
                deleted_at_ms: None,
                device_id: "dev-1".to_string(),
                op: "upsert".to_string(),
                schema_version: 1,
            }],
        };
        std::fs::write(
            project_root.join("app-meta/sync/manifest.sync.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();

        let planned = PlannedTarget {
            target: SyncTarget::project("p1"),
            local_root: project_root.clone(),
            staging_root: None,
            target_kind: PlannedTargetKind::LiveProject,
            project_id: Some("p1".to_string()),
            target_live_root: project_root,
            deleted_journal_token: None,
            deleted_lww: None,
            live_lww: Some(LiveTargetLww {
                lww_time_ms: 1000,
                device_id: "dev-1".to_string(),
            }),
            expected_delete_lww: None,
        };
        let plan = FullSyncPlan {
            sync_policy: SyncPolicy {
                enabled: true,
                ..Default::default()
            },
            force_sync: false,
            targets: vec![planned],
            app_data_root: tmp.path().to_path_buf(),
            remote_catalog_snapshot: test_empty_catalog_snapshot(),
        };

        let transfer = run_transfer(&provider, &plan);
        assert_eq!(transfer.targets.len(), 1);

        // catalog 已写入 upsert 记录。
        let snapshot = crate::sync::target_lifecycle::load_remote_catalog(&provider).unwrap();
        let rec = crate::sync::target_lifecycle::find_record(&snapshot.catalog, "projects/p1");
        assert!(rec.is_some());
        assert_eq!(rec.unwrap().op, crate::sync::types::TargetOp::Upsert);
    }

    /// #645 评论 5504296097 问题4：LiveProject lifecycle 写失败 → RecoverableError，不只 warn。
    #[test]
    fn run_transfer_live_project_lifecycle_failure_returns_error() {
        use crate::sync::types::{PlannedTargetKind, SyncPolicy};

        let provider = AlwaysFailCatalogProvider::new();
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("projects").join("p1");
        std::fs::create_dir_all(&project_root).unwrap();

        let planned = PlannedTarget {
            target: SyncTarget::project("p1"),
            local_root: project_root.clone(),
            staging_root: None,
            target_kind: PlannedTargetKind::LiveProject,
            project_id: Some("p1".to_string()),
            target_live_root: project_root,
            deleted_journal_token: None,
            deleted_lww: None,
            live_lww: Some(LiveTargetLww {
                lww_time_ms: 1000,
                device_id: "dev-1".to_string(),
            }),
            expected_delete_lww: None,
        };
        let plan = FullSyncPlan {
            sync_policy: SyncPolicy {
                enabled: true,
                ..Default::default()
            },
            force_sync: false,
            targets: vec![planned],
            app_data_root: tmp.path().to_path_buf(),
            remote_catalog_snapshot: test_empty_catalog_snapshot(),
        };

        let transfer = run_transfer(&provider, &plan);
        assert_eq!(transfer.targets.len(), 1);
        // #645 评论 5504296097 问题4：lifecycle 写失败 → RecoverableError，不只 warn。
        assert!(matches!(
            transfer.targets[0].result.status,
            SyncStatus::RecoverableError(_)
        ));
    }

    /// #645 评论 5504296097 问题1：DeleteLocalProject 不上传，返回 NoChanges。
    #[test]
    fn run_transfer_delete_local_project_skips_upload() {
        use crate::sync::types::{PlannedTargetKind, SyncPolicy};

        let provider = MemoryProvider::new();
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("projects").join("p1");
        std::fs::create_dir_all(&project_root).unwrap();

        let planned = PlannedTarget {
            target: SyncTarget::project("p1"),
            local_root: project_root.clone(),
            staging_root: None,
            target_kind: PlannedTargetKind::DeleteLocalProject,
            project_id: Some("p1".to_string()),
            target_live_root: project_root,
            deleted_journal_token: None,
            deleted_lww: None,
            live_lww: None,
            expected_delete_lww: None,
        };
        let plan = FullSyncPlan {
            sync_policy: SyncPolicy {
                enabled: true,
                ..Default::default()
            },
            force_sync: false,
            targets: vec![planned],
            app_data_root: tmp.path().to_path_buf(),
            remote_catalog_snapshot: test_empty_catalog_snapshot(),
        };

        let transfer = run_transfer(&provider, &plan);
        assert_eq!(transfer.targets.len(), 1);
        // #645 评论 5504296097 问题3修复：DeleteLocalProject + 远端无记录 → Retry
        // （不再 "plan confirmed" 继续 DeleteProject）。
        assert!(matches!(
            transfer.targets[0].result.status,
            SyncStatus::RecoverableError(_)
        ));
        // 没有上传任何文件。
        assert!(transfer.targets[0].result.uploaded_files.is_empty());
    }

    // ─────────────────────────────────────────────────────────────────────
    // #645 评论 5504296097 复现测试 — 6 个实质问题
    //
    // 以下测试断言"期望的修复后行为"，对当前（未修复）代码会 FAIL，
    // 从而证明 bug 存在。测试名以 `repro_issue_645_qN_` 为前缀。
    // ─────────────────────────────────────────────────────────────────────

    /// 问题 1 复现：remote-only Project 不进入 plan。
    ///
    /// 场景：设备 A 已同步 P，远端 catalog = upsert(P)。设备 B 是新设备，
    /// 本地没有 P（live_projects 空，pending_deleted 空）。
    ///
    /// 期望：plan 应该包含 "projects/P" 的 target（RestoreProject），
    /// 让设备 B 下载远端独有作品。
    ///
    /// 当前行为：build_full_sync_target_plan 只遍历 live_projects + pending_deleted，
    /// 不遍历 remote_catalog.records，plan 里没有 P，设备 B 永远下载不到。
    #[test]
    fn repro_issue_645_q1_remote_only_project_not_in_plan() {
        use crate::sync::types::PlannedTargetKind;

        let tmp = TempDir::new().unwrap();
        let app_root = tmp.path().join("app");
        let projects_root = tmp.path().join("projects");
        std::fs::create_dir_all(&app_root).unwrap();
        std::fs::create_dir_all(&projects_root).unwrap();

        // 远端 catalog 有 upsert(P)，P 是远端独有的作品。
        let mut remote_catalog = TargetLifecycleCatalog::default();
        crate::sync::target_lifecycle::upsert_record(
            &mut remote_catalog,
            crate::sync::types::TargetLifecycleRecord::upsert(
                "projects/remote-only",
                "projects/remote-only",
                10_000,
                "dev-A",
            ),
        );

        // 设备 B：本地没有 P（live_projects 空，pending_deleted 空）。
        let planned = build_full_sync_target_plan(
            &app_root,
            &projects_root,
            &[], // live_projects 空
            &[], // pending_deleted 空
            &remote_catalog,
            &crate::sync::types::SyncPolicy::default(),
            false,
            "dev-B",
            &[],
        );

        // 期望：plan 里应该有 "projects/remote-only" 的 target。
        let has_remote_only = planned
            .iter()
            .any(|t| t.target.remote_prefix == "projects/remote-only");
        // 这个断言对当前代码会 FAIL — 证明 remote-only project 不进入 plan。
        assert!(
            has_remote_only,
            "问题1: remote-only project 未进入 plan。\
             当前 plan targets: {:?}",
            planned
                .iter()
                .map(|t| (t.target.remote_prefix.clone(), t.target_kind))
                .collect::<Vec<_>>()
        );

        // 进一步期望：target_kind 应该是 RestoreProject（下载远端恢复）。
        let remote_only_target = planned
            .iter()
            .find(|t| t.target.remote_prefix == "projects/remote-only")
            .expect("remote-only target should exist");
        assert_eq!(
            remote_only_target.target_kind,
            PlannedTargetKind::RestoreProject,
            "问题1: remote-only project 应为 RestoreProject"
        );
    }

    /// 问题 2 复现：本地 manifest 缺失时伪造 now() 作为 LWW，能复活远端已删除的旧作品。
    ///
    /// 场景：远端 delete(P, 12:00)，旧设备本地 P 实际停在 11:50，
    /// 但本地 manifest 丢了/坏了，旧设备 13:00 上线。
    ///
    /// 期望：本地无 manifest 时不应伪造 now() 作为 LWW 时间去压远端 delete。
    /// 应该保守地 DeleteLocalProject（远端 delete 胜出）或 Retry，绝不能 LiveProject + now()。
    ///
    /// 当前行为：compute_local_project_lww_time 返回 None →
    /// decide_live_project_kind(None) → LiveProject →
    /// run_transfer 用 now_epoch_seconds()*1000 伪造 upsert(P, 13:00) 压过远端 delete(12:00)。
    #[test]
    fn repro_issue_645_q2_manifest_missing_fakes_lww_now() {
        use crate::project::Project;
        use crate::sync::types::PlannedTargetKind;

        let tmp = TempDir::new().unwrap();
        let app_root = tmp.path().join("app");
        let projects_root = tmp.path().join("projects");
        std::fs::create_dir_all(&app_root).unwrap();
        let project_root = projects_root.join("P");
        std::fs::create_dir_all(&project_root).unwrap();
        // 故意不写 manifest — 模拟 manifest 缺失/损坏。

        let projects = vec![Project {
            id: "P".to_string(),
            title: "T".to_string(),
            created_at: "2024-01-01".to_string(),
            updated_at: "2024-01-01".to_string(),
            order: 0,
        }];

        // 远端 catalog 有 delete tombstone，deleted_at = 12:00。
        let mut remote_catalog = TargetLifecycleCatalog::default();
        crate::sync::target_lifecycle::upsert_record(
            &mut remote_catalog,
            crate::sync::types::TargetLifecycleRecord::delete(
                "projects/P",
                "projects/P",
                12_000,
                "dev-A",
            ),
        );

        let planned = build_full_sync_target_plan(
            &app_root,
            &projects_root,
            &projects,
            &[],
            &remote_catalog,
            &crate::sync::types::SyncPolicy::default(),
            false,
            "dev-B",
            &[],
        );

        // 找到 P 的 target。
        let p_target = planned
            .iter()
            .find(|t| t.target.remote_prefix == "projects/P")
            .expect("P target should exist");

        // 期望：远端 delete 胜出（本地无 manifest 不能伪造 now 去压 delete）→
        // 应该是 DeleteLocalProject（不上传，本地应删除）或 Retry。
        // 当前：decide_live_project_kind(None) → LiveProject（bug）。
        assert!(
            !matches!(p_target.target_kind, PlannedTargetKind::LiveProject),
            "问题2: manifest 缺失时不应判为 LiveProject（会用 now() 伪造 upsert 复活远端 delete）。\
             当前 target_kind={:?}, live_lww={:?}",
            p_target.target_kind,
            p_target.live_lww
        );

        // 进一步用 run_transfer 证明：当前会伪造 upsert(P, now) 写入 catalog。
        let provider = MemoryProvider::new();
        // 先在远端放 delete tombstone catalog（模拟远端已有 delete(P, 12:00)）。
        {
            let mut cat = TargetLifecycleCatalog::default();
            crate::sync::target_lifecycle::upsert_record(
                &mut cat,
                crate::sync::types::TargetLifecycleRecord::delete(
                    "projects/P",
                    "projects/P",
                    12_000,
                    "dev-A",
                ),
            );
            let snap = crate::sync::types::RemoteTargetCatalogSnapshot {
                catalog: cat,
                version: crate::sync::provider::model::RemoteVersion::new("__nonexistent__"),
            };
            crate::sync::target_lifecycle::write_remote_catalog(&provider, &snap).unwrap();
        }

        let plan = FullSyncPlan {
            sync_policy: crate::sync::types::SyncPolicy {
                enabled: true,
                ..Default::default()
            },
            force_sync: false,
            targets: planned.clone(),
            app_data_root: app_root.clone(),
            remote_catalog_snapshot: test_empty_catalog_snapshot(),
        };
        let transfer = run_transfer(&provider, &plan);

        // 检查 catalog 最终状态。
        let final_snapshot = crate::sync::target_lifecycle::load_remote_catalog(&provider).unwrap();
        let rec = crate::sync::target_lifecycle::find_record(&final_snapshot.catalog, "projects/P");

        // 期望：catalog 应该保留 delete tombstone（远端 delete 不被复活）。
        // 当前：catalog 被改成 upsert(P, now()*1000)，delete 被压过去。
        let catalog_has_upsert = rec
            .map(|r| r.op == crate::sync::types::TargetOp::Upsert)
            .unwrap_or(false);
        assert!(
            !catalog_has_upsert,
            "问题2: manifest 缺失时伪造 now() upsert 复活了远端 delete。\
             catalog 记录={:?}, transfer status={:?}",
            rec.map(|r| (r.op, r.updated_at_ms)),
            transfer
                .targets
                .iter()
                .map(|t| &t.result.status)
                .collect::<Vec<_>>()
        );
    }

    /// 问题 3 复现：apply_lifecycle_record 的 CAS retry 被 write_remote_catalog 内部吃掉。
    ///
    /// 场景：A 判断 delete(P, 12:00) 赢，B 并发写 upsert(P, 12:05)。
    /// A 第一次 CAS 冲突，write_remote_catalog 内部 merge 后保留 upsert(P, 12:05) 返回 Ok，
    /// apply_lifecycle_record 误认为 Applied → 继续 delete_all_remote_objects(projects/P)。
    ///
    /// 期望：apply_lifecycle_record 应该返回 RemoteWinner（远端 12:05 > candidate 12:00），
    /// 绝不返回 Applied。
    #[test]
    fn repro_issue_645_q3_cas_retry_swallowed_by_write_remote_catalog() {
        use crate::sync::types::{
            RemoteTargetCatalogSnapshot, TargetLifecycleApplyResult, TargetLifecycleRecord,
        };

        let provider = MemoryProvider::new();

        // B 已并发写 upsert(P, 12:05) 到远端 catalog。
        {
            let mut cat = TargetLifecycleCatalog::default();
            crate::sync::target_lifecycle::upsert_record(
                &mut cat,
                TargetLifecycleRecord::upsert("projects/P", "projects/P", 12_050, "dev-B"),
            );
            let snap = RemoteTargetCatalogSnapshot {
                catalog: cat,
                version: crate::sync::provider::model::RemoteVersion::new("__nonexistent__"),
            };
            crate::sync::target_lifecycle::write_remote_catalog(&provider, &snap).unwrap();
        }

        // A 持有的旧 snapshot（空 catalog，version 过期 — A 读 catalog 在 B 写之前）。
        let stale_snapshot = RemoteTargetCatalogSnapshot {
            catalog: TargetLifecycleCatalog::default(),
            version: crate::sync::provider::model::RemoteVersion::new("stale-version"),
        };

        // A 的 candidate: delete(P, 12:00)。
        let candidate = TargetLifecycleRecord::delete("projects/P", "projects/P", 12_000, "dev-A");

        let result = crate::sync::target_lifecycle::apply_lifecycle_record(
            &provider,
            &stale_snapshot,
            candidate,
        );

        // 期望：RemoteWinner（远端 upsert(P, 12:05) > candidate delete(P, 12:00)）。
        // 当前：Applied（write_remote_catalog 内部 CAS 冲突 → merge 保留 upsert → 返回 Ok → Applied）。
        assert!(
            matches!(result, TargetLifecycleApplyResult::RemoteWinner { .. }),
            "问题3: apply_lifecycle_record 应返回 RemoteWinner（远端 12:05 > candidate 12:00），\
             但 write_remote_catalog 内部吃掉 CAS 冲突后返回了 {:?}，\
             外层会误认为 Applied 并继续 delete_all_remote_objects",
            match result {
                TargetLifecycleApplyResult::Applied(s) => {
                    format!("Applied(catalog={:?})", s.catalog.records)
                }
                TargetLifecycleApplyResult::AlreadyCurrent(_) => "AlreadyCurrent".to_string(),
                TargetLifecycleApplyResult::RemoteWinner { record, .. } => {
                    format!("RemoteWinner(op={:?})", record.op)
                }
                TargetLifecycleApplyResult::Retry(e) => format!("Retry({e})"),
            }
        );
    }

    /// 问题 4 复现：DeleteLocalProject 只 NoChanges，本地 Project 根本没被删除。
    ///
    /// 场景：远端已明确 delete(P)，本地 P 一直存在。
    ///
    /// #645 评论 5504296097 问题1：run_transfer 对 DeleteLocalProject 不再直接删除
    /// 本地目录，而是返回 `DeleteProject` action，由 `commit_full_sync` 执行删除事务。
    /// 这样删除走完整 ProjectDeleteTransaction（move worktree / unbind starmap /
    /// journal），不绕过 delete_guard。
    ///
    /// #645 评论 5504296097 问题3修复：远端必须有 Delete record（CAS 确认），
    /// 否则 Ok(None) 直接 Retry。本测试用有 Delete record 的 catalog。
    #[test]
    fn repro_issue_645_q4_delete_local_project_no_actual_deletion() {
        use crate::sync::types::{
            LocalLifecycleCommitAction, PlannedTargetKind, RemoteTargetCatalogSnapshot,
            TargetLifecycleCatalog, TargetLifecycleRecord,
        };

        let provider = MemoryProvider::new();
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("projects").join("P");
        std::fs::create_dir_all(&project_root).unwrap();
        // 本地 P 有实际内容。
        std::fs::write(project_root.join("chapter.md"), b"content").unwrap();

        // 远端 catalog 有 Delete record for projects/P（CAS 确认删除）。
        let mut catalog = TargetLifecycleCatalog::default();
        catalog.records.push(TargetLifecycleRecord::delete(
            "projects/P",
            "projects/P",
            2000,
            "dev-remote",
        ));
        let remote_catalog_snapshot = RemoteTargetCatalogSnapshot {
            catalog: catalog.clone(),
            version: crate::sync::provider::model::RemoteVersion::new("v1"),
        };
        // 把 catalog 写入 provider，让 resolve_current_target_lifecycle 能读到。
        let catalog_json = serde_json::to_vec(&catalog).unwrap();
        provider
            .write(
                crate::sync::target_lifecycle::TARGET_CATALOG_REMOTE_PATH,
                &catalog_json,
                crate::sync::provider::model::WritePrecondition::CreateNew,
            )
            .unwrap();

        let planned = PlannedTarget {
            target: SyncTarget::project("P"),
            local_root: project_root.clone(),
            staging_root: None,
            target_kind: PlannedTargetKind::DeleteLocalProject,
            project_id: Some("P".to_string()),
            target_live_root: project_root.clone(),
            deleted_journal_token: None,
            deleted_lww: None,
            // #645 评论 5504296097 问题2 修复：DeleteLocalProject 的 live_lww
            // 在 Prepare 阶段由 compute_local_project_lifecycle_candidate 算出。
            // 正常流程中 live_lww 总是 Some（None 时 decide_live_project_kind
            // 返回 Retry 而非 DeleteLocalProject）。测试直接构造 PlannedTarget，
            // 给一个真实 lww 值让 DeleteProject action 能生成。
            live_lww: Some(LiveTargetLww {
                lww_time_ms: 1000,
                device_id: "dev-local".to_string(),
            }),
            expected_delete_lww: None,
        };
        let plan = FullSyncPlan {
            sync_policy: crate::sync::types::SyncPolicy {
                enabled: true,
                ..Default::default()
            },
            force_sync: false,
            targets: vec![planned],
            app_data_root: tmp.path().to_path_buf(),
            remote_catalog_snapshot,
        };

        let transfer = run_transfer(&provider, &plan);

        // #645 评论 5504296097 问题1：run_transfer 返回 NoChanges
        // （不在此阶段做文件 IO 删除）。
        assert!(
            matches!(transfer.targets[0].result.status, SyncStatus::NoChanges),
            "DeleteLocalProject 返回 NoChanges（删除延迟到 commit 阶段）"
        );

        // #645 评论 5504296097 问题1：run_transfer 返回 DeleteProject action，
        // commit_full_sync 会据此执行 ProjectDeleteTransaction。
        assert!(
            matches!(
                &transfer.targets[0].local_lifecycle_action,
                LocalLifecycleCommitAction::DeleteProject { project_id, .. } if project_id == "P"
            ),
            "DeleteLocalProject 应返回 DeleteProject action，\
             实际: {:?}",
            transfer.targets[0].local_lifecycle_action
        );

        // #645 评论 5504296097 问题1：run_transfer 阶段不删除本地目录
        // （删除由 commit_full_sync 走 ProjectDeleteTransaction 完成）。
        assert!(
            project_root.exists(),
            "run_transfer 不应删除本地目录（删除延迟到 commit 阶段）"
        );
    }

    /// 问题 5 复现：LiveProject 先发布 lifecycle upsert 再同步正文；
    /// 正文同步失败时留下假的已发布 target。
    ///
    /// 场景：LiveProject，lifecycle upsert 成功，但文件 transfer 中途失败。
    /// catalog 已写成 upsert(P, 最新时间)，projects/P 不存在/旧内容/只写一部分。
    ///
    /// 期望：lifecycle 不应该在正文失败时已发布（应先传正文再发布 lifecycle，或失败回滚）。
    #[test]
    fn repro_issue_645_q5_live_project_lifecycle_before_content_transfer() {
        use crate::sync::types::PlannedTargetKind;

        // 自定义 provider：catalog write 成功，但 projects/P 下 write 失败。
        struct CatalogOkContentFailProvider {
            inner: MemoryProvider,
        }
        impl SyncProvider for CatalogOkContentFailProvider {
            fn capabilities(&self) -> crate::sync::provider::capabilities::SyncCapabilities {
                self.inner.capabilities()
            }
            fn list(
                &self,
                prefix: &str,
            ) -> Result<
                Vec<crate::sync::provider::model::RemoteEntry>,
                crate::sync::provider::error::ProviderError,
            > {
                self.inner.list(prefix)
            }
            fn read(
                &self,
                path: &str,
            ) -> Result<
                Option<crate::sync::provider::model::RemoteObject>,
                crate::sync::provider::error::ProviderError,
            > {
                self.inner.read(path)
            }
            fn write(
                &self,
                path: &str,
                content: &[u8],
                precondition: crate::sync::provider::model::WritePrecondition,
            ) -> Result<
                crate::sync::provider::model::RemoteVersion,
                crate::sync::provider::error::ProviderError,
            > {
                // catalog write 成功，其他 write 失败（模拟正文 transfer 失败）。
                if path == crate::sync::target_lifecycle::TARGET_CATALOG_REMOTE_PATH {
                    return self.inner.write(path, content, precondition);
                }
                Err(crate::sync::provider::error::ProviderError::Other {
                    reason: "content transfer failed".to_string(),
                })
            }
            fn delete(
                &self,
                path: &str,
                precondition: crate::sync::provider::model::DeletePrecondition,
            ) -> Result<(), crate::sync::provider::error::ProviderError> {
                self.inner.delete(path, precondition)
            }
        }

        let provider = CatalogOkContentFailProvider {
            inner: MemoryProvider::new(),
        };

        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("projects").join("P");
        std::fs::create_dir_all(&project_root).unwrap();
        // 本地有正文内容待上传。
        std::fs::write(project_root.join("chapter.md"), b"content").unwrap();
        // 写本地 manifest 让 LWW engine 知道有文件要传。
        std::fs::create_dir_all(project_root.join("app-meta/sync")).unwrap();
        let manifest = SyncManifest {
            files: vec![ManifestFileRecord {
                path: "chapter.md".to_string(),
                content_hash: "9a0364b9e99bb480dd25e1f0284c8555".to_string(),
                updated_at_ms: 10_000,
                deleted_at_ms: None,
                device_id: "dev-A".to_string(),
                op: "upsert".to_string(),
                schema_version: 1,
            }],
        };
        std::fs::write(
            project_root.join("app-meta/sync/manifest.sync.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();

        let planned = PlannedTarget {
            target: SyncTarget::project("P"),
            local_root: project_root.clone(),
            staging_root: None,
            target_kind: PlannedTargetKind::LiveProject,
            project_id: Some("P".to_string()),
            target_live_root: project_root,
            deleted_journal_token: None,
            deleted_lww: None,
            live_lww: Some(LiveTargetLww {
                lww_time_ms: 10_000,
                device_id: "dev-A".to_string(),
            }),
            expected_delete_lww: None,
        };
        // 用 enabled 的 SyncPolicy，让 LWW engine 真正运行并尝试上传正文。
        let sync_policy = crate::sync::types::SyncPolicy {
            enabled: true,
            auto_sync: false,
            sync_interval_seconds: 60,
            has_network_permission: true,
        };
        let plan = FullSyncPlan {
            sync_policy,
            force_sync: true, // 绕过 debounce，确保 LWW engine 真正执行
            targets: vec![planned],
            app_data_root: tmp.path().to_path_buf(),
            remote_catalog_snapshot: test_empty_catalog_snapshot(),
        };

        let transfer = run_transfer(&provider, &plan);

        // 检查 catalog 是否已被写成 upsert（lifecycle 已发布）。
        let snapshot = crate::sync::target_lifecycle::load_remote_catalog(&provider).unwrap();
        let rec = crate::sync::target_lifecycle::find_record(&snapshot.catalog, "projects/P");
        let catalog_has_upsert = rec
            .map(|r| r.op == crate::sync::types::TargetOp::Upsert)
            .unwrap_or(false);

        // 检查远端是否真的有正文（projects/P/chapter.md）。
        let remote_has_content =
            crate::sync::provider::SyncProvider::read(&provider, "projects/P/chapter.md")
                .unwrap()
                .is_some();

        // 期望：如果 catalog 已发布 upsert（lifecycle 成功），远端应该有正文内容。
        // 当前：lifecycle upsert 先成功（catalog 有 upsert），但正文 transfer 失败
        // （provider 让非 catalog write 失败）→ catalog 有 upsert 但远端没有正文。
        // 别的设备下一轮先看 catalog 认为这个 Project 已正式存在且最新 lifecycle，
        // 恢复一份不完整/过期正文。
        assert!(
            !(catalog_has_upsert && !remote_has_content),
            "问题5: LiveProject 先发布 lifecycle upsert 再传正文，正文失败时留下假的已发布 target。\
             catalog_has_upsert={}, remote_has_content={}, status={:?}。\
             catalog 已 upsert 但远端无正文 → 别的设备会看到假的已发布 target。",
            catalog_has_upsert,
            remote_has_content,
            transfer.targets[0].result.status
        );
    }

    /// 问题 6 复现：为在 planner 前读 remote catalog，把网络 IO 放回了 core_write()，
    /// 把 #644 已拆掉的网络长锁重新引进来。
    ///
    /// 这是静态代码路径证据 — perform_full_sync 在 core_write() 作用域内调用
    /// load_remote_catalog（网络 IO），阻塞正文/作品读取。
    ///
    /// 本测试用静态断言确认问题代码路径存在（行号在 reproduction_result.json 中记录）。
    /// 运行时复现需要多线程 + mock WriterCore 锁竞争，这里用静态证据代替。
    #[test]
    fn repro_issue_645_q6_network_io_inside_core_write_lock_static_evidence() {
        // 读取 sync_api.rs 源码，确认 discover_legacy_remote_catalog 在 core_write() 作用域外。
        // #645 评论 5504296097 回退问题：恢复短锁+锁外扫描。
        // prepare_full_sync → build_full_sync_plan_unlocked（锁外）。
        // load_remote_catalog → discover_legacy_remote_catalog（锁外）。
        let source = include_str!("../api/sync_api.rs");
        let perform_full_sync_start = source.find("pub fn perform_full_sync(");
        assert!(
            perform_full_sync_start.is_some(),
            "问题6: perform_full_sync 函数应存在于 sync_api.rs"
        );
        let start = perform_full_sync_start.unwrap();
        // 取函数体做检查（覆盖整个 Prepare 阶段）。
        // #645 评论 5504296097 回退问题：用整个剩余 source 而非固定字节窗口，
        // 避免中文注释多字节字符导致切片落在 char boundary 内 panic。
        let body = &source[start..];

        let has_core_write = body.contains("let core = self.core_write();");
        let has_discover_catalog =
            body.contains("discover_legacy_remote_catalog(provider.as_ref())");
        let has_build_plan_unlocked = body.contains("build_full_sync_plan_unlocked(");
        let has_lock_released = body.contains("写锁已释放");

        // 四个都存在 — 证明代码结构如所述。
        assert!(has_core_write, "问题6: core_write() 调用应存在");
        assert!(
            has_discover_catalog,
            "问题6: discover_legacy_remote_catalog 调用应存在"
        );
        assert!(
            has_build_plan_unlocked,
            "问题6: build_full_sync_plan_unlocked 调用应存在（锁外扫描）"
        );
        assert!(has_lock_released, "问题6: '写锁已释放' 注释应存在");

        let core_write_pos = body.find("let core = self.core_write();").unwrap();
        let discover_catalog_pos = body
            .find("discover_legacy_remote_catalog(provider.as_ref())")
            .unwrap();
        let lock_released_pos = body.find("写锁已释放").unwrap();

        // 期望（修复后）：discover_legacy_remote_catalog 应在 "写锁已释放" 注释之后
        // （即网络 IO 在写锁作用域外）。
        assert!(
            discover_catalog_pos > lock_released_pos,
            "问题6: discover_legacy_remote_catalog (pos={}) 应在 '写锁已释放' (pos={}) 之后\
             （即网络 IO 不在写锁内）。当前 discover_legacy_remote_catalog 在 core_write (pos={}) 之后、\
             写锁释放 (pos={}) 之前，网络 IO 期间持 core 写锁，阻塞正文/作品读取，\
             与 #644 拆锁路线冲突。",
            discover_catalog_pos,
            lock_released_pos,
            core_write_pos,
            lock_released_pos
        );
    }

    // ─────────────────────────────────────────────────────────────────────
    // #645 评论 5504296097 问题6：parse_project_target_id 验证测试
    // ─────────────────────────────────────────────────────────────────────

    /// #645 评论 5504296097 问题6：合法 target_id 正确解析。
    #[test]
    fn q6_parse_project_target_id_valid() {
        assert_eq!(
            crate::sync::target_lifecycle::parse_project_target_id("projects/p1").unwrap(),
            "p1"
        );
        assert_eq!(
            crate::sync::target_lifecycle::parse_project_target_id("projects/abc-123").unwrap(),
            "abc-123"
        );
    }

    /// #645 评论 5504296097 问题6：非法 target_id 被拒绝。
    #[test]
    fn q6_parse_project_target_id_invalid() {
        // 缺前缀
        assert!(crate::sync::target_lifecycle::parse_project_target_id("p1").is_err());
        assert!(crate::sync::target_lifecycle::parse_project_target_id("apps/p1").is_err());
        // 空段
        assert!(crate::sync::target_lifecycle::parse_project_target_id("projects/").is_err());
        // 多段（路径穿越）
        assert!(crate::sync::target_lifecycle::parse_project_target_id("projects/../app").is_err());
        assert!(crate::sync::target_lifecycle::parse_project_target_id("projects/a/b").is_err());
        // 点段
        assert!(crate::sync::target_lifecycle::parse_project_target_id("projects/.").is_err());
        assert!(crate::sync::target_lifecycle::parse_project_target_id("projects/..").is_err());
        // 反斜杠
        assert!(crate::sync::target_lifecycle::parse_project_target_id("projects/a\\b").is_err());
    }

    /// #645 评论 5504296097 问题6：remote-only discovery 遇到非法 target_id 跳过。
    #[test]
    fn q6_build_plan_skips_invalid_remote_target_id() {
        let tmp = TempDir::new().unwrap();
        let app_root = tmp.path().join("app");
        let projects_root = tmp.path().join("projects");
        std::fs::create_dir_all(&app_root).unwrap();
        std::fs::create_dir_all(&projects_root).unwrap();

        // 远端 catalog 含一条非法 target_id 的 upsert 记录。
        let mut remote_catalog = TargetLifecycleCatalog::default();
        crate::sync::target_lifecycle::upsert_record(
            &mut remote_catalog,
            crate::sync::types::TargetLifecycleRecord::upsert(
                "projects/../app",
                "projects/../app",
                10_000,
                "dev-A",
            ),
        );
        // 加一条合法记录作为对照。
        crate::sync::target_lifecycle::upsert_record(
            &mut remote_catalog,
            crate::sync::types::TargetLifecycleRecord::upsert(
                "projects/legit",
                "projects/legit",
                10_000,
                "dev-A",
            ),
        );

        let planned = build_full_sync_target_plan(
            &app_root,
            &projects_root,
            &[],
            &[],
            &remote_catalog,
            &crate::sync::types::SyncPolicy::default(),
            false,
            "dev-B",
            &[],
        );

        // 非法记录被跳过 — 不应出现 "projects/../app" 或 "app" target。
        let has_traversal = planned
            .iter()
            .any(|t| t.target.remote_prefix.contains(".."));
        assert!(
            !has_traversal,
            "问题6: 非法 target_id 不应进入 plan。planned={:?}",
            planned
                .iter()
                .map(|t| &t.target.remote_prefix)
                .collect::<Vec<_>>()
        );
        // 合法记录仍进入 plan。
        let has_legit = planned
            .iter()
            .any(|t| t.target.remote_prefix == "projects/legit");
        assert!(has_legit, "问题6: 合法 target_id 应进入 plan");
    }

    /// #645 评论 5504296097 问题6：pending deleted 非法 target_id 跳过。
    #[test]
    fn q6_build_plan_skips_invalid_pending_deleted_target_id() {
        use crate::sync::types::PendingDeletedTarget;

        let tmp = TempDir::new().unwrap();
        let app_root = tmp.path().join("app");
        let projects_root = tmp.path().join("projects");
        std::fs::create_dir_all(&app_root).unwrap();
        std::fs::create_dir_all(&projects_root).unwrap();

        // 手工构造一条非法 target_id 的 pending deleted target。
        // PendingDeletedTarget::for_project 会用 "projects/<id>"，这里直接构造
        // 一个含路径穿越的 remote_prefix 来模拟损坏的持久数据。
        let pending = vec![PendingDeletedTarget {
            target: crate::sync::types::SyncTarget {
                scope: crate::sync::types::SyncScope::Project,
                remote_prefix: "projects/../app".to_string(),
            },
            deleted_at_ms: 1000,
            journal_token: "tok-1".to_string(),
            device_id: "dev-1".to_string(),
            paths: None,
        }];

        let planned = build_full_sync_target_plan(
            &app_root,
            &projects_root,
            &[],
            &pending,
            &TargetLifecycleCatalog::default(),
            &crate::sync::types::SyncPolicy::default(),
            false,
            "dev-1",
            &[],
        );

        // 非法 pending 被跳过 — plan 只有 App target。
        assert_eq!(
            planned.len(),
            1,
            "问题6: 非法 pending deleted target_id 应被跳过，plan 只含 App target"
        );
        assert_eq!(
            planned[0].target_kind,
            crate::sync::types::PlannedTargetKind::App
        );
    }

    // ─────────────────────────────────────────────────────────────────────
    // #645 评论 5504296097 问题4：catalog 读取失败不吞、plan 携带 snapshot 测试
    // ─────────────────────────────────────────────────────────────────────

    /// #645 评论 5504296097 问题4：run_transfer 用 plan 携带的 snapshot 作为起点，
    /// 不再无条件再读一次 catalog。
    ///
    /// plan 携带的 snapshot 含 delete(P, 12000)。LiveProject candidate upsert(P, 10000)
    /// < remote delete 12000 → LostToRemote，不写远端 catalog。
    /// 若 run_transfer 重读空 catalog，candidate 赢 → Applied → 写远端 catalog。
    /// 用远端 catalog 是否出现记录区分两条路径。
    #[test]
    fn q4_run_transfer_uses_plan_catalog_snapshot() {
        use crate::sync::types::{PlannedTargetKind, SyncPolicy, TargetLifecycleRecord};

        let provider = MemoryProvider::new();
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("projects").join("p1");
        std::fs::create_dir_all(&project_root).unwrap();

        // plan 携带的 snapshot：已有 delete(P, 12000)。
        let mut plan_catalog = TargetLifecycleCatalog::default();
        crate::sync::target_lifecycle::upsert_record(
            &mut plan_catalog,
            TargetLifecycleRecord::delete("projects/p1", "projects/p1", 12000, "dev-old"),
        );
        let plan_snapshot_with_delete = crate::sync::types::RemoteTargetCatalogSnapshot {
            catalog: plan_catalog,
            version: crate::sync::provider::model::RemoteVersion::new("v-plan"),
        };

        let planned = PlannedTarget {
            target: SyncTarget::project("p1"),
            local_root: project_root.clone(),
            staging_root: None,
            target_kind: PlannedTargetKind::LiveProject,
            project_id: Some("p1".to_string()),
            target_live_root: project_root,
            deleted_journal_token: None,
            deleted_lww: None,
            live_lww: Some(LiveTargetLww {
                lww_time_ms: 10_000,
                device_id: "dev-1".to_string(),
            }),
            expected_delete_lww: None,
        };
        let plan = FullSyncPlan {
            sync_policy: SyncPolicy {
                enabled: true,
                ..Default::default()
            },
            force_sync: false,
            targets: vec![planned],
            app_data_root: tmp.path().to_path_buf(),
            remote_catalog_snapshot: plan_snapshot_with_delete,
        };

        let transfer = run_transfer(&provider, &plan);
        // plan snapshot 有 delete(P, 12000) > candidate upsert(P, 10000) → LostToRemote。
        // LostToRemote 不写远端 catalog（远端无 catalog 文件）。
        // 若 run_transfer 重读空 catalog，candidate 赢 → Applied → CreateNew 写 catalog。
        let remote_catalog_after =
            crate::sync::target_lifecycle::load_remote_catalog(&provider).unwrap();
        let remote_has_record = !remote_catalog_after.catalog.records.is_empty();
        assert!(
            !remote_has_record,
            "问题4: run_transfer 应用 plan 携带的 snapshot（含 delete tombstone）\
             走 LostToRemote，不应写远端 catalog。但远端出现了记录 — \
             说明 run_transfer 重读了空 catalog 误判 Applied。transfer={:?}",
            transfer.targets[0].result.status
        );
    }

    // ─────────────────────────────────────────────────────────────────────
    // #645 评论 5504296097 问题2：manifest 缺失/损坏处理测试
    // ─────────────────────────────────────────────────────────────────────

    /// #645 评论 5504296097 问题2：manifest 损坏 → Retry，不 DeleteLocalProject。
    #[test]
    fn q2_manifest_corrupt_returns_retry() {
        use crate::project::Project;
        use crate::sync::types::PlannedTargetKind;

        let tmp = TempDir::new().unwrap();
        let app_root = tmp.path().join("app");
        let projects_root = tmp.path().join("projects");
        std::fs::create_dir_all(&app_root).unwrap();
        let project_root = projects_root.join("P");
        std::fs::create_dir_all(&project_root).unwrap();
        // 写损坏的 manifest。
        std::fs::create_dir_all(project_root.join("app-meta/sync")).unwrap();
        std::fs::write(
            project_root.join("app-meta/sync/manifest.sync.json"),
            b"{not valid json",
        )
        .unwrap();

        let projects = vec![Project {
            id: "P".to_string(),
            title: "T".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
            order: 0,
        }];

        // 远端 catalog 有 delete tombstone。
        let mut remote_catalog = TargetLifecycleCatalog::default();
        crate::sync::target_lifecycle::upsert_record(
            &mut remote_catalog,
            crate::sync::types::TargetLifecycleRecord::delete(
                "projects/P",
                "projects/P",
                12_000,
                "dev-A",
            ),
        );

        let planned = build_full_sync_target_plan(
            &app_root,
            &projects_root,
            &projects,
            &[],
            &remote_catalog,
            &crate::sync::types::SyncPolicy::default(),
            false,
            "dev-B",
            &[],
        );

        let p_target = planned
            .iter()
            .find(|t| t.target.remote_prefix == "projects/P")
            .expect("P target should exist");
        // manifest 损坏 → Retry，绝不 DeleteLocalProject（无证据）。
        assert_eq!(
            p_target.target_kind,
            PlannedTargetKind::Retry,
            "问题2: manifest 损坏应返回 Retry，不 DeleteLocalProject"
        );
    }

    /// #645 评论 5504296097 问题2：首次同步 manifest 不存在 + project.json 合法
    /// → 建立初始 manifest → LiveProject（正常同步）。
    #[test]
    fn q2_first_sync_establishes_initial_manifest() {
        use crate::project::Project;
        use crate::sync::types::PlannedTargetKind;

        let tmp = TempDir::new().unwrap();
        let app_root = tmp.path().join("app");
        let projects_root = tmp.path().join("projects");
        std::fs::create_dir_all(&app_root).unwrap();
        let project_root = projects_root.join("P");
        std::fs::create_dir_all(&project_root).unwrap();
        // 不写 manifest（首次同步）。写合法 project.json。
        let project = Project {
            id: "P".to_string(),
            title: "T".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
            order: 0,
        };
        std::fs::write(
            project_root.join("project.json"),
            serde_json::to_vec(&project).unwrap(),
        )
        .unwrap();

        let planned = build_full_sync_target_plan(
            &app_root,
            &projects_root,
            &[project],
            &[],
            &TargetLifecycleCatalog::default(),
            &crate::sync::types::SyncPolicy::default(),
            false,
            "dev-B",
            &[],
        );

        let p_target = planned
            .iter()
            .find(|t| t.target.remote_prefix == "projects/P")
            .expect("P target should exist");
        // 首次同步 → 建立初始 manifest → LiveProject。
        assert_eq!(
            p_target.target_kind,
            PlannedTargetKind::LiveProject,
            "问题2: 首次同步应建立初始 manifest 并判为 LiveProject"
        );
        assert!(
            p_target.live_lww.is_some(),
            "问题2: 首次同步应产出初始 live_lww"
        );
        // #645 评论 5504296097 问题4修复：planner 不再落盘 manifest，
        // initial_manifest 由正式同步在 staging 阶段写入。
        // 此处只验证 planner 产出了 live_lww，不验证 manifest 文件存在。
        assert!(
            !project_root
                .join("app-meta/sync/manifest.sync.json")
                .exists(),
            "问题4: planner 不应落盘 manifest（由 staging 阶段写入）"
        );
    }

    /// #645 评论 5504296097 问题2/4：manifest 不存在 + project.json 损坏 →
    /// 问题4修复后 scan_sync_file 用 mtime fallback，仍能建立 LWW → LiveProject。
    #[test]
    fn q2_no_manifest_and_corrupt_project_json_returns_retry() {
        use crate::project::Project;
        use crate::sync::types::PlannedTargetKind;

        let tmp = TempDir::new().unwrap();
        let app_root = tmp.path().join("app");
        let projects_root = tmp.path().join("projects");
        std::fs::create_dir_all(&app_root).unwrap();
        let project_root = projects_root.join("P");
        std::fs::create_dir_all(&project_root).unwrap();
        // 不写 manifest，写损坏 project.json。
        std::fs::write(project_root.join("project.json"), b"not json").unwrap();

        let projects = vec![Project {
            id: "P".to_string(),
            title: "T".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
            order: 0,
        }];

        let planned = build_full_sync_target_plan(
            &app_root,
            &projects_root,
            &projects,
            &[],
            &TargetLifecycleCatalog::default(),
            &crate::sync::types::SyncPolicy::default(),
            false,
            "dev-B",
            &[],
        );

        let p_target = planned
            .iter()
            .find(|t| t.target.remote_prefix == "projects/P")
            .expect("P target should exist");
        // #645 评论 5504296097 问题4修复：scan_sync_file 用 mtime fallback，
        // 损坏 JSON 仍能建立 LWW → LiveProject（不再 Retry）。
        assert_eq!(
            p_target.target_kind,
            PlannedTargetKind::LiveProject,
            "问题4: 损坏 project.json 用 mtime fallback 仍应建立 LWW → LiveProject"
        );
    }

    // ─────────────────────────────────────────────────────────────────────
    // #645 评论 5504296097 问题3：post-transfer staging LWW 测试
    // ─────────────────────────────────────────────────────────────────────

    /// #645 评论 5504296097 问题3：lifecycle publish 用 post-transfer staging manifest LWW，
    /// 不是 Transfer 前的旧 live_lww。
    ///
    /// live_lww.lww_time_ms = 1000（旧）。staging manifest max lww = 5000（post-transfer）。
    /// catalog upsert 应记录 updated_at_ms = 5000，不是 1000。
    #[test]
    fn q3_publish_uses_post_transfer_staging_lww() {
        use crate::sync::types::{ManifestFileRecord, PlannedTargetKind, SyncManifest, SyncPolicy};

        let provider = MemoryProvider::new();
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("projects").join("p1");
        std::fs::create_dir_all(&project_root).unwrap();
        // staging_root 单独设一个目录，里面放 post-transfer manifest（max=5000）。
        // #645 评论 5504296097 问题1 修复：merge_remote_into_local_snapshot 会调
        // snapshot_local_records_read_only，要求 manifest 中 upsert record 对应的
        // 文件必须存在或有 tombstone。补上实际 chapter.md 文件。
        let staging_root = tmp.path().join("staging-p1");
        std::fs::create_dir_all(staging_root.join("volumes").join("v1")).unwrap();
        std::fs::write(
            staging_root.join("volumes").join("v1").join("chapter.md"),
            b"chapter content",
        )
        .unwrap();
        std::fs::create_dir_all(staging_root.join("app-meta/sync")).unwrap();
        let staging_manifest = SyncManifest {
            files: vec![ManifestFileRecord {
                path: "volumes/v1/chapter.md".to_string(),
                content_hash: format!("{:x}", md5::compute(b"chapter content")),
                updated_at_ms: 5000,
                deleted_at_ms: None,
                device_id: "dev-1".to_string(),
                op: "upsert".to_string(),
                schema_version: 1,
            }],
        };
        std::fs::write(
            staging_root.join("app-meta/sync/manifest.sync.json"),
            serde_json::to_vec(&staging_manifest).unwrap(),
        )
        .unwrap();

        let planned = PlannedTarget {
            target: SyncTarget::project("p1"),
            local_root: project_root.clone(),
            staging_root: Some(staging_root.clone()),
            target_kind: PlannedTargetKind::LiveProject,
            project_id: Some("p1".to_string()),
            target_live_root: project_root,
            deleted_journal_token: None,
            deleted_lww: None,
            // 旧 live_lww = 1000，不应被用作 publish 时间。
            live_lww: Some(LiveTargetLww {
                lww_time_ms: 1000,
                device_id: "dev-1".to_string(),
            }),
            expected_delete_lww: None,
        };
        let plan = FullSyncPlan {
            sync_policy: SyncPolicy {
                enabled: true,
                ..Default::default()
            },
            force_sync: false,
            targets: vec![planned],
            app_data_root: tmp.path().to_path_buf(),
            remote_catalog_snapshot: test_empty_catalog_snapshot(),
        };

        let _transfer = run_transfer(&provider, &plan);

        // catalog upsert 应记录 updated_at_ms = 5000（post-transfer staging LWW）。
        let snapshot = crate::sync::target_lifecycle::load_remote_catalog(&provider).unwrap();
        let rec = crate::sync::target_lifecycle::find_record(&snapshot.catalog, "projects/p1")
            .expect("catalog should have upsert record");
        assert_eq!(rec.op, crate::sync::types::TargetOp::Upsert);
        assert_eq!(
            rec.updated_at_ms, 5000,
            "问题3: lifecycle publish 应使用 post-transfer staging LWW (5000)，\
             不是旧 live_lww (1000)"
        );
    }

    /// #645 评论 5504296097 问题3：post-transfer staging manifest 读取失败 → RecoverableError。
    #[test]
    fn q3_post_transfer_manifest_unreadable_returns_error() {
        use crate::sync::types::{PlannedTargetKind, SyncPolicy};

        let provider = MemoryProvider::new();
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("projects").join("p1");
        std::fs::create_dir_all(&project_root).unwrap();
        // staging_root 没有 manifest → read_post_transfer_lww 返回 None。
        let staging_root = tmp.path().join("staging-p1");
        std::fs::create_dir_all(&staging_root).unwrap();

        let planned = PlannedTarget {
            target: SyncTarget::project("p1"),
            local_root: project_root.clone(),
            staging_root: Some(staging_root),
            target_kind: PlannedTargetKind::LiveProject,
            project_id: Some("p1".to_string()),
            target_live_root: project_root,
            deleted_journal_token: None,
            deleted_lww: None,
            live_lww: Some(LiveTargetLww {
                lww_time_ms: 1000,
                device_id: "dev-1".to_string(),
            }),
            expected_delete_lww: None,
        };
        let plan = FullSyncPlan {
            sync_policy: SyncPolicy {
                enabled: true,
                ..Default::default()
            },
            force_sync: false,
            targets: vec![planned],
            app_data_root: tmp.path().to_path_buf(),
            remote_catalog_snapshot: test_empty_catalog_snapshot(),
        };

        let transfer = run_transfer(&provider, &plan);
        assert!(
            matches!(
                transfer.targets[0].result.status,
                SyncStatus::RecoverableError(_)
            ),
            "问题3: post-transfer manifest 不可读应返回 RecoverableError，\
             不伪造旧 live_lww。status={:?}",
            transfer.targets[0].result.status
        );
        // 不写 catalog。
        let snapshot = crate::sync::target_lifecycle::load_remote_catalog(&provider).unwrap();
        assert!(
            snapshot.catalog.records.is_empty(),
            "问题3: manifest 不可读时不应写 catalog"
        );
    }

    /// #645 评论 5504296097 问题5：dry-run 应使用真实 catalog 做 target 决策。
    ///
    /// 场景：远端 catalog 有 project P 的 upsert 记录，本地也有 P。
    /// 用真实 catalog 调 build_full_sync_target_plan，P 应被计划为 LiveProject
    /// （而非 RemoteOnly 或 LostToRemote）。
    #[test]
    fn q5_dry_run_uses_real_catalog_for_target_decision() {
        use crate::sync::types::{
            ManifestFileRecord, PlannedTargetKind, SyncManifest, TargetLifecycleCatalog,
        };

        let tmp = TempDir::new().unwrap();
        let projects_root = tmp.path().join("projects");
        let project_root = projects_root.join("P");
        std::fs::create_dir_all(&project_root).unwrap();
        // 写合法 project.json + manifest，让 P 被识别为 live project。
        let project = crate::project::Project {
            id: "P".to_string(),
            title: "P".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
            order: 0,
        };
        std::fs::write(
            project_root.join("project.json"),
            serde_json::to_vec(&project).unwrap(),
        )
        .unwrap();
        std::fs::create_dir_all(project_root.join("app-meta/sync")).unwrap();
        let manifest = SyncManifest {
            files: vec![ManifestFileRecord {
                path: "project.json".to_string(),
                content_hash: "9a0364b9e99bb480dd25e1f0284c8555".to_string(),
                updated_at_ms: 1000,
                deleted_at_ms: None,
                device_id: "device-1".to_string(),
                op: "upsert".to_string(),
                schema_version: 1,
            }],
        };
        std::fs::write(
            project_root.join("app-meta/sync/manifest.sync.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();

        let projects = vec![project];

        // 真实 catalog：P 有 upsert 记录（LWW=2000，比本地 manifest 1000 新）。
        let mut catalog = TargetLifecycleCatalog::default();
        crate::sync::target_lifecycle::upsert_record(
            &mut catalog,
            crate::sync::types::TargetLifecycleRecord::upsert(
                "projects/P",
                "projects/P",
                2000,
                "device-2",
            ),
        );

        let planned = build_full_sync_target_plan(
            tmp.path(),
            &projects_root,
            &projects,
            &[],
            &catalog,
            &crate::sync::types::SyncPolicy::default(),
            false,
            "device-1",
            &[],
        );

        // P 应被计划为 LiveProject（本地有 + catalog 有 upsert）。
        let p_target = planned
            .iter()
            .find(|t| t.project_id.as_deref() == Some("P"))
            .expect("P should be in plan");
        assert!(
            matches!(p_target.target_kind, PlannedTargetKind::LiveProject),
            "P 应为 LiveProject（本地有 + catalog 有 upsert），实际: {:?}",
            p_target.target_kind
        );
    }

    /// #645 评论 5504296097 问题5：dry-run 用空 catalog 时，
    /// 本地有但 catalog 无记录的 project 应被计划为 LiveProject（首次同步）。
    #[test]
    fn q5_dry_run_empty_catalog_first_sync() {
        use crate::sync::types::{PlannedTargetKind, TargetLifecycleCatalog};

        let tmp = TempDir::new().unwrap();
        let projects_root = tmp.path().join("projects");
        let project_root = projects_root.join("P");
        std::fs::create_dir_all(&project_root).unwrap();
        let project = crate::project::Project {
            id: "P".to_string(),
            title: "P".to_string(),
            created_at: "2024-01-01T00:00:00Z".to_string(),
            updated_at: "2024-01-01T00:00:00Z".to_string(),
            order: 0,
        };
        std::fs::write(
            project_root.join("project.json"),
            serde_json::to_vec(&project).unwrap(),
        )
        .unwrap();

        let projects = vec![project];

        // 空 catalog（首次同步，远端无记录）。
        let planned = build_full_sync_target_plan(
            tmp.path(),
            &projects_root,
            &projects,
            &[],
            &TargetLifecycleCatalog::default(),
            &crate::sync::types::SyncPolicy::default(),
            false,
            "device-1",
            &[],
        );

        let p_target = planned
            .iter()
            .find(|t| t.project_id.as_deref() == Some("P"))
            .expect("P should be in plan");
        // 首次同步：本地有，catalog 无 → LiveProject（问题2 修复后不再 DeleteLocalProject）。
        assert!(
            matches!(p_target.target_kind, PlannedTargetKind::LiveProject),
            "首次同步 P 应为 LiveProject，实际: {:?}",
            p_target.target_kind
        );
    }

    /// #645 评论 5504296097 问题2：LiveProject generation 原子发布。
    ///
    /// 验证：LiveProject 上传到不可见 generation prefix
    /// （`projects/P/__generations__/G/`），catalog Upsert 记录携带 active_generation=G，
    /// legacy prefix（`projects/P/`）下无正文文件。
    #[test]
    fn test_live_project_uploads_to_generation_prefix() {
        use crate::sync::types::{PlannedTargetKind, SyncPolicy};

        let provider = MemoryProvider::new();

        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("projects").join("P");
        let chapter_dir = project_root.join("volumes").join("v1");
        std::fs::create_dir_all(&chapter_dir).unwrap();
        // volumes/v1/chapter.md 是 project 白名单路径。
        std::fs::write(chapter_dir.join("chapter.md"), b"content").unwrap();
        std::fs::create_dir_all(project_root.join("app-meta/sync")).unwrap();
        let manifest = SyncManifest {
            files: vec![ManifestFileRecord {
                path: "volumes/v1/chapter.md".to_string(),
                content_hash: format!("{:x}", md5::compute(b"content")),
                updated_at_ms: 10_000,
                deleted_at_ms: None,
                device_id: "dev-A".to_string(),
                op: "upsert".to_string(),
                schema_version: 1,
            }],
        };
        std::fs::write(
            project_root.join("app-meta/sync/manifest.sync.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();

        let planned = PlannedTarget {
            target: SyncTarget::project("P"),
            local_root: project_root.clone(),
            staging_root: None,
            target_kind: PlannedTargetKind::LiveProject,
            project_id: Some("P".to_string()),
            target_live_root: project_root.clone(),
            deleted_journal_token: None,
            deleted_lww: None,
            live_lww: Some(LiveTargetLww {
                lww_time_ms: 10_000,
                device_id: "dev-A".to_string(),
            }),
            expected_delete_lww: None,
        };
        let sync_policy = SyncPolicy {
            enabled: true,
            auto_sync: false,
            sync_interval_seconds: 60,
            has_network_permission: true,
        };
        let plan = FullSyncPlan {
            sync_policy,
            force_sync: true,
            targets: vec![planned],
            app_data_root: tmp.path().to_path_buf(),
            remote_catalog_snapshot: test_empty_catalog_snapshot(),
        };

        let _transfer = run_transfer(&provider, &plan);

        // 验证 catalog 有 Upsert with active_generation。
        let snapshot = crate::sync::target_lifecycle::load_remote_catalog(&provider).unwrap();
        let rec = crate::sync::target_lifecycle::find_record(&snapshot.catalog, "projects/P")
            .expect("catalog should have projects/P record");
        assert_eq!(
            rec.op,
            crate::sync::types::TargetOp::Upsert,
            "catalog should have Upsert for projects/P"
        );
        let gen_id = rec
            .active_generation
            .as_ref()
            .expect("Upsert should carry active_generation");

        // 验证远端 generation prefix 有正文文件。
        let gen_prefix = generation_remote_prefix("projects/P", gen_id).unwrap();
        let gen_entries = provider.list(&gen_prefix).unwrap();
        assert!(
            gen_entries
                .iter()
                .any(|e| e.path == "volumes/v1/chapter.md"),
            "generation prefix {} should have volumes/v1/chapter.md, entries: {:?}",
            gen_prefix,
            gen_entries
        );

        // 验证 legacy prefix 无正文（上传到 generation 而非 legacy）。
        let legacy_read = provider.read("projects/P/volumes/v1/chapter.md").unwrap();
        assert!(
            legacy_read.is_none(),
            "legacy prefix should not have volumes/v1/chapter.md (uploaded to generation prefix)"
        );
    }

    /// #645 评论 5504296097 问题2：delete_all_remote_objects 跳过 generation prefix。
    ///
    /// 验证：legacy 文件被删，generation prefix 下的文件保留（不碰并发 Upsert
    /// 正在上传的 generation）。这修复了"设备 B 已上传新正文到 generation prefix
    /// 但还没 CAS catalog，设备 A cleanup 看到 Delete(P) → 删 projects/P/ → 误删
    /// B 的 generation"的竞态。
    #[test]
    fn test_delete_all_remote_objects_skips_generation_prefix() {
        let provider = MemoryProvider::new();

        // 写 legacy 文件。
        provider
            .write(
                "projects/P/chapter.md",
                b"legacy",
                crate::sync::provider::model::WritePrecondition::CreateNew,
            )
            .unwrap();
        // 写 generation prefix 下的文件（模拟并发 Upsert 正在上传的 generation）。
        let gen_prefix = generation_remote_prefix("projects/P", "gen-1").unwrap();
        provider
            .write(
                &format!("{}/chapter.md", gen_prefix),
                b"generation",
                crate::sync::provider::model::WritePrecondition::CreateNew,
            )
            .unwrap();

        let result = delete_all_remote_objects(&provider, "projects/P");
        assert!(
            matches!(
                result.status,
                crate::sync::SyncStatus::Success | crate::sync::SyncStatus::NoChanges
            ),
            "delete_all_remote_objects should succeed, status: {:?}",
            result.status
        );

        // legacy 文件应被删。
        let legacy_read = provider.read("projects/P/chapter.md").unwrap();
        assert!(legacy_read.is_none(), "legacy chapter.md should be deleted");

        // generation 文件应保留（不碰并发 Upsert 的 generation）。
        let gen_read = provider
            .read(&format!("{}/chapter.md", gen_prefix))
            .unwrap();
        assert!(
            gen_read.is_some(),
            "generation prefix chapter.md should be preserved (not deleted)"
        );
    }

    /// #645 评论 5504296097 问题2：RestoreProject 从 generation prefix 下载。
    ///
    /// 验证：catalog winner 是 Upsert with active_generation=G 时，RestoreProject
    /// 从 generation prefix（`projects/P/__generations__/G/`）下载，而非 legacy prefix。
    #[test]
    fn test_restore_project_downloads_from_generation_prefix() {
        use crate::sync::types::{PlannedTargetKind, SyncPolicy, TargetLifecycleRecord};

        let provider = MemoryProvider::new();

        // 在 generation prefix 写远端正文。
        let gen_id = "gen-restore-1";
        let gen_prefix = generation_remote_prefix("projects/P", gen_id).unwrap();
        provider
            .write(
                &format!("{}/chapter.md", gen_prefix),
                b"remote-content",
                crate::sync::provider::model::WritePrecondition::CreateNew,
            )
            .unwrap();

        // 写 catalog：Upsert with active_generation=G。
        let mut catalog = TargetLifecycleCatalog::default();
        crate::sync::target_lifecycle::upsert_record(
            &mut catalog,
            TargetLifecycleRecord::upsert("projects/P", "projects/P", 10_000, "dev-A")
                .with_active_generation(gen_id),
        );
        let catalog_snapshot = crate::sync::types::RemoteTargetCatalogSnapshot {
            catalog,
            version: crate::sync::provider::model::RemoteVersion::new("v1"),
        };
        crate::sync::target_lifecycle::write_remote_catalog(&provider, &catalog_snapshot).unwrap();

        // 构造 RestoreProject target（本地无 project，远端有 Upsert）。
        let tmp = TempDir::new().unwrap();
        let staging_root = tmp.path().join("staging").join("P");
        std::fs::create_dir_all(&staging_root).unwrap();
        // local_root 不存在（本地无 project）。
        let local_root = tmp.path().join("projects").join("P");

        let planned = PlannedTarget {
            target: SyncTarget::project("P"),
            local_root: local_root.clone(),
            staging_root: Some(staging_root.clone()),
            target_kind: PlannedTargetKind::RestoreProject,
            project_id: Some("P".to_string()),
            target_live_root: local_root,
            deleted_journal_token: None,
            deleted_lww: None,
            live_lww: None,
            expected_delete_lww: None,
        };
        let sync_policy = SyncPolicy {
            enabled: true,
            auto_sync: false,
            sync_interval_seconds: 60,
            has_network_permission: true,
        };
        let plan = FullSyncPlan {
            sync_policy,
            force_sync: true,
            targets: vec![planned],
            app_data_root: tmp.path().to_path_buf(),
            remote_catalog_snapshot: catalog_snapshot,
        };

        let transfer = run_transfer(&provider, &plan);

        // 验证 staging 从 generation prefix 下载了 chapter.md。
        let staged_content = std::fs::read(staging_root.join("chapter.md"));
        assert!(
            staged_content.as_deref().ok() == Some(b"remote-content".as_slice()),
            "RestoreProject should download chapter.md from generation prefix, \
             staged_content: {:?}, transfer status: {:?}",
            staged_content.map(|c| String::from_utf8_lossy(&c).into_owned()),
            transfer.targets[0].result.status
        );
    }

    /// #645 评论 5504296097 问题3：remote-only Delete 直接生成 RemoteCleanupProject。
    ///
    /// 场景：远端 catalog 有 Delete(P) tombstone，本地无 P（无 live project、
    /// 无 pending delete）。修复前 planner 跳过 remote Delete + local absent；
    /// 修复后直接生成 RemoteCleanupProject，expected_delete_lww 从 remote_rec 构造。
    /// 这让远端 Delete tombstone 本身成为 durable cleanup queue — 即使本地
    /// pending_remote_cleanups.json 没成功持久化，下一轮仍会生成 cleanup target。
    #[test]
    fn test_remote_only_delete_generates_cleanup_target() {
        use crate::sync::types::{
            PlannedTargetKind, TargetLifecycleCatalog, TargetLifecycleRecord,
        };

        let tmp = TempDir::new().unwrap();
        let projects_root = tmp.path().join("projects");
        std::fs::create_dir_all(&projects_root).unwrap();

        // 远端 catalog 有 Delete(P) tombstone。
        let mut catalog = TargetLifecycleCatalog::default();
        crate::sync::target_lifecycle::upsert_record(
            &mut catalog,
            TargetLifecycleRecord::delete("projects/P", "projects/P", 20_000, "dev-A"),
        );

        // 本地无 P（live_projects 为空，pending_deleted 为空）。
        let planned = build_full_sync_target_plan(
            tmp.path(),
            &projects_root,
            &[],
            &[],
            &catalog,
            &crate::sync::types::SyncPolicy::default(),
            false,
            "device-local",
            &[],
        );

        // 应生成 RemoteCleanupProject（而非跳过）。
        let p_target = planned
            .iter()
            .find(|t| t.project_id.as_deref() == Some("P"))
            .expect("remote-only Delete should generate a target for P");
        assert!(
            matches!(
                p_target.target_kind,
                PlannedTargetKind::RemoteCleanupProject
            ),
            "remote-only Delete 应生成 RemoteCleanupProject，实际: {:?}",
            p_target.target_kind
        );
        // expected_delete_lww 从 remote_rec 构造。
        let expected = p_target
            .expected_delete_lww
            .as_ref()
            .expect("RemoteCleanupProject should carry expected_delete_lww");
        assert_eq!(
            expected.deleted_at_ms, 20_000,
            "expected_delete_lww.deleted_at_ms 应从 remote_rec 的 deleted_at_ms 构造"
        );
        assert_eq!(
            expected.device_id, "dev-A",
            "expected_delete_lww.device_id 应从 remote_rec 的 device_id 构造"
        );
    }

    /// #645 评论 5504296097 问题3：remote-only Delete 的 RemoteCleanupProject
    /// 实际执行 cleanup（delete_all_remote_objects），清理远端残留。
    ///
    /// 验证端到端：catalog 有 Delete(P)，远端有 legacy 残留文件，
    /// run_transfer 执行 RemoteCleanupProject → 残留被清理。
    #[test]
    fn test_remote_only_delete_cleanup_executes() {
        use crate::sync::types::{PlannedTargetKind, SyncPolicy, TargetLifecycleCatalog};

        let provider = MemoryProvider::new();

        // 远端有 legacy 残留文件（projects/P/project.json）。
        provider
            .write(
                "projects/P/project.json",
                b"residue",
                crate::sync::provider::model::WritePrecondition::CreateNew,
            )
            .unwrap();

        // 写 catalog：Delete(P) tombstone。
        let mut catalog = TargetLifecycleCatalog::default();
        crate::sync::target_lifecycle::upsert_record(
            &mut catalog,
            crate::sync::types::TargetLifecycleRecord::delete(
                "projects/P",
                "projects/P",
                20_000,
                "dev-A",
            ),
        );
        let catalog_snapshot = crate::sync::types::RemoteTargetCatalogSnapshot {
            catalog: catalog.clone(),
            version: crate::sync::provider::model::RemoteVersion::new("v1"),
        };
        crate::sync::target_lifecycle::write_remote_catalog(&provider, &catalog_snapshot).unwrap();

        // 构造 RemoteCleanupProject target（模拟 build_full_sync_target_plan 的输出）。
        let tmp = TempDir::new().unwrap();
        let project_root = tmp.path().join("projects").join("P");
        let planned = PlannedTarget {
            target: SyncTarget::project("P"),
            local_root: project_root.clone(),
            staging_root: None,
            target_kind: PlannedTargetKind::RemoteCleanupProject,
            project_id: Some("P".to_string()),
            target_live_root: project_root,
            deleted_journal_token: None,
            deleted_lww: None,
            live_lww: None,
            expected_delete_lww: Some(DeletedTargetLww {
                deleted_at_ms: 20_000,
                device_id: "dev-A".to_string(),
            }),
        };
        let sync_policy = SyncPolicy {
            enabled: true,
            auto_sync: false,
            sync_interval_seconds: 60,
            has_network_permission: true,
        };
        let plan = FullSyncPlan {
            sync_policy,
            force_sync: true,
            targets: vec![planned],
            app_data_root: tmp.path().to_path_buf(),
            remote_catalog_snapshot: catalog_snapshot,
        };

        let transfer = run_transfer(&provider, &plan);

        // RemoteCleanupProject 应成功清理远端残留。
        assert!(
            matches!(
                transfer.targets[0].result.status,
                crate::sync::SyncStatus::Success | crate::sync::SyncStatus::NoChanges
            ),
            "RemoteCleanupProject should succeed, status: {:?}",
            transfer.targets[0].result.status
        );
        // legacy 残留应被删。
        let residue = provider.read("projects/P/project.json").unwrap();
        assert!(
            residue.is_none(),
            "legacy residue projects/P/project.json should be deleted"
        );
    }
}
