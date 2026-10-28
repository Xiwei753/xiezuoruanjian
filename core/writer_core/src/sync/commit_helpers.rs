use std::path::{Path, PathBuf};

/// 把 target-relative `rel_path` 转成 workspace-relative path，供
/// `record_workspace_history` 精确 stage。
///
/// - App target：`live_root = app_data_root`，`rel_path` 已是 workspace-relative。
/// - Project target：`live_root = projects_root/<project_id>`，
///   workspace-relative = `projects/<project_id>/<rel_path>`。
/// - DeletedProject target with `RemoteTargetWins`：`target_kind` 保持为
///   `"deleted_project"`，但 `target_live_root` 已设为 `projects_root/<project_id>`。
fn to_workspace_rel_path(target_kind: &str, project_id: Option<&str>, rel_path: &Path) -> PathBuf {
    if target_kind == "project" || target_kind == "deleted_project" {
        if let Some(pid) = project_id {
            return PathBuf::from("projects").join(pid).join(rel_path);
        }
    }
    PathBuf::from(rel_path)
}

/// 章节正文在 commit 前的真实快照。同步证明必须用这个快照作为 before，
/// 不能等应用后再从 live 倒推，也不能把“文件不存在”当成空正文。
#[derive(Debug, Clone)]
pub(crate) enum CommittedBefore {
    /// 非章节正文路径，不为证明读取。
    NotTracked,
    /// 章节正文在 commit 前不存在（首次导入/删除已不存在）。
    Missing,
    /// commit 前的原始字节（空文件也是空 Vec）。
    Text(Vec<u8>),
    /// 读取失败，证明事件必须明确报错而不是伪造 before。
    Error(String),
}

#[derive(Debug, Clone)]
pub(crate) struct CommittedPath {
    pub path: PathBuf,
    pub deleted: bool,
    pub before: CommittedBefore,
}

/// 本次 commit 真正落盘的一个 workspace-relative 路径，以及它是写入还是删除。
///
/// 证明事件不能只看路径是否存在来判断 Apply/Delete：删除要记录真实同步删除
/// 事实，写入则要读取落盘后的正文。动作类型由 commit plan 的 `CommitAction`
/// 原样带出。

/// commit 前为每个章节正文动作保存真实快照。快照发生在事务写入之前，
/// 且只读取章节正文，避免为图片等无关文件增加 I/O。
fn capture_action_before(
    live_root: &Path,
    actions: &[crate::sync::staging::CommitAction],
) -> Vec<CommittedBefore> {
    actions
        .iter()
        .map(|action| {
            let rel = match action {
                crate::sync::staging::CommitAction::Apply { rel_path, .. } => rel_path,
                crate::sync::staging::CommitAction::Delete { rel_path } => rel_path,
            };
            if !is_chapter_content_path(rel) {
                return CommittedBefore::NotTracked;
            }
            match std::fs::read(live_root.join(rel)) {
                Ok(bytes) => CommittedBefore::Text(bytes),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => CommittedBefore::Missing,
                Err(e) => CommittedBefore::Error(format!(
                    "读取同步章节前置正文 {} 失败: {}",
                    rel.display(),
                    e
                )),
            }
        })
        .collect()
}

fn is_chapter_content_path(path: &Path) -> bool {
    let mut has_chapters = false;
    let mut last = None;
    for component in path.components() {
        let part = component.as_os_str();
        if part == "chapters" {
            has_chapters = true;
        }
        last = Some(part);
    }
    has_chapters && last.is_some_and(|name| name == "chapter.md")
}

/// 收集一批 `CommitAction` 的 rel_path，转成 workspace-relative 后追加到 `out`。
fn collect_action_paths(
    target_kind: &str,
    project_id: Option<&str>,
    actions: &[crate::sync::staging::CommitAction],
    before: &[CommittedBefore],
    out: &mut Vec<CommittedPath>,
) {
    for (index, action) in actions.iter().enumerate() {
        let rel = match action {
            crate::sync::staging::CommitAction::Apply { rel_path, .. } => rel_path,
            crate::sync::staging::CommitAction::Delete { rel_path } => rel_path,
        };
        out.push(CommittedPath {
            path: to_workspace_rel_path(target_kind, project_id, rel),
            deleted: matches!(action, crate::sync::staging::CommitAction::Delete { .. }),
            before: before
                .get(index)
                .cloned()
                .unwrap_or(CommittedBefore::NotTracked),
        });
    }
}

/// Rebase locally stored text anchors when an incoming chapter body is applied
/// without an incoming record for that anchor. The rebased anchor files join
/// the same SaveTransaction as the body so a crash cannot leave new text with
/// stale offsets. Ambiguous, missing, or invalid UTF-8 inputs are left alone.
fn stage_rebased_chapter_anchors(
    live_root: &Path,
    content_actions: &[crate::sync::staging::CommitAction],
    tx: &mut crate::storage::transaction::SaveTransaction,
) -> crate::error::Result<()> {
    use crate::sync::staging::CommitAction;
    use std::collections::HashSet;

    let mut handled_chapters = HashSet::new();
    let mut staged_anchor_paths = HashSet::new();
    let mut rebased_anchors = Vec::new();
    let mut newest_existing_version = i64::MIN;
    for action in content_actions {
        let CommitAction::Apply { rel_path, content } = action else {
            continue;
        };
        let Some((project_namespace, project_relative, project_id)) =
            project_relative_sync_path(live_root, rel_path)
        else {
            continue;
        };
        let Some(chapter_id) = chapter_id_for_body_path(&project_relative) else {
            continue;
        };
        let chapter_key = (project_namespace.clone(), chapter_id.to_string());
        if !handled_chapters.insert(chapter_key) {
            continue;
        }
        let Ok(incoming_text) = std::str::from_utf8(content) else {
            continue;
        };

        let project_root = live_root.join(&project_namespace);
        let store = crate::annotations::AnnotationStore::new(&project_root, &project_id);
        for anchor in store.list_text_anchors_for_chapter(chapter_id)? {
            if anchor.project_id != project_id || !is_safe_annotation_id(&anchor.anchor_id) {
                continue;
            }
            let anchor_relative = PathBuf::from("annotations")
                .join("anchors")
                .join(format!("{}.json", anchor.anchor_id));
            let explicitly_changed = content_actions.iter().any(|candidate| {
                let candidate_path = match candidate {
                    CommitAction::Apply { rel_path, .. } | CommitAction::Delete { rel_path } => {
                        rel_path
                    }
                };
                project_relative_sync_path(live_root, candidate_path).is_some_and(
                    |(namespace, relative, _)| {
                        namespace == project_namespace && relative == anchor_relative
                    },
                )
            });
            if explicitly_changed
                || !staged_anchor_paths
                    .insert((project_namespace.clone(), anchor.anchor_id.clone()))
            {
                continue;
            }

            let Some(rebased) =
                crate::annotations::ranges::rebase_text_anchor(&anchor, incoming_text)?
            else {
                continue;
            };
            if rebased == anchor {
                continue;
            }
            newest_existing_version = newest_existing_version.max(anchor.updated_at_ms);
            rebased_anchors.push((project_namespace.join(&anchor_relative), rebased));
        }
    }
    if !rebased_anchors.is_empty() {
        let updated_at_ms = chrono::Utc::now()
            .timestamp_millis()
            .max(newest_existing_version.saturating_add(1));
        for (target_relative, mut anchor) in rebased_anchors {
            anchor.updated_at_ms = updated_at_ms;
            let serialized = serde_json::to_vec_pretty(&anchor)?;
            let target_relative = target_relative.to_string_lossy();
            tx.add_bytes(&target_relative, &serialized)?;
        }
    }
    Ok(())
}

/// Return the project namespace within `live_root`, the project-relative path,
/// and project ID for project-target (`volumes/...`) or app-target
/// (`projects/<id>/volumes/...`) sync paths.
fn project_relative_sync_path(
    live_root: &Path,
    rel_path: &Path,
) -> Option<(PathBuf, PathBuf, String)> {
    use std::path::Component;

    let components = rel_path
        .components()
        .map(|component| match component {
            Component::Normal(value) => value.to_str().map(str::to_string),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    if components.is_empty() {
        return None;
    }

    if components.len() >= 3 && components[0] == "projects" {
        let project_id = components[1].clone();
        let namespace = PathBuf::from("projects").join(&project_id);
        let project_relative = components[2..].iter().collect::<PathBuf>();
        return Some((namespace, project_relative, project_id));
    }

    let project_id = live_root.file_name()?.to_str()?.to_string();
    Some((PathBuf::new(), components.iter().collect(), project_id))
}

fn chapter_id_for_body_path(project_relative: &Path) -> Option<&str> {
    let components = project_relative
        .components()
        .map(|component| component.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()?;
    match components.as_slice() {
        ["volumes", _volume_id, "chapters", chapter_id, "chapter.md"] => Some(chapter_id),
        _ => None,
    }
}

fn is_safe_annotation_id(id: &str) -> bool {
    !id.is_empty()
        && id != "."
        && id != ".."
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

/// Add automatically rebased anchor files to the workspace-history path list.
/// Their transaction paths are not present in `content_actions` by definition.
fn collect_rebased_anchor_paths(
    tx: &crate::storage::transaction::SaveTransaction,
    target_kind: &str,
    project_id: Option<&str>,
    content_actions: &[crate::sync::staging::CommitAction],
    out: &mut Vec<CommittedPath>,
) {
    let action_paths = content_actions
        .iter()
        .map(|action| match action {
            crate::sync::staging::CommitAction::Apply { rel_path, .. }
            | crate::sync::staging::CommitAction::Delete { rel_path } => rel_path,
        })
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<std::collections::HashSet<_>>();
    for entry in &tx.entries {
        let path = Path::new(&entry.target_relative);
        let components = path
            .components()
            .map(|component| component.as_os_str().to_str())
            .collect::<Option<Vec<_>>>();
        let is_anchor_json = components.as_deref().is_some_and(|components| {
            matches!(components, ["annotations", "anchors", file] if file.ends_with(".json"))
                || matches!(components, ["projects", _, "annotations", "anchors", file] if file.ends_with(".json"))
        });
        if entry.is_delete || !is_anchor_json || action_paths.contains(&entry.target_relative) {
            continue;
        }
        out.push(CommittedPath {
            path: to_workspace_rel_path(target_kind, project_id, path),
            deleted: false,
            before: CommittedBefore::NotTracked,
        });
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
    proof_intent_ids: &[String],
) -> crate::error::Result<crate::storage::transaction::SaveTransaction> {
    if content_actions.is_empty() && engine_state_actions.is_empty() && !backup_mode {
        return Ok(crate::storage::transaction::SaveTransaction::new(live_root));
    }
    let mut tx = crate::storage::transaction::SaveTransaction::new(live_root);
    tx.link_proof_intents(proof_intent_ids.to_vec());
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
    stage_rebased_chapter_anchors(live_root, content_actions, &mut tx)?;
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
    apply_commit_plan_with_sync_state_merge_for_group(
        live_root,
        content_actions,
        engine_state_actions,
        merged_state,
        merged_conflicts,
        None,
        backup_mode,
        &[],
    )
}

fn apply_commit_plan_with_sync_state_merge_for_group(
    live_root: &Path,
    content_actions: &[crate::sync::staging::CommitAction],
    engine_state_actions: &[crate::sync::staging::CommitAction],
    merged_state: &crate::sync::types::SyncState,
    merged_conflicts: &[crate::sync::types::SyncConflict],
    app_group: Option<&crate::sync::types::AppDataGroup>,
    backup_mode: bool,
    proof_intent_ids: &[String],
) -> crate::error::Result<crate::storage::transaction::SaveTransaction> {
    // 注意：此处不像 apply_commit_plan_to_live 那样对
    // `content_actions.is_empty() && engine_state_actions.is_empty() && !backup_mode`
    // 做 early return。进入本函数本身就说明 `plan.needs_sync_state_merge == true`，
    // `merged_state` / `merged_conflicts` 是需要提交的动作。即使 content_actions 和
    // engine_state_actions 都为空，也必须把 `app-meta/sync/state.local.json` 和
    // `app-meta/sync/conflicts.json` 加入 SaveTransaction，否则已算好的 merged state
    // 会被静默丢掉，live 的 state/conflicts 停留在旧值（Issue #762 评论 5831990584）。
    let mut tx = crate::storage::transaction::SaveTransaction::new(live_root);
    tx.link_proof_intents(proof_intent_ids.to_vec());
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
    let (state_path, conflicts_path) = match app_group {
        Some(group) => (group.local_state_path(), group.local_conflicts_path()),
        None => (
            "app-meta/sync/state.local.json".to_string(),
            "app-meta/sync/conflicts.json".to_string(),
        ),
    };
    tx.add_bytes(&state_path, state_json.as_bytes())?;
    tx.add_bytes(&conflicts_path, conflicts_json.as_bytes())?;
    // content_actions（安全正文）。
    stage_rebased_chapter_anchors(live_root, content_actions, &mut tx)?;
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

/// Inject a staging-commit failure into the target result.
///
/// Manifest read/parse failures carry `manifest_read_failed: path=... cause=...`
/// from the staging layer. Preserve that concrete path and report the failure
/// as `merge`, so a corrupted manifest is not misreported as a transfer error.
pub(crate) fn apply_commit_failure_to_target(
    target: &mut crate::sync::types::TargetSyncResult,
    message: &str,
) {
    target.result.status =
        crate::sync::SyncStatus::RecoverableError(format!("staging_commit_failed: {message}"));
    target.result.error = Some(format!("staging commit failed: {message}"));

    if message.contains("manifest_read_failed:") {
        target.failure_phase = Some("merge".to_string());
        if let Some((_, path)) = message.split_once("path=") {
            let path = path
                .split(|character: char| character.is_whitespace() || character == ',')
                .next()
                .unwrap_or_default();
            if !path.is_empty() {
                target.failure_path = Some(path.to_string());
            }
        }
    }
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
    proof_intent_ids: &[String],
) -> crate::error::Result<crate::storage::transaction::SaveTransaction> {
    if plan.needs_sync_state_merge {
        let base_root = run.base_root();
        let staging_root = run.staging_root();
        let app_group = run.app_group();
        let (mut merged_state, mut merged_conflicts) =
            crate::sync::staging::sync_state_merge::merge_sync_state_three_way_for_target(
                &base_root,
                live_root,
                &staging_root,
                app_group,
            )?;
        for path in &plan.starmap_pruned_object_paths {
            merged_state.known_files.remove(path);
            merged_state.known_files_updated_at.remove(path);
            merged_state.pending_take_remote.remove(path);
            merged_state.conflicted_files.remove(path);
            merged_state
                .conflicts
                .retain(|conflict| conflict.local_path != *path);
            merged_conflicts.retain(|conflict| conflict.local_path != *path);
        }
        apply_commit_plan_with_sync_state_merge_for_group(
            live_root,
            content_actions,
            &plan.engine_state_actions,
            &merged_state,
            &merged_conflicts,
            app_group,
            backup_mode,
            proof_intent_ids,
        )
    } else {
        apply_commit_plan_to_live(
            live_root,
            content_actions,
            &plan.engine_state_actions,
            backup_mode,
            proof_intent_ids,
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
///
/// **issue #902 第十四轮复核**：对 graph 级 BothChanged 冲突
/// （`is_starmap_graph_meta_path(rel_path)` 且 kind == BothChanged），额外从
/// staging_root 读取该星图的**所有子对象文件**，保存到
/// `<live_root>/.conflict-snapshot/starmaps/<id>/<timestamp>/` 下保留原相对结构，
/// 填入 `StagingConflict.remote_graph_snapshot_dir`。`apply_starmap_graph_remote_modify`
/// 据此恢复远端整图。
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

        // graph 级 BothChanged：额外保存远端整图子对象快照。
        if crate::sync::staging::starmap_merge::is_starmap_graph_meta_path(&rel_str) {
            match save_graph_conflict_snapshot(live_root, staging_root, &rel_str, &incoming) {
                Ok(dir) => sc.remote_graph_snapshot_dir = Some(dir),
                Err(e) => {
                    let msg = format!("save_graph_conflict_snapshot failed for {}: {}", rel_str, e);
                    return Err(crate::error::Error::Other(msg));
                }
            }
        }

        // meta-only 星图冲突：保存远端 meta + index 身份事实快照
        // （issue #902 第十九轮复核问题1b）。
        if crate::sync::staging::starmap_merge::is_starmap_meta_path(&rel_str) {
            match save_meta_only_conflict_snapshot(live_root, staging_root, &rel_str) {
                Ok(dir) => sc.remote_graph_snapshot_dir = Some(dir),
                Err(e) => {
                    let msg = format!(
                        "save_meta_only_conflict_snapshot failed for {}: {}",
                        rel_str, e
                    );
                    return Err(crate::error::Error::Other(msg));
                }
            }
        }
    }
    Ok(())
}

/// 为 graph 级 BothChanged 冲突保存远端整图子对象快照（issue #902 第十四轮复核）。
///
/// 从 staging_root 读取该星图的 incoming graph.json（`incoming_graph_bytes`），
/// 解析成员表，把所有子对象文件从 staging_root 复制到
/// `<live_root>/.conflict-snapshot/starmaps/<id>/<timestamp>/` 下保留原相对结构
/// （`starmaps/<id>/<type_dir>/<bucket>/<obj_id>.json`）。
///
/// 返回快照目录相对 `live_root` 的路径（如
/// `.conflict-snapshot/starmaps/<id>/<timestamp>`）。
///
/// staging_root 中缺失的成员对象返回 `Err`——incoming graph 引用了不存在的对象
/// 是远端数据不一致，不能静默跳过。
///
/// **issue #902 第十五轮复核**：额外从 staging_root 读取 incoming
/// `starmaps/index.json`，提取该星图的 root/main 身份事实，写入快照目录下的
/// `index_facts.json`，供 `apply_starmap_graph_remote_modify` 恢复身份。
fn save_graph_conflict_snapshot(
    live_root: &Path,
    staging_root: &Path,
    graph_rel: &str,
    incoming_graph_bytes: &[u8],
) -> crate::error::Result<String> {
    use crate::starmap::package_storage::bucket_for_id;
    use crate::starmap::store::meta::GraphMeta;

    // 解析 starmap_id：starmaps/<id>/graph.json
    let normalized = graph_rel.replace('\\', "/");
    let parts: Vec<&str> = normalized.split('/').collect();
    // is_starmap_graph_meta_path 已验证 parts.len() == 3
    let starmap_id = parts[1];

    // 解析 incoming graph.json 得到成员表
    let graph: GraphMeta = serde_json::from_slice(incoming_graph_bytes).map_err(|e| {
        crate::error::Error::Other(format!(
            "save_graph_conflict_snapshot: 解析 incoming graph.json 失败 ({}): {}",
            graph_rel, e
        ))
    })?;

    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let snapshot_dir_rel = format!(".conflict-snapshot/starmaps/{}/{}", starmap_id, timestamp);

    // 收集所有成员 (type_dir, id)
    let members: Vec<(&str, &String)> = graph
        .node_ids
        .iter()
        .map(|id| ("nodes", id))
        .chain(graph.edge_ids.iter().map(|id| ("edges", id)))
        .chain(graph.embed_instance_ids.iter().map(|id| ("embeds", id)))
        .chain(graph.link_ids.iter().map(|id| ("links", id)))
        .chain(graph.hyperlink_ids.iter().map(|id| ("hyperlinks", id)))
        .collect();

    for (type_dir, id) in members {
        let bucket = bucket_for_id(id);
        let obj_rel = format!(
            "starmaps/{}/{}/{}/{}.json",
            starmap_id, type_dir, bucket, id
        );
        let staging_obj_path = staging_root.join(&obj_rel);
        if !staging_obj_path.exists() {
            return Err(crate::error::Error::Other(format!(
                "save_graph_conflict_snapshot: incoming graph 成员 {} '{}' 在 staging_root 中不存在 ({})",
                type_dir, id, obj_rel
            )));
        }
        let obj_bytes = std::fs::read(&staging_obj_path)?;
        // 保存到快照目录，保留原相对结构
        let snap_rel = format!("{}/{}", snapshot_dir_rel, obj_rel);
        let snap_path = live_root.join(&snap_rel);
        if let Some(parent) = snap_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        crate::storage::transaction::atomic_write_bytes(&snap_path, &obj_bytes).map_err(|e| {
            crate::error::Error::Io(std::io::Error::other(format!(
                "save_graph_conflict_snapshot: 写入快照对象 {} 失败: {}",
                snap_rel, e
            )))
        })?;
    }

    // issue #902 第十六轮复核：保存远端 meta.json 快照，
    // 供 apply_starmap_graph_remote_modify 恢复 meta。
    // issue #902 第十七轮复核问题4：远端整图修改冲突的快照要求 meta.json 必须存在。
    // 远端有 graph 就必须有 meta（graph 和 meta 是同一整图实体的组成部分），
    // 缺少 meta 无法完整恢复远端星图，返回明确错误而非静默跳过。
    let meta_rel = format!("starmaps/{}.meta.json", starmap_id);
    let staging_meta_path = staging_root.join(&meta_rel);
    if !staging_meta_path.exists() {
        return Err(crate::error::Error::Other(format!(
            "save_graph_conflict_snapshot: 远端整图修改冲突的快照要求 meta.json 存在 ({})，\
             缺少 meta 无法完整恢复远端星图",
            meta_rel
        )));
    }
    let meta_bytes = std::fs::read(&staging_meta_path)?;
    let snap_meta_rel = format!("{}/{}", snapshot_dir_rel, meta_rel);
    let snap_meta_path = live_root.join(&snap_meta_rel);
    if let Some(parent) = snap_meta_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::storage::transaction::atomic_write_bytes(&snap_meta_path, &meta_bytes).map_err(|e| {
        crate::error::Error::Io(std::io::Error::other(format!(
            "save_graph_conflict_snapshot: 写入快照 meta {} 失败: {}",
            snap_meta_rel, e
        )))
    })?;

    // issue #902 第十五轮复核：保存远端 index 身份事实（root/main），
    // 供 apply_starmap_graph_remote_modify 恢复 root_starmap_ids 和
    // main_starmap_by_project。issue #902 第十九轮复核问题3：staging_root
    // 中没有 index.json 时返回错误（不再静默跳过）。
    save_starmap_identity_facts(live_root, staging_root, starmap_id, &snapshot_dir_rel)?;

    Ok(snapshot_dir_rel)
}

/// 为 meta-only 星图冲突保存远端 meta + index 身份事实快照
/// （issue #902 第十九轮复核问题1b）。
///
/// meta-only 星图（无 graph.json）的 DeleteModifyConflict 以 meta.json 为锚点。
/// BothChanged（本地删除远端修改）时保存远端 meta.json 和 index 身份事实，
/// 供 apply_starmap_meta_remote_modify 恢复。不保存 graph 对象（graph 不存在）。
fn save_meta_only_conflict_snapshot(
    live_root: &Path,
    staging_root: &Path,
    meta_rel: &str,
) -> crate::error::Result<String> {
    // 解析 starmap_id：starmaps/<id>.meta.json
    let normalized = meta_rel.replace('\\', "/");
    let parts: Vec<&str> = normalized.split('/').collect();
    // is_starmap_meta_path 已验证 parts.len() == 2
    let file_name = parts[1];
    let starmap_id = &file_name[..file_name.len() - ".meta.json".len()];

    let timestamp = chrono::Utc::now().format("%Y%m%d-%H%M%S");
    let snapshot_dir_rel = format!(".conflict-snapshot/starmaps/{}/{}", starmap_id, timestamp);

    // 保存远端 meta.json 到快照目录
    let staging_meta_path = staging_root.join(meta_rel);
    if !staging_meta_path.exists() {
        return Err(crate::error::Error::Other(format!(
            "save_meta_only_conflict_snapshot: incoming meta.json 在 staging_root 中不存在 ({})",
            meta_rel
        )));
    }
    let meta_bytes = std::fs::read(&staging_meta_path)?;
    let snap_meta_rel = format!("{}/{}", snapshot_dir_rel, meta_rel);
    let snap_meta_path = live_root.join(&snap_meta_rel);
    if let Some(parent) = snap_meta_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::storage::transaction::atomic_write_bytes(&snap_meta_path, &meta_bytes).map_err(|e| {
        crate::error::Error::Io(std::io::Error::other(format!(
            "save_meta_only_conflict_snapshot: 写入快照 meta {} 失败: {}",
            snap_meta_rel, e
        )))
    })?;

    // 保存 index 身份事实（复用现有函数）
    save_starmap_identity_facts(live_root, staging_root, starmap_id, &snapshot_dir_rel)?;

    Ok(snapshot_dir_rel)
}

/// 远端星图 index 身份事实（issue #902 第十五轮复核）。
///
/// 由 `save_graph_conflict_snapshot` 从 staging_root 的 `starmaps/index.json`
/// 提取，写入快照目录下的 `index_facts.json`。`apply_starmap_graph_remote_modify`
/// 读取后恢复 `root_starmap_ids` 和 `main_starmap_by_project`。
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct StarmapIdentityFacts {
    /// 该星图是否在 `root_starmap_ids` 中。
    pub is_root: bool,
    /// 该星图作为哪些 project 的 main starmap（`main_starmap_by_project` 中
    /// value == starmap_id 的所有 key）。
    pub main_for_projects: Vec<String>,
}

/// 从 staging_root 的 `starmaps/index.json` 提取指定星图的 root/main 身份事实，
/// 写入快照目录下的 `index_facts.json`（issue #902 第十五轮复核）。
///
/// issue #902 第十九轮复核问题3：staging_root 中没有 `starmaps/index.json` 时
/// 返回 `Err`（不再静默跳过）。新生成的远端整图/meta-only 冲突快照要求 staging
/// index.json 存在以提取身份事实，否则后续 `apply_starmap_*_remote_modify` 无法
/// 恢复 root_starmap_ids 和 main_starmap_by_project，恢复的根星图会从一级入口
/// 消失。
fn save_starmap_identity_facts(
    live_root: &Path,
    staging_root: &Path,
    starmap_id: &str,
    snapshot_dir_rel: &str,
) -> crate::error::Result<()> {
    let staging_index_path = staging_root.join("starmaps").join("index.json");
    if !staging_index_path.exists() {
        return Err(crate::error::Error::Other(format!(
            "save_starmap_identity_facts: 新生成的远端整图/meta-only 冲突快照要求 \
             staging index.json 存在以提取身份事实 (starmap_id='{}', staging_root='{}')",
            starmap_id,
            staging_root.display()
        )));
    }
    let index_bytes = std::fs::read(&staging_index_path)?;
    let idx: crate::starmap::StarMapIndexRecord =
        serde_json::from_slice(&index_bytes).map_err(|e| {
            crate::error::Error::Other(format!(
                "save_starmap_identity_facts: 解析 staging index.json 失败: {}",
                e
            ))
        })?;

    let is_root = idx.root_starmap_ids.iter().any(|x| x == starmap_id);
    let main_for_projects: Vec<String> = idx
        .main_starmap_by_project
        .iter()
        .filter(|(_, v)| *v == starmap_id)
        .map(|(k, _)| k.clone())
        .collect();

    let facts = StarmapIdentityFacts {
        is_root,
        main_for_projects,
    };
    let facts_bytes = serde_json::to_vec_pretty(&facts).map_err(|e| {
        crate::error::Error::Other(format!(
            "save_starmap_identity_facts: 序列化 index_facts.json 失败: {}",
            e
        ))
    })?;

    let facts_rel = format!("{}/index_facts.json", snapshot_dir_rel);
    let facts_path = live_root.join(&facts_rel);
    if let Some(parent) = facts_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    crate::storage::transaction::atomic_write_bytes(&facts_path, &facts_bytes).map_err(|e| {
        crate::error::Error::Io(std::io::Error::other(format!(
            "save_starmap_identity_facts: 写入 index_facts.json 失败: {}",
            e
        )))
    })?;
    Ok(())
}

pub(crate) struct StagingCommitOutcome {
    pub(crate) target_results: Vec<TargetCommitResult>,
    pub(crate) target_conflicts: Vec<Vec<crate::sync::staging::StagingConflict>>,
    /// 本次 commit 真正落盘（Apply/Delete）的
    /// workspace-relative paths。供 `record_workspace_history` 精确 stage，
    /// 替代全量 `&[]` 扫描。
    pub(crate) committed_paths: Vec<CommittedPath>,
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
    let mut committed_paths: Vec<CommittedPath> = Vec::new();

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
                    &[],
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
                            &[],
                            &mut committed_paths,
                        );
                        collect_rebased_anchor_paths(
                            &tx,
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
                let tx = match apply_commit_plan_with_optional_merge(
                    run,
                    live_root,
                    &plan,
                    &safe_content_actions,
                    false,
                    &[],
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
                // ConflictMetadataOnly 也落盘了
                // safe_content_actions + engine_state_actions，收集它们的 rel_path。
                //   committed_paths 只收集
                // safe_content_actions，不收集 engine_state_actions。
                let (kind, pid) = transfer_targets
                    .get(idx)
                    .map(|t| (t.target_kind.as_str(), t.project_id.as_deref()))
                    .unwrap_or(("", None));
                collect_action_paths(kind, pid, &safe_content_actions, &[], &mut committed_paths);
                collect_rebased_anchor_paths(
                    &tx,
                    kind,
                    pid,
                    &safe_content_actions,
                    &mut committed_paths,
                );
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
                    &[],
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
                            &[],
                            &mut committed_paths,
                        );
                        collect_rebased_anchor_paths(
                            &tx,
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
/// - **输出是 `(TargetCommitResult, Vec<StagingConflict>, Vec<CommittedPath>)`**：
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
    intent_sink: Option<&crate::sync::proof_intent::ProofIntentSink<'_>>,
) -> (
    TargetCommitResult,
    Vec<crate::sync::staging::StagingConflict>,
    Vec<CommittedPath>,
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
            let before = capture_action_before(live_root, &plan.content_actions);

            let proof_intent_ids = match intent_sink {
                Some(sink) => match sink.prepare(
                    run.run_id(),
                    target.target_kind.as_str(),
                    target.project_id.as_deref(),
                    &plan.content_actions,
                    &before,
                ) {
                    Ok(ids) => ids,
                    Err(error) => {
                        let msg = format!("prepare proof intent failed: {error}");
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        return (TargetCommitResult::Failed(msg), Vec::new(), Vec::new());
                    }
                },
                None => Vec::new(),
            };

            let mut tx = match apply_commit_plan_with_optional_merge(
                run,
                live_root,
                &plan,
                &plan.content_actions,
                false,
                &proof_intent_ids,
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
                        &before,
                        &mut committed_paths,
                    );
                    collect_rebased_anchor_paths(
                        &tx,
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
            let before = capture_action_before(live_root, &safe_content_actions);
            let proof_intent_ids = match intent_sink {
                Some(sink) => match sink.prepare(
                    run.run_id(),
                    target.target_kind.as_str(),
                    target.project_id.as_deref(),
                    &safe_content_actions,
                    &before,
                ) {
                    Ok(ids) => ids,
                    Err(error) => {
                        let msg = format!("prepare proof intent failed: {error}");
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        return (TargetCommitResult::Failed(msg), Vec::new(), Vec::new());
                    }
                },
                None => Vec::new(),
            };
            let tx = match apply_commit_plan_with_optional_merge(
                run,
                live_root,
                &plan,
                &safe_content_actions,
                false,
                &proof_intent_ids,
            ) {
                Ok(tx) => tx,
                Err(e) => {
                    let msg = format!("apply_commit_plan failed: {}", e);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    return (TargetCommitResult::Failed(msg), Vec::new(), Vec::new());
                }
            };
            let mut committed_paths = Vec::new();
            collect_action_paths(
                target.target_kind.as_str(),
                target.project_id.as_deref(),
                &safe_content_actions,
                &before,
                &mut committed_paths,
            );
            collect_rebased_anchor_paths(
                &tx,
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

            let before = capture_action_before(live_root, &plan.content_actions);
            let proof_intent_ids = match intent_sink {
                Some(sink) => match sink.prepare(
                    run.run_id(),
                    target.target_kind.as_str(),
                    target.project_id.as_deref(),
                    &plan.content_actions,
                    &before,
                ) {
                    Ok(ids) => ids,
                    Err(error) => {
                        let msg = format!("prepare proof intent failed: {error}");
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        return (TargetCommitResult::Failed(msg), Vec::new(), Vec::new());
                    }
                },
                None => Vec::new(),
            };
            let mut tx = match apply_commit_plan_to_live(
                live_root,
                &plan.content_actions,
                &plan.engine_state_actions,
                false,
                &proof_intent_ids,
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
                        &before,
                        &mut committed_paths,
                    );
                    collect_rebased_anchor_paths(
                        &tx,
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
mod tests;
