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

/// 章节协调器已提交但后续事务失败的路径，转成同步 receipt 输入。
fn partial_committed_paths(
    target: &crate::sync::types::TargetSyncResult,
    actions: &[crate::sync::staging::CommitAction],
    partial: &std::collections::HashSet<PathBuf>,
    before: &[CommittedBefore],
) -> Vec<CommittedPath> {
    let mut out = Vec::new();
    for (index, action) in actions.iter().enumerate() {
        let rel = match action {
            crate::sync::staging::CommitAction::Apply { rel_path, .. }
            | crate::sync::staging::CommitAction::Delete { rel_path } => rel_path,
        };
        if !partial.contains(rel) {
            continue;
        }
        out.push(CommittedPath {
            path: to_workspace_rel_path(
                target.target_kind.as_str(),
                target.project_id.as_deref(),
                rel,
            ),
            deleted: matches!(action, crate::sync::staging::CommitAction::Delete { .. }),
            before: before
                .get(index)
                .cloned()
                .unwrap_or(CommittedBefore::NotTracked),
        });
    }
    out
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

fn stage_rebased_chapter_anchors(
    live_root: &Path,
    content_actions: &[crate::sync::staging::CommitAction],
    tx: &mut crate::storage::transaction::SaveTransaction,
) -> crate::error::Result<()> {
    use crate::sync::staging::CommitAction;
    for action in rebase_chapter_anchors(live_root, content_actions)? {
        if let CommitAction::Apply { rel_path, content } = action {
            tx.add_bytes(&rel_path.to_string_lossy(), &content)?;
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

/// 引擎状态写回：普通状态文件直接写；清单则落成 v2 权威存储。
///
/// staging commit 的清单动作来自三方合并后的完整清单字节。这里把它转成
/// v2 存储计划（root + index + 内容寻址 objects），只写变化的
/// objects/index/root，并回收陈旧对象与索引；旧单文件
/// `manifest.sync.json` 若仍存在则在同一事务里删除——同步流程里不再有
/// "整份清单文件"这个权威容器（#1062 评论 #7219676521 第 2 项）。
fn stage_engine_state_write(
    tx: &mut crate::storage::transaction::SaveTransaction,
    live_root: &Path,
    rel_path: &Path,
    content: &[u8],
) -> crate::error::Result<()> {
    let rel_str = rel_path.to_string_lossy();
    if !crate::sync::lww::is_manifest_rel_path(&rel_str) {
        tx.add_bytes(&rel_str, content)?;
        return Ok(());
    }
    let manifest: crate::sync::types::SyncManifest =
        serde_json::from_slice(content).map_err(|e| {
            crate::Error::Other(format!(
                "commit manifest {rel_str} is unreadable while staging manifest store: {e}"
            ))
        })?;
    let store = crate::sync::lww::ManifestStore::new(live_root, &rel_str);
    let generation = uuid::Uuid::new_v4().to_string();
    let plan = store.plan_sync_manifest(&manifest, &generation)?;
    // 对象 → 索引 → root（plan.writes 最后一项是 root）；deletes 排在 root
    // 之后，只回收已不被新根引用的旧对象/索引。
    for (rel, bytes) in &plan.writes {
        tx.add_bytes(rel, bytes)?;
    }
    for rel in &plan.deletes {
        tx.add_delete(rel)?;
    }
    if live_root.join(&*rel_str).exists() {
        tx.add_delete(&rel_str)?;
    }
    Ok(())
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
    apply_confirmed_remote_actions(&mut state, &content_actions);
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
            }
        }
    }
    if confirmed_any {
        crate::sync::SyncService::save_sync_state_for_target(sync_root, target, &state)?;
    }
    Ok(unconfirmed)
}

/// 把"实际应用到本机的远端事实"推进到已确认基线。
///
/// 只有远端下载/远端删除是本轮已确认的远端事实；本地上传与本地删除要等
/// 远端发布回执才能推进，否则失败重试时会把未发布的本地内容当成已同步。
fn apply_confirmed_remote_actions(
    state: &mut crate::sync::types::SyncState,
    actions: &[crate::sync::staging::CommitAction],
) {
    use crate::sync::staging::CommitAction;
    let now_ms = chrono::Utc::now().timestamp_millis();
    for action in actions {
        let (rel, hash) = match action {
            CommitAction::Apply { rel_path, content } => (
                rel_path.to_string_lossy().replace('\\', "/"),
                Some(crate::sync::hash::content_md5(content)),
            ),
            CommitAction::Delete { rel_path } => {
                (rel_path.to_string_lossy().replace('\\', "/"), None)
            }
        };
        match hash {
            Some(hash) => {
                state.known_files.insert(rel.clone(), hash);
                state.known_files_updated_at.insert(rel.clone(), now_ms);
                state.deleted_files.remove(&rel);
                state.conflicted_files.remove(&rel);
                state.pending_take_remote.remove(&rel);
            }
            None => {
                state.known_files.remove(&rel);
                state.known_files_updated_at.remove(&rel);
                state.tombstones.retain(|item| item.original_path != rel);
                state.deleted_files.insert(rel.clone());
                state.conflicted_files.remove(&rel);
                state.pending_take_remote.remove(&rel);
            }
        }
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
                stage_engine_state_write(&mut tx, live_root, rel_path, content)?;
            }
            crate::sync::staging::CommitAction::Delete { rel_path } => {
                let rel_str = rel_path.to_string_lossy();
                tx.add_delete(&rel_str)?;
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
                tx.add_delete(&rel_str)?;
            }
        }
    }
    tx.commit()?;
    Ok(tx)
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

#[allow(clippy::too_many_arguments)]
fn apply_commit_plan_with_optional_merge(
    run: &crate::sync::staging::StagingRun,
    live_root: &Path,
    plan: &crate::sync::staging::CommitPlan,
    content_actions: &[crate::sync::staging::CommitAction],
    backup_mode: bool,
    published_candidates: &[String],
    mut chapter_receipts: Option<&mut Vec<crate::chapter::ChapterSyncCommitReceipt>>,
) -> std::result::Result<
    (
        crate::storage::transaction::SaveTransaction,
        std::collections::HashSet<PathBuf>,
    ),
    (crate::error::Error, std::collections::HashSet<PathBuf>),
> {
    // plan 到 apply 之间必须持有与 MCP 相同的跨进程工作区锁。统计事件还需要
    // 在锁内重新读取 live/staging 求并集；章节正文协调器也必须用同一把锁
    // 做恢复读取和版本 CAS。外层若已持锁，这里按同一 root 嵌套获取。
    let workspace_lock = match crate::storage::WorkspaceWriteLock::acquire(run.app_data_root()) {
        Ok(lock) => lock,
        Err(error) => return Err((error, std::collections::HashSet::new())),
    };
    let mut content_actions = content_actions.to_vec();
    let default_project_id = live_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let chapter_sources = staging_chapter_sources(run, Some(default_project_id));
    let mut no_receipts = Vec::new();
    let receipts = chapter_receipts.as_deref_mut().unwrap_or(&mut no_receipts);
    let (handled_chapters, chapter_result) = commit_synced_chapter_bodies(
        live_root,
        &workspace_lock,
        &content_actions,
        run.run_id(),
        run.app_data_root(),
        &run.base_root(),
        &chapter_sources,
        receipts,
    );
    if let Err(error) = chapter_result {
        return Err((error, handled_chapters));
    }
    // 正文 action 会从 SaveTransaction 中过滤，但本地标注仍需按同一 incoming
    // 正文重定位。先基于完整 content_actions 生成 anchor action，再随过滤后的
    // transaction 一起提交，避免把正文过滤后丢失 anchor rebase。
    let mut rebased_anchor_actions = match rebase_chapter_anchors(live_root, &content_actions) {
        Ok(actions) => actions,
        Err(error) => return Err((error, handled_chapters)),
    };
    let apply_result: crate::error::Result<crate::storage::transaction::SaveTransaction> = (|| {
        if !handled_chapters.is_empty() {
            content_actions.retain(|action| {
                let rel = match action {
                    crate::sync::staging::CommitAction::Apply { rel_path, .. }
                    | crate::sync::staging::CommitAction::Delete { rel_path } => rel_path,
                };
                !handled_chapters.contains(rel)
            });
            content_actions.append(&mut rebased_anchor_actions);
        }
        let engine_state_actions = plan.engine_state_actions.clone();

        // Issue #1066：提交层不再做第二套业务合并。plan 已经是唯一裁决结果，
        // content_actions 就是本轮要写回的内容（含远端采用/本地保留），
        // engine_state_actions 直接是最终对象索引。
        let result = apply_commit_plan_to_live(
            live_root,
            &content_actions,
            &engine_state_actions,
            backup_mode,
        );

        // 账本不再在 commit 阶段压缩：确认改为"远端提交回执 + 序号"驱动
        // （见 local_mutations::mark_confirmed），避免用本地 known_files 冒充已发布。
        if result.is_ok()
            && matches!(
                run.app_group(),
                Some(crate::sync::types::AppDataGroup::StatsDevice(_))
            )
        {
            // 同步直接搬运 JSONL 不会经过 ContributionStore 写入路径；提交成功后
            // 显式递增事件库代数，避免回放缓存/事件 ID 缓存继续信任旧目录指纹。
            crate::writing_stats::contribution_store::invalidate_stats_event_store(live_root);
        }
        result
    })();
    match apply_result {
        Ok(tx) => Ok((tx, handled_chapters)),
        Err(error) => Err((error, handled_chapters)),
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

        // Issue #1039 复核：所有会读写 live 的提交分支都与 MCP 共用应用数据根上的
        // 跨进程工作区锁，并一直持有到 SaveTransaction 与账本压缩结束。不能只为
        // StatsDevice 组加锁，否则星图/素材组的 plan 与 compact_confirmed 仍会与
        // MCP 写事务竞争。
        let _workspace_lock = if matches!(mode, TargetCommitMode::Skip) {
            None
        } else {
            match crate::storage::WorkspaceWriteLock::acquire(run.app_data_root()) {
                Ok(lock) => Some(lock),
                Err(error) => {
                    target_results.push(TargetCommitResult::Failed(format!(
                        "workspace_write_lock_failed: {error}"
                    )));
                    target_conflicts.push(Vec::new());
                    run.cleanup();
                    continue;
                }
            }
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
                    None,
                ) {
                    Ok((tx, _)) => tx,
                    Err((e, _)) => {
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
                        invalidate_correction_result_caches(
                            run.app_data_root(),
                            live_root,
                            pid,
                            &plan.content_actions,
                        );
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
                    None,
                ) {
                    Ok((tx, _)) => tx,
                    Err((e, _)) => {
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
                invalidate_correction_result_caches(
                    run.app_data_root(),
                    live_root,
                    pid,
                    &safe_content_actions,
                );
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
                        invalidate_correction_result_caches(
                            run.app_data_root(),
                            live_root,
                            pid,
                            &plan.content_actions,
                        );
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
/// 从本轮已落盘的提交结果构造不可变对象决议（复审 #7214105551 第 2 项）。
///
/// 必须在提交刚完成、尚未释放本轮上下文时调用：内容在这里读取一次即冻结，
/// 后续发布只使用决议里的字节，不再回读可变 live。
pub(crate) fn durable_decisions_for_target(
    live_root: &std::path::Path,
    project_id: Option<&str>,
    result: &crate::sync::types::SyncResult,
    committed: &[CommittedPath],
    facts: &std::collections::BTreeMap<String, crate::sync::local_mutations::LocalMutationRecord>,
) -> Vec<crate::sync::decision::DurableObjectDecision> {
    use crate::sync::decision::{DecisionOp, DecisionOrigin, DurableObjectDecision};
    let project_prefix = project_id.map(|id| format!("projects/{id}/"));
    let mut decisions = Vec::new();
    for entry in committed {
        let workspace_rel = entry.path.to_string_lossy().replace('\\', "/");
        let identity = match project_prefix.as_deref() {
            Some(prefix) => match workspace_rel.strip_prefix(prefix) {
                Some(rest) => rest.to_string(),
                None => continue,
            },
            None => workspace_rel.clone(),
        };
        if identity.is_empty() {
            continue;
        }
        let fact = facts.get(&identity);
        let base_hash = fact
            .and_then(|record| record.base_hash.clone())
            .or_else(|| {
                crate::sync::SyncService::load_sync_state_read_only(live_root, project_id)
                    .ok()
                    .and_then(|state| state.known_files.get(&identity).cloned())
            });
        let consumed = result
            .overwritten_files
            .iter()
            .any(|path| path == &identity)
            || result.remote_deletes.iter().any(|path| path == &identity);
        let downloaded = result.downloaded_files.iter().any(|path| path == &identity);
        let origin = if consumed {
            DecisionOrigin::RemoteConsumed
        } else if downloaded {
            DecisionOrigin::Merged
        } else {
            DecisionOrigin::Local
        };
        let (op, content, final_hash) = if entry.deleted {
            (DecisionOp::Delete, None, String::new())
        } else {
            let path = live_root.join(&identity);
            match std::fs::read(&path) {
                Ok(bytes) => {
                    let hash = crate::sync::hash::content_md5(&bytes);
                    (DecisionOp::Upsert, Some(bytes), hash)
                }
                Err(error) => {
                    log::warn!(
                        "[sync] committed object unreadable while freezing decision: {}: {error}",
                        path.display()
                    );
                    continue;
                }
            }
        };
        decisions.push(DurableObjectDecision {
            transaction_id: fact
                .map(|record| record.transaction_id.clone())
                .unwrap_or_default(),
            identity,
            op,
            base_hash,
            final_hash,
            final_version: fact.and_then(|record| record.object_revision),
            mutation_seq: fact.map(|record| record.seq).unwrap_or(0),
            origin,
            content,
            dependencies: Vec::new(),
        });
    }
    // 未决冲突：本轮不发布本地旧版本，决议显式标记为 Conflict，避免发布路径
    // 只按文件清单把冲突路径上传（评审 #7214314073 第 2 项）。
    let covered: std::collections::BTreeSet<String> = decisions
        .iter()
        .map(|decision| decision.identity.clone())
        .collect();
    for conflict in &result.conflicts {
        let workspace_rel = conflict.local_path.replace('\\', "/");
        let identity = match project_prefix.as_deref() {
            Some(prefix) => match workspace_rel.strip_prefix(prefix) {
                Some(rest) => rest.to_string(),
                None => continue,
            },
            None => workspace_rel.clone(),
        };
        if identity.is_empty() || covered.contains(&identity) {
            continue;
        }
        decisions.push(DurableObjectDecision {
            transaction_id: String::new(),
            identity,
            op: DecisionOp::Upsert,
            base_hash: Some(conflict.base_hash.clone()),
            final_hash: conflict.local_hash.clone(),
            final_version: None,
            mutation_seq: 0,
            origin: DecisionOrigin::Conflict,
            content: None,
            dependencies: Vec::new(),
        });
    }
    decisions
}

pub(crate) fn commit_single_target_staging(
    run: &crate::sync::staging::StagingRun,
    target: &crate::sync::types::TargetSyncResult,
    chapter_receipts: &mut Vec<crate::chapter::ChapterSyncCommitReceipt>,
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

            // 本轮真正要发布的候选路径：上传 + 远端删除。只对这些推迟
            // known_files；已经与远端一致的 pending 事实保持 merge 结果。
            // 统一对象比较产出的上传/删除清单也在本轮发布范围内。
            let mut published_candidates: Vec<String> = target.result.uploaded_files.clone();
            published_candidates.extend(target.result.local_deletes.iter().cloned());
            published_candidates.extend(
                plan.uploads
                    .iter()
                    .map(|path| path.to_string_lossy().replace('\\', "/")),
            );
            published_candidates.extend(
                plan.publish_deletes
                    .iter()
                    .map(|path| path.to_string_lossy().replace('\\', "/")),
            );
            let mut tx = if plan.object_engine {
                // 统一对象提交：内容 + 对象索引 + 已确认基线一次落盘，
                // 不做 state 三方合并，也不回读 live 猜胜负。
                match commit_object_plan(
                    run,
                    &plan,
                    &plan.content_actions,
                    false,
                    &mut *chapter_receipts,
                ) {
                    Ok(tx) => tx,
                    Err(e) => {
                        let msg = format!("commit_object_plan failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        return (TargetCommitResult::Failed(msg), Vec::new(), Vec::new());
                    }
                }
            } else {
                match apply_commit_plan_with_optional_merge(
                    run,
                    live_root,
                    &plan,
                    &plan.content_actions,
                    false,
                    &published_candidates,
                    Some(&mut *chapter_receipts),
                ) {
                    Ok((tx, _)) => tx,
                    Err((e, partial)) => {
                        let msg = format!("apply_commit_plan failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        let committed_paths = partial_committed_paths(
                            target,
                            &plan.content_actions,
                            &partial,
                            &before,
                        );
                        return (TargetCommitResult::Failed(msg), Vec::new(), committed_paths);
                    }
                }
            };

            match tx.finish() {
                Ok(()) => {
                    invalidate_correction_result_caches(
                        run.app_data_root(),
                        live_root,
                        target.project_id.as_deref(),
                        &plan.content_actions,
                    );
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
                    // finish 失败不代表所有 action 都没落盘。把本次 plan 涉及
                    // 的路径带回，调用方按 live 实际状态生成章节回执，
                    // 避免已写入正文却丢失同步回执。
                    let mut partial_paths = Vec::new();
                    collect_action_paths(
                        target.target_kind.as_str(),
                        target.project_id.as_deref(),
                        &plan.content_actions,
                        &before,
                        &mut partial_paths,
                    );
                    collect_rebased_anchor_paths(
                        &tx,
                        target.target_kind.as_str(),
                        target.project_id.as_deref(),
                        &plan.content_actions,
                        &mut partial_paths,
                    );
                    (TargetCommitResult::Failed(msg), Vec::new(), partial_paths)
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
            let tx = if plan.object_engine {
                match commit_object_plan(run, &plan, &safe_content_actions, false, &mut Vec::new())
                {
                    Ok(tx) => tx,
                    Err(e) => {
                        let msg = format!("commit_object_plan failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        return (TargetCommitResult::Failed(msg), Vec::new(), Vec::new());
                    }
                }
            } else {
                match apply_commit_plan_with_optional_merge(
                    run,
                    live_root,
                    &plan,
                    &safe_content_actions,
                    false,
                    &[],
                    None,
                ) {
                    Ok((tx, _)) => tx,
                    Err((e, partial)) => {
                        let msg = format!("apply_commit_plan failed: {}", e);
                        log::warn!("Staging commit: {} for run {}", msg, run.run_id());
                        let committed_paths = partial_committed_paths(
                            target,
                            &safe_content_actions,
                            &partial,
                            &before,
                        );
                        return (TargetCommitResult::Failed(msg), Vec::new(), committed_paths);
                    }
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
                    invalidate_correction_result_caches(
                        run.app_data_root(),
                        live_root,
                        target.project_id.as_deref(),
                        &plan.content_actions,
                    );
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
                    // finish 失败不代表所有 action 都没落盘。把本次 plan 涉及
                    // 的路径带回，调用方按 live 实际状态生成章节回执，
                    // 避免已写入正文却丢失同步回执。
                    let mut partial_paths = Vec::new();
                    collect_action_paths(
                        target.target_kind.as_str(),
                        target.project_id.as_deref(),
                        &plan.content_actions,
                        &before,
                        &mut partial_paths,
                    );
                    collect_rebased_anchor_paths(
                        &tx,
                        target.target_kind.as_str(),
                        target.project_id.as_deref(),
                        &plan.content_actions,
                        &mut partial_paths,
                    );
                    (TargetCommitResult::Failed(msg), Vec::new(), partial_paths)
                }
            }
        }
    }
}

#