use std::path::{Path, PathBuf};

///    把 target-relative `rel_path` 转成
/// workspace-relative path，供 `record_workspace_history` 精确 stage。
///
/// - App target：`live_root = app_data_root`，`rel_path` 已是 workspace-relative。
/// - Project target：`live_root = projects_root/<project_id>`，
///   workspace-relative = `projects/<project_id>/<rel_path>`。
/// - DeletedProject target with `RemoteTargetWins`：`target_kind` 保持为
///   `"deleted_project"`，但 `target_live_root` 已设为 `projects_root/<project_id>`，
///   所以 workspace-relative 也是 `projects/<project_id>/<rel_path>`。
///
/// `target_kind` / `project_id` 来自 `TargetSyncResult`，与 staging_runs
/// 按索引对应。未知 target_kind 时退化为直接返回 `rel_path`（保守不丢路径）。
fn to_workspace_rel_path(target_kind: &str, project_id: Option<&str>, rel_path: &Path) -> PathBuf {
    //   deleted_project 的 RemoteTargetWins 恢复路径
    // 与普通 project 共用同一分支，不再走 rel-only 路径。
    if target_kind == "project" || target_kind == "deleted_project" {
        if let Some(pid) = project_id {
            return PathBuf::from("projects").join(pid).join(rel_path);
        }
    }
    PathBuf::from(rel_path)
}

/// 收集一批 `CommitAction` 的 rel_path，转成 workspace-relative 后追加到 `out`。
fn collect_action_paths(
    target_kind: &str,
    project_id: Option<&str>,
    actions: &[crate::sync::staging::CommitAction],
    out: &mut Vec<PathBuf>,
) {
    for action in actions {
        let rel = match action {
            crate::sync::staging::CommitAction::Apply { rel_path, .. } => rel_path,
            crate::sync::staging::CommitAction::Delete { rel_path } => rel_path,
        };
        out.push(to_workspace_rel_path(target_kind, project_id, rel));
    }
}

/// 将 commit plan 中的 Apply/Delete 变更通过 SaveTransaction 写回 live root。
///
/// 删除 `git_finalize_recovery` 参数。
/// staging commit 不再承担 Git repo metadata finalize 职责；
/// workspace 本地 Git 如果要参与本地版本历史，放在 commit 完成后的 workspace
/// Git 层统一处理，不作为某个 remote provider 的 staging 模式。
pub(crate) fn apply_commit_plan_to_live(
    live_root: &Path,
    content_actions: &[crate::sync::staging::CommitAction],
    engine_state_actions: &[crate::sync::staging::CommitAction],
    backup_mode: bool,
) -> crate::error::Result<crate::storage::transaction::SaveTransaction> {
    if content_actions.is_empty() && engine_state_actions.is_empty() && !backup_mode {
        return Ok(crate::storage::transaction::SaveTransaction::new(live_root));
    }
    let mut tx = crate::storage::transaction::SaveTransaction::new(live_root);
    if backup_mode {
        tx.enable_backup_mode();
    }
    for action in engine_state_actions {
        match action {
            crate::sync::staging::CommitAction::Apply { rel_path, content } => {
                let rel_str = rel_path.to_string_lossy();
                tx.add_bytes(&rel_str, content)?;
            }
            crate::sync::staging::CommitAction::Delete { rel_path } => {
                let rel_str = rel_path.to_string_lossy();
                tx.add_delete(&rel_str);
            }
        }
    }
    for action in content_actions {
        match action {
            crate::sync::staging::CommitAction::Apply { rel_path, content } => {
                let rel_str = rel_path.to_string_lossy();
                tx.add_bytes(&rel_str, content)?;
            }
            crate::sync::staging::CommitAction::Delete { rel_path } => {
                let rel_str = rel_path.to_string_lossy();
                tx.add_delete(&rel_str);
            }
        }
    }
    tx.commit()?;
    Ok(tx)
}

/// 带 sync state 三方语义合并的 commit plan 写回 live。
///
/// 与 [`apply_commit_plan_to_live`] 的区别：`state.local.json` 和
/// `conflicts.json` 不从 `engine_state_actions` 直接 apply incoming，
/// 而是用 `merged_state` / `merged_conflicts`（由
/// [`crate::sync::staging::sync_state_merge::merge_sync_state_three_way`]
/// 产出）在同一个 `SaveTransaction` 里提交。
///
/// `engine_state_actions` 此时只剩 `manifest.sync.json`（`state.local.json` /
/// `conflicts.json` 已被 `compute_commit_plan` 排除，走 `EngineStateMerge` 分支）。
///
/// Issue #762 评论 5830266600：用户在同步期间解决的冲突不能被 staging 旧基线
/// 覆盖复活。
pub(crate) fn apply_commit_plan_with_sync_state_merge(
    live_root: &Path,
    content_actions: &[crate::sync::staging::CommitAction],
    engine_state_actions: &[crate::sync::staging::CommitAction],
    merged_state: &crate::sync::types::SyncState,
    merged_conflicts: &[crate::sync::types::SyncConflict],
    backup_mode: bool,
) -> crate::error::Result<crate::storage::transaction::SaveTransaction> {
    // 注意：此处不像 apply_commit_plan_to_live 那样对
    // `content_actions.is_empty() && engine_state_actions.is_empty() && !backup_mode`
    // 做 early return。进入本函数本身就说明 `plan.needs_sync_state_merge == true`，
    // `merged_state` / `merged_conflicts` 是需要提交的动作。即使 content_actions 和
    // engine_state_actions 都为空，也必须把 `app-meta/sync/state.local.json` 和
    // `app-meta/sync/conflicts.json` 加入 SaveTransaction，否则已算好的 merged state
    // 会被静默丢掉，live 的 state/conflicts 停留在旧值（Issue #762 评论 5831990584）。
    let mut tx = crate::storage::transaction::SaveTransaction::new(live_root);
    if backup_mode {
        tx.enable_backup_mode();
    }
    // engine_state_actions 只剩 manifest.sync.json。
    for action in engine_state_actions {
        match action {
            crate::sync::staging::CommitAction::Apply { rel_path, content } => {
                let rel_str = rel_path.to_string_lossy();
                tx.add_bytes(&rel_str, content)?;
            }
            crate::sync::staging::CommitAction::Delete { rel_path } => {
                let rel_str = rel_path.to_string_lossy();
                tx.add_delete(&rel_str);
            }
        }
    }
    // 合并后的 state.local.json + conflicts.json。
    let state_json = serde_json::to_string_pretty(merged_state)
        .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
    let conflicts_json = serde_json::to_string_pretty(merged_conflicts)
        .map_err(|e| crate::Error::Io(std::io::Error::other(e.to_string())))?;
    tx.add_bytes("app-meta/sync/state.local.json", state_json.as_bytes())?;
    tx.add_bytes("app-meta/sync/conflicts.json", conflicts_json.as_bytes())?;
    // content_actions（安全正文）。
    for action in content_actions {
        match action {
            crate::sync::staging::CommitAction::Apply { rel_path, content } => {
                let rel_str = rel_path.to_string_lossy();
                tx.add_bytes(&rel_str, content)?;
            }
            crate::sync::staging::CommitAction::Delete { rel_path } => {
                let rel_str = rel_path.to_string_lossy();
                tx.add_delete(&rel_str);
            }
        }
    }
    tx.commit()?;
    Ok(tx)
}

pub(crate) enum TargetCommitResult {
    Ok,
    Skipped,
    Failed(String),
}

/// 根据 `plan.needs_sync_state_merge` 选择走三方语义合并还是直接 apply。
///
/// - `needs_sync_state_merge == true`：调
///   [`merge_sync_state_three_way`] 得到合并后的 state/conflicts，
///   再调 [`apply_commit_plan_with_sync_state_merge`] 在同一个 tx 里提交。
/// - `needs_sync_state_merge == false`：走原 [`apply_commit_plan_to_live`]。
///
/// `content_actions` 由调用方过滤（`ConflictMetadataOnly` 会过滤掉 transfer
/// 冲突路径），`engine_state_actions` 直接用 `plan.engine_state_actions`
/// （只剩 `manifest.sync.json`）。
fn apply_commit_plan_with_optional_merge(
    run: &crate::sync::staging::StagingRun,
    live_root: &Path,
    plan: &crate::sync::staging::CommitPlan,
    content_actions: &[crate::sync::staging::CommitAction],
    backup_mode: bool,
) -> crate::error::Result<crate::storage::transaction::SaveTransaction> {
    if plan.needs_sync_state_merge {
        let base_root = run.base_root();
        let staging_root = run.staging_root();
        let (merged_state, merged_conflicts) =
            crate::sync::staging::sync_state_merge::merge_sync_state_three_way(
                &base_root,
                live_root,
                &staging_root,
            )?;
        apply_commit_plan_with_sync_state_merge(
            live_root,
            content_actions,
            &plan.engine_state_actions,
            &merged_state,
            &merged_conflicts,
            backup_mode,
        )
    } else {
        apply_commit_plan_to_live(
            live_root,
            content_actions,
            &plan.engine_state_actions,
            backup_mode,
        )
    }
}

/// 在 staging cleanup 前，对 plan.conflict 的每个 StagingConflict 读取 incoming
/// 正文（staging_root/rel_path），用 save_conflict_copy 保存到 live target 的冲突快照，
/// 填入 StagingConflict.remote_snapshot_path。这样 record_staging_conflicts 映射成
/// SyncConflict 时能带上远端 snapshot，#757 冲突侧栏"用户看到什么就选择什么"。
///
/// 必须在 `run.cleanup()` 之前调用——cleanup 会删除 staging_root，之后读不到 incoming。
/// 已填快照（`remote_snapshot_path.is_some()`）的条目跳过，避免重复保存。
fn save_conflict_snapshots(
    live_root: &Path,
    staging_root: &Path,
    conflicts: &mut [crate::sync::staging::StagingConflict],
) -> crate::error::Result<()> {
    for sc in conflicts.iter_mut() {
        if sc.remote_snapshot_path.is_some() {
            continue;
        }
        // 只有 BothChanged 才保存 incoming snapshot。
        // RemoteDeleted 远端已删除，无 incoming 正文，不保存快照，
        // remote_snapshot_path 保持 None。
        if sc.kind != crate::sync::types::SyncConflictKind::BothChanged {
            continue;
        }
        let rel_str = sc.rel_path.to_string_lossy().to_string();
        let incoming_path = staging_root.join(&sc.rel_path);
        if !incoming_path.exists() {
            // incoming 不存在（理论上 BothChanged 应有 incoming），跳过，保留 None 兜底。
            continue;
        }
        let incoming = std::fs::read(&incoming_path)?;
        let snapshot_rel = crate::sync::lww::save_conflict_copy(live_root, &rel_str, &incoming)?;
        sc.remote_snapshot_path = Some(snapshot_rel);
    }
    Ok(())
}

pub(crate) struct StagingCommitOutcome {
    pub(crate) target_results: Vec<TargetCommitResult>,
    pub(crate) target_conflicts: Vec<Vec<crate::sync::staging::StagingConflict>>,
    /// 本次 commit 真正落盘（Apply/Delete）的
    /// workspace-relative paths。供 `record_workspace_history` 精确 stage，
    /// 替代全量 `&[]` 扫描。
    pub(crate) committed_paths: Vec<PathBuf>,
}

pub(crate) enum TargetCommitMode {
    Full,
    ConflictMetadataOnly,
    Skip,
    /// ReplaceProject 走专用 replace plan
    /// （staging 有 → Apply；live 有但 staging 没有 → Delete），不走普通三方合并。
    ReplaceProject,
}

pub(crate) fn target_commit_mode(status: &crate::sync::SyncStatus) -> TargetCommitMode {
    use crate::sync::SyncStatus;
    match status {
        SyncStatus::Success | SyncStatus::NoChanges | SyncStatus::LatestWinsApplied => {
            TargetCommitMode::Full
        }
        SyncStatus::Conflict | SyncStatus::PartialConflict => {
            TargetCommitMode::ConflictMetadataOnly
        }
        _ => TargetCommitMode::Skip,
    }
}

/// staging commit 简化为纯文件级 commit。
///
/// 旧 Git finalize 逻辑（`prepare_git_finalize` / `try_commit_git_finalize` /
/// `cleanup_repo_create_owner_marker` / `coordinate_rollback_after_finalize_failure`）
/// 已删除。workspace 本地 Git 如果要参与本地版本历史，放在 commit 完成后的
/// workspace Git 层统一处理，不作为某个 remote provider 的 staging 模式。
#[allow(
    clippy::excessive_nesting,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]
pub(crate) fn apply_staging_commits_for_targets(
    staging_runs: &[crate::sync::staging::StagingRun],
    transfer_targets: &[crate::sync::types::TargetSyncResult],
) -> StagingCommitOutcome {
    let mut target_conflicts: Vec<Vec<crate::sync::staging::StagingConflict>> = Vec::new();
    let mut target_results: Vec<TargetCommitResult> = Vec::new();
    let mut committed_paths: Vec<PathBuf> = Vec::new();

    for (idx, run) in staging_runs.iter().enumerate() {
        //   有 DeleteProject lifecycle action 的 target
        // 跳过 staging commit（本地删除由 commit_full_sync 的 lifecycle action 处理）。
        // 否则 staging commit 会把 staging 里的旧作品内容写回 live，复活刚删掉的作品。
        // ReplaceProject 走整树替换 commit
        // （staging 有 → Apply；live 有但 staging 没有 → Delete），不再走普通
        // compute_commit_plan。在真正 Apply/Delete 之前用 snapshot_local_records_read_only
        // 重新计算当前 local target LWW，与 expected_local_lww guard 比较。
        let has_delete_action = transfer_targets
            .get(idx)
            .map(|t| {
                matches!(
                    t.local_lifecycle_action,
                    crate::sync::types::LocalLifecycleCommitAction::DeleteProject { .. }
                )
            })
            .unwrap_or(false);
        let has_replace_action = transfer_targets
            .get(idx)
            .map(|t| {
                matches!(
                    t.local_lifecycle_action,
                    crate::sync::types::LocalLifecycleCommitAction::ReplaceProject { .. }
                )
            })
            .unwrap_or(false);
        let mode = if has_delete_action {
            TargetCommitMode::Skip
        } else if has_replace_action {
            // ReplaceProject 走专用 replace plan。
            TargetCommitMode::ReplaceProject
        } else if let Some(target) = transfer_targets.get(idx) {
            target_commit_mode(&target.result.status)
        } else {
            TargetCommitMode::Skip
        };

        match mode {
            TargetCommitMode::Skip => {
                log::warn!(
                    "Staging commit: skipping target {} (run_id={})",
                    idx,
                    run.run_id()
                );
                target_results.push(TargetCommitResult::Skipped);
                target_conflicts.push(Vec::new());
                run.cleanup();
                continue;
            }
            TargetCommitMode::Full => {
                let live_root = run.target_live_root();
                let mut plan = match run.compute_commit_plan(live_root) {
                    Ok(plan) => plan,
                    Err(e) => {
                        let msg = format!("compute_commit_plan failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        target_results.push(TargetCommitResult::Failed(msg));
                        target_conflicts.push(Vec::new());
                        run.cleanup();
                        continue;
                    }
                };

                let mut tx = match apply_commit_plan_with_optional_merge(
                    run,
                    live_root,
                    &plan,
                    &plan.content_actions,
                    false,
                ) {
                    Ok(tx) => tx,
                    Err(e) => {
                        let msg = format!("apply_commit_plan failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        target_results.push(TargetCommitResult::Failed(msg));
                        target_conflicts.push(Vec::new());
                        run.cleanup();
                        continue;
                    }
                };

                match tx.finish() {
                    Ok(()) => {
                        // 收集本 target 真正
                        // Apply/Delete 的 rel_path，转成 workspace-relative。
                        //   committed_paths 只收集
                        // content_actions，不收集 engine_state_actions——
                        // sync engine state（manifest.sync.json/state.local.json/
                        // conflicts.json 等）不进入本地 Git history。
                        let (kind, pid) = transfer_targets
                            .get(idx)
                            .map(|t| (t.target_kind.as_str(), t.project_id.as_deref()))
                            .unwrap_or(("", None));
                        collect_action_paths(
                            kind,
                            pid,
                            &plan.content_actions,
                            &mut committed_paths,
                        );
                        // #757：在 cleanup 前保存远端快照到 plan.conflict，
                        // 让 record_staging_conflicts 映射成 SyncConflict 时带上
                        // remote_snapshot_path。cleanup 后 staging_root 已删，无法再读。
                        let staging_root = run.staging_root();
                        if let Err(e) =
                            save_conflict_snapshots(live_root, &staging_root, &mut plan.conflict)
                        {
                            let msg = format!("save_conflict_snapshots failed: {}", e);
                            log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                            target_results.push(TargetCommitResult::Failed(msg));
                            target_conflicts.push(Vec::new());
                            run.cleanup();
                            continue;
                        }
                        target_conflicts.push(plan.conflict);
                        target_results.push(TargetCommitResult::Ok);
                        run.cleanup();
                    }
                    Err(e) => {
                        let msg = format!("tx.finish() failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        target_results.push(TargetCommitResult::Failed(msg));
                        target_conflicts.push(Vec::new());
                    }
                }
            }
            TargetCommitMode::ConflictMetadataOnly => {
                let live_root = run.target_live_root();
                let mut plan = match run.compute_commit_plan(live_root) {
                    Ok(plan) => plan,
                    Err(e) => {
                        let msg = format!("compute_commit_plan failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        target_results.push(TargetCommitResult::Failed(msg));
                        target_conflicts.push(Vec::new());
                        run.cleanup();
                        continue;
                    }
                };
                let transfer_conflict_paths: std::collections::HashSet<String> = transfer_targets
                    .get(idx)
                    .map(|t| {
                        t.result
                            .conflicts
                            .iter()
                            .map(|c| c.local_path.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                let safe_content_actions: Vec<_> = plan
                    .content_actions
                    .iter()
                    .filter(|action| {
                        let rel = match action {
                            crate::sync::staging::CommitAction::Apply { rel_path, .. } => {
                                rel_path.to_string_lossy().to_string()
                            }
                            crate::sync::staging::CommitAction::Delete { rel_path } => {
                                rel_path.to_string_lossy().to_string()
                            }
                        };
                        !transfer_conflict_paths.contains(&rel)
                    })
                    .cloned()
                    .collect();
                if let Err(e) = apply_commit_plan_with_optional_merge(
                    run,
                    live_root,
                    &plan,
                    &safe_content_actions,
                    false,
                )
                .map(|_tx| ())
                {
                    let msg = format!("apply_commit_plan failed: {}", e);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    target_results.push(TargetCommitResult::Failed(msg));
                    target_conflicts.push(Vec::new());
                    run.cleanup();
                    continue;
                }
                // ConflictMetadataOnly 也落盘了
                // safe_content_actions + engine_state_actions，收集它们的 rel_path。
                //   committed_paths 只收集
                // safe_content_actions，不收集 engine_state_actions。
                let (kind, pid) = transfer_targets
                    .get(idx)
                    .map(|t| (t.target_kind.as_str(), t.project_id.as_deref()))
                    .unwrap_or(("", None));
                collect_action_paths(kind, pid, &safe_content_actions, &mut committed_paths);
                // #757：在 cleanup 前保存远端快照到 plan.conflict（同 Full 模式）。
                let staging_root = run.staging_root();
                if let Err(e) =
                    save_conflict_snapshots(live_root, &staging_root, &mut plan.conflict)
                {
                    let msg = format!("save_conflict_snapshots failed: {}", e);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    target_results.push(TargetCommitResult::Failed(msg));
                    target_conflicts.push(Vec::new());
                    run.cleanup();
                    continue;
                }
                target_conflicts.push(plan.conflict);
                target_results.push(TargetCommitResult::Ok);
                run.cleanup();
            }
            TargetCommitMode::ReplaceProject => {
                // ReplaceProject 走专用 replace plan。
                let live_root = run.target_live_root();
                let staging_root = run.staging_root();

                // 1. guard 检查：用 snapshot_local_records_read_only 重新计算当前
                //    local target LWW，与 expected_local_lww 严格比较。
                // expected_local_lww 非 Option —
                // 破坏性 action 必须携带 guard。
                let expected_lww =
                    transfer_targets
                        .get(idx)
                        .and_then(|t| match &t.local_lifecycle_action {
                            crate::sync::types::LocalLifecycleCommitAction::ReplaceProject {
                                expected_local_lww,
                                ..
                            } => Some(crate::sync::full_sync::LiveTargetLww {
                                lww_time_ms: expected_local_lww.lww_time_ms,
                                device_id: expected_local_lww.device_id.clone(),
                            }),
                            _ => None,
                        });
                let expected_lww = match expected_lww {
                    Some(lww) => lww,
                    None => {
                        // 不应发生：TargetCommitMode::ReplaceProject 只对 ReplaceProject action 设置。
                        let msg =
                            "ReplaceProject commit mode but no ReplaceProject action with guard"
                                .to_string();
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        target_results.push(TargetCommitResult::Failed(msg));
                        target_conflicts.push(Vec::new());
                        run.cleanup();
                        continue;
                    }
                };
                match crate::sync::staging::replace::check_replace_project_guard(
                    live_root,
                    &expected_lww,
                ) {
                    crate::sync::staging::replace::ReplaceProjectGuardResult::Ok => {
                        // guard 通过，继续执行 replace plan。
                    }
                    crate::sync::staging::replace::ReplaceProjectGuardResult::Err(e) => {
                        let msg = match e {
                            crate::sync::staging::replace::ReplaceProjectGuardError::LocalAdvanced {
                                expected,
                                current,
                            } => format!(
                                "ReplaceProject guard failed: local advanced \
                                 (expected lww_time={} device_id={}, current lww_time={} device_id={}) \
                                 — not touching live",
                                expected.lww_time_ms,
                                expected.device_id,
                                current.lww_time_ms,
                                current.device_id
                            ),
                            crate::sync::staging::replace::ReplaceProjectGuardError::SnapshotFailed(err) => {
                                format!("ReplaceProject guard snapshot failed: {err}")
                            }
                        };
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        target_results.push(TargetCommitResult::Failed(msg));
                        target_conflicts.push(Vec::new());
                        run.cleanup();
                        continue;
                    }
                }

                // 2. 构建 replace plan（staging 有 → Apply；live 有但 staging 没有 → Delete）。
                let mut plan = match crate::sync::staging::replace::build_replace_project_plan(
                    live_root,
                    &staging_root,
                ) {
                    Ok(plan) => plan,
                    Err(e) => {
                        let msg = format!("build_replace_project_plan failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        target_results.push(TargetCommitResult::Failed(msg));
                        target_conflicts.push(Vec::new());
                        run.cleanup();
                        continue;
                    }
                };

                // 3. 应用 replace plan 到 live。
                let mut tx = match apply_commit_plan_to_live(
                    live_root,
                    &plan.content_actions,
                    &plan.engine_state_actions,
                    false,
                ) {
                    Ok(tx) => tx,
                    Err(e) => {
                        let msg = format!("apply_commit_plan_to_live failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        target_results.push(TargetCommitResult::Failed(msg));
                        target_conflicts.push(Vec::new());
                        run.cleanup();
                        continue;
                    }
                };

                match tx.finish() {
                    Ok(()) => {
                        let (kind, pid) = transfer_targets
                            .get(idx)
                            .map(|t| (t.target_kind.as_str(), t.project_id.as_deref()))
                            .unwrap_or(("", None));
                        collect_action_paths(
                            kind,
                            pid,
                            &plan.content_actions,
                            &mut committed_paths,
                        );
                        // #757：在 cleanup 前保存远端快照（防御性——ReplaceProject
                        // 通常无冲突，但保持与其他分支一致）。
                        if let Err(e) =
                            save_conflict_snapshots(live_root, &staging_root, &mut plan.conflict)
                        {
                            let msg = format!("save_conflict_snapshots failed: {}", e);
                            log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                            target_results.push(TargetCommitResult::Failed(msg));
                            target_conflicts.push(Vec::new());
                            run.cleanup();
                            continue;
                        }
                        target_conflicts.push(plan.conflict);
                        target_results.push(TargetCommitResult::Ok);
                        run.cleanup();
                    }
                    Err(e) => {
                        let msg = format!("tx.finish() failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        target_results.push(TargetCommitResult::Failed(msg));
                        target_conflicts.push(Vec::new());
                    }
                }
            }
        }
    }

    StagingCommitOutcome {
        target_results,
        target_conflicts,
        committed_paths,
    }
}

/// 单 target staging commit — 新编排入口。
///
/// 与 `apply_staging_commits_for_targets` 中对单个 target 的处理逻辑一致，
/// 但有以下区别：
/// - **不调用 `run.cleanup()`**：由调用方负责 cleanup，便于在 commit 后、cleanup 前
///   插入 `record_staging_conflicts` 等操作。
/// - **输入是单个 `&StagingRun` 和 `&TargetSyncResult`**，不是批量切片。
/// - **输出是 `(TargetCommitResult, Vec<StagingConflict>, Vec<PathBuf>)`**：
///   commit result, conflicts, committed_paths。
///
/// 复用现有 Full / ConflictMetadataOnly / ReplaceProject / Skip 规则。
#[allow(
    clippy::excessive_nesting,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]
pub(crate) fn commit_single_target_staging(
    run: &crate::sync::staging::StagingRun,
    target: &crate::sync::types::TargetSyncResult,
) -> (
    TargetCommitResult,
    Vec<crate::sync::staging::StagingConflict>,
    Vec<PathBuf>,
) {
    let has_delete_action = matches!(
        target.local_lifecycle_action,
        crate::sync::types::LocalLifecycleCommitAction::DeleteProject { .. }
    );
    let has_replace_action = matches!(
        target.local_lifecycle_action,
        crate::sync::types::LocalLifecycleCommitAction::ReplaceProject { .. }
    );
    let mode = if has_delete_action {
        TargetCommitMode::Skip
    } else if has_replace_action {
        TargetCommitMode::ReplaceProject
    } else {
        target_commit_mode(&target.result.status)
    };

    match mode {
        TargetCommitMode::Skip => {
            log::warn!("Staging commit: skipping target (run_id={})", run.run_id());
            (TargetCommitResult::Skipped, Vec::new(), Vec::new())
        }
        TargetCommitMode::Full => {
            let live_root = run.target_live_root();
            let mut plan = match run.compute_commit_plan(live_root) {
                Ok(plan) => plan,
                Err(e) => {
                    let msg = format!("compute_commit_plan failed: {}", e);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    return (TargetCommitResult::Failed(msg), Vec::new(), Vec::new());
                }
            };

            let mut tx = match apply_commit_plan_with_optional_merge(
                run,
                live_root,
                &plan,
                &plan.content_actions,
                false,
            ) {
                Ok(tx) => tx,
                Err(e) => {
                    let msg = format!("apply_commit_plan failed: {}", e);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    return (TargetCommitResult::Failed(msg), Vec::new(), Vec::new());
                }
            };

            match tx.finish() {
                Ok(()) => {
                    let mut committed_paths = Vec::new();
                    collect_action_paths(
                        target.target_kind.as_str(),
                        target.project_id.as_deref(),
                        &plan.content_actions,
                        &mut committed_paths,
                    );
                    let staging_root = run.staging_root();
                    if let Err(e) =
                        save_conflict_snapshots(live_root, &staging_root, &mut plan.conflict)
                    {
                        let msg = format!("save_conflict_snapshots failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        return (TargetCommitResult::Failed(msg), Vec::new(), committed_paths);
                    }
                    (TargetCommitResult::Ok, plan.conflict, committed_paths)
                }
                Err(e) => {
                    let msg = format!("tx.finish() failed: {}", e);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    (TargetCommitResult::Failed(msg), Vec::new(), Vec::new())
                }
            }
        }
        TargetCommitMode::ConflictMetadataOnly => {
            let live_root = run.target_live_root();
            let mut plan = match run.compute_commit_plan(live_root) {
                Ok(plan) => plan,
                Err(e) => {
                    let msg = format!("compute_commit_plan failed: {}", e);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    return (TargetCommitResult::Failed(msg), Vec::new(), Vec::new());
                }
            };
            let transfer_conflict_paths: std::collections::HashSet<String> = target
                .result
                .conflicts
                .iter()
                .map(|c| c.local_path.clone())
                .collect();
            let safe_content_actions: Vec<_> = plan
                .content_actions
                .iter()
                .filter(|action| {
                    let rel = match action {
                        crate::sync::staging::CommitAction::Apply { rel_path, .. } => {
                            rel_path.to_string_lossy().to_string()
                        }
                        crate::sync::staging::CommitAction::Delete { rel_path } => {
                            rel_path.to_string_lossy().to_string()
                        }
                    };
                    !transfer_conflict_paths.contains(&rel)
                })
                .cloned()
                .collect();
            if let Err(e) = apply_commit_plan_with_optional_merge(
                run,
                live_root,
                &plan,
                &safe_content_actions,
                false,
            )
            .map(|_tx| ())
            {
                let msg = format!("apply_commit_plan failed: {}", e);
                log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                return (TargetCommitResult::Failed(msg), Vec::new(), Vec::new());
            }
            let mut committed_paths = Vec::new();
            collect_action_paths(
                target.target_kind.as_str(),
                target.project_id.as_deref(),
                &safe_content_actions,
                &mut committed_paths,
            );
            let staging_root = run.staging_root();
            if let Err(e) = save_conflict_snapshots(live_root, &staging_root, &mut plan.conflict) {
                let msg = format!("save_conflict_snapshots failed: {}", e);
                log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                return (TargetCommitResult::Failed(msg), Vec::new(), committed_paths);
            }
            (TargetCommitResult::Ok, plan.conflict, committed_paths)
        }
        TargetCommitMode::ReplaceProject => {
            let live_root = run.target_live_root();
            let staging_root = run.staging_root();

            let expected_lww = match &target.local_lifecycle_action {
                crate::sync::types::LocalLifecycleCommitAction::ReplaceProject {
                    expected_local_lww,
                    ..
                } => Some(crate::sync::full_sync::LiveTargetLww {
                    lww_time_ms: expected_local_lww.lww_time_ms,
                    device_id: expected_local_lww.device_id.clone(),
                }),
                _ => None,
            };
            let expected_lww = match expected_lww {
                Some(lww) => lww,
                None => {
                    let msg = "ReplaceProject commit mode but no ReplaceProject action with guard"
                        .to_string();
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    return (TargetCommitResult::Failed(msg), Vec::new(), Vec::new());
                }
            };
            match crate::sync::staging::replace::check_replace_project_guard(
                live_root,
                &expected_lww,
            ) {
                crate::sync::staging::replace::ReplaceProjectGuardResult::Ok => {}
                crate::sync::staging::replace::ReplaceProjectGuardResult::Err(e) => {
                    let msg = match e {
                        crate::sync::staging::replace::ReplaceProjectGuardError::LocalAdvanced {
                            expected,
                            current,
                        } => format!(
                            "ReplaceProject guard failed: local advanced \
                             (expected lww_time={} device_id={}, current lww_time={} device_id={}) \
                             — not touching live",
                            expected.lww_time_ms,
                            expected.device_id,
                            current.lww_time_ms,
                            current.device_id
                        ),
                        crate::sync::staging::replace::ReplaceProjectGuardError::SnapshotFailed(err) => {
                            format!("ReplaceProject guard snapshot failed: {err}")
                        }
                    };
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    return (TargetCommitResult::Failed(msg), Vec::new(), Vec::new());
                }
            }

            let mut plan = match crate::sync::staging::replace::build_replace_project_plan(
                live_root,
                &staging_root,
            ) {
                Ok(plan) => plan,
                Err(e) => {
                    let msg = format!("build_replace_project_plan failed: {}", e);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    return (TargetCommitResult::Failed(msg), Vec::new(), Vec::new());
                }
            };

            let mut tx = match apply_commit_plan_to_live(
                live_root,
                &plan.content_actions,
                &plan.engine_state_actions,
                false,
            ) {
                Ok(tx) => tx,
                Err(e) => {
                    let msg = format!("apply_commit_plan_to_live failed: {}", e);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    return (TargetCommitResult::Failed(msg), Vec::new(), Vec::new());
                }
            };

            match tx.finish() {
                Ok(()) => {
                    let mut committed_paths = Vec::new();
                    collect_action_paths(
                        target.target_kind.as_str(),
                        target.project_id.as_deref(),
                        &plan.content_actions,
                        &mut committed_paths,
                    );
                    if let Err(e) =
                        save_conflict_snapshots(live_root, &staging_root, &mut plan.conflict)
                    {
                        let msg = format!("save_conflict_snapshots failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        return (TargetCommitResult::Failed(msg), Vec::new(), committed_paths);
                    }
                    (TargetCommitResult::Ok, plan.conflict, committed_paths)
                }
                Err(e) => {
                    let msg = format!("tx.finish() failed: {}", e);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    (TargetCommitResult::Failed(msg), Vec::new(), Vec::new())
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::staging::CommitAction;
    use crate::sync::types::{SyncConflict, SyncConflictKind, SyncState};
    use tempfile::TempDir;

    /// Issue #762 评论 5831990584 问题 2 复现：
    /// `apply_commit_plan_with_sync_state_merge()` 开头的 early return：
    /// ```ignore
    /// if content_actions.is_empty() && engine_state_actions.is_empty() && !backup_mode {
    ///     return Ok(SaveTransaction::new(live_root));
    /// }
    /// ```
    /// 会把已经算好的 `merged_state` / `merged_conflicts` 静默丢掉。
    ///
    /// 进入这个函数本身就说明 `plan.needs_sync_state_merge == true`，
    /// merged_state / merged_conflicts 是需要提交的动作。即使没有正文 Apply/Delete、
    /// manifest 也没动作，只要 state.local.json / conflicts.json 的三方结果
    /// 发生了变化，这个 early return 会跳过写回，让 live 的 state/conflicts
    /// 停留在旧值。
    ///
    /// 修复前：本测试在读取 live state.local.json 的断言处失败——
    /// 文件内容仍是 live 旧值，merged state 被丢掉。
    #[test]
    fn empty_actions_must_still_persist_merged_sync_state() {
        let tmp = TempDir::new().unwrap();
        let live_root = tmp.path().join("live");
        std::fs::create_dir_all(live_root.join("app-meta/sync")).unwrap();

        // live 旧状态：device_id = "live-device"，known_files 空。
        let mut live_state = SyncState::default();
        live_state.device_id = "live-device".to_string();
        let live_state_json = serde_json::to_string_pretty(&live_state).unwrap();
        std::fs::write(
            live_root.join("app-meta/sync/state.local.json"),
            live_state_json,
        )
        .unwrap();
        std::fs::write(live_root.join("app-meta/sync/conflicts.json"), "[]").unwrap();

        // merged_state 与 live 不同：device_id = "merged-device"。
        let mut merged_state = SyncState::default();
        merged_state.device_id = "merged-device".to_string();
        merged_state
            .known_files
            .insert("a.md".to_string(), "hash-a".to_string());

        // merged_conflicts 非空，和 live 的空 conflicts 不同。
        let merged_conflict = SyncConflict {
            local_path: "a.md".to_string(),
            remote_path: "a.md".to_string(),
            kind: SyncConflictKind::BothChanged,
            local_hash: "local".to_string(),
            remote_hash: "remote".to_string(),
            base_hash: "base".to_string(),
            created_at: 1,
            description: "merged".to_string(),
            remote_snapshot_path: None,
        };
        let merged_conflicts = vec![merged_conflict];

        // content_actions=[]、engine_state_actions=[]、backup_mode=false
        // → 命中当前代码的 early return，merged state 被丢掉。
        let content_actions: &[CommitAction] = &[];
        let engine_state_actions: &[CommitAction] = &[];
        let mut tx = apply_commit_plan_with_sync_state_merge(
            &live_root,
            content_actions,
            engine_state_actions,
            &merged_state,
            &merged_conflicts,
            false,
        )
        .expect("apply_commit_plan_with_sync_state_merge 不应返回 Err");
        tx.finish().expect("tx.finish 不应失败");

        // 验证 live 的 state.local.json 已更新为 merged_state。
        let live_state_path = live_root.join("app-meta/sync/state.local.json");
        assert!(
            live_state_path.exists(),
            "state.local.json 应被写入（merged state 必须落盘）"
        );
        let content = std::fs::read_to_string(&live_state_path).unwrap();
        let persisted: SyncState = serde_json::from_str(&content).unwrap();
        assert_eq!(
            persisted.device_id, "merged-device",
            "live 的 state.local.json 必须更新为 merged_state 的 device_id；\
             当前（未修复）代码因 early return 仍是 live 旧值 'live-device'"
        );
        assert_eq!(
            persisted.known_files.get("a.md").map(String::as_str),
            Some("hash-a"),
            "live 的 state.local.json 必须包含 merged_state 的 known_files"
        );

        // 验证 live 的 conflicts.json 已更新为 merged_conflicts。
        let conflicts_path = live_root.join("app-meta/sync/conflicts.json");
        assert!(
            conflicts_path.exists(),
            "conflicts.json 应被写入（merged conflicts 必须落盘）"
        );
        let conflicts_content = std::fs::read_to_string(&conflicts_path).unwrap();
        let persisted_conflicts: Vec<SyncConflict> =
            serde_json::from_str(&conflicts_content).unwrap();
        assert!(
            persisted_conflicts.iter().any(|c| c.local_path == "a.md"),
            "live 的 conflicts.json 必须更新为 merged_conflicts；\
             当前（未修复）代码因 early return 仍是 live 旧值（空）"
        );
    }
}
