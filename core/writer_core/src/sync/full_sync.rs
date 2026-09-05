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

// ── Plan / Transfer 结果 ──

/// Prepare 阶段产出 — Transfer 阶段需要的全部数据（owned，不依赖 core 锁）。
///
/// 包含 sync_policy、force_sync 标志和已枚举的 target 列表（含每个 target
/// 的 `local_root`）。Transfer 阶段只读这份 plan，不再回 core 取数据。
///
/// #645 评论 5504296097 第2点：不再携带 `workspace_git_layout`。
/// 本地 Git 仓库由 bootstrap 阶段初始化，同步计划不负责 Git 生命周期。
#[derive(Debug, Clone)]
pub struct FullSyncPlan {
    pub sync_policy: SyncPolicy,
    pub force_sync: bool,
    pub targets: Vec<PlannedTarget>,
    /// #644 评论 5473401065 第1节：app_data_root 供 API 层在无锁状态下
    /// 调用 `prepare_staging_runs` 时传给 `StagingRun::create`。
    pub app_data_root: PathBuf,
}

/// 单个 target 的执行计划 — target 元数据 + 本地根 + 分类标签。
#[derive(Debug, Clone)]
pub struct PlannedTarget {
    pub target: SyncTarget,
    pub local_root: PathBuf,
    /// staging root for isolated transfer（三段式 staging 路径）。
    /// `Some` 时 Transfer 阶段写 staging 而非 live；`None` 时回退 `local_root`。
    pub staging_root: Option<PathBuf>,
    /// `"app"`、`"project"` 或 `"deleted_project"`，用于 `TargetSyncResult.target_kind`。
    ///
    /// #645 评论 5504296097 问题1：`"deleted_project"` 表示该 target 已删除，
    /// `run_transfer` 走 target-delete 计划（枚举远端前缀下所有对象逐个删除），
    /// 而非普通 `perform_lww_sync`（那会扫描本地目录生成 upsert）。
    pub target_kind: String,
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

impl PlannedTarget {
    /// 是否为待删除 target（`target_kind == "deleted_project"`）。
    pub fn is_deleted_target(&self) -> bool {
        self.target_kind == "deleted_project"
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
/// `remote_catalog` 当前不参与 target 枚举（catalog 在 Transfer 阶段读取做 LWW 决策），
/// 保留参数供未来 dry-run 展示 deleted target 决策使用。`sync_policy` / `force_sync`
/// 同理保留（planner 只做 target 发现，不做同步决策）。
pub fn build_full_sync_target_plan(
    app_data_root: &Path,
    projects_root: &Path,
    live_projects: &[crate::project::Project],
    pending_deleted: &[crate::sync::types::PendingDeletedTarget],
    _remote_catalog: &crate::sync::types::TargetLifecycleCatalog,
    _sync_policy: &crate::sync::types::SyncPolicy,
    _force_sync: bool,
) -> Vec<PlannedTarget> {
    let mut targets = Vec::new();

    // App target
    let app_target = crate::sync::types::SyncTarget::app();
    targets.push(PlannedTarget {
        target: app_target,
        local_root: app_data_root.to_path_buf(),
        staging_root: None,
        target_kind: "app".to_string(),
        project_id: None,
        target_live_root: app_data_root.to_path_buf(),
        deleted_journal_token: None,
        deleted_lww: None,
    });

    // Project targets
    for project in live_projects {
        let target = crate::sync::types::SyncTarget::project(&project.id);
        let project_local_root = projects_root.join(&project.id);
        targets.push(PlannedTarget {
            target,
            local_root: project_local_root.clone(),
            staging_root: None,
            target_kind: "project".to_string(),
            project_id: Some(project.id.clone()),
            target_live_root: project_local_root,
            deleted_journal_token: None,
            deleted_lww: None,
        });
    }

    // #645 评论 5504296097 问题1/4：Pending deleted targets —
    // 已删除作品的远端前缀需要清理。target_kind="deleted_project"，
    // run_transfer 走 target-level LWW 决策。local_root 指向 app_data_root
    // （deleted target 不读本地目录，只枚举远端），staging_root=None。
    // deleted_lww 携带 deleted_at_ms/device_id，供 run_transfer 做 LWW 比较。
    for pending in pending_deleted {
        targets.push(PlannedTarget {
            target: pending.target.clone(),
            local_root: app_data_root.to_path_buf(),
            staging_root: None,
            target_kind: "deleted_project".to_string(),
            project_id: Some(
                pending
                    .target
                    .remote_prefix
                    .strip_prefix("projects/")
                    .map(|s| s.to_string())
                    .unwrap_or_default(),
            ),
            target_live_root: app_data_root.to_path_buf(),
            deleted_journal_token: Some(pending.journal_token.clone()),
            deleted_lww: Some(DeletedTargetLww {
                deleted_at_ms: pending.deleted_at_ms,
                device_id: pending.device_id.clone(),
            }),
        });
    }

    targets
}

/// Transfer 阶段产出 — 各 target 的 `SyncResult`，待 Commit 聚合。
#[derive(Debug, Clone)]
pub struct FullSyncTransferResult {
    pub targets: Vec<TargetSyncResult>,
}

// ── Transfer ──

/// 执行 Transfer 阶段：对 plan 中每个 target 调对应同步函数，收集结果。
///
/// - 普通 target（app/project）调 `perform_lww_sync`；
/// - deleted target（`target_kind == "deleted_project"`）调
///   [`run_deleted_target_sync`] 走 target-level LWW 决策（返回 typed
///   [`DeletedTargetResolution`]），不调 `perform_lww_sync`（那会扫描本地目录
///   生成 upsert，反而把删掉的作品重新上传）。
///
/// 单个 target 的 `Err`（本地 root IO 错、provider 调用失败等）不提前打断：
/// 转为该 target 的 `SyncResult::error(...)` 后 push，继续下一 target。
///
/// #645 评论 5504296097 问题3：Transfer 开始先通过 `SyncProvider` 读取并合并
/// target lifecycle catalog；LocalDeleteWins 时 upsert catalog delete tombstone，
/// 循环结束后统一 `write_remote_catalog` 一次（单线程串行，无竞态）。
///
/// 本函数是纯函数 — 不接触 `WriterCore`、不持锁、不写 `FullSyncState`。
/// 调用方（API 层）在释放 core 写锁后调用。
pub fn run_transfer(provider: &dyn SyncProvider, plan: &FullSyncPlan) -> FullSyncTransferResult {
    // #645 评论 5504296097 问题3：加载远端 target lifecycle catalog。
    // 加载失败 → catalog_load_failed=true，所有 deleted target 走 Retry（不删除）。
    let catalog_load = crate::sync::target_lifecycle::load_remote_catalog(provider);
    let (mut catalog, catalog_load_failed) = match catalog_load {
        Ok(c) => (c, false),
        Err(e) => {
            log::warn!(
                "[sync] run_transfer: load_remote_catalog failed: {e} \
                 — deleted targets will Retry"
            );
            (crate::sync::types::TargetLifecycleCatalog::default(), true)
        }
    };
    let mut catalog_dirty = false;

    let mut targets = Vec::with_capacity(plan.targets.len());
    for planned in &plan.targets {
        let (result, resolution) = if planned.is_deleted_target() {
            // #645 评论 5504296097 问题1/2/3：deleted target 走 target-level LWW
            // 决策，返回 typed DeletedTargetResolution。
            run_deleted_target_sync(
                provider,
                &planned.target,
                planned.deleted_lww.as_ref(),
                &catalog,
                catalog_load_failed,
                planned.staging_root.as_deref(),
            )
        } else {
            // 三段式 staging：staging_root 有值时写隔离目录，否则回退 local_root。
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
            (r, None)
        };

        // #645 评论 5504296097 问题3：LocalDeleteWins 且远端删除成功时，
        // upsert catalog delete tombstone（循环结束后统一写一次）。
        if resolution == Some(crate::sync::types::DeletedTargetResolution::LocalDeleteWins)
            && is_success_status(&result.status)
        {
            let deleted_at_ms = planned
                .deleted_lww
                .as_ref()
                .map(|l| l.deleted_at_ms)
                .unwrap_or_else(|| crate::sync::full_sync_utils::now_epoch_seconds() * 1000);
            let device_id = planned
                .deleted_lww
                .as_ref()
                .map(|l| l.device_id.as_str())
                .unwrap_or("");
            let record = crate::sync::types::TargetLifecycleRecord::delete(
                &planned.target.remote_prefix,
                &planned.target.remote_prefix,
                deleted_at_ms,
                device_id,
            );
            crate::sync::target_lifecycle::upsert_record(&mut catalog, record);
            catalog_dirty = true;
        }

        targets.push(TargetSyncResult {
            target_kind: planned.target_kind.clone(),
            project_id: planned.project_id.clone(),
            remote_prefix: planned.target.remote_prefix.clone(),
            result,
            deleted_resolution: resolution,
        });
    }

    // #645 评论 5504296097 问题3：统一写 catalog（一次 full sync 只写一次，
    // 避免多个 deleted target 串行 read-modify-write 竞态）。写失败只 warn，
    // 不让 catalog 持久化失败覆盖本次同步的 target 结果（下次同步会重写）。
    if catalog_dirty {
        if let Err(e) = crate::sync::target_lifecycle::write_remote_catalog(provider, &catalog) {
            log::warn!("[sync] run_transfer: write_remote_catalog failed: {e}");
        }
    }

    FullSyncTransferResult { targets }
}

/// #645 评论 5504296097 问题1/2/3：执行已删除 target 的 target-level LWW 决策。
///
/// 返回 `(SyncResult, Option<DeletedTargetResolution>)`：
/// - `LocalDeleteWins` → 执行远端删除（catalog tombstone 由 `run_transfer` 统一写）；
/// - `RemoteTargetWins` → 下载远端内容到 staging，commit 阶段恢复本地 project；
/// - `Retry` → 什么都不删/恢复，pending 保留。
///
/// ## LWW 判断规则（完整）
///
/// - catalog 加载失败 → `Retry`（不删除）；
/// - manifest / target state 解析失败、读取失败 → `Retry`（RecoverableError），
///   **不执行任何 provider.delete**，PendingDeletedTarget 保留；
/// - 远端前缀完全不存在（`provider.list` 返回空 且 catalog 无该 target 的 upsert 记录
///   且 manifest 无 upsert 记录）→ 本地 delete 已达成 → `LocalDeleteWins`；
/// - 能读取完整 target lifecycle 状态 → 正常比较 `(time, device_id)`：
///   本地 tombstone 胜出 → `LocalDeleteWins`；远端更晚 → `RemoteTargetWins`。
///
/// **不把"manifest 不存在"和"远端 target 不存在"混成一个意思**：
/// manifest 不存在时先通过 catalog + provider.list 判断 target 是否真的为空。
///
/// provider-neutral：只使用 `SyncProvider::read/list/delete`，不写 GitHub 专用逻辑。
#[allow(clippy::excessive_nesting, clippy::too_many_lines)]
fn run_deleted_target_sync(
    provider: &dyn SyncProvider,
    target: &SyncTarget,
    deleted_lww: Option<&DeletedTargetLww>,
    remote_catalog: &crate::sync::types::TargetLifecycleCatalog,
    catalog_load_failed: bool,
    staging_root: Option<&Path>,
) -> (
    SyncResult,
    Option<crate::sync::types::DeletedTargetResolution>,
) {
    use crate::sync::types::DeletedTargetResolution;
    let remote_prefix = &target.remote_prefix;
    log::debug!(
        "[sync] entry=run_deleted_target_sync remote_prefix={}",
        remote_prefix
    );

    // #645 评论 5504296097 问题2：catalog 加载失败 → Retry（不删除）。
    if catalog_load_failed {
        let msg = "target catalog load failed".to_string();
        return (
            SyncResult::error(SyncStatus::RecoverableError(msg.clone()), msg, None),
            Some(DeletedTargetResolution::Retry),
        );
    }

    // 1. 读远端 manifest（区分不存在 vs 解析失败）。
    let remote_records = match fetch_remote_manifest_for_prefix(provider, remote_prefix) {
        Ok(r) => r,
        Err(e) => {
            // #645 评论 5504296097 问题2：manifest 读取/解析失败 → Retry，不删除。
            log::warn!(
                "[sync] run_deleted_target_sync: manifest read failed for {}: {} — Retry",
                remote_prefix,
                e
            );
            let msg = format!("manifest read failed: {e}");
            return (
                SyncResult::error(SyncStatus::RecoverableError(msg.clone()), msg, None),
                Some(DeletedTargetResolution::Retry),
            );
        }
    };

    // 2. provider.list 判断 target 是否真的有内容。
    let remote_entries = match provider.list(remote_prefix) {
        Ok(e) => e,
        Err(e) => {
            let core_err = crate::Error::from(e);
            let msg = core_err.to_string();
            return (
                SyncResult::error(SyncStatus::RecoverableError(msg.clone()), msg, None),
                Some(DeletedTargetResolution::Retry),
            );
        }
    };

    // 3. 无 LWW 元数据（旧 pending 或非 deleted target）→ 直接删除远端。
    let Some(lww) = deleted_lww else {
        let result = delete_all_remote_objects(provider, remote_prefix);
        let resolution = if is_success_status(&result.status) {
            DeletedTargetResolution::LocalDeleteWins
        } else {
            DeletedTargetResolution::Retry
        };
        return (result, Some(resolution));
    };

    // 4. 判断 target 是否真的不存在：
    //    provider.list 空 且 catalog 无该 target 的 upsert 记录 且 manifest 无 upsert 记录。
    let catalog_has_upsert =
        crate::sync::target_lifecycle::catalog_has_upsert(remote_catalog, remote_prefix);
    let manifest_has_upsert = remote_records.values().any(|r| r.op == "upsert");
    let target_truly_absent =
        remote_entries.is_empty() && !catalog_has_upsert && !manifest_has_upsert;

    // 5. target 真的不存在 → LocalDeleteWins（本地 delete 已达成，远端已空）。
    if target_truly_absent {
        log::info!(
            "[sync] run_deleted_target_sync: target {} truly absent → LocalDeleteWins",
            remote_prefix
        );
        return (
            SyncResult::success(),
            Some(DeletedTargetResolution::LocalDeleteWins),
        );
    }

    // 6. target 存在 → LWW 比较。
    //    远端最新时间 = max(manifest records lww time, catalog record lww time)。
    let remote_max = compute_remote_lww_max(&remote_records, remote_catalog, remote_prefix);
    let local_wins = lww_local_wins(lww, remote_max.as_ref());
    if local_wins {
        log::info!(
            "[sync] run_deleted_target_sync: local_tombstone_wins_lww \
             remote_max={:?} local_deleted_at={} local_device={} → LocalDeleteWins",
            remote_max,
            lww.deleted_at_ms,
            lww.device_id
        );
        // LocalDeleteWins → 执行远端删除。
        let result = delete_all_remote_objects(provider, remote_prefix);
        let resolution = if is_success_status(&result.status) {
            DeletedTargetResolution::LocalDeleteWins
        } else {
            DeletedTargetResolution::Retry
        };
        (result, Some(resolution))
    } else {
        log::info!(
            "[sync] run_deleted_target_sync: remote_wins_lww \
             remote_max={:?} local_deleted_at={} local_device={} → RemoteTargetWins",
            remote_max,
            lww.deleted_at_ms,
            lww.device_id
        );
        // RemoteTargetWins → 下载远端内容到 staging，commit 阶段恢复本地 project。
        let result = download_remote_to_staging(provider, remote_prefix, staging_root);
        (result, Some(DeletedTargetResolution::RemoteTargetWins))
    }
}

/// 判断 `SyncStatus` 是否为成功类（Success / LatestWinsApplied）。
fn is_success_status(status: &SyncStatus) -> bool {
    matches!(status, SyncStatus::Success | SyncStatus::LatestWinsApplied)
}

/// 计算远端 target 的 LWW 最大 `(time, device_id)`：取 manifest 记录和 catalog 记录的最大值。
///
/// 返回 `None` 表示远端无任何记录（本地 tombstone 胜出）。
fn compute_remote_lww_max(
    remote_records: &std::collections::HashMap<String, crate::sync::types::ManifestFileRecord>,
    remote_catalog: &crate::sync::types::TargetLifecycleCatalog,
    remote_prefix: &str,
) -> Option<(i64, String)> {
    let mut max = remote_lww_max(remote_records);
    if let Some(rec) = crate::sync::target_lifecycle::find_record(remote_catalog, remote_prefix) {
        let t = crate::sync::target_lifecycle::record_lww_time(rec);
        let candidate = (t, rec.device_id.clone());
        max = Some(match max {
            Some(existing) => {
                // 与 resolve_lww_path 同规则：(time, device_id) 大的胜出。
                if candidate.0 > existing.0
                    || (candidate.0 == existing.0 && candidate.1 > existing.1)
                {
                    candidate
                } else {
                    existing
                }
            }
            None => candidate,
        });
    }
    max
}

/// 本地 tombstone 是否 LWW 胜出（与 `resolve_lww_path` 同规则）。
fn lww_local_wins(lww: &DeletedTargetLww, remote_max: Option<&(i64, String)>) -> bool {
    let Some((remote_time, remote_device)) = remote_max else {
        return true; // 远端无记录，本地 tombstone 胜出
    };
    if lww.deleted_at_ms > *remote_time {
        true
    } else if lww.deleted_at_ms == *remote_time {
        lww.device_id > *remote_device
    } else {
        false
    }
}

/// #645 评论 5504296097 问题1：RemoteTargetWins 时把远端 `projects/<id>/` 下所有对象
/// 下载到 staging，commit 阶段把 staging 写回 live 恢复本地 project。
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

/// 从远端 manifest 记录中提取最大的 LWW 记录时间和对应的 device_id。
///
/// 返回 `(max_lww_time, device_id_of_max)` 或 `None`（空记录）。
fn remote_lww_max(
    remote_records: &std::collections::HashMap<String, crate::sync::types::ManifestFileRecord>,
) -> Option<(i64, String)> {
    remote_records
        .values()
        .map(|r| (lww_record_time_for_record(r), r.device_id.clone()))
        .max_by_key(|(t, d)| (*t, d.clone()))
}

/// 计算单条 manifest 记录的 LWW 时间（delete 用 `deleted_at_ms`，upsert 用 `updated_at_ms`）。
fn lww_record_time_for_record(r: &crate::sync::types::ManifestFileRecord) -> i64 {
    if r.op == "delete" {
        r.deleted_at_ms.unwrap_or(r.updated_at_ms)
    } else {
        r.updated_at_ms
    }
}

/// 枚举远端前缀下所有对象并逐个删除（Unconditional 幂等）。
///
/// 全部删除成功返回 `SyncResult::success()`（`remote_deletes` 记录已删路径）。
/// 远端 list 失败 → `RecoverableError`。单个 delete 失败 → `RecoverableError`（已删的保留）。
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

/// 从远端读取指定前缀下的 manifest.sync.json 并解析为 `HashMap`。
///
/// provider-neutral：使用 `SyncProvider::read()` 读取 manifest 文件。
/// manifest 路径固定为 `{remote_prefix}/app-meta/sync/manifest.sync.json`。
///
/// 远端不存在 manifest 时返回空 HashMap（视为无远端记录，本地 tombstone 胜出）。
/// manifest 损坏时返回 Err（调用方保守处理：继续删除）。
fn fetch_remote_manifest_for_prefix(
    provider: &dyn SyncProvider,
    remote_prefix: &str,
) -> crate::error::Result<std::collections::HashMap<String, crate::sync::types::ManifestFileRecord>>
{
    let manifest_path = format!("{}/app-meta/sync/manifest.sync.json", remote_prefix);
    let manifest_obj = provider.read(&manifest_path).map_err(crate::Error::from)?;
    let Some(obj) = manifest_obj else {
        return Ok(std::collections::HashMap::new());
    };
    let manifest: crate::sync::types::SyncManifest =
        serde_json::from_slice(&obj.content).map_err(|e| {
            crate::Error::Io(std::io::Error::other(format!(
                "fetch_remote_manifest_for_prefix: parse {}: {e}",
                manifest_path
            )))
        })?;
    let mut map = std::collections::HashMap::new();
    for rec in manifest.files {
        map.insert(rec.path.clone(), rec);
    }
    Ok(map)
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
    match crate::sync::lww::perform_lww_sync(local_root, provider, sync_policy, target, force_sync)
    {
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

    /// 构造一个 deleted target LWW 元数据。
    fn lww(deleted_at_ms: i64, device_id: &str) -> DeletedTargetLww {
        DeletedTargetLww {
            deleted_at_ms,
            device_id: device_id.to_string(),
        }
    }

    /// 在远端写入一个 manifest（含一条 upsert 记录）。
    fn write_remote_manifest(
        provider: &dyn SyncProvider,
        remote_prefix: &str,
        record: ManifestFileRecord,
    ) {
        let manifest = SyncManifest {
            files: vec![record],
        };
        let content = serde_json::to_vec(&manifest).unwrap();
        let path = format!("{}/app-meta/sync/manifest.sync.json", remote_prefix);
        provider
            .write(
                &path,
                &content,
                crate::sync::provider::model::WritePrecondition::Unconditional,
            )
            .unwrap();
    }

    /// #645 评论 5504296097 问题1：target 真不存在（远端空 + catalog 无 upsert）→ LocalDeleteWins。
    #[test]
    fn deleted_target_truly_absent_returns_local_delete_wins() {
        let provider = MemoryProvider::new();
        let target = SyncTarget::project("p1");
        let lww = lww(2000, "dev-1");
        let catalog = TargetLifecycleCatalog::default();

        let (result, resolution) =
            run_deleted_target_sync(&provider, &target, Some(&lww), &catalog, false, None);
        assert_eq!(resolution, Some(DeletedTargetResolution::LocalDeleteWins));
        assert!(is_success_status(&result.status));
    }

    /// #645 评论 5504296097 问题1：本地 tombstone 时间更晚 → LocalDeleteWins，远端被删除。
    #[test]
    fn deleted_target_local_tombstone_wins_deletes_remote() {
        let provider = MemoryProvider::with_entries([(
            "projects/p1/chapter.md".to_string(),
            b"hello".to_vec(),
        )]);
        let target = SyncTarget::project("p1");
        let lww = lww(2000, "dev-1");
        let catalog = TargetLifecycleCatalog::default();

        let (result, resolution) =
            run_deleted_target_sync(&provider, &target, Some(&lww), &catalog, false, None);
        assert_eq!(resolution, Some(DeletedTargetResolution::LocalDeleteWins));
        assert!(is_success_status(&result.status));
        // 远端对象已被删除。
        assert!(provider.read("projects/p1/chapter.md").unwrap().is_none());
    }

    /// #645 评论 5504296097 问题1：远端 manifest 时间更晚 → RemoteTargetWins，下载到 staging。
    #[test]
    fn deleted_target_remote_wins_downloads_to_staging() {
        let provider = MemoryProvider::with_entries([(
            "projects/p1/chapter.md".to_string(),
            b"remote-content".to_vec(),
        )]);
        // 写远端 manifest：updated_at_ms=3000 > 本地 deleted_at_ms=2000。
        write_remote_manifest(
            &provider,
            "projects/p1",
            ManifestFileRecord {
                path: "chapter.md".to_string(),
                content_hash: "h".to_string(),
                updated_at_ms: 3000,
                deleted_at_ms: None,
                device_id: "dev-2".to_string(),
                op: "upsert".to_string(),
                schema_version: 1,
            },
        );
        let target = SyncTarget::project("p1");
        let lww = lww(2000, "dev-1");
        let catalog = TargetLifecycleCatalog::default();

        let tmp = TempDir::new().unwrap();
        let staging = tmp.path().join("staging");
        std::fs::create_dir_all(&staging).unwrap();

        let (result, resolution) = run_deleted_target_sync(
            &provider,
            &target,
            Some(&lww),
            &catalog,
            false,
            Some(&staging),
        );
        assert_eq!(resolution, Some(DeletedTargetResolution::RemoteTargetWins));
        assert!(is_success_status(&result.status));
        // 远端内容已下载到 staging。
        let staged = std::fs::read(staging.join("chapter.md")).unwrap();
        assert_eq!(staged, b"remote-content");
        // 远端对象未被删除（RemoteTargetWins 不删远端）。
        assert!(provider.read("projects/p1/chapter.md").unwrap().is_some());
    }

    /// #645 评论 5504296097 问题2：manifest 解析失败 → Retry，不删除远端。
    #[test]
    fn deleted_target_manifest_parse_failure_returns_retry() {
        let provider = MemoryProvider::with_entries([
            ("projects/p1/chapter.md".to_string(), b"hello".to_vec()),
            (
                "projects/p1/app-meta/sync/manifest.sync.json".to_string(),
                b"not json".to_vec(),
            ),
        ]);
        let target = SyncTarget::project("p1");
        let lww = lww(2000, "dev-1");
        let catalog = TargetLifecycleCatalog::default();

        let (result, resolution) =
            run_deleted_target_sync(&provider, &target, Some(&lww), &catalog, false, None);
        assert_eq!(resolution, Some(DeletedTargetResolution::Retry));
        assert!(matches!(result.status, SyncStatus::RecoverableError(_)));
        // 远端对象未被删除。
        assert!(provider.read("projects/p1/chapter.md").unwrap().is_some());
    }

    /// #645 评论 5504296097 问题2：catalog 加载失败 → Retry，不删除远端。
    #[test]
    fn deleted_target_catalog_load_failed_returns_retry() {
        let provider = MemoryProvider::with_entries([(
            "projects/p1/chapter.md".to_string(),
            b"hello".to_vec(),
        )]);
        let target = SyncTarget::project("p1");
        let lww = lww(2000, "dev-1");
        let catalog = TargetLifecycleCatalog::default();

        let (result, resolution) = run_deleted_target_sync(
            &provider,
            &target,
            Some(&lww),
            &catalog,
            true, // catalog_load_failed
            None,
        );
        assert_eq!(resolution, Some(DeletedTargetResolution::Retry));
        assert!(matches!(result.status, SyncStatus::RecoverableError(_)));
        // 远端对象未被删除。
        assert!(provider.read("projects/p1/chapter.md").unwrap().is_some());
    }

    /// #645 评论 5504296097 问题3：catalog 有该 target 的 upsert 记录 → target 不算 absent，
    /// 走 LWW 比较。本地 tombstone 时间更晚 → LocalDeleteWins。
    #[test]
    fn deleted_target_catalog_upsert_prevents_absent_shortcut() {
        let provider = MemoryProvider::new(); // 远端空
        let target = SyncTarget::project("p1");
        let lww = lww(3000, "dev-1");
        // catalog 有 upsert 记录（target 存在过），时间 1000 < 本地 3000。
        let mut catalog = TargetLifecycleCatalog::default();
        crate::sync::target_lifecycle::upsert_record(
            &mut catalog,
            crate::sync::types::TargetLifecycleRecord::upsert(
                "projects/p1",
                "projects/p1",
                1000,
                "dev-2",
            ),
        );

        let (result, resolution) =
            run_deleted_target_sync(&provider, &target, Some(&lww), &catalog, false, None);
        // target 不算 absent（catalog 有 upsert），走 LWW：本地 3000 > catalog 1000 → LocalDeleteWins。
        assert_eq!(resolution, Some(DeletedTargetResolution::LocalDeleteWins));
        assert!(is_success_status(&result.status));
    }

    /// #645 评论 5504296097 问题3：catalog upsert 时间更晚 + 远端有内容 → RemoteTargetWins
    /// （远端 target 比本地 tombstone 晚，远端胜出，下载到 staging 恢复）。
    #[test]
    fn deleted_target_catalog_upsert_later_returns_remote_wins() {
        let provider = MemoryProvider::with_entries([(
            "projects/p1/chapter.md".to_string(),
            b"remote-content".to_vec(),
        )]);
        let target = SyncTarget::project("p1");
        let lww = lww(1000, "dev-1");
        // catalog 有 upsert 记录，时间 3000 > 本地 1000。
        let mut catalog = TargetLifecycleCatalog::default();
        crate::sync::target_lifecycle::upsert_record(
            &mut catalog,
            crate::sync::types::TargetLifecycleRecord::upsert(
                "projects/p1",
                "projects/p1",
                3000,
                "dev-2",
            ),
        );

        let tmp = TempDir::new().unwrap();
        let staging = tmp.path().join("staging");
        std::fs::create_dir_all(&staging).unwrap();

        let (result, resolution) = run_deleted_target_sync(
            &provider,
            &target,
            Some(&lww),
            &catalog,
            false,
            Some(&staging),
        );
        // 远端有内容 + catalog upsert lww_time=3000 > 本地 1000 → remote wins。
        assert_eq!(resolution, Some(DeletedTargetResolution::RemoteTargetWins));
        assert!(is_success_status(&result.status));
        // 远端内容已下载到 staging。
        let staged = std::fs::read(staging.join("chapter.md")).unwrap();
        assert_eq!(staged, b"remote-content");
    }

    /// #645 评论 5504296097 问题4：build_full_sync_target_plan 包含 pending deleted target。
    #[test]
    fn build_plan_includes_pending_deleted_targets() {
        use crate::sync::types::PendingDeletedTarget;

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
        );

        // App target + 1 deleted target。
        assert_eq!(planned.len(), 2);
        assert_eq!(planned[0].target_kind, "app");
        assert_eq!(planned[1].target_kind, "deleted_project");
        assert_eq!(planned[1].target.remote_prefix, "projects/p-deleted");
        assert_eq!(planned[1].deleted_journal_token.as_deref(), Some("token-1"));
        assert!(planned[1].deleted_lww.is_some());
    }

    /// #645 评论 5504296097 问题4：build_full_sync_target_plan 包含 live project targets。
    #[test]
    fn build_plan_includes_live_projects() {
        use crate::project::Project;

        let tmp = TempDir::new().unwrap();
        let app_root = tmp.path().join("app");
        let projects_root = tmp.path().join("projects");
        std::fs::create_dir_all(&app_root).unwrap();
        std::fs::create_dir_all(&projects_root).unwrap();

        let projects = vec![Project {
            id: "p1".to_string(),
            title: "T1".to_string(),
            created_at: "2024-01-01".to_string(),
            updated_at: "2024-01-01".to_string(),
            order: 0,
        }];
        let sync_policy = crate::sync::types::SyncPolicy::default();

        let planned = build_full_sync_target_plan(
            &app_root,
            &projects_root,
            &projects,
            &[],
            &TargetLifecycleCatalog::default(),
            &sync_policy,
            false,
        );

        // App target + 1 project target。
        assert_eq!(planned.len(), 2);
        assert_eq!(planned[0].target_kind, "app");
        assert_eq!(planned[1].target_kind, "project");
        assert_eq!(planned[1].target.remote_prefix, "projects/p1");
        assert_eq!(planned[1].local_root, projects_root.join("p1"));
    }

    /// #645 评论 5504296097 问题3：run_transfer 在 LocalDeleteWins 时写 catalog delete tombstone。
    #[test]
    fn run_transfer_writes_catalog_tombstone_on_local_delete_wins() {
        use crate::sync::types::SyncPolicy;

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
            target_kind: "deleted_project".to_string(),
            project_id: Some("p1".to_string()),
            target_live_root: tmp.path().to_path_buf(),
            deleted_journal_token: Some("token-1".to_string()),
            deleted_lww: Some(lww_val),
        };
        let plan = FullSyncPlan {
            sync_policy: SyncPolicy::default(),
            force_sync: false,
            targets: vec![planned],
            app_data_root: tmp.path().to_path_buf(),
        };

        let transfer = run_transfer(&provider, &plan);
        assert_eq!(transfer.targets.len(), 1);
        assert_eq!(
            transfer.targets[0].deleted_resolution,
            Some(DeletedTargetResolution::LocalDeleteWins)
        );

        // catalog 已写入 delete tombstone。
        let catalog = crate::sync::target_lifecycle::load_remote_catalog(&provider).unwrap();
        let rec = crate::sync::target_lifecycle::find_record(&catalog, "projects/p1");
        assert!(rec.is_some());
        assert_eq!(rec.unwrap().op, crate::sync::types::TargetOp::Delete);
    }
}
