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
///   [`run_deleted_target_sync`] 走 target-delete 计划（枚举远端前缀下所有对象
///   逐个 `provider.delete`），不调 `perform_lww_sync`（那会扫描本地目录生成
///   upsert，反而把删掉的作品重新上传）。
///
/// 单个 target 的 `Err`（本地 root IO 错、provider 调用失败等）不提前打断：
/// 转为该 target 的 `SyncResult::error(...)` 后 push，继续下一 target。
///
/// 本函数是纯函数 — 不接触 `WriterCore`、不持锁、不写 `FullSyncState`。
/// 调用方（API 层）在释放 core 写锁后调用。
pub fn run_transfer(provider: &dyn SyncProvider, plan: &FullSyncPlan) -> FullSyncTransferResult {
    let mut targets = Vec::with_capacity(plan.targets.len());
    for planned in &plan.targets {
        let result = if planned.is_deleted_target() {
            // #645 评论 5504296097 问题1/3：deleted target 走 target-delete 计划，
            // 先做 LWW 比较（deleted_at_ms vs 远端 manifest），本地 tombstone
            // 胜出才枚举远端前缀下所有对象逐个删除，不调 perform_lww_sync。
            // 远端更晚时跳过删除（远端有更新内容，不应被无条件清空）。
            run_deleted_target_sync(provider, &planned.target, planned.deleted_lww.as_ref())
        } else {
            // 三段式 staging：staging_root 有值时写隔离目录，否则回退 local_root。
            let sync_root = planned
                .staging_root
                .as_deref()
                .unwrap_or(&planned.local_root);
            run_single_target(
                provider,
                sync_root,
                &plan.sync_policy,
                &planned.target,
                plan.force_sync,
            )
        };
        targets.push(TargetSyncResult {
            target_kind: planned.target_kind.clone(),
            project_id: planned.project_id.clone(),
            remote_prefix: planned.target.remote_prefix.clone(),
            result,
        });
    }
    FullSyncTransferResult { targets }
}

/// #645 评论 5504296097 问题1/3：执行已删除 target 的远端清理。
///
/// 走 target-delete 计划：
/// 1. 若有 LWW 元数据，先读远端 manifest 做 LWW 比较（provider-neutral）；
/// 2. `provider.list(remote_prefix)` 枚举远端该 target 下所有对象；
/// 3. 对每个远端对象调 `provider.delete(remote_prefix/path, DeletePrecondition::Unconditional)`；
/// 4. 全部删除成功返回 `SyncResult::success()`（`remote_deletes` 记录已删路径）。
///
/// ## LWW 语义（#645 评论 5504296097 问题3）
///
/// 本地 target tombstone（`deleted_at_ms` / `device_id`）与远端 manifest 的
/// `max(lww_record_time)` 做 LWW 比较（与 `resolve_lww_path` 同规则）：
/// - 本地 tombstone 胜出 → 枚举远端前缀下所有对象逐个删除；
/// - 远端更新更晚 → 不删（远端有更新内容），返回 `NoChanges`（跳过删除）；
/// - 时间相同 → 按 `device_id` 字典序 tie-break（大的胜出）。
///
/// provider-neutral：只使用 `SyncProvider::read/list/delete`，不写 GitHub 专用逻辑。
/// `Unconditional` 删除幂等（远端对象已不存在也返回 Ok）。
///
/// 远端 list 失败 → `RecoverableError`（下次同步可重试）。
/// 单个 delete 失败 → `RecoverableError`（已删的保留，下次同步继续清理剩余）。
#[allow(clippy::excessive_nesting)]
fn run_deleted_target_sync(
    provider: &dyn SyncProvider,
    target: &SyncTarget,
    deleted_lww: Option<&DeletedTargetLww>,
) -> SyncResult {
    let remote_prefix = &target.remote_prefix;
    log::debug!(
        "[sync] entry=run_deleted_target_sync remote_prefix={}",
        remote_prefix
    );

    // #645 评论 5504296097 问题3：LWW 比较 — 读远端 manifest 判断是否应跳过删除。
    if let Some(lww) = deleted_lww {
        let skip = match fetch_remote_manifest_for_prefix(provider, remote_prefix) {
            Ok(remote_records) => should_skip_delete_for_lww(&remote_records, lww),
            Err(e) => {
                log::warn!(
                    "[sync] run_deleted_target_sync: failed to read remote manifest for {}: {} \
                     — proceeding with delete (conservative)",
                    remote_prefix,
                    e
                );
                false
            }
        };
        if skip {
            return SyncResult::no_changes();
        }
    }

    delete_all_remote_objects(provider, remote_prefix)
}

/// 从远端 manifest 记录判断是否应跳过删除（远端 LWW 胜出）。
///
/// #645 评论 5504296097 问题3：用 `lww_record_time`（与 `resolve_lww_path` 同规则）
/// 计算远端最新记录时间，与本地 tombstone 的 `deleted_at_ms` 比较。
/// 时间相同时按 `device_id` 字典序 tie-break。
fn should_skip_delete_for_lww(
    remote_records: &std::collections::HashMap<String, crate::sync::types::ManifestFileRecord>,
    lww: &DeletedTargetLww,
) -> bool {
    let Some((remote_max_time, remote_device_id)) = remote_lww_max(remote_records) else {
        return false;
    };
    // 与 resolve_lww_path 同规则：时间大的胜出，时间相同 device_id 大的胜出。
    let remote_wins = if remote_max_time > lww.deleted_at_ms {
        true
    } else if remote_max_time == lww.deleted_at_ms {
        remote_device_id.as_str() > lww.device_id.as_str()
    } else {
        false
    };
    if remote_wins {
        log::info!(
            "[sync] run_deleted_target_sync: remote_wins_lww \
             remote_time={} remote_device={} local_deleted_at={} local_device={} → skip delete",
            remote_max_time,
            remote_device_id,
            lww.deleted_at_ms,
            lww.device_id
        );
    } else {
        log::info!(
            "[sync] run_deleted_target_sync: local_tombstone_wins_lww \
             remote_time={} remote_device={} local_deleted_at={} local_device={} → proceed with delete",
            remote_max_time,
            remote_device_id,
            lww.deleted_at_ms,
            lww.device_id
        );
    }
    remote_wins
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
