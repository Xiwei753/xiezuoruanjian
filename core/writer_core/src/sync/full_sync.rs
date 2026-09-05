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
#[allow(clippy::too_many_arguments)] // 8 个参数均为独立决策输入，打包会掩盖各自语义
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
    });

    // Project targets — #645 评论 5504296097 问题1：按 remote catalog 决策。
    for project in live_projects {
        let target = crate::sync::types::SyncTarget::project(&project.id);
        let project_local_root = projects_root.join(&project.id);
        let target_id = &target.remote_prefix;

        // #645 评论 5504296097 问题2：用 lifecycle candidate 替代直接读 manifest。
        // manifest 缺失 → 建立初始 manifest；manifest 损坏 → Retry。
        let candidate = compute_local_project_lifecycle_candidate(&project_local_root, device_id);
        let (live_lww, kind) = match &candidate {
            LifecycleCandidate::Live {
                lww,
                initial_manifest,
            } => {
                let lww_clone = lww.clone();
                let kind = decide_live_project_kind(remote_catalog, target_id, Some(lww));
                // #645 评论 5504296097 问题4：initial_manifest 由正式同步在
                // staging 阶段写入，planner 不落盘。此处仅记录日志。
                if let Some(m) = initial_manifest {
                    log::debug!(
                        "[sync] build_full_sync_target_plan: project {} has initial \
                         manifest with {} files — will write during staging",
                        project.id,
                        m.files.len()
                    );
                }
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
        });
    }

    // #645 评论 5504296097 问题1：遍历 remote_catalog.records 补远端独有 target。
    // 对本地既没有 live project 也没有 pending delete 的远端记录：
    // - 远端 Upsert → RestoreProject（下载远端恢复，让新设备发现远端独有作品）；
    // - 远端 Delete → 跳过（本地没有该 project，无需删除）。
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
            if remote_rec.op != crate::sync::types::TargetOp::Upsert {
                // 远端 delete tombstone 且本地没有该 project → 无需删除，跳过。
                continue;
            }
            // 远端 upsert 且本地没有 → RestoreProject。
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
            });
        }
    }

    targets
}

/// #645 评论 5504296097 问题2：本地 project lifecycle candidate。
///
/// `compute_local_project_lifecycle_candidate` 按优先级产出：
/// - `Live(lww)`：已有合法 manifest，或首次同步成功建立初始 manifest；
/// - `Retry`：manifest 损坏或本地状态无法可靠求时间，调用方应让 target 走 Retry。
///
/// #645 评论 5504296097 问题4修复：planner 不再落盘 manifest。
/// `Live { initial_manifest: Some(_) }` 携带待写入的初始 manifest，
/// 正式同步在 staging/transfer 阶段写入，commit 成功后进入 live。
/// dry-run 只计算计划，绝不改文件。
#[derive(Debug, Clone)]
pub(crate) enum LifecycleCandidate {
    Live {
        lww: LiveTargetLww,
        /// #645 评论 5504296097 问题4修复：首次同步时携带待写入的初始 manifest。
        /// 正式同步在 staging 阶段写入；dry-run 不写入。`None` 表示已有 manifest。
        initial_manifest: Option<crate::sync::types::SyncManifest>,
    },
    Retry,
}

/// #645 评论 5504296097 问题2：从本地 sync manifest 或元数据建立 lifecycle candidate。
///
/// 来源按优先级：
/// 1. 已有合法 sync manifest → 用 manifest max LWW；
/// 2. manifest 不存在（首次同步）→ 从 `project.json` 的 `updated_at` 建立初始
///    LWW，并写一份只含 `project.json` 记录的初始 manifest（让后续同步有据可依）；
/// 3. manifest 存在但损坏 / `project.json` 也读不出 → `Retry`（绝不伪造 now()）。
///
/// `device_id` 来自真实 `DeviceInfo.device_id`，用于 LWW tie-break。
#[allow(clippy::excessive_nesting)]
fn compute_local_project_lifecycle_candidate(
    project_root: &Path,
    device_id: &str,
) -> LifecycleCandidate {
    let manifest_path = project_root.join("app-meta/sync/manifest.sync.json");

    // 1. 已有 manifest — 尝试解析。
    if manifest_path.exists() {
        match std::fs::read(&manifest_path) {
            Ok(content) => {
                match serde_json::from_slice::<crate::sync::types::SyncManifest>(&content) {
                    Ok(manifest) => {
                        // #645 评论 5504296097 问题3修复：用 manifest_target_lww
                        // 取 winner 的 device_id，不再硬塞本机设备。
                        if let Some(lww) = manifest_target_lww(&manifest) {
                            return LifecycleCandidate::Live {
                                lww,
                                initial_manifest: None,
                            };
                        }
                        // manifest 合法但 files 为空 — 视为首次同步的空 manifest。
                        // 走建立初始流程（扫描整个 Project）。
                    }
                    Err(e) => {
                        log::warn!(
                            "[sync] compute_local_project_lifecycle_candidate: \
                         manifest corrupt at {}: {e} — returning Retry",
                            manifest_path.display()
                        );
                        return LifecycleCandidate::Retry;
                    }
                }
            }
            Err(e) => {
                log::warn!(
                    "[sync] compute_local_project_lifecycle_candidate: \
                     manifest read failed at {}: {e} — returning Retry",
                    manifest_path.display()
                );
                return LifecycleCandidate::Retry;
            }
        }
    }

    // 2. manifest 不存在或 files 为空 — 首次同步，扫描整个 Project 建立初始。
    // #645 评论 5504296097 问题4修复：planner 不再落盘 manifest，
    // 返回 initial_manifest 让正式同步在 staging 阶段写入。
    match build_initial_lww_from_project_scan(project_root, device_id) {
        Some((initial_lww, initial_manifest)) => {
            log::debug!(
                "[sync2: compute_local_project_lifecycle_candidate: \
                 initial manifest prepared (not written) at {} — lww_time={} device_id={}",
                project_root.display(),
                initial_lww.lww_time_ms,
                initial_lww.device_id
            );
            LifecycleCandidate::Live {
                lww: initial_lww,
                initial_manifest: Some(initial_manifest),
            }
        }
        None => {
            log::warn!(
                "[sync] compute_local_project_lifecycle_candidate: \
                 cannot establish initial lww from project scan at {} — returning Retry",
                project_root.display()
            );
            LifecycleCandidate::Retry
        }
    }
}

/// #645 评论 5504296097 问题4修复：从整个 Project 扫描建立初始 LWW + 初始 manifest。
///
/// 扫描这个 Project 的全部可同步内容（project.json, volumes/*/volume.json,
/// volumes/*/chapters/*/chapter.meta.json, characters/*.json 等），
/// 构造完整初始 manifest，然后从完整 manifest 取最大 `(time, device_id)`。
///
/// 成功 → 返回 `(LiveTargetLww, SyncManifest)`；失败 → None。
/// 调用方（planner）不落盘 manifest，由正式同步在 staging 阶段写入。
fn build_initial_lww_from_project_scan(
    project_root: &Path,
    device_id: &str,
) -> Option<(LiveTargetLww, crate::sync::types::SyncManifest)> {
    let mut files: Vec<crate::sync::types::ManifestFileRecord> = Vec::new();

    // 1. project.json
    if let Some(rec) = scan_sync_file(project_root, "project.json", device_id) {
        files.push(rec);
    }

    // 2. volumes/*/volume.json
    let volumes_dir = project_root.join("volumes");
    if let Ok(volume_entries) = std::fs::read_dir(&volumes_dir) {
        for volume_entry in volume_entries.flatten() {
            if !volume_entry
                .file_type()
                .map(|t| t.is_dir())
                .unwrap_or(false)
            {
                continue;
            }
            let volume_id = volume_entry.file_name().to_string_lossy().to_string();
            let volume_json_rel = format!("volumes/{}/volume.json", volume_id);
            if let Some(rec) = scan_sync_file(project_root, &volume_json_rel, device_id) {
                files.push(rec);
            }

            // 3. volumes/*/chapters/*/chapter.meta.json
            // 直接构造 chapters_dir，避免 .parent().unwrap()（clippy unwrap_used + AGENTS 禁止 unwrap）。
            let chapters_dir = project_root
                .join("volumes")
                .join(&volume_id)
                .join("chapters");
            append_chapter_meta_records(
                project_root,
                &chapters_dir,
                &volume_id,
                device_id,
                &mut files,
            );
        }
    }

    // 4. characters/*.json（Project scope metadata）
    let characters_dir = project_root.join("characters");
    append_character_records(project_root, &characters_dir, device_id, &mut files);

    if files.is_empty() {
        return None;
    }

    let manifest = crate::sync::types::SyncManifest { files };
    let lww = manifest_target_lww(&manifest)?;
    Some((lww, manifest))
}

/// #645 评论 5504296097 问题4修复：扫描单个可同步文件，构造 manifest 记录。
///
/// 读文件的元数据（updated_at from JSON 或文件 mtime），返回 `ManifestFileRecord`。
/// 文件不存在或读取失败 → None。
fn scan_sync_file(
    project_root: &Path,
    rel: &str,
    device_id: &str,
) -> Option<crate::sync::types::ManifestFileRecord> {
    let path = project_root.join(rel);
    let content = std::fs::read(&path).ok()?;

    // 尝试从 JSON 读 updated_at（RFC3339），失败时用文件 mtime。
    let updated_at_ms = serde_json::from_slice::<serde_json::Value>(&content)
        .ok()
        .and_then(|v| {
            v.get("updated_at")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .and_then(|s| rfc3339_to_epoch_millis(&s))
        .or_else(|| {
            std::fs::metadata(&path)
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|t| {
                    t.duration_since(std::time::UNIX_EPOCH)
                        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
                        .unwrap_or(0)
                })
        })?;

    let content_hash = format!("{:x}", md5::compute(&content));
    Some(crate::sync::types::ManifestFileRecord {
        path: rel.to_string(),
        content_hash,
        updated_at_ms,
        deleted_at_ms: None,
        device_id: device_id.to_string(),
        op: "upsert".to_string(),
        schema_version: 1,
    })
}

/// #645 评论 5504296097 问题4修复：扫描 `volumes/<volume_id>/chapters/*/chapter.meta.json`。
///
/// 抽出为 helper 以降低 `build_initial_lww_from_project_scan` 的嵌套深度（clippy too_many_lines）。
fn append_chapter_meta_records(
    project_root: &Path,
    chapters_dir: &Path,
    volume_id: &str,
    device_id: &str,
    files: &mut Vec<crate::sync::types::ManifestFileRecord>,
) {
    let Ok(chapter_entries) = std::fs::read_dir(chapters_dir) else {
        return;
    };
    for chapter_entry in chapter_entries.flatten() {
        if !chapter_entry
            .file_type()
            .map(|t| t.is_dir())
            .unwrap_or(false)
        {
            continue;
        }
        let chapter_id = chapter_entry.file_name().to_string_lossy().to_string();
        let chapter_meta_rel = format!(
            "volumes/{}/chapters/{}/chapter.meta.json",
            volume_id, chapter_id
        );
        if let Some(rec) = scan_sync_file(project_root, &chapter_meta_rel, device_id) {
            files.push(rec);
        }
    }
}

/// #645 评论 5504296097 问题4修复：扫描 `characters/*.json`。
fn append_character_records(
    project_root: &Path,
    characters_dir: &Path,
    device_id: &str,
    files: &mut Vec<crate::sync::types::ManifestFileRecord>,
) {
    let Ok(char_entries) = std::fs::read_dir(characters_dir) else {
        return;
    };
    for char_entry in char_entries.flatten() {
        if !char_entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        let name = char_entry.file_name().to_string_lossy().to_string();
        if name.ends_with(".json") {
            let rel = format!("characters/{}", name);
            if let Some(rec) = scan_sync_file(project_root, &rel, device_id) {
                files.push(rec);
            }
        }
    }
}

/// #645 评论 5504296097 问题2：RFC3339 字符串 → Unix 毫秒时间戳。
fn rfc3339_to_epoch_millis(s: &str) -> Option<i64> {
    let dt = chrono::DateTime::parse_from_rfc3339(s).ok()?;
    Some(dt.timestamp_millis())
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
                    let sync_root = planned
                        .staging_root
                        .as_deref()
                        .unwrap_or(&planned.local_root);
                    let content_result = run_single_target(
                        provider,
                        sync_root,
                        &plan.sync_policy,
                        &planned.target,
                        plan.force_sync,
                    );
                    // 正文 transfer 失败 → 不发布 lifecycle，直接返回错误。
                    let content_ok = matches!(
                        content_result.status,
                        SyncStatus::Success | SyncStatus::NoChanges | SyncStatus::LatestWinsApplied
                    );
                    if !content_ok {
                        (content_result, None, None)
                    } else {
                        // 2. 正文 transfer 成功 → 从 post-transfer staging manifest
                        //    算最终 LWW（#645 评论 5504296097 问题3修复）。
                        //    live_lww 只是 Transfer 前的资格判断，不是最终状态。
                        //    用 winner 的 device_id 构造 candidate，不再硬塞本机设备。
                        let post_transfer_root = planned
                            .staging_root
                            .as_deref()
                            .unwrap_or(&planned.local_root);
                        match read_post_transfer_lww(post_transfer_root) {
                            Some(post_transfer_lww) => {
                                let candidate = crate::sync::types::TargetLifecycleRecord::upsert(
                                    &planned.target.remote_prefix,
                                    &planned.target.remote_prefix,
                                    post_transfer_lww.lww_time_ms,
                                    &post_transfer_lww.device_id,
                                );
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
                                    TargetLifecycleApplyResult::AlreadyCurrent(persisted) => {
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
                                                log::info!(
                                                    "[sync] run_transfer: LiveProject \
                                                     RemoteWinner(Upsert) {} — keeping live",
                                                    planned.target.remote_prefix
                                                );
                                                (content_result, None, None)
                                            }
                                            crate::sync::types::TargetOp::Delete => {
                                                // #645 评论 5504296097 问题1：不在 Transfer 里
                                                // remove_dir_all()。返回 DeleteProject action，
                                                // 由 Commit 阶段执行完整 Project 本地删除事务。
                                                // #645 评论 5504296097 问题3：携带 expected_local_lww guard。
                                                // #645 评论 5504296097 问题4：RemoteWinner(Delete) 后
                                                // 清理远端 projects/P/ 下所有对象（含本轮上传正文 +
                                                // manifest.sync.json + 旧残留），删对 prefix。
                                                log::info!(
                                                    "[sync] run_transfer: LiveProject \
                                                     RemoteWinner(Delete) {} — cleaning remote + deferring delete \
                                                     to Commit",
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
                                                    // #645 评论 5504296097 问题4：远端清理失败 →
                                                    // RecoverableError，不删本地（catalog Delete 已持久，
                                                    // 下轮重新清理远端残留）。
                                                    (cleanup_result, None, None)
                                                } else {
                                                    let action = planned.project_id.as_ref().map(
                                                        |pid| {
                                                            crate::sync::types::
                                                                LocalLifecycleCommitAction::DeleteProject {
                                                                    project_id: pid.clone(),
                                                                    expected_local_lww:
                                                                        planned.live_lww.as_ref().map(
                                                                            crate::sync::types::
                                                                                LiveTargetLwwSerde::from_lww,
                                                                        ),
                                                                }
                                                        },
                                                    );
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
                                let msg = "post-transfer staging manifest unreadable".to_string();
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
                match resolve_current_target_lifecycle(provider, &planned.target.remote_prefix) {
                    Ok(Some(current_rec)) => {
                        use crate::sync::types::TargetOp;
                        match current_rec.op {
                            TargetOp::Upsert => {
                                // #645 评论 5504296097 问题2：远端已变成 upsert 且本地已有 Project
                                // → ReplaceProject 整树替换（不再用空 staging + 普通三方 commit 冒充）。
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
                                let action = planned.project_id.as_ref().map(|pid| {
                                    crate::sync::types::LocalLifecycleCommitAction::ReplaceProject {
                                        project_id: pid.clone(),
                                        expected_local_lww: planned
                                            .live_lww
                                            .as_ref()
                                            .map(crate::sync::types::LiveTargetLwwSerde::from_lww),
                                    }
                                });
                                (result, None, action)
                            }
                            TargetOp::Delete => {
                                // 远端仍是 delete — 确认删除，defer to Commit。
                                // #645 评论 5504296097 问题3：携带 expected_local_lww guard。
                                log::info!(
                                    "[sync] run_transfer: DeleteLocalProject {} — \
                                     CAS confirmed remote Delete, deferring to Commit",
                                    planned.target.remote_prefix
                                );
                                let action = planned.project_id.as_ref().map(|pid| {
                                    crate::sync::types::LocalLifecycleCommitAction::DeleteProject {
                                        project_id: pid.clone(),
                                        expected_local_lww: planned
                                            .live_lww
                                            .as_ref()
                                            .map(crate::sync::types::LiveTargetLwwSerde::from_lww),
                                    }
                                });
                                (SyncResult::no_changes(), None, action)
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
                match resolve_current_target_lifecycle(provider, &planned.target.remote_prefix) {
                    Ok(Some(current_rec)) => {
                        use crate::sync::types::TargetOp;
                        match current_rec.op {
                            TargetOp::Delete => {
                                // 远端已变成 delete — 改走 DeleteLocalProject。
                                // #645 评论 5504296097 问题3：携带 expected_local_lww guard。
                                log::info!(
                                    "[sync] run_transfer: RestoreProject {} — \
                                     CAS: remote changed to Delete, switching to DeleteLocalProject",
                                    planned.target.remote_prefix
                                );
                                let action = planned.project_id.as_ref().map(|pid| {
                                    crate::sync::types::LocalLifecycleCommitAction::DeleteProject {
                                        project_id: pid.clone(),
                                        expected_local_lww: planned
                                            .live_lww
                                            .as_ref()
                                            .map(crate::sync::types::LiveTargetLwwSerde::from_lww),
                                    }
                                });
                                (SyncResult::no_changes(), None, action)
                            }
                            TargetOp::Upsert => {
                                // 远端仍是 upsert — 确认恢复，下载远端内容。
                                log::info!(
                                    "[sync] run_transfer: RestoreProject {} — \
                                     CAS confirmed remote Upsert, downloading remote to staging",
                                    planned.target.remote_prefix
                                );
                                let result = download_remote_to_staging(
                                    provider,
                                    &planned.target.remote_prefix,
                                    planned.staging_root.as_deref(),
                                );
                                (
                                    result,
                                    Some(DeletedTargetResolution::RemoteTargetWins),
                                    None,
                                )
                            }
                        }
                    }
                    Ok(None) => {
                        // 远端无记录 — 无法确认 upsert winner，保守下载恢复
                        // （plan 阶段已确认远端 upsert 胜出）。
                        log::info!(
                            "[sync] run_transfer: RestoreProject {} — \
                             CAS: no remote record, downloading (plan confirmed)",
                            planned.target.remote_prefix
                        );
                        let result = download_remote_to_staging(
                            provider,
                            &planned.target.remote_prefix,
                            planned.staging_root.as_deref(),
                        );
                        (
                            result,
                            Some(DeletedTargetResolution::RemoteTargetWins),
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

    FullSyncTransferResult { targets }
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
                path: "chapter.md".to_string(),
                content_hash: "h".to_string(),
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
                path: "chapter.md".to_string(),
                content_hash: "h".to_string(),
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
        };
        let plan = FullSyncPlan {
            sync_policy: SyncPolicy::default(),
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
        };
        let plan = FullSyncPlan {
            sync_policy: SyncPolicy::default(),
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
        std::fs::create_dir_all(project_root.join("app-meta/sync")).unwrap();
        let manifest = SyncManifest {
            files: vec![ManifestFileRecord {
                path: "project.json".to_string(),
                content_hash: "h".to_string(),
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
        };
        let plan = FullSyncPlan {
            sync_policy: SyncPolicy::default(),
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
        };
        let plan = FullSyncPlan {
            sync_policy: SyncPolicy::default(),
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
        };
        let plan = FullSyncPlan {
            sync_policy: SyncPolicy::default(),
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
            sync_policy: crate::sync::types::SyncPolicy::default(),
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
            live_lww: None,
        };
        let plan = FullSyncPlan {
            sync_policy: crate::sync::types::SyncPolicy::default(),
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
                content_hash: "h".to_string(),
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
        // 读取 sync_api.rs 源码，确认 load_remote_catalog 在 core_write() 作用域内。
        let source = include_str!("../api/sync_api.rs");
        let perform_full_sync_start = source.find("pub fn perform_full_sync(");
        assert!(
            perform_full_sync_start.is_some(),
            "问题6: perform_full_sync 函数应存在于 sync_api.rs"
        );
        let start = perform_full_sync_start.unwrap();
        // 取函数体前 4000 字符做检查（覆盖整个 Prepare 阶段）。
        let body = &source[start..source.len().min(start + 4000)];

        let has_core_write = body.contains("let core = self.core_write();");
        let has_load_remote_catalog = body.contains("load_remote_catalog(provider.as_ref())");
        let has_prepare_full_sync = body.contains("core.prepare_full_sync(");
        let has_lock_released = body.contains("写锁已释放");

        // 四个都存在 — 证明代码结构如所述。
        assert!(has_core_write, "问题6: core_write() 调用应存在");
        assert!(
            has_load_remote_catalog,
            "问题6: load_remote_catalog 调用应存在"
        );
        assert!(has_prepare_full_sync, "问题6: prepare_full_sync 调用应存在");
        assert!(has_lock_released, "问题6: '写锁已释放' 注释应存在");

        let core_write_pos = body.find("let core = self.core_write();").unwrap();
        let load_catalog_pos = body.find("load_remote_catalog(provider.as_ref())").unwrap();
        let lock_released_pos = body.find("写锁已释放").unwrap();

        // 期望（修复后）：load_remote_catalog 应在 "写锁已释放" 注释之后
        // （即网络 IO 在写锁作用域外）。
        // 当前：load_remote_catalog 在 core_write() 之后、"写锁已释放" 之前，
        // 即网络 IO 期间持 core 写锁。
        // 这个断言对当前代码会 FAIL — 证明网络 IO 在写锁内。
        assert!(
            load_catalog_pos > lock_released_pos,
            "问题6: load_remote_catalog (pos={}) 应在 '写锁已释放' (pos={}) 之后\
             （即网络 IO 不在写锁内）。当前 load_remote_catalog 在 core_write (pos={}) 之后、\
             写锁释放 (pos={}) 之前，网络 IO 期间持 core 写锁，阻塞正文/作品读取，\
             与 #644 拆锁路线冲突。",
            load_catalog_pos,
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
        };
        let plan = FullSyncPlan {
            sync_policy: SyncPolicy::default(),
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
        let staging_root = tmp.path().join("staging-p1");
        std::fs::create_dir_all(staging_root.join("app-meta/sync")).unwrap();
        let staging_manifest = SyncManifest {
            files: vec![ManifestFileRecord {
                path: "chapter.md".to_string(),
                content_hash: "h".to_string(),
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
        };
        let plan = FullSyncPlan {
            sync_policy: SyncPolicy::default(),
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
        };
        let plan = FullSyncPlan {
            sync_policy: SyncPolicy::default(),
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
                content_hash: "h".to_string(),
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
}
