//! Transfer helper functions — shared utilities used by transfer.rs and other submodules.
//!
//! Contains error conversion, remote object operations, staging downloads, single-target sync,
//! and per-kind transfer functions extracted from run_transfer.

use std::path::Path;

use crate::sync::cancellation_token::SyncCancellationToken;
use crate::sync::provider::SyncProvider;
use crate::sync::types::{SyncPolicy, SyncResult, SyncTarget};

use super::{FullSyncPlan, PlannedTarget};

// ── Lifecycle CAS helper ──

pub(super) fn resolve_current_target_lifecycle(
    provider: &dyn SyncProvider,
    target_id: &str,
) -> crate::error::Result<Option<crate::sync::types::TargetLifecycleRecord>> {
    let snapshot = crate::sync::target_lifecycle::load_remote_catalog(provider)?;
    Ok(crate::sync::target_lifecycle::find_record(&snapshot.catalog, target_id).cloned())
}

// ── Error conversion ──

pub(super) fn sync_result_from_error(err: crate::Error) -> SyncResult {
    use crate::sync::SyncStatus;
    let msg = err.to_string();
    let category = err.sync_category();
    let status = if err.recoverable() {
        SyncStatus::RecoverableError(msg.clone())
    } else {
        SyncStatus::FatalError(msg.clone())
    };
    SyncResult::error(
        status,
        msg,
        (!category.is_empty()).then(|| category.to_string()),
    )
}

pub(super) fn sync_result_from_provider_error(
    err: crate::sync::provider::ProviderError,
) -> SyncResult {
    sync_result_from_error(crate::Error::from(err))
}

// ── Remote operations ──

pub(super) fn download_remote_to_staging(
    provider: &dyn SyncProvider,
    remote_prefix: &str,
    staging_root: Option<&Path>,
) -> SyncResult {
    let entries = match provider.list(remote_prefix) {
        Ok(e) => e,
        Err(e) => return sync_result_from_provider_error(e),
    };
    let mut downloaded: Vec<String> = Vec::new();
    for entry in &entries {
        let full_remote_path = format!("{}/{}", remote_prefix, entry.path);
        let obj = match provider.read(&full_remote_path) {
            Ok(Some(obj)) => obj,
            Ok(None) => continue,
            Err(e) => return sync_result_from_provider_error(e),
        };
        if let Some(staging) = staging_root {
            if let Err(e) =
                super::generation::write_staging_file(staging, &entry.path, &obj.content)
            {
                return sync_result_from_error(crate::Error::Io(e));
            }
        }
        downloaded.push(full_remote_path);
    }
    let mut result = SyncResult::success();
    result.downloaded_files = downloaded;
    result
}

pub(super) fn delete_all_remote_objects(
    provider: &dyn SyncProvider,
    remote_prefix: &str,
    cancellation_token: Option<&SyncCancellationToken>,
) -> SyncResult {
    let remote_entries = match provider.list(remote_prefix) {
        Ok(entries) => entries,
        Err(err) => return sync_result_from_provider_error(err),
    };
    // Issue #729 评论 5765306162 问题5：provider.list 返回后检查取消令牌。
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            log::info!(
                "[sync] delete_all_remote_objects: cancellation requested after list {} — returning success",
                remote_prefix
            );
            return SyncResult::success();
        }
    }
    let mut remote_deletes: Vec<String> = Vec::new();
    for entry in &remote_entries {
        if super::generation::is_generation_path(&entry.path) {
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
                remote_deletes.push(full_remote_path.clone());
            }
            Err(err) => {
                let mut result = sync_result_from_provider_error(err);
                result.remote_deletes = remote_deletes;
                return result;
            }
        }
        // Issue #729 评论 5765306162 问题5：每次 provider.delete 返回后检查取消令牌。
        // 取消则 break 并返回已完成的结果（已删除的保留，未完成的不继续）。
        if let Some(token) = cancellation_token {
            if token.is_cancelled() {
                log::info!(
                    "[sync] delete_all_remote_objects: cancellation requested after delete {} — breaking with {} deletes done",
                    full_remote_path,
                    remote_deletes.len()
                );
                let mut result = SyncResult::success();
                result.remote_deletes = remote_deletes;
                return result;
            }
        }
    }
    let mut result = SyncResult::success();
    result.remote_deletes = remote_deletes;
    result
}

pub(super) fn run_single_target(
    provider: &dyn SyncProvider,
    local_root: &Path,
    sync_policy: &SyncPolicy,
    target: &SyncTarget,
    force_sync: bool,
    cancellation_token: Option<&SyncCancellationToken>,
) -> SyncResult {
    // Issue #729：关键写入操作前检查取消令牌。
    // 如果已取消，提前返回，不继续写入远端。
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            log::info!("[sync] run_single_target: cancellation requested — skipping target");
            return SyncResult::success();
        }
    }
    // Issue #729 评论 5765306162 问题4：把 cancellation_token 传进 perform_lww_sync，
    // 不再只在入口检查一次。perform_lww_sync 内部会在 debounce 后、重试循环每次
    // 迭代前、execute_lww_sync_attempt 内部 merge/upload/delete 后各检查一次。
    match crate::sync::SyncService::perform_lww_sync(
        local_root,
        provider,
        sync_policy,
        target,
        force_sync,
        cancellation_token,
    ) {
        Ok(result) => result,
        Err(err) => sync_result_from_error(err),
    }
}

// ── Per-kind transfer functions ──

/// 合并累计的 local merge effects 到最终结果。
///
/// Issue #716 评论 5742849844 问题 2：CAS 重试时，前几轮的本地变化
/// （downloaded_files/local_trashed/overwritten/ignored）不能被后续
/// NoOp 轮次丢弃。任何 attempt 发生过真实本地变化，最终成功类结果
/// 不能再降回 `NoChanges`。
///
/// 只合并本地效果（downloaded/remote_deletes=本地trashed/overwritten/ignored），
/// 不碰 `local_deletes`（远端侧动作），避免覆盖 publish 路径已正确设置的远端删除。
fn merge_accumulated_local_effects(
    result: &mut SyncResult,
    downloaded: &[String],
    trashed: &[String],
    overwritten: &[String],
    ignored: &[String],
) {
    use crate::sync::SyncStatus;
    for f in downloaded {
        if !result.downloaded_files.contains(f) {
            result.downloaded_files.push(f.clone());
        }
    }
    for f in trashed {
        if !result.remote_deletes.contains(f) {
            result.remote_deletes.push(f.clone());
        }
    }
    for f in overwritten {
        if !result.overwritten_files.contains(f) {
            result.overwritten_files.push(f.clone());
        }
    }
    for f in ignored {
        if !result.ignored_files.contains(f) {
            result.ignored_files.push(f.clone());
        }
    }
    // 如果累计有本地变化，状态不能降回 NoChanges。
    let has_accumulated_changes = !downloaded.is_empty() || !trashed.is_empty();
    if has_accumulated_changes && matches!(result.status, SyncStatus::NoChanges) {
        result.status = SyncStatus::LatestWinsApplied;
    }
}

#[allow(
    clippy::excessive_nesting,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]
pub(super) fn transfer_live_project(
    provider: &dyn SyncProvider,
    planned: &PlannedTarget,
    catalog_snapshot: &mut crate::sync::types::RemoteTargetCatalogSnapshot,
    plan: &FullSyncPlan,
    cancellation_token: Option<&SyncCancellationToken>,
) -> (
    SyncResult,
    Option<crate::sync::types::DeletedTargetResolution>,
    Option<crate::sync::types::LocalLifecycleCommitAction>,
) {
    use crate::sync::types::TargetLifecycleApplyResult;
    use crate::sync::SyncStatus;

    /// LiveProject lifecycle CAS 重试上限。
    ///
    /// 远端 generation 持续变化时，最多重试这么多次 merge→publish→CAS。
    /// 超过后返回 `RecoverableError`（错误文字不含 "retrying"，因为已经不重试了，
    /// 是最终失败）。旧未引用 generation 留给现有 generation GC 清理。
    const MAX_CAS_RETRIES: usize = 5;

    // Issue #729：关键写入操作前检查取消令牌。
    // 如果已取消，提前返回，不继续写入远端。
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            log::info!(
                "[sync] transfer_live_project: cancellation requested — skipping target {}",
                planned.target.remote_prefix
            );
            return (SyncResult::success(), None, None);
        }
    }

    if planned.live_lww.is_some() {
        let sync_root = planned
            .staging_root
            .as_deref()
            .unwrap_or(&planned.local_root);

        // 保留冲突状态：多次 merge 可能产生冲突，最终 CAS 成功后仍要返回 PartialConflict，
        // 不让 CAS 竞争错误覆盖成泛化 RecoverableError。
        // Issue #716 评论 5741695768：retained_conflict 在每次循环迭代开始时由
        // merge_outcome 归一化代码无条件赋值（Err 直接 return，Ok 分支都赋值），
        // 因此无需初始化。
        let mut retained_conflict: Option<SyncResult>;

        // 累计 local merge effects（跨 CAS 重试轮次）。
        // Issue #716 评论 5742849844 问题 2：merge_result 是循环内临时变量，
        // CAS 重试 continue 时前几轮的本地变化会丢失。这里在循环外累计
        // 每轮 merge 对本地产生的实际变化，在各终点合并到最终结果。
        // - accumulated_downloaded_files: 已下载到本地的远端文件
        // - accumulated_local_trashed_files: 本地已移到 trash 的文件（outcome.local_deletes）
        // - accumulated_overwritten_files: 本地被覆盖的文件
        // - accumulated_ignored_files: 本地被跳过的文件
        let mut accumulated_downloaded_files: Vec<String> = Vec::new();
        let mut accumulated_local_trashed_files: Vec<String> = Vec::new();
        let mut accumulated_overwritten_files: Vec<String> = Vec::new();
        let mut accumulated_ignored_files: Vec<String> = Vec::new();

        for attempt in 0..MAX_CAS_RETRIES {
            let generation_id = uuid::Uuid::new_v4().to_string();

            // 1. 用当前 catalog_snapshot 找 visible generation → merge 到 staging。
            //    每次重试都重新 load_sync_state，merge 对同一个 staging 可重复执行。
            //
            //    Issue #686 评论 5666452462：闭包同时返回当前 staging 的完整未解决冲突
            //    状态。不用 outcome.conflicts 判断——第二次 merge 会 skip 已在
            //    state.conflicted_files 中的路径，outcome.conflicts 可能为空，
            //    但 staging 的 SyncState 仍保留着未解决冲突。
            //
            //    Issue #716 评论 5740946551：merge 仍先做，但 publish 前移到 LWW 判定之后。
            //    candidate 不赢时直接收敛，不 publish，避免明知 winner 没变仍重复 upload。
            let merge_outcome: crate::error::Result<
                Option<(
                    crate::sync::lww::LwwMergeOutcome,
                    Vec<crate::sync::types::SyncConflict>,
                )>,
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
                            "[sync] run_transfer: LiveProject {} (attempt {}) — merging from visible source {}",
                            planned.target.remote_prefix,
                            attempt + 1,
                            source_prefix
                        );
                        let mut merge_state = crate::sync::SyncService::load_sync_state(sync_root)?;
                        let outcome = crate::sync::lww::merge_remote_into_local_snapshot(
                            sync_root,
                            provider,
                            &source_prefix,
                            planned.target.scope,
                            &mut merge_state,
                        )?;
                        // 从当前 staging 的完整未解决冲突状态生成快照。
                        let unresolved_conflicts: Vec<crate::sync::types::SyncConflict> =
                            merge_state
                                .conflicts
                                .iter()
                                .filter(|c| merge_state.conflicted_files.contains(&c.local_path))
                                .cloned()
                                .collect();
                        return Ok(Some((outcome, unresolved_conflicts)));
                    }
                }
                Ok(None)
            })();

            // 归一化 merge_outcome：在 lifecycle winner 比较之前统一处理 merge 结果。
            // Issue #716 评论 5741695768：merge 结果先收口，再做 publish 前 winner 判定。
            // - Err(e) 立即返回错误，任何 winner 分支都不能吞掉 merge 错误。
            // - Ok(Some((outcome, unresolved_conflicts))) 先更新 retained_conflict，
            //   保存 outcome 供 CandidateWins 后续 publish 使用；同时构造 merge_result，
            //   携带 downloaded_files/local_deletes/remote_deletes/overwritten_files/ignored_files。
            // - Ok(None) 表示无需 merge（远端无 visible source），retained_conflict 清掉。
            let merge_outcome_opt: Option<crate::sync::lww::LwwMergeOutcome> = match merge_outcome {
                Err(e) => return (sync_result_from_error(e), None, None),
                Ok(None) => {
                    retained_conflict = None;
                    None
                }
                Ok(Some((outcome, unresolved_conflicts))) => {
                    // 用当前 staging 的完整未解决冲突状态重建 retained_conflict。
                    // CAS 重试后冲突仍未解决 → 继续返回 PartialConflict；
                    // 这一轮确实没有未解决冲突 → 才允许回到 Success。
                    if !unresolved_conflicts.is_empty() {
                        let mut r = SyncResult::success();
                        r.status = SyncStatus::PartialConflict;
                        r.conflicts = unresolved_conflicts;
                        r.downloaded_files = outcome.downloaded_files.clone();
                        // Issue #716 评论 5743264448 问题 2：retained_conflict 不报告
                        // 未执行的远端删除。remote_delete_paths 是"调用方应从远端删除的
                        // 路径"，merge 本身没执行远端删除。只有 CandidateWins 的 generation
                        // 真正 publish 且 CAS 成功后，才把 content_result.local_deletes
                        // 合并进最终 PartialConflict。
                        r.local_deletes = Vec::new();
                        r.remote_deletes = outcome.local_deletes.clone();
                        r.overwritten_files = outcome.overwritten_files.clone();
                        r.ignored_files = outcome.ignored_files.clone();
                        retained_conflict = Some(r);
                    } else {
                        retained_conflict = None;
                    }
                    Some(outcome)
                }
            };

            // 构造 merge_result：携带本轮 merge 对本地产生的实际变化，
            // 供 RemoteWins(Upsert)/AlreadyCurrent 不 publish 时返回。
            // 规则与 sync/lww/attempt.rs 对齐：
            // - 有 unresolved conflict → PartialConflict（已在 retained_conflict 中）
            // - 有 pending_take_remote_failed → RecoverableError（Issue #716 评论 5742849844 问题 1）
            // - 无冲突但 downloaded_files/local_deletes 任一非空（对本地产生的实际变化）→ LatestWinsApplied
            // - 真正全空 → NoChanges
            // 注意：remote_upload_paths/remote_delete_paths 是对远端的操作，不是对本地产生的变化，
            // 在 RemoteWins 分支中不应影响状态判断。
            //
            // Issue #716 评论 5742849844 问题 3：no-publish 路径不报告未执行的远端删除。
            // local_deletes 语义是"本地已删除的文件"，merge 阶段没有执行远端删除
            // （remote_delete_paths 是"调用方应从远端删除的路径"，merge 本身没执行）。
            // no-publish 路径既没有 delete_remote_files()，也没有发布新 generation，
            // 这些远端删除实际上没有发生，不应报告为 local_deletes。
            // outcome.local_deletes 是"远端发起的删除已在本地 move_to_trash"，
            // 那是 remote_deletes 语义，在下面 r.remote_deletes 赋值。
            let merge_result: Option<SyncResult> = match &merge_outcome_opt {
                Some(outcome) => {
                    // 只考虑对本地产生的实际变化：下载到本地的文件、本地被删除的文件
                    let has_local_changes =
                        !outcome.downloaded_files.is_empty() || !outcome.local_deletes.is_empty();
                    let mut r = SyncResult::success();
                    r.downloaded_files = outcome.downloaded_files.clone();
                    // 问题 3 修复：no-publish 路径不报告未执行的远端删除。
                    r.local_deletes = Vec::new();
                    r.remote_deletes = outcome.local_deletes.clone();
                    r.overwritten_files = outcome.overwritten_files.clone();
                    r.ignored_files = outcome.ignored_files.clone();
                    // 问题 1 修复：pending_take_remote_failed 非空 → RecoverableError。
                    // 与 sync/lww/attempt.rs 第 70-78 行对齐：用户要求以远端为准，
                    // 但对应远端文件缺失，必须留到下一轮重试，不能吞成成功。
                    if !outcome.pending_take_remote_failed.is_empty() {
                        r.status = SyncStatus::RecoverableError(format!(
                            "pending_take_remote_failed: {}",
                            outcome.pending_take_remote_failed.join(", ")
                        ));
                        r.error = Some(format!(
                            "pending_take_remote: remote file missing for paths: {}",
                            outcome.pending_take_remote_failed.join(", ")
                        ));
                    } else if has_local_changes {
                        r.status = SyncStatus::LatestWinsApplied;
                    } else {
                        r.status = SyncStatus::NoChanges;
                    }
                    Some(r)
                }
                None => None,
            };

            // 累计本轮 merge 的 local effects（去重）。
            // Issue #716 评论 5742849844 问题 2：CAS 重试时前几轮的本地变化不能丢失。
            if let Some(outcome) = &merge_outcome_opt {
                for f in &outcome.downloaded_files {
                    if !accumulated_downloaded_files.contains(f) {
                        accumulated_downloaded_files.push(f.clone());
                    }
                }
                for f in &outcome.local_deletes {
                    if !accumulated_local_trashed_files.contains(f) {
                        accumulated_local_trashed_files.push(f.clone());
                    }
                }
                for f in &outcome.overwritten_files {
                    if !accumulated_overwritten_files.contains(f) {
                        accumulated_overwritten_files.push(f.clone());
                    }
                }
                for f in &outcome.ignored_files {
                    if !accumulated_ignored_files.contains(f) {
                        accumulated_ignored_files.push(f.clone());
                    }
                }
            }

            // 2. read_post_transfer_lww → 构造 candidate（winner 身份只来自 merge 后真实 manifest，
            //    不伪造 lww_time+1 / device_id / 新时间戳）。
            let post_transfer_root = planned
                .staging_root
                .as_deref()
                .unwrap_or(&planned.local_root);
            let post_transfer_lww = match super::plan::read_post_transfer_lww(post_transfer_root) {
                Some(lww) => lww,
                None => {
                    let msg = "post-transfer staging manifest unreadable".to_string();
                    return (
                        SyncResult::error(SyncStatus::RecoverableError(msg.clone()), msg, None),
                        None,
                        None,
                    );
                }
            };
            let candidate = crate::sync::types::TargetLifecycleRecord::upsert(
                &planned.target.remote_prefix,
                &planned.target.remote_prefix,
                post_transfer_lww.lww_time_ms,
                &post_transfer_lww.device_id,
            )
            .with_active_generation(&generation_id);

            // 3. publish 前用复用函数做 LWW 判定。candidate 不赢直接收敛，不 publish。
            let remote_record = crate::sync::target_lifecycle::find_record(
                &catalog_snapshot.catalog,
                &planned.target.remote_prefix,
            );
            match crate::sync::target_lifecycle::compare_lifecycle_candidate(
                &candidate,
                remote_record,
            ) {
                crate::sync::target_lifecycle::LifecycleCandidateComparison::CandidateWins => {
                    // Issue #729：publish 前检查取消令牌。
                    // 如果已取消，不执行远端写入，提前返回已收集的 merge 结果。
                    if let Some(token) = cancellation_token {
                        if token.is_cancelled() {
                            log::info!(
                                "[sync] transfer_live_project: cancellation requested before publish — skipping {}",
                                planned.target.remote_prefix
                            );
                            let mut r = merge_result.unwrap_or_else(SyncResult::success);
                            merge_accumulated_local_effects(
                                &mut r,
                                &accumulated_downloaded_files,
                                &accumulated_local_trashed_files,
                                &accumulated_overwritten_files,
                                &accumulated_ignored_files,
                            );
                            return (r, None, None);
                        }
                    }
                    // Issue #716 评论 5743264448 问题 1：CandidateWins 分支在 publish 前必须
                    // 检查 pending_take_remote_failed。如果没有 unresolved conflict 但
                    // pending_take_remote_failed 非空，直接返回 RecoverableError，不 publish。
                    // 原因：pending 路径远端缺失时 staging 里还留着本地旧文件，merged_manifest
                    // 可能保留本地记录，generation publisher 会把完整 staging 快照上传，
                    // 可能在"用户要求取远端但远端缺失"的情况下把本地版本重新发布成新 generation。
                    // 优先级：unresolved conflict > pending_take_remote_failed > 正常 publish。
                    if retained_conflict.is_none() {
                        if let Some(outcome) = &merge_outcome_opt {
                            if !outcome.pending_take_remote_failed.is_empty() {
                                let mut r = SyncResult::success();
                                r.status = SyncStatus::RecoverableError(format!(
                                    "pending_take_remote_failed: {}",
                                    outcome.pending_take_remote_failed.join(", ")
                                ));
                                r.error = Some(format!(
                                    "pending_take_remote: remote file missing for paths: {}",
                                    outcome.pending_take_remote_failed.join(", ")
                                ));
                                r.downloaded_files = outcome.downloaded_files.clone();
                                r.local_deletes = Vec::new();
                                r.remote_deletes = outcome.local_deletes.clone();
                                r.overwritten_files = outcome.overwritten_files.clone();
                                r.ignored_files = outcome.ignored_files.clone();
                                merge_accumulated_local_effects(
                                    &mut r,
                                    &accumulated_downloaded_files,
                                    &accumulated_local_trashed_files,
                                    &accumulated_overwritten_files,
                                    &accumulated_ignored_files,
                                );
                                return (r, None, None);
                            }
                        }
                    }

                    // 只有 candidate 严格赢（或远端无 record）才真正 publish。
                    let gen_remote_prefix = match super::generation::generation_remote_prefix(
                        &planned.target.remote_prefix,
                        &generation_id,
                    ) {
                        Ok(p) => p,
                        Err(e) => return (sync_result_from_error(e), None, None),
                    };
                    log::info!(
                        "[sync] run_transfer: LiveProject {} (attempt {}) — uploading to generation prefix {}",
                        planned.target.remote_prefix,
                        attempt + 1,
                        gen_remote_prefix
                    );

                    // publish generation。
                    //    冲突时仍 publish + CAS（非冲突文件需同步），但记录 PartialConflict 状态，
                    //    CAS 成功后返回 PartialConflict 而非 Success，确保冲突信息不被丢失。
                    //    Issue #716 评论 5741695768：merge_outcome 已在前面归一化，
                    //    这里只负责根据归一化后的 outcome 决定 publish 参数。
                    let content_result = match &merge_outcome_opt {
                        Some(outcome) => super::generation::publish_generation(
                            provider,
                            sync_root,
                            &gen_remote_prefix,
                            &generation_id,
                            planned.project_id.as_deref().unwrap_or(""),
                            planned.target.scope,
                            &plan.sync_policy,
                            plan.force_sync,
                            Some(outcome),
                            cancellation_token,
                        ),
                        None => super::generation::publish_generation(
                            provider,
                            sync_root,
                            &gen_remote_prefix,
                            &generation_id,
                            planned.project_id.as_deref().unwrap_or(""),
                            planned.target.scope,
                            &plan.sync_policy,
                            plan.force_sync,
                            None,
                            cancellation_token,
                        ),
                    };

                    let content_ok = matches!(
                        content_result.status,
                        SyncStatus::Success | SyncStatus::NoChanges | SyncStatus::LatestWinsApplied
                    );
                    if !content_ok {
                        return (content_result, None, None);
                    }

                    // Issue #729：publish_generation 返回后、apply_lifecycle_record 前检查取消令牌。
                    // 取消则不执行 CAS 写入，返回已收集的 content_result（已上传但未 CAS）。
                    if let Some(token) = cancellation_token {
                        if token.is_cancelled() {
                            log::info!(
                                "[sync] transfer_live_project: cancellation requested after publish_generation, before apply_lifecycle_record — skipping {}",
                                planned.target.remote_prefix
                            );
                            let mut r = content_result;
                            merge_accumulated_local_effects(
                                &mut r,
                                &accumulated_downloaded_files,
                                &accumulated_local_trashed_files,
                                &accumulated_overwritten_files,
                                &accumulated_ignored_files,
                            );
                            return (r, None, None);
                        }
                    }

                    // 4. CAS apply_lifecycle_record。
                    match crate::sync::target_lifecycle::apply_lifecycle_record(
                        provider,
                        catalog_snapshot,
                        candidate,
                    ) {
                        TargetLifecycleApplyResult::Applied(persisted) => {
                            *catalog_snapshot = persisted;
                            // Issue #716 评论 5742849844 问题 2：合并累计 local effects，
                            // 保留前几轮 CAS 重试的本地变化。content_result 已包含本轮
                            // local effects 和 publish 后的远端侧动作（local_deletes）。
                            //
                            // Issue #716 评论 5743264448 问题 2：retained_conflict 的
                            // local_deletes 已清空（不报告未执行的远端删除）。CAS 成功后
                            // content_result.local_deletes 是真正已生效的远端删除，需要合并
                            // 到 retained_conflict。同理 uploaded_files 也是远端侧已生效动作。
                            let mut final_result = if let Some(mut rc) = retained_conflict {
                                for f in &content_result.uploaded_files {
                                    if !rc.uploaded_files.contains(f) {
                                        rc.uploaded_files.push(f.clone());
                                    }
                                }
                                for f in &content_result.local_deletes {
                                    if !rc.local_deletes.contains(f) {
                                        rc.local_deletes.push(f.clone());
                                    }
                                }
                                rc
                            } else {
                                content_result
                            };
                            merge_accumulated_local_effects(
                                &mut final_result,
                                &accumulated_downloaded_files,
                                &accumulated_local_trashed_files,
                                &accumulated_overwritten_files,
                                &accumulated_ignored_files,
                            );
                            return (final_result, None, None);
                        }
                        TargetLifecycleApplyResult::AlreadyCurrent(persisted) => {
                            *catalog_snapshot = persisted;
                            // 同 Applied：CAS 成功后合并 content_result 远端侧已生效动作。
                            let mut final_result = if let Some(mut rc) = retained_conflict {
                                for f in &content_result.uploaded_files {
                                    if !rc.uploaded_files.contains(f) {
                                        rc.uploaded_files.push(f.clone());
                                    }
                                }
                                for f in &content_result.local_deletes {
                                    if !rc.local_deletes.contains(f) {
                                        rc.local_deletes.push(f.clone());
                                    }
                                }
                                rc
                            } else {
                                content_result
                            };
                            merge_accumulated_local_effects(
                                &mut final_result,
                                &accumulated_downloaded_files,
                                &accumulated_local_trashed_files,
                                &accumulated_overwritten_files,
                                &accumulated_ignored_files,
                            );
                            return (final_result, None, None);
                        }
                        TargetLifecycleApplyResult::RemoteWinner {
                            snapshot: persisted,
                            record: winner,
                        } => {
                            *catalog_snapshot = persisted;
                            match winner.op {
                                crate::sync::types::TargetOp::Upsert => {
                                    log::info!(
                                        "[sync] run_transfer: LiveProject RemoteWinner(Upsert) {} (attempt {}) — re-merging with latest snapshot",
                                        planned.target.remote_prefix,
                                        attempt + 1
                                    );
                                    // CAS 期间远端 snapshot 真正变化才下一轮。
                                    // 旧未引用 generation 留给 generation GC 清理。
                                    continue;
                                }
                                crate::sync::types::TargetOp::Delete => {
                                    log::info!(
                                        "[sync] run_transfer: LiveProject RemoteWinner(Delete) {} — cleaning remote + deferring to Commit",
                                        planned.target.remote_prefix
                                    );
                                    let cleanup_result = delete_all_remote_objects(
                                        provider,
                                        &planned.target.remote_prefix,
                                        cancellation_token,
                                    );
                                    let cleanup_ok = matches!(
                                        cleanup_result.status,
                                        SyncStatus::Success | SyncStatus::NoChanges
                                    );
                                    if !cleanup_ok {
                                        let expected_time =
                                            crate::sync::target_lifecycle::record_lww_time(&winner);
                                        let expected_device = &winner.device_id;
                                        let record_result = crate::sync::pending_remote_cleanup::record_pending_remote_cleanup(
                                            &plan.app_data_root, &planned.target.remote_prefix,
                                            planned.project_id.as_deref().unwrap_or(""),
                                            &format!("LiveProject RemoteWinner(Delete) cleanup failed: {:?}", cleanup_result.status),
                                            expected_time, expected_device,
                                        );
                                        if let Err(e) = record_result {
                                            return (sync_result_from_error(e), None, None);
                                        } else {
                                            return (cleanup_result, None, None);
                                        }
                                    } else {
                                        let action = planned.project_id.as_ref().and_then(|pid| {
                                            planned.live_lww.as_ref().map(|lww| {
                                                crate::sync::types::LocalLifecycleCommitAction::DeleteProject {
                                                    project_id: pid.clone(),
                                                    expected_local_lww: crate::sync::types::LiveTargetLwwSerde::from_lww(lww),
                                                }
                                            })
                                        });
                                        let mut final_result =
                                            retained_conflict.unwrap_or(content_result);
                                        merge_accumulated_local_effects(
                                            &mut final_result,
                                            &accumulated_downloaded_files,
                                            &accumulated_local_trashed_files,
                                            &accumulated_overwritten_files,
                                            &accumulated_ignored_files,
                                        );
                                        return (final_result, None, action);
                                    }
                                }
                            }
                        }
                        TargetLifecycleApplyResult::Retry(e) => {
                            return (sync_result_from_error(e), None, None);
                        }
                    }
                }
                crate::sync::target_lifecycle::LifecycleCandidateComparison::AlreadyCurrent => {
                    // candidate 与远端完全相等，不需要 publish。
                    log::info!(
                        "[sync] run_transfer: LiveProject AlreadyCurrent {} (attempt {}) — candidate identical to remote record, skipping publish",
                        planned.target.remote_prefix,
                        attempt + 1
                    );
                    let mut result = retained_conflict.unwrap_or_else(|| {
                        merge_result.clone().unwrap_or(SyncResult::no_changes())
                    });
                    // Issue #716 评论 5742849844 问题 2：合并累计 local effects。
                    merge_accumulated_local_effects(
                        &mut result,
                        &accumulated_downloaded_files,
                        &accumulated_local_trashed_files,
                        &accumulated_overwritten_files,
                        &accumulated_ignored_files,
                    );
                    return (result, None, None);
                }
                crate::sync::target_lifecycle::LifecycleCandidateComparison::RemoteWins(winner) => {
                    // remote 严格赢且 candidate 不赢，这一轮无法靠 publish 改变 winner。
                    match winner.op {
                        crate::sync::types::TargetOp::Upsert => {
                            log::info!(
                                "[sync] run_transfer: LiveProject RemoteWins(Upsert) {} (attempt {}) — remote strictly wins, converging without publish",
                                planned.target.remote_prefix,
                                attempt + 1
                            );
                            // 直接按最新 remote 收敛，不再 publish。
                            let mut result = retained_conflict.unwrap_or_else(|| {
                                merge_result.clone().unwrap_or(SyncResult::no_changes())
                            });
                            // Issue #716 评论 5742849844 问题 2：合并累计 local effects。
                            merge_accumulated_local_effects(
                                &mut result,
                                &accumulated_downloaded_files,
                                &accumulated_local_trashed_files,
                                &accumulated_overwritten_files,
                                &accumulated_ignored_files,
                            );
                            return (result, None, None);
                        }
                        crate::sync::types::TargetOp::Delete => {
                            log::info!(
                                "[sync] run_transfer: LiveProject RemoteWins(Delete) {} (attempt {}) — cleaning remote + deferring to Commit",
                                planned.target.remote_prefix,
                                attempt + 1
                            );
                            let cleanup_result = delete_all_remote_objects(
                                provider,
                                &planned.target.remote_prefix,
                                cancellation_token,
                            );
                            let cleanup_ok = matches!(
                                cleanup_result.status,
                                SyncStatus::Success | SyncStatus::NoChanges
                            );
                            if !cleanup_ok {
                                let expected_time =
                                    crate::sync::target_lifecycle::record_lww_time(&winner);
                                let expected_device = &winner.device_id;
                                let record_result = crate::sync::pending_remote_cleanup::record_pending_remote_cleanup(
                                    &plan.app_data_root, &planned.target.remote_prefix,
                                    planned.project_id.as_deref().unwrap_or(""),
                                    &format!("LiveProject RemoteWins(Delete) cleanup failed: {:?}", cleanup_result.status),
                                    expected_time, expected_device,
                                );
                                if let Err(e) = record_result {
                                    return (sync_result_from_error(e), None, None);
                                } else {
                                    return (cleanup_result, None, None);
                                }
                            } else {
                                let action = planned.project_id.as_ref().and_then(|pid| {
                                    planned.live_lww.as_ref().map(|lww| {
                                        crate::sync::types::LocalLifecycleCommitAction::DeleteProject {
                                            project_id: pid.clone(),
                                            expected_local_lww: crate::sync::types::LiveTargetLwwSerde::from_lww(lww),
                                        }
                                    })
                                });
                                let mut final_result =
                                    retained_conflict.unwrap_or(SyncResult::no_changes());
                                merge_accumulated_local_effects(
                                    &mut final_result,
                                    &accumulated_downloaded_files,
                                    &accumulated_local_trashed_files,
                                    &accumulated_overwritten_files,
                                    &accumulated_ignored_files,
                                );
                                return (final_result, None, action);
                            }
                        }
                    }
                }
            }
        }

        // CAS 持续竞争达到上限 → 远端持续变化，无法完成同步。
        // 错误文字不含 "retrying"（已经不重试了，是最终失败）。
        let msg = format!(
            "LiveProject: remote generation kept changing during merge, exceeded retry limit ({} attempts) for {}",
            MAX_CAS_RETRIES, planned.target.remote_prefix
        );
        (
            SyncResult::error(SyncStatus::RecoverableError(msg.clone()), msg, None),
            None,
            None,
        )
    } else {
        let msg = "live project missing lww (manifest unreadable)".to_string();
        (
            SyncResult::error(SyncStatus::RecoverableError(msg.clone()), msg, None),
            None,
            None,
        )
    }
}

#[allow(
    clippy::excessive_nesting,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]
pub(super) fn transfer_restore_project(
    provider: &dyn SyncProvider,
    planned: &PlannedTarget,
    plan: &FullSyncPlan,
    cancellation_token: Option<&SyncCancellationToken>,
) -> (
    SyncResult,
    Option<crate::sync::types::DeletedTargetResolution>,
    Option<crate::sync::types::LocalLifecycleCommitAction>,
) {
    use crate::sync::SyncStatus;

    // Issue #729：关键写/delete 操作前检查取消令牌。取消则跳过整个 helper。
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            log::info!(
                "[sync] transfer_restore_project: cancellation requested — skipping target {}",
                planned.target.remote_prefix
            );
            return (SyncResult::success(), None, None);
        }
    }

    match resolve_current_target_lifecycle(provider, &planned.target.remote_prefix) {
        Ok(Some(current_rec)) => {
            use crate::sync::types::TargetOp;
            match current_rec.op {
                TargetOp::Delete => {
                    let local_project_exists = planned.local_root.exists() && {
                        std::fs::read_dir(&planned.local_root)
                            .map(|mut it| it.next().is_some())
                            .unwrap_or(false)
                    };
                    if !local_project_exists {
                        log::info!("[sync] run_transfer: RestoreProject {} — CAS: remote changed to Delete, local project absent — cleaning remote residue", planned.target.remote_prefix);
                        let cleanup_result = delete_all_remote_objects(
                            provider,
                            &planned.target.remote_prefix,
                            cancellation_token,
                        );
                        let cleanup_ok = matches!(
                            cleanup_result.status,
                            SyncStatus::Success | SyncStatus::NoChanges
                        );
                        if !cleanup_ok {
                            let expected_time =
                                crate::sync::target_lifecycle::record_lww_time(&current_rec);
                            let expected_device = &current_rec.device_id;
                            let record_result =
                                crate::sync::pending_remote_cleanup::record_pending_remote_cleanup(
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
                                (sync_result_from_error(e), None, None)
                            } else {
                                (cleanup_result, None, None)
                            }
                        } else {
                            (cleanup_result, None, None)
                        }
                    } else {
                        log::info!("[sync] run_transfer: RestoreProject {} — CAS: remote changed to Delete, local project exists — re-evaluating", planned.target.remote_prefix);
                        let current_candidate =
                            super::plan::compute_local_project_lifecycle_candidate(
                                &planned.local_root,
                                planned
                                    .live_lww
                                    .as_ref()
                                    .map(|l| l.device_id.as_str())
                                    .unwrap_or(""),
                            );
                        match current_candidate {
                            super::LifecycleCandidate::Live { lww: current_lww } => {
                                let remote_time =
                                    crate::sync::target_lifecycle::record_lww_time(&current_rec);
                                let local_wins = current_lww.lww_time_ms > remote_time
                                    || (current_lww.lww_time_ms == remote_time
                                        && current_lww.device_id > current_rec.device_id);
                                if !local_wins {
                                    let action = planned.project_id.as_ref().map(|pid| {
                                        crate::sync::types::LocalLifecycleCommitAction::DeleteProject {
                                            project_id: pid.clone(),
                                            expected_local_lww: crate::sync::types::LiveTargetLwwSerde::from_lww(&current_lww),
                                        }
                                    });
                                    (SyncResult::no_changes(), None, action)
                                } else {
                                    log::info!("[sync] run_transfer: RestoreProject {} — CAS: remote Delete but local LWW wins", planned.target.remote_prefix);
                                    (SyncResult::no_changes(), None, None)
                                }
                            }
                            super::LifecycleCandidate::Retry => {
                                log::info!("[sync] run_transfer: RestoreProject {} — CAS: remote Delete but local snapshot failed — Retry", planned.target.remote_prefix);
                                (
                                    SyncResult::error(
                                        SyncStatus::RecoverableError(
                                            "RestoreProject: local snapshot failed".to_string(),
                                        ),
                                        "RestoreProject: local snapshot failed".to_string(),
                                        None,
                                    ),
                                    Some(crate::sync::types::DeletedTargetResolution::Retry),
                                    None,
                                )
                            }
                        }
                    }
                }
                TargetOp::Upsert => {
                    let download_prefix: crate::error::Result<String> = match &current_rec
                        .active_generation
                    {
                        Some(gen_id) => {
                            let gen_prefix = super::generation::generation_remote_prefix(
                                &planned.target.remote_prefix,
                                gen_id,
                            );
                            if let Ok(ref p) = gen_prefix {
                                log::info!("[sync] run_transfer: RestoreProject {} — CAS confirmed Upsert with active_generation={}, downloading from {}", planned.target.remote_prefix, gen_id, p);
                            }
                            gen_prefix
                        }
                        None => {
                            log::info!("[sync] run_transfer: RestoreProject {} — CAS confirmed Upsert (no active_generation), downloading from legacy", planned.target.remote_prefix);
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
                                Some(crate::sync::types::DeletedTargetResolution::RemoteTargetWins),
                                None,
                            )
                        }
                        Err(e) => (sync_result_from_error(e), None, None),
                    }
                }
            }
        }
        Ok(None) => {
            log::info!(
                "[sync] run_transfer: RestoreProject {} — CAS: no remote record, returning Retry",
                planned.target.remote_prefix
            );
            (
                SyncResult::error(
                    SyncStatus::RecoverableError(
                        "RestoreProject: no remote record, retrying".to_string(),
                    ),
                    "RestoreProject: no remote record to confirm upsert winner".to_string(),
                    None,
                ),
                Some(crate::sync::types::DeletedTargetResolution::Retry),
                None,
            )
        }
        Err(e) => {
            log::warn!(
                "[sync] RestoreProject CAS failed for {}: {e}",
                planned.target.remote_prefix
            );
            (
                sync_result_from_error(e),
                Some(crate::sync::types::DeletedTargetResolution::Retry),
                None,
            )
        }
    }
}

#[allow(
    clippy::excessive_nesting,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]
pub(super) fn transfer_delete_local_project(
    provider: &dyn SyncProvider,
    planned: &PlannedTarget,
    plan: &FullSyncPlan,
    cancellation_token: Option<&SyncCancellationToken>,
) -> (
    SyncResult,
    Option<crate::sync::types::DeletedTargetResolution>,
    Option<crate::sync::types::LocalLifecycleCommitAction>,
) {
    // Issue #729：关键写/delete 操作前检查取消令牌。取消则跳过整个 helper。
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            log::info!(
                "[sync] transfer_delete_local_project: cancellation requested — skipping target {}",
                planned.target.remote_prefix
            );
            return (SyncResult::success(), None, None);
        }
    }

    match resolve_current_target_lifecycle(provider, &planned.target.remote_prefix) {
        Ok(Some(current_rec)) => {
            use crate::sync::types::TargetOp;
            match current_rec.op {
                TargetOp::Upsert => {
                    log::info!("[sync] run_transfer: DeleteLocalProject {} — CAS: remote changed to Upsert, switching to ReplaceProject", planned.target.remote_prefix);
                    let result = download_remote_to_staging(
                        provider,
                        &planned.target.remote_prefix,
                        planned.staging_root.as_deref(),
                    );
                    let action = planned.project_id.as_ref().and_then(|pid| {
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
                    log::info!("[sync] run_transfer: DeleteLocalProject {} — CAS confirmed remote Delete, cleaning remote residue", planned.target.remote_prefix);
                    let cleanup_result = delete_all_remote_objects(
                        provider,
                        &planned.target.remote_prefix,
                        cancellation_token,
                    );
                    let cleanup_ok = matches!(
                        cleanup_result.status,
                        crate::sync::SyncStatus::Success | crate::sync::SyncStatus::NoChanges
                    );
                    if !cleanup_ok {
                        let expected_time =
                            crate::sync::target_lifecycle::record_lww_time(&current_rec);
                        let expected_device = &current_rec.device_id;
                        let record_result =
                            crate::sync::pending_remote_cleanup::record_pending_remote_cleanup(
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
                            (sync_result_from_error(e), None, None)
                        } else {
                            (cleanup_result, None, None)
                        }
                    } else {
                        let action = planned.project_id.as_ref().and_then(|pid| {
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
            log::info!("[sync] run_transfer: DeleteLocalProject {} — CAS: no remote record, returning Retry", planned.target.remote_prefix);
            (
                SyncResult::error(
                    crate::sync::SyncStatus::RecoverableError(
                        "DeleteLocalProject: no remote record, retrying".to_string(),
                    ),
                    "DeleteLocalProject: no remote record to confirm delete winner".to_string(),
                    None,
                ),
                Some(crate::sync::types::DeletedTargetResolution::Retry),
                None,
            )
        }
        Err(e) => {
            log::warn!(
                "[sync] DeleteLocalProject CAS failed for {}: {e}",
                planned.target.remote_prefix
            );
            (
                sync_result_from_error(e),
                Some(crate::sync::types::DeletedTargetResolution::Retry),
                None,
            )
        }
    }
}

#[allow(
    clippy::excessive_nesting,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]
pub(super) fn transfer_delete_remote_project(
    provider: &dyn SyncProvider,
    planned: &PlannedTarget,
    catalog_snapshot: &mut crate::sync::types::RemoteTargetCatalogSnapshot,
    cancellation_token: Option<&SyncCancellationToken>,
) -> (
    SyncResult,
    Option<crate::sync::types::DeletedTargetResolution>,
    Option<crate::sync::types::LocalLifecycleCommitAction>,
) {
    use crate::sync::types::TargetLifecycleApplyResult;

    // Issue #729：关键写/delete 操作前检查取消令牌。取消则跳过整个 helper。
    if let Some(token) = cancellation_token {
        if token.is_cancelled() {
            log::info!(
                "[sync] transfer_delete_remote_project: cancellation requested — skipping target {}",
                planned.target.remote_prefix
            );
            return (SyncResult::success(), None, None);
        }
    }

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
    let candidate = crate::sync::types::TargetLifecycleRecord::delete(
        &planned.target.remote_prefix,
        &planned.target.remote_prefix,
        deleted_at_ms,
        device_id,
    );
    match crate::sync::target_lifecycle::apply_lifecycle_record(
        provider,
        catalog_snapshot,
        candidate,
    ) {
        TargetLifecycleApplyResult::Applied(persisted) => {
            *catalog_snapshot = persisted;
            let del_result = delete_all_remote_objects(
                provider,
                &planned.target.remote_prefix,
                cancellation_token,
            );
            (
                del_result,
                Some(crate::sync::types::DeletedTargetResolution::LocalDeleteWins),
                None,
            )
        }
        TargetLifecycleApplyResult::AlreadyCurrent(persisted) => {
            *catalog_snapshot = persisted;
            log::info!("[sync] run_transfer: DeleteRemoteProject AlreadyCurrent(Delete) {} — continuing cleanup", planned.target.remote_prefix);
            let del_result = delete_all_remote_objects(
                provider,
                &planned.target.remote_prefix,
                cancellation_token,
            );
            (
                del_result,
                Some(crate::sync::types::DeletedTargetResolution::LocalDeleteWins),
                None,
            )
        }
        TargetLifecycleApplyResult::RemoteWinner {
            snapshot: persisted,
            record: winner,
        } => {
            *catalog_snapshot = persisted;
            match winner.op {
                crate::sync::types::TargetOp::Delete => {
                    log::info!("[sync] run_transfer: DeleteRemoteProject RemoteWinner(Delete) {} — continuing cleanup", planned.target.remote_prefix);
                    let del_result = delete_all_remote_objects(
                        provider,
                        &planned.target.remote_prefix,
                        cancellation_token,
                    );
                    (
                        del_result,
                        Some(crate::sync::types::DeletedTargetResolution::LocalDeleteWins),
                        None,
                    )
                }
                crate::sync::types::TargetOp::Upsert => {
                    log::info!("[sync] run_transfer: DeleteRemoteProject RemoteWinner(Upsert) {} — switching to restore", planned.target.remote_prefix);
                    let restore_result = download_remote_to_staging(
                        provider,
                        &planned.target.remote_prefix,
                        planned.staging_root.as_deref(),
                    );
                    (
                        restore_result,
                        Some(crate::sync::types::DeletedTargetResolution::RemoteTargetWins),
                        None,
                    )
                }
            }
        }
        TargetLifecycleApplyResult::Retry(e) => (
            sync_result_from_error(e),
            Some(crate::sync::types::DeletedTargetResolution::Retry),
            None,
        ),
    }
}
