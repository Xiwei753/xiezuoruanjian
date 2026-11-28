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

/// 同步 durable commit 实际写入 `correction/results/<chapter>/` 后，显式失效
/// 该章的校对结果索引；目录只读路径仍走 O(1) mtime 检查，不逐文件探测。
fn invalidate_correction_result_caches(
    app_data_root: &Path,
    live_root: &Path,
    project_id: Option<&str>,
    actions: &[crate::sync::staging::CommitAction],
) {
    let project_id = project_id
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .or_else(|| {
            live_root
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        });
    let Some(project_id) = project_id else {
        return;
    };
    let mut chapters = std::collections::HashSet::new();
    for action in actions {
        let rel_path = match action {
            crate::sync::staging::CommitAction::Apply { rel_path, .. } => rel_path,
            crate::sync::staging::CommitAction::Delete { rel_path } => rel_path,
        };
        let normalized = rel_path.to_string_lossy().replace('\\', "/");
        let Some(rest) = normalized.strip_prefix("correction/results/") else {
            continue;
        };
        let Some(chapter_id) = rest.split('/').next() else {
            continue;
        };
        if chapter_id.is_empty()
            || chapter_id == "."
            || chapter_id == ".."
            || chapter_id.contains('\\')
        {
            continue;
        }
        chapters.insert(chapter_id.to_string());
    }
    for chapter_id in chapters {
        crate::correction::invalidate_synced_results(app_data_root, &project_id, &chapter_id);
    }
}

/// 章节正文在 commit 前的真实快照。章节回执必须用这个快照作为 before，
/// 不能等应用后再从 live 倒推，也不能把“文件不存在”当成空正文。
#[derive(Debug, Clone)]
pub(crate) enum CommittedBefore {
    /// 非章节正文路径，不需要正文快照。
    NotTracked,
    /// 章节正文在 commit 前不存在（首次导入/删除已不存在）。
    Missing,
    /// commit 前的原始字节与版本快照（空文件也是空 Vec）。
    Text {
        bytes: Vec<u8>,
        version: Option<crate::chapter::ChapterVersion>,
    },
    /// 读取失败，回执必须明确报错而不是伪造 before。
    Error(String),
}

#[derive(Debug, Clone)]
pub(crate) struct CommittedPath {
    pub path: PathBuf,
    pub deleted: bool,
    pub before: CommittedBefore,
}

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
                Ok(bytes) => {
                    let version = chapter_version_at(live_root, rel, &bytes);
                    CommittedBefore::Text { bytes, version }
                }
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

/// 从 workspace-relative 路径解析章节身份。
fn chapter_key_from_workspace_path(path: &Path) -> Option<crate::chapter::ChapterKey> {
    let components: Vec<&str> = path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect();
    let volumes = components.iter().position(|part| *part == "volumes")?;
    if components.get(volumes + 2) != Some(&"chapters") {
        return None;
    }
    let volume_id = *components.get(volumes + 1)?;
    let chapter_id = *components.get(volumes + 3)?;
    let project_id = if volumes >= 2 && components[volumes - 2] == "projects" {
        components[volumes - 1]
    } else {
        components.first().copied().unwrap_or_default()
    };
    Some(crate::chapter::ChapterKey::new(
        project_id, volume_id, chapter_id,
    ))
}

fn content_version(text: &str) -> crate::chapter::ChapterVersion {
    crate::chapter::ChapterVersion {
        object_generation: String::new(),
        revision: 0,
        content_sha256: {
            use sha2::Digest;
            format!("{:x}", sha2::Sha256::digest(text.as_bytes()))
        },
    }
}

/// 把本次 commit 的章节正文路径转成按章节的 change receipt。
///
/// before 来自 commit 前快照，after 读提交后的 live 正文；删除只记录
/// before + `deleted=true`。平台端据此只重载真正变化的章节。
pub(crate) fn build_chapter_change_receipts(
    live_root: &Path,
    paths: &[CommittedPath],
    operation_id: &str,
) -> Vec<crate::chapter::ChapterChangeReceipt> {
    let mut receipts = Vec::new();
    for committed in paths {
        if !is_chapter_content_path(&committed.path) {
            continue;
        }
        let Some(key) = chapter_key_from_workspace_path(&committed.path) else {
            continue;
        };
        let before = match &committed.before {
            CommittedBefore::Text { version, bytes } => version
                .clone()
                .or_else(|| Some(content_version(&String::from_utf8_lossy(bytes)))),
            CommittedBefore::Missing | CommittedBefore::NotTracked | CommittedBefore::Error(_) => {
                None
            }
        };
        let after = if committed.deleted {
            None
        } else {
            match std::fs::read(live_root.join(&committed.path)) {
                Ok(bytes) => chapter_version_at(live_root, &committed.path, &bytes),
                Err(error) => {
                    log::warn!(
                        "chapter change receipt read after commit failed: {}: {}",
                        committed.path.display(),
                        error
                    );
                    None
                }
            }
        };
        let content_changed = committed.deleted
            || before.as_ref().map(|version| &version.content_sha256)
                != after.as_ref().map(|version| &version.content_sha256);
        receipts.push(crate::chapter::ChapterChangeReceipt {
            key,
            before,
            after,
            content_changed,
            deleted: committed.deleted,
            operation_id: operation_id.to_string(),
        });
    }
    for committed in paths {
        if is_chapter_content_path(&committed.path) {
            continue;
        }
        if !is_chapter_meta_path(&committed.path) {
            continue;
        }
        let Some(key) = chapter_key_from_workspace_path(&committed.path) else {
            continue;
        };
        if receipts.iter().any(|receipt| receipt.key == key) {
            continue;
        }
        let before = match &committed.before {
            CommittedBefore::Text { version, bytes } => version
                .clone()
                .or_else(|| Some(content_version(&String::from_utf8_lossy(bytes)))),
            CommittedBefore::Missing | CommittedBefore::NotTracked | CommittedBefore::Error(_) => {
                None
            }
        };
        let after = if committed.deleted {
            None
        } else {
            let body_path = committed.path.with_file_name("chapter.md");
            match std::fs::read(live_root.join(&body_path)) {
                Ok(bytes) => chapter_version_at(live_root, &body_path, &bytes),
                Err(_) => None,
            }
        };
        receipts.push(crate::chapter::ChapterChangeReceipt {
            key,
            before,
            after,
            // 只有 meta 变化时不改正文；still 需要回执让编辑器协调标题/顺序。
            content_changed: false,
            deleted: false,
            operation_id: operation_id.to_string(),
        });
    }
    receipts
}

/// 读取章节正文同一时刻的 meta 版本；缺失/损坏时返回 None，调用方保留
/// 只有正文摘要的旧行为，不伪造 generation/revision。
fn chapter_version_at(
    live_root: &Path,
    body_rel: &Path,
    body: &[u8],
) -> Option<crate::chapter::ChapterVersion> {
    let meta_path = body_rel.with_file_name("chapter.meta.json");
    let meta: crate::chapter::Chapter =
        serde_json::from_str(&std::fs::read_to_string(live_root.join(meta_path)).ok()?).ok()?;
    Some(crate::chapter::ChapterVersion {
        object_generation: meta.object_generation,
        revision: meta.revision,
        content_sha256: {
            use sha2::Digest;
            format!("{:x}", sha2::Sha256::digest(body))
        },
    })
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

fn is_chapter_meta_path(path: &Path) -> bool {
    let mut has_chapters = false;
    let mut last = None;
    for component in path.components() {
        let part = component.as_os_str();
        if part == "chapters" {
            has_chapters = true;
        }
        last = Some(part);
    }
    has_chapters && last.is_some_and(|name| name == "chapter.meta.json")
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
fn rebase_chapter_anchors(
    live_root: &Path,
    content_actions: &[crate::sync::staging::CommitAction],
) -> crate::error::Result<Vec<crate::sync::staging::CommitAction>> {
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
    if rebased_anchors.is_empty() {
        return Ok(Vec::new());
    }
    let updated_at_ms = chrono::Utc::now()
        .timestamp_millis()
        .max(newest_existing_version.saturating_add(1));
    let mut actions = Vec::with_capacity(rebased_anchors.len());
    for (rel_path, mut anchor) in rebased_anchors {
        anchor.updated_at_ms = updated_at_ms;
        actions.push(CommitAction::Apply {
            rel_path,
            content: serde_json::to_vec_pretty(&anchor)?,
        });
    }
    Ok(actions)
}

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

/// 统一对象比较产出的提交：内容、对象索引与已确认基线一次原子落盘。
///
/// 与旧 [`apply_commit_plan_with_optional_merge`] 的区别：
/// - 不再做 state.local.json 三方语义合并：`known_files` 只在这里按"实际应用了
///   远端内容/删除"推进；
/// - 本地上传/本地删除不推进基线（要等远端发布回执确认），因此不会出现
///   "本地内容冒充已同步"的旧问题；
/// - 冲突路径保留共同基线，由冲突记录驱动用户选择；
/// - 正文仍走既有章节提交入口（journal / 证明链），不绕过业务事务。
/// 把统一裁决给出的已确认基线推进到 state（内容摘要 / 删除事实）。
///
/// 与正文事务同一次提交：章节正文由章节事务落盘后，这里只应用同一份
/// `plan.confirmations`，不再按 content_actions 再猜一遍。
fn apply_baseline_updates(
    state: &mut crate::sync::types::SyncState,
    updates: &[crate::sync::staging::BaselineUpdate],
) {
    let now_ms = chrono::Utc::now().timestamp_millis();
    for update in updates {
        let path = update.rel_path.to_string_lossy().replace('\\', "/");
        match &update.content_hash {
            Some(hash) => {
                state.known_files.insert(path.clone(), hash.clone());
                state.known_files_updated_at.insert(path.clone(), now_ms);
                state.deleted_files.remove(&path);
                state.conflicted_files.remove(&path);
                state.pending_take_remote.remove(&path);
                state.tombstones.retain(|tombstone| tombstone.original_path != path);
            }
            None => {
                state.known_files.remove(&path);
                state.known_files_updated_at.remove(&path);
                state.deleted_files.insert(path.clone());
                state.conflicted_files.remove(&path);
                state.pending_take_remote.remove(&path);
                state
                    .tombstones
                    .retain(|tombstone| tombstone.original_path != path);
            }
        }
    }
}

pub(crate) fn commit_object_plan(
    run: &crate::sync::staging::StagingRun,
    plan: &crate::sync::staging::CommitPlan,
    content_actions: &[crate::sync::staging::CommitAction],
    backup_mode: bool,
    chapter_receipts: &mut Vec<crate::chapter::ChapterSyncCommitReceipt>,
) -> crate::error::Result<crate::storage::transaction::SaveTransaction> {
    use crate::sync::staging::CommitAction;

    let live_root = run.target_live_root();
    let workspace_lock = crate::storage::WorkspaceWriteLock::acquire(run.app_data_root())?;

    let mut content_actions = content_actions.to_vec();
    let default_project_id = live_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let chapter_sources = staging_chapter_sources(run, Some(default_project_id));
    let (handled_chapters, chapter_result) = commit_synced_chapter_bodies(
        live_root,
        &workspace_lock,
        &content_actions,
        run.run_id(),
        run.app_data_root(),
        &run.base_root(),
        &chapter_sources,
        chapter_receipts,
    );
    chapter_result?;
    if !handled_chapters.is_empty() {
        content_actions.retain(|action| {
            let rel = match action {
                CommitAction::Apply { rel_path, .. } | CommitAction::Delete { rel_path } => rel_path,
            };
            !handled_chapters.contains(rel)
        });
    }
    let mut rebased_anchors = rebase_chapter_anchors(live_root, &content_actions)?;
    content_actions.append(&mut rebased_anchors);

    let mut tx = crate::storage::transaction::SaveTransaction::new(live_root);
    if backup_mode {
        tx.enable_backup_mode();
    }
    for action in &plan.engine_state_actions {
        match action {
            CommitAction::Apply { rel_path, content } => {
                let rel = rel_path.to_string_lossy();
                tx.add_bytes(&rel, content)?;
            }
            CommitAction::Delete { rel_path } => {
                let rel = rel_path.to_string_lossy();
                tx.add_delete(&rel)?;
            }
        }
    }
    for action in &content_actions {
        match action {
            CommitAction::Apply { rel_path, content } => {
                let rel = rel_path.to_string_lossy();
                tx.add_bytes(&rel, content)?;
            }
            CommitAction::Delete { rel_path } => {
                let rel = rel_path.to_string_lossy();
                tx.add_delete(&rel)?;
            }
        }
    }

    let mut state = match run.app_group() {
        Some(group) => crate::sync::SyncService::load_sync_state_read_only_for_group(
            live_root, group, None,
        )?,
        None => crate::sync::SyncService::load_sync_state_read_only(live_root, None)?,
    };
    apply_baseline_updates(&mut state, &plan.confirmations);
    let state_rel = run
        .app_group()
        .map(crate::sync::types::AppDataGroup::local_state_path)
        .unwrap_or_else(|| "app-meta/sync/state.local.json".to_string());
    let state_bytes = serde_json::to_vec_pretty(&state)
        .map_err(|error| crate::Error::Io(std::io::Error::other(error.to_string())))?;
    tx.add_bytes(&state_rel, &state_bytes)?;
    tx.commit()?;
    Ok(tx)
}

/// 按发布回执确认本轮真正的远端事实（Issue #1066）。
///
/// 只有 flush 实际写出的对象回执才能推进 `known_files`：本地上传在远端确认前
/// 仍是"未确认事实"，失败重试时不会被误当成已同步。远端采用/删除已在提交阶段
/// 推进过基线，这里重复确认是幂等的。
///
/// 返回无法确认的决议数量（调用方据此上报未结算）。
pub(crate) fn confirm_published_objects(
    sync_root: &Path,
    target: &crate::sync::types::SyncTarget,
    decisions: &[(String, crate::sync::decision::DurableObjectDecision)],
    receipts: &[crate::sync::provider::round::PublishReceipt],
) -> crate::Result<usize> {
    use crate::sync::decision::DecisionOp;

    let prefix = format!("{}/", target.remote_prefix.trim_end_matches('/'));
    let mut receipt_by_path: std::collections::HashMap<
        String,
        &crate::sync::provider::round::PublishReceipt,
    > = std::collections::HashMap::new();
    for receipt in receipts {
        receipt_by_path.insert(receipt.path.clone(), receipt);
    }

    // 只有本轮真正允许发布的对象才需要回执确认；未决冲突/纯远端消费的
    // 决议不进入远端批次，不能因为它们没有回执就报"未结算"。
    let relevant: Vec<(&str, &crate::sync::decision::DurableObjectDecision)> = decisions
        .iter()
        .filter(|(_, decision)| decision.is_publishable())
        .filter_map(|(remote_path, decision)| {
            remote_path
                .strip_prefix(&prefix)
                .map(|rel| (rel, decision))
        })
        .collect();
    if relevant.is_empty() {
        return Ok(0);
    }

    let mut state =
        crate::sync::SyncService::load_sync_state_for_target(sync_root, target)?;
    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut unconfirmed = 0usize;
    let mut confirmed_any = false;
    for (rel, decision) in relevant {
        let remote_path = format!("{prefix}{rel}");
        let receipt = receipt_by_path.get(&remote_path);
        let confirmed = match decision.op {
            DecisionOp::Upsert => receipt.is_some_and(|receipt| {
                receipt.op == "upsert"
                    && receipt.content_digest.as_deref() == Some(decision.final_hash.as_str())
            }),
            DecisionOp::Delete => receipt.is_some_and(|receipt| receipt.op == "delete"),
        };
        if !confirmed {
            unconfirmed += 1;
            continue;
        }
        confirmed_any = true;
        match decision.op {
            DecisionOp::Upsert => {
                state
                    .known_files
                    .insert(rel.to_string(), decision.final_hash.clone());
                state.known_files_updated_at.insert(rel.to_string(), now_ms);
                state.deleted_files.remove(rel);
                state.conflicted_files.remove(rel);
                state.pending_take_remote.remove(rel);
                state
                    .tombstones
                    .retain(|tombstone| tombstone.original_path != rel);
            }
            DecisionOp::Delete => {
                state.known_files.remove(rel);
                state.known_files_updated_at.remove(rel);
                state.deleted_files.insert(rel.to_string());
                state.conflicted_files.remove(rel);
                state.pending_take_remote.remove(rel);
                state
                    .tombstones
                    .retain(|tombstone| tombstone.original_path != rel);
            }
        }
    }
    if confirmed_any {
        crate::sync::SyncService::save_sync_state_for_target(sync_root, target, &state)?;
    }
    Ok(unconfirmed)
}

/// 章节正文协调 + 一次 SaveTransaction 落盘（Issue #1066）。
///
/// `content_actions` 由调用方过滤（`ConflictMetadataOnly` 会过滤掉 transfer
/// 冲突路径），`engine_state_actions` 直接用 `plan.engine_state_actions`。

/// staging manifest 中章节正文的来源设备与原始发生时间。事件 ID 和回执必须
/// 复用这份事实，不能在提交时重新取本机时间或空设备。
type ChapterSourceMap = std::collections::HashMap<(String, String, String), (String, i64)>;

fn staging_chapter_sources(
    run: &crate::sync::staging::StagingRun,
    default_project_id: Option<&str>,
) -> ChapterSourceMap {
    // 清单权威在 v2 store（staging root 下）；单文件只是 prepare 时物化的
    // 只读视图，合并后可能已过期。先读 store，没有 root 才回退单文件。
    let staging_root = run.staging_root();
    let manifest =
        crate::sync::lww::ManifestStore::new(&staging_root, crate::sync::lww::SYNC_MANIFEST_PATH)
            .load_all()
            .ok()
            .flatten()
            .or_else(|| {
                let text = std::fs::read_to_string(
                    staging_root.join(crate::sync::lww::SYNC_MANIFEST_PATH),
                )
                .ok()?;
                serde_json::from_str::<crate::sync::types::SyncManifest>(&text).ok()
            });
    let Some(manifest) = manifest else {
        return ChapterSourceMap::new();
    };
    let mut out = ChapterSourceMap::new();
    for record in manifest.files {
        if record.device_id.is_empty() {
            continue;
        }
        let Some(key) = chapter_key_from_workspace_path(Path::new(&record.path)) else {
            continue;
        };
        if !record.path.ends_with("chapter.md") {
            continue;
        }
        let project_id = if key.project_id.is_empty()
            || Path::new(&record.path)
                .components()
                .next()
                .is_some_and(|part| part.as_os_str() == "volumes")
        {
            default_project_id.unwrap_or_default().to_string()
        } else {
            key.project_id
        };
        out.entry((project_id, key.volume_id, key.chapter_id))
            .or_insert((record.device_id, record.updated_at_ms));
    }
    out
}

fn sync_chapter_event_id(
    run_id: &str,
    key: &crate::chapter::ChapterKey,
    mutation: crate::chapter::ChapterMutationKind,
) -> String {
    let suffix = match mutation {
        crate::chapter::ChapterMutationKind::Create => "create",
        crate::chapter::ChapterMutationKind::Delete => "delete",
        crate::chapter::ChapterMutationKind::SyncMerge => "sync_merge",
        crate::chapter::ChapterMutationKind::Replace => "replace",
        crate::chapter::ChapterMutationKind::Clear => "clear",
        crate::chapter::ChapterMutationKind::EditBatch => "edit_batch",
        crate::chapter::ChapterMutationKind::Restore => "restore",
    };
    format!(
        "sync:{run_id}:{}/{}/{}:{suffix}",
        key.project_id, key.volume_id, key.chapter_id
    )
}

fn build_sync_commit_receipt(
    receipt: &crate::chapter::ChapterCommitReceipt,
    fact: Option<&crate::chapter::ChapterSyncProofFact>,
) -> crate::chapter::ChapterSyncCommitReceipt {
    crate::chapter::ChapterSyncCommitReceipt {
        transaction_id: receipt.transaction_id.clone(),
        key: receipt.key.clone(),
        event_id: fact.map(|fact| fact.event_id.clone()),
        source_device_id: fact
            .map(|fact| fact.source_device_id.clone())
            .unwrap_or_default(),
        occurred_at_ms: fact.map_or(0, |fact| fact.occurred_at_ms),
        before_text: fact.and_then(|fact| fact.before_text.clone()),
        after_text: fact.and_then(|fact| fact.after_text.clone()),
        deleted: fact.is_some_and(|fact| fact.after_text.is_none()),
    }
}

fn chapter_project_root(root: &Path, key: &crate::chapter::ChapterKey) -> PathBuf {
    if root.join("projects").is_dir() {
        root.join("projects").join(&key.project_id)
    } else {
        root.to_path_buf()
    }
}

fn chapter_target_rel_path(
    target_root: &Path,
    key: &crate::chapter::ChapterKey,
    meta: bool,
) -> PathBuf {
    let relative = if meta {
        PathBuf::from("volumes")
            .join(&key.volume_id)
            .join("chapters")
            .join(&key.chapter_id)
            .join("chapter.meta.json")
    } else {
        PathBuf::from("volumes")
            .join(&key.volume_id)
            .join("chapters")
            .join(&key.chapter_id)
            .join("chapter.md")
    };
    if target_root.join("projects").is_dir() {
        PathBuf::from("projects")
            .join(&key.project_id)
            .join(relative)
    } else {
        relative
    }
}

/// 先通过章节协调器提交现有章节正文，返回已处理的 target-relative 路径。
///
/// 章节正文与 `chapter.meta.json` 不能继续由通用 SaveTransaction 直接写：
/// 否则同步会绕过 generation/revision/digest CAS，把旧快照盖回新正文。这里
/// 在已持有的 workspace guard 下重新读取当前版本，以调用方刚读到的版本作
/// expected（防止读后到提交前又被编辑），正文和 meta 一起由 journal 决议。
/// 新章节（live 尚不存在）仍留给通用事务，直到协调器支持 Create。
fn commit_synced_chapter_bodies(
    live_root: &Path,
    guard: &crate::storage::WorkspaceWriteLock,
    content_actions: &[crate::sync::staging::CommitAction],
    run_id: &str,
    app_data_root: &Path,
    base_root: &Path,
    chapter_sources: &ChapterSourceMap,
    receipts: &mut Vec<crate::chapter::ChapterSyncCommitReceipt>,
) -> (std::collections::HashSet<PathBuf>, crate::error::Result<()>) {
    use super::chapter_mutation_plan::{plan_synced_chapter_mutations, SyncedChapterMutation};

    let mut handled = std::collections::HashSet::new();
    let project_id = live_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string();
    if project_id.is_empty() {
        return (
            handled,
            Err(crate::error::Error::Other(
                "sync chapter commit cannot derive project id from live root".to_string(),
            )),
        );
    }

    let result: crate::error::Result<()> = (|| {
        for change in plan_synced_chapter_mutations(&project_id, content_actions)? {
            let chapter_root = chapter_project_root(live_root, &change.key);
            let base_chapter_root = chapter_project_root(base_root, &change.key);
            let body_rel = chapter_target_rel_path(live_root, &change.key, false);
            let meta_rel = chapter_target_rel_path(live_root, &change.key, true);
            if change.mutation == SyncedChapterMutation::Delete {
                let base = crate::chapter::commit::read_chapter_state(
                    &base_chapter_root,
                    &change.key.volume_id,
                    &change.key.chapter_id,
                );
                let live = crate::chapter::commit::read_chapter_state_recovering(
                    &chapter_root,
                    guard,
                    &change.key.volume_id,
                    &change.key.chapter_id,
                );
                match (base, live) {
                    (
                        Err(crate::error::Error::ChapterNotFound),
                        Err(crate::error::Error::ChapterNotFound),
                    ) => {
                        // base 与 live 都不存在：删除已幂等完成。
                        handled.insert(body_rel);
                        handled.insert(meta_rel);
                        continue;
                    }
                    (Err(crate::error::Error::ChapterNotFound), Ok(_)) => {
                        return Err(crate::error::Error::Other(format!(
                            "sync delete chapter created after base: {}/{}",
                            change.key.volume_id, change.key.chapter_id
                        )));
                    }
                    (Ok((_, base_text, base_version)), Ok((_, live_text, live_version))) => {
                        let expected = if base_version.object_generation.is_empty()
                            || base_version.content_sha256.is_empty()
                        {
                            if base_text != live_text
                                || base_version.revision != live_version.revision
                            {
                                return Err(crate::error::Error::Other(format!(
                                    "sync delete legacy base changed: {}/{}",
                                    change.key.volume_id, change.key.chapter_id
                                )));
                            }
                            live_version
                        } else {
                            base_version
                        };
                        let mut request =
                            crate::chapter::ChapterCommitRequest::new(change.key.clone(), "");
                        request.expected = Some(expected);
                        request.mutation = crate::chapter::ChapterMutationKind::Delete;
                        request.source = crate::chapter::ChapterCommitSource::Sync;
                        request.allow_empty_overwrite = true;
                        request.app_data_root = Some(app_data_root.to_path_buf());
                        let (source_device_id, occurred_at_ms) = chapter_sources
                            .get(&(
                                change.key.project_id.clone(),
                                change.key.volume_id.clone(),
                                change.key.chapter_id.clone(),
                            ))
                            .cloned()
                            .unwrap_or_else(|| {
                                (String::new(), chrono::Utc::now().timestamp_millis())
                            });
                        request
                            .sync_proof_facts
                            .push(crate::chapter::ChapterSyncProofFact {
                                event_id: sync_chapter_event_id(
                                    run_id,
                                    &change.key,
                                    crate::chapter::ChapterMutationKind::Delete,
                                ),
                                key: request.key.clone(),
                                before_text: Some(base_text),
                                after_text: None,
                                source_device_id,
                                occurred_at_ms,
                            });
                        let receipt_fact = request.sync_proof_facts.first().cloned();
                        match crate::chapter::commit::commit_chapter_with_guard(
                            &chapter_root,
                            Some(guard),
                            request,
                        )? {
                            crate::chapter::ChapterCommitOutcome::Committed(receipt) => {
                                receipts.push(build_sync_commit_receipt(
                                    &receipt,
                                    receipt_fact.as_ref(),
                                ));
                                handled.insert(body_rel);
                                handled.insert(meta_rel);
                            }
                            crate::chapter::ChapterCommitOutcome::Conflict(conflict) => {
                                return Err(crate::error::Error::Other(format!(
                                    "sync delete conflicted for {}/{}: actual={:?}, dir={}",
                                    change.key.volume_id,
                                    change.key.chapter_id,
                                    conflict.actual,
                                    conflict.conflict_dir
                                )));
                            }
                        }
                        continue;
                    }
                    (Ok(_), Err(crate::error::Error::ChapterNotFound)) => {
                        // live 已被删除（可能是外部）；不再重复发布。
                        handled.insert(body_rel);
                        handled.insert(meta_rel);
                        continue;
                    }
                    (Err(error), _) => return Err(error),
                    (_, Err(error)) => return Err(error),
                }
            }

            let current = crate::chapter::commit::read_chapter_state_recovering(
                &chapter_root,
                guard,
                &change.key.volume_id,
                &change.key.chapter_id,
            );
            let (current_text, current_version) = match current {
                Ok((_, text, version)) => (text, Some(version)),
                Err(crate::error::Error::ChapterNotFound) => {
                    if change.mutation == SyncedChapterMutation::Meta {
                        // 只有 meta 的新章节无法证明正文来源；不能退回通用
                        // SaveTransaction 留下无正文的章节。
                        return Err(crate::error::Error::Other(format!(
                            "sync chapter meta without body: volumes/{}/chapters/{}",
                            change.key.volume_id, change.key.chapter_id
                        )));
                    }
                    (String::new(), None)
                }
                Err(error) => return Err(error),
            };

            let mut request = crate::chapter::ChapterCommitRequest::new(
                change.key.clone(),
                change
                    .body
                    .as_deref()
                    .filter(|_| change.mutation != SyncedChapterMutation::Meta)
                    .unwrap_or(&current_text),
            );
            if let Some(meta) = change.meta.as_ref() {
                request.meta_title = Some(meta.title.clone());
                request.meta_order = Some(meta.order);
            }
            request.source = crate::chapter::ChapterCommitSource::Sync;
            request.allow_empty_overwrite = request.content.is_empty();
            let (source_device_id, occurred_at_ms) = chapter_sources
                .get(&(
                    change.key.project_id.clone(),
                    change.key.volume_id.clone(),
                    change.key.chapter_id.clone(),
                ))
                .cloned()
                .unwrap_or_else(|| (String::new(), chrono::Utc::now().timestamp_millis()));
            match current_version {
                None => {
                    let Some(meta_template) = change.meta.clone() else {
                        return Err(crate::error::Error::Other(format!(
                            "sync chapter create without meta: volumes/{}/chapters/{}",
                            change.key.volume_id, change.key.chapter_id
                        )));
                    };
                    request.mutation = crate::chapter::ChapterMutationKind::Create;
                    request.meta_template = Some(meta_template);
                    request
                        .sync_proof_facts
                        .push(crate::chapter::ChapterSyncProofFact {
                            event_id: sync_chapter_event_id(
                                run_id,
                                &change.key,
                                crate::chapter::ChapterMutationKind::Create,
                            ),
                            key: request.key.clone(),
                            before_text: None,
                            after_text: Some(request.content.clone()),
                            source_device_id,
                            occurred_at_ms,
                        });
                }
                Some(version) => {
                    request.expected = Some(version);
                    request.base_content = Some(current_text.clone());
                    request.mutation = crate::chapter::ChapterMutationKind::SyncMerge;
                    if change.mutation != SyncedChapterMutation::Meta {
                        request
                            .sync_proof_facts
                            .push(crate::chapter::ChapterSyncProofFact {
                                event_id: sync_chapter_event_id(
                                    run_id,
                                    &change.key,
                                    request.mutation,
                                ),
                                key: request.key.clone(),
                                before_text: Some(current_text.clone()),
                                after_text: Some(request.content.clone()),
                                source_device_id,
                                occurred_at_ms,
                            });
                    }
                }
            }

            let receipt_fact = request.sync_proof_facts.first().cloned();
            match crate::chapter::commit::commit_chapter_with_guard(
                &chapter_root,
                Some(guard),
                request,
            )? {
                crate::chapter::ChapterCommitOutcome::Committed(receipt) => {
                    receipts.push(build_sync_commit_receipt(&receipt, receipt_fact.as_ref()));
                    handled.insert(body_rel);
                    handled.insert(meta_rel);
                }
                crate::chapter::ChapterCommitOutcome::Conflict(conflict) => {
                    return Err(crate::error::Error::Other(format!(
                        "sync chapter commit conflicted for {}/{}: actual={:?}",
                        change.key.volume_id, change.key.chapter_id, conflict.actual
                    )));
                }
            }
        }
        Ok(())
    })();
    (handled, result)
}

/// 在 staging cleanup 前，对 plan.conflict 的每个 StagingConflict 读取 incoming
/// 正文（staging_root/rel_path），用 save_conflict_copy 保存到 live target 的冲突快照，
/// 填入 StagingConflict.remote_snapshot_path。这样 record_staging_conflicts 映射成
/// SyncConflict 时能带上远端 snapshot，#757 冲突侧栏"用户看到什么就选择什么"。
///
/// 必须在 `run.cleanup()` 之前调用——cleanup 会删除 staging_root，之后读不到 incoming。
/// 已填快照（`remote_snapshot_path.is_some()`）的条目跳过，避免重复保存。
///
/// Issue #1066：废弃整图冲突副本协议，冲突只按对象保存远端内容快照。
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

pub(crate) enum TargetCommitResult {
    Ok,
    Skipped,
    Failed(String),
}

/// 把 staging commit 失败注入 target 结果。
///
/// 清单读取/解析失败带 `manifest_read_failed: path=... cause=...`；保留具体
/// 路径并归到 merge 阶段，避免把损坏的清单误报成传输失败。
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

pub(crate) enum TargetCommitMode {
    Full,
    ConflictMetadataOnly,
    Skip,
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

/// 单 target staging commit：Full 与 ConflictMetadataOnly 共用同一套对象提交。
///
/// 只有 `SaveTransaction::finish()` 成功后，才把本轮真实成功的上传对象、
/// 远端删除与最终对象索引冻结成 [`crate::sync::staging::PublishBatch`]；
/// 失败 target 不产生任何发布事实。
#[allow(
    clippy::excessive_nesting,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]
pub(crate) fn commit_single_target_staging(
    run: &crate::sync::staging::StagingRun,
    target: &crate::sync::types::TargetSyncResult,
    chapter_receipts: &mut Vec<crate::chapter::ChapterSyncCommitReceipt>,
    publish: &mut crate::sync::staging::PublishBatch,
) -> (
    TargetCommitResult,
    Vec<crate::sync::staging::StagingConflict>,
    Vec<CommittedPath>,
) {
    let mode = target_commit_mode(&target.result.status);

    // 直接调用本入口的提交也必须遵守与 MCP 共享的工作区锁约定。
    let _workspace_lock = if matches!(mode, TargetCommitMode::Skip) {
        None
    } else {
        match crate::storage::WorkspaceWriteLock::acquire(run.app_data_root()) {
            Ok(lock) => Some(lock),
            Err(error) => {
                return (
                    TargetCommitResult::Failed(format!("workspace_write_lock_failed: {error}")),
                    Vec::new(),
                    Vec::new(),
                );
            }
        }
    };

    match mode {
        TargetCommitMode::Skip => {
            log::warn!("Staging commit: skipping target (run_id={})", run.run_id());
            (TargetCommitResult::Skipped, Vec::new(), Vec::new())
        }
        TargetCommitMode::Full | TargetCommitMode::ConflictMetadataOnly => {
            let live_root = run.target_live_root();
            let mut plan = match run.compute_commit_plan(live_root) {
                Ok(plan) => plan,
                Err(e) => {
                    let msg = format!("compute_commit_plan failed: {}", e);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    return (TargetCommitResult::Failed(msg), Vec::new(), Vec::new());
                }
            };
            let actions = plan.content_actions.clone();
            let before = capture_action_before(live_root, &actions);
            let mut tx = match commit_object_plan(run, &plan, &actions, false, &mut *chapter_receipts)
            {
                Ok(tx) => tx,
                Err(e) => {
                    let msg = format!("commit_object_plan failed: {}", e);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    return (TargetCommitResult::Failed(msg), Vec::new(), Vec::new());
                }
            };

            match tx.finish() {
                Ok(()) => {
                    invalidate_correction_result_caches(
                        run.app_data_root(),
                        live_root,
                        target.project_id.as_deref(),
                        &actions,
                    );
                    let mut committed_paths = Vec::new();
                    collect_action_paths(
                        target.target_kind.as_str(),
                        target.project_id.as_deref(),
                        &actions,
                        &before,
                        &mut committed_paths,
                    );
                    collect_rebased_anchor_paths(
                        &tx,
                        target.target_kind.as_str(),
                        target.project_id.as_deref(),
                        &actions,
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
                    // 提交确认成功后才冻结发布事实：内容已在比较阶段冻结，
                    // 清单就是"固定远端索引 + 本次真实决议"。
                    *publish = crate::sync::staging::PublishBatch {
                        objects: plan.uploads.clone(),
                        deletes: plan.publish_deletes.clone(),
                        manifest: plan.remote_manifest.clone(),
                    };
                    (TargetCommitResult::Ok, plan.conflict, committed_paths)
                }
                Err(e) => {
                    let msg = format!("tx.finish() failed: {}", e);
                    log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                    // finish 失败不代表所有 action 都没落盘。把本次 plan 涉及
                    // 的路径带回，调用方按 live 实际状态生成章节回执，
                    // 避免已写入正文却丢失同步回执。
                    let mut partial_paths = Vec::new();
                    collect_action_paths(
                        target.target_kind.as_str(),
                        target.project_id.as_deref(),
                        &actions,
                        &before,
                        &mut partial_paths,
                    );
                    collect_rebased_anchor_paths(
                        &tx,
                        target.target_kind.as_str(),
                        target.project_id.as_deref(),
                        &actions,
                        &mut partial_paths,
                    );
                    (TargetCommitResult::Failed(msg), Vec::new(), partial_paths)
                }
            }
        }
    }
}
