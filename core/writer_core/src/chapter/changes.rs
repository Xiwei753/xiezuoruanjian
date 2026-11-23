use super::*;

/// 创建章节并返回变更集。
///
/// 返回 `(Chapter, WorkspaceChangeSet)`，变更集包含
/// `Upsert(projects/{pid}/volumes/{vid}/chapters/{cid}/chapter.meta.json) + Upsert(chapter.md)`。
///
/// `workspace_root` 是 Git worktree 根目录（即 `app_data_root`），用于把绝对路径
/// 转成 workspace-relative。`project_root = workspace_root/projects/{project_id}`。
pub fn create_chapter_with_changes(
    project_root: &Path,
    volume_id: &str,
    title: &str,
    workspace_root: &Path,
) -> Result<(Chapter, crate::storage::workspace_git::WorkspaceChangeSet)> {
    let chapter = create_chapter(project_root, volume_id, title)?;

    let chapter_dir = project_root
        .join("volumes")
        .join(volume_id)
        .join("chapters")
        .join(&chapter.id);

    let meta_path = chapter_dir.join("chapter.meta.json");
    let md_path = chapter_dir.join("chapter.md");

    let meta_rel_path = meta_path
        .strip_prefix(workspace_root)
        .unwrap_or(&meta_path)
        .to_string_lossy()
        .replace('\\', "/");
    let md_rel_path = md_path
        .strip_prefix(workspace_root)
        .unwrap_or(&md_path)
        .to_string_lossy()
        .replace('\\', "/");

    let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
        .add_upsert(std::path::PathBuf::from(meta_rel_path))
        .add_upsert(std::path::PathBuf::from(md_rel_path));

    Ok((chapter, change_set))
}

/// 保存章节正文并返回变更集。
///
/// 返回 `(ChapterSaveReceipt, WorkspaceChangeSet)`，变更集包含
/// `Upsert(chapter.meta.json) + Upsert(chapter.md)`。
pub fn save_chapter_verified_with_changes(
    project_root: &Path,
    volume_id: &str,
    chapter_id: &str,
    content: &str,
    workspace_root: &Path,
) -> Result<(
    ChapterSaveReceipt,
    crate::storage::workspace_git::WorkspaceChangeSet,
)> {
    save_chapter_verified_with_changes_inner(
        project_root,
        volume_id,
        chapter_id,
        content,
        false,
        &[],
        workspace_root,
    )
}

/// 保存章节正文并返回变更集（带空覆盖控制）。
///
/// 返回 `(ChapterSaveReceipt, WorkspaceChangeSet)`，变更集包含
/// `Upsert(chapter.meta.json) + Upsert(chapter.md)`。
pub fn save_chapter_verified_with_changes_with_options(
    project_root: &Path,
    volume_id: &str,
    chapter_id: &str,
    content: &str,
    allow_empty_overwrite: bool,
    workspace_root: &Path,
) -> Result<(
    ChapterSaveReceipt,
    crate::storage::workspace_git::WorkspaceChangeSet,
)> {
    save_chapter_verified_with_changes_inner(
        project_root,
        volume_id,
        chapter_id,
        content,
        allow_empty_overwrite,
        &[],
        workspace_root,
    )
}

/// Save chapter content and additional project-local records in one file
/// transaction, returning every path in the workspace change set.
pub fn save_chapter_verified_with_changes_and_files_with_options(
    project_root: &Path,
    volume_id: &str,
    chapter_id: &str,
    content: &str,
    allow_empty_overwrite: bool,
    additional_files: &[(String, String)],
    workspace_root: &Path,
) -> Result<(
    ChapterSaveReceipt,
    crate::storage::workspace_git::WorkspaceChangeSet,
)> {
    save_chapter_verified_with_changes_inner(
        project_root,
        volume_id,
        chapter_id,
        content,
        allow_empty_overwrite,
        additional_files,
        workspace_root,
    )
}

/// 内部实现，避免重复代码。
fn save_chapter_verified_with_changes_inner(
    project_root: &Path,
    volume_id: &str,
    chapter_id: &str,
    content: &str,
    allow_empty_overwrite: bool,
    additional_files: &[(String, String)],
    workspace_root: &Path,
) -> Result<(
    ChapterSaveReceipt,
    crate::storage::workspace_git::WorkspaceChangeSet,
)> {
    let receipt = save_chapter_verified_with_options_and_files(
        project_root,
        volume_id,
        chapter_id,
        content,
        allow_empty_overwrite,
        additional_files,
    )?;

    let chapter_dir = project_root
        .join("volumes")
        .join(volume_id)
        .join("chapters")
        .join(chapter_id);

    let meta_path = chapter_dir.join("chapter.meta.json");
    let md_path = chapter_dir.join("chapter.md");

    let meta_rel_path = meta_path
        .strip_prefix(workspace_root)
        .unwrap_or(&meta_path)
        .to_string_lossy()
        .replace('\\', "/");
    let md_rel_path = md_path
        .strip_prefix(workspace_root)
        .unwrap_or(&md_path)
        .to_string_lossy()
        .replace('\\', "/");

    let project_relative = project_root
        .strip_prefix(workspace_root)
        .unwrap_or(project_root);
    let mut change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
        .add_upsert(std::path::PathBuf::from(meta_rel_path))
        .add_upsert(std::path::PathBuf::from(md_rel_path));
    for (relative_path, _) in additional_files {
        change_set = change_set.add_upsert(project_relative.join(relative_path));
    }

    Ok((receipt, change_set))
}

/// 重命名章节并返回变更集。
///
/// 返回 `(Chapter, WorkspaceChangeSet)`，变更集包含 `Upsert(chapter.meta.json)`。
pub fn rename_chapter_with_changes(
    project_root: &Path,
    volume_id: &str,
    chapter_id: &str,
    new_title: &str,
    workspace_root: &Path,
) -> Result<(Chapter, crate::storage::workspace_git::WorkspaceChangeSet)> {
    rename_chapter(project_root, volume_id, chapter_id, new_title)?;

    // 重新读取以获取更新后的章节信息
    let chapter_dir = project_root
        .join("volumes")
        .join(volume_id)
        .join("chapters")
        .join(chapter_id);
    let meta_path = chapter_dir.join("chapter.meta.json");
    let meta_str = fs::read_to_string(&meta_path)?;
    let chapter: Chapter = serde_json::from_str(&meta_str)?;

    let meta_rel_path = meta_path
        .strip_prefix(workspace_root)
        .unwrap_or(&meta_path)
        .to_string_lossy()
        .replace('\\', "/");

    let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
        .add_upsert(std::path::PathBuf::from(meta_rel_path));

    Ok((chapter, change_set))
}

/// 删除章节并返回变更集。
///
/// 返回 `WorkspaceChangeSet`，变更集包含
/// `Delete(chapter.meta.json) + Delete(chapter.md)`。
///
/// 注意：本函数会先执行本地删除再返回 change_set。新的 durable 删除事务
/// （先 save_pending 落盘 journal 再本地删除）应改用
/// [`plan_delete_chapter_changes`] + [`delete_chapter`] 两步，不要再用本函数
/// 作为 durable 删除事务起点。本函数保留供非事务场景使用。
pub fn delete_chapter_with_changes(
    project_root: &Path,
    volume_id: &str,
    chapter_id: &str,
    app_data_root: &Path,
    workspace_root: &Path,
) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
    let chapter_dir = project_root
        .join("volumes")
        .join(volume_id)
        .join("chapters")
        .join(chapter_id);

    let meta_path = chapter_dir.join("chapter.meta.json");
    let md_path = chapter_dir.join("chapter.md");

    let meta_rel_path = meta_path
        .strip_prefix(workspace_root)
        .unwrap_or(&meta_path)
        .to_string_lossy()
        .replace('\\', "/");
    let md_rel_path = md_path
        .strip_prefix(workspace_root)
        .unwrap_or(&md_path)
        .to_string_lossy()
        .replace('\\', "/");

    delete_chapter(project_root, volume_id, chapter_id, app_data_root)?;

    let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
        .add_delete(std::path::PathBuf::from(meta_rel_path))
        .add_delete(std::path::PathBuf::from(md_rel_path));

    Ok(change_set)
}

/// 删除章节的"先计划"版本（不修改磁盘）。
///
/// 只校验目标存在并构造 `WorkspaceChangeSet`，包含
/// `Delete(chapter.meta.json) + Delete(chapter.md)`。
/// 不执行本地删除，供 durable 删除事务在 `save_pending` 落盘 journal 前调用。
/// 真正的本地删除继续由 [`delete_chapter`] 完成。
///
/// 这样保证顺序为：plan change_set -> save_pending -> 本地删除 ->
/// mark_local_applied -> record history -> mark_history_recorded -> clear_journal。
pub fn plan_delete_chapter_changes(
    project_root: &Path,
    volume_id: &str,
    chapter_id: &str,
    _app_data_root: &Path,
    workspace_root: &Path,
) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
    // 校验目标存在（与 delete_chapter 相同的校验，但不执行移动）。
    let volume_id = crate::delete_guard::validate_id_segment(volume_id)?;
    let chapter_id = crate::delete_guard::validate_id_segment(chapter_id)?;
    let chapter_dir = project_root
        .join("volumes")
        .join(volume_id)
        .join("chapters")
        .join(chapter_id);
    crate::delete_guard::validate_delete_target(project_root, &chapter_dir, "chapter.meta.json")?;

    let meta_path = chapter_dir.join("chapter.meta.json");
    let md_path = chapter_dir.join("chapter.md");

    let meta_rel_path = meta_path
        .strip_prefix(workspace_root)
        .unwrap_or(&meta_path)
        .to_string_lossy()
        .replace('\\', "/");
    let md_rel_path = md_path
        .strip_prefix(workspace_root)
        .unwrap_or(&md_path)
        .to_string_lossy()
        .replace('\\', "/");

    let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
        .add_delete(std::path::PathBuf::from(meta_rel_path))
        .add_delete(std::path::PathBuf::from(md_rel_path));

    Ok(change_set)
}

/// 构造完整的章节删除计划（不修改磁盘），包含固定的 trash 路径和完整 sync_delete_facts。
///
/// 在任何 `rename()` 之前生成固定的 trash token，并遍历源目录所有文件
/// 提前构造每个 `SyncDeleteFact`。供 durable 删除事务 `save_pending` 使用。
///
/// `device_id` 用真实设备 ID，不写固定 `"local"`。
/// `project_id` 从 `project_root.file_name()` 提取。
pub fn plan_delete_chapter(
    project_root: &Path,
    volume_id: &str,
    chapter_id: &str,
    _app_data_root: &Path,
    workspace_root: &Path,
    device_id: &str,
) -> Result<(
    crate::storage::workspace_git::WorkspaceChangeSet,
    crate::storage::journal::workspace_change::PlannedWorkspaceDelete,
)> {
    use crate::storage::journal::workspace_change::{DeleteTarget, PlannedWorkspaceDelete};

    let volume_id = crate::delete_guard::validate_id_segment(volume_id)?;
    let chapter_id = crate::delete_guard::validate_id_segment(chapter_id)?;
    let chapter_dir = project_root
        .join("volumes")
        .join(volume_id)
        .join("chapters")
        .join(chapter_id);
    crate::delete_guard::validate_delete_target(project_root, &chapter_dir, "chapter.meta.json")?;

    let meta_path = chapter_dir.join("chapter.meta.json");
    let md_path = chapter_dir.join("chapter.md");
    let meta_rel_path = meta_path
        .strip_prefix(workspace_root)
        .unwrap_or(&meta_path)
        .to_string_lossy()
        .replace('\\', "/");
    let md_rel_path = md_path
        .strip_prefix(workspace_root)
        .unwrap_or(&md_path)
        .to_string_lossy()
        .replace('\\', "/");
    let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
        .add_delete(std::path::PathBuf::from(meta_rel_path))
        .add_delete(std::path::PathBuf::from(md_rel_path));

    let project_id = project_root
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            crate::error::Error::Other(format!(
                "plan_delete_chapter: cannot extract project_id from project_root {}",
                project_root.display()
            ))
        })?
        .to_string();

    // 在任何 rename 之前生成固定 trash token。
    let trash_token = format!(
        "{}_{}_{}",
        chrono::Utc::now().timestamp_millis(),
        uuid::Uuid::new_v4(),
        chapter_id
    );
    let trash_rel_path = format!("sync/trash/{trash_token}");

    let facts = PlannedWorkspaceDelete::build_facts(
        project_root,
        &chapter_dir,
        &trash_rel_path,
        device_id,
    )?;
    if facts.is_empty() {
        return Err(crate::error::Error::Other(format!(
            "plan_delete_chapter: no sync_delete_facts generated for chapter {chapter_id} \
             — directory may be empty or contain only app-meta files"
        )));
    }

    let planned = PlannedWorkspaceDelete {
        delete_target: DeleteTarget::Chapter {
            project_id,
            volume_id: volume_id.to_string(),
            chapter_id: chapter_id.to_string(),
        },
        trash_rel_path,
        sync_delete_facts: facts,
    };
    Ok((change_set, planned))
}

/// 消费 `PlannedWorkspaceDelete` 执行章节删除：rename 源目录到固定 trash 路径，
/// 再按 journal facts 幂等写入 project_root SyncState tombstone。
///
/// apply 顺序固定为：rename -> 写 tombstone。两步都成功后才算本地删除完成。
/// 不在 apply 阶段重新生成 trash 路径或重新扫描磁盘——事实来自 plan 阶段。
pub fn apply_planned_delete_chapter(
    project_root: &Path,
    volume_id: &str,
    chapter_id: &str,
    app_data_root: &Path,
    planned: &crate::storage::journal::workspace_change::PlannedWorkspaceDelete,
) -> Result<()> {
    use crate::storage::journal::workspace_change::DeleteTarget;

    let volume_id = crate::delete_guard::validate_id_segment(volume_id)?;
    let chapter_id = crate::delete_guard::validate_id_segment(chapter_id)?;
    let chapter_dir = project_root
        .join("volumes")
        .join(volume_id)
        .join("chapters")
        .join(chapter_id);
    let target_canon = crate::delete_guard::validate_delete_target(
        project_root,
        &chapter_dir,
        "chapter.meta.json",
    )?;

    // 校验 planned_delete 的 delete_target 匹配。
    match &planned.delete_target {
        DeleteTarget::Chapter {
            volume_id: planned_vid,
            chapter_id: planned_cid,
            ..
        } if planned_vid == volume_id && planned_cid == chapter_id => {}
        other => {
            return Err(crate::error::Error::Other(format!(
                "apply_planned_delete_chapter: delete_target mismatch — expected Chapter({volume_id},{chapter_id}), got {other:?}"
            )));
        }
    }

    let trash_dir = app_data_root.join("sync/trash");
    fs::create_dir_all(&trash_dir)?;
    let trash_path = app_data_root.join(&planned.trash_rel_path);
    crate::storage::durable_rename(&target_canon, &trash_path)?;

    // 按 journal facts 幂等写入 project_root SyncState tombstone。
    crate::volume::ensure_tombstones_from_facts(project_root, &planned.sync_delete_facts)?;
    Ok(())
}

/// 重新排序章节并返回变更集。
///
/// 返回 `WorkspaceChangeSet`，变更集包含所有被改 order 的 chapter.meta.json 的 Upsert 路径。
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
pub fn reorder_chapters_with_changes(
    project_root: &Path,
    volume_id: &str,
    ordered_ids: &[String],
    workspace_root: &Path,
) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
    // 先获取当前章节状态，用于确定哪些章节的 order 会发生变化
    let chapters = list_chapters(project_root, volume_id)?;
    let mut chapter_map = std::collections::HashMap::new();
    for ch in chapters {
        chapter_map.insert(ch.id.clone(), ch.order);
    }

    // 计算哪些章节的 order 会发生变化
    let mut changed_ids = Vec::new();
    for (index, id) in ordered_ids.iter().enumerate() {
        let new_order = index as i32;
        if let Some(&old_order) = chapter_map.get(id) {
            if old_order != new_order {
                changed_ids.push(id.clone());
            }
        }
    }

    reorder_chapters(project_root, volume_id, ordered_ids)?;

    // 构造变更集
    let mut change_set = crate::storage::workspace_git::WorkspaceChangeSet::new();
    for id in changed_ids {
        let meta_path = project_root
            .join("volumes")
            .join(volume_id)
            .join("chapters")
            .join(&id)
            .join("chapter.meta.json");

        let meta_rel_path = meta_path
            .strip_prefix(workspace_root)
            .unwrap_or(&meta_path)
            .to_string_lossy()
            .replace('\\', "/");

        change_set = change_set.add_upsert(std::path::PathBuf::from(meta_rel_path));
    }

    Ok(change_set)
}

/// 跨卷移动提交成功后的事务事实。
///
/// [`move_chapter_to_volume_with_changes`] 只能从这份"已提交的计划"生成
/// `WorkspaceChangeSet`：提交后重新扫盘既看不到已经删除的旧目录，也会让
/// change set 与事务里真正落盘的文件漂移。
pub(crate) struct CommittedChapterMove {
    /// 旧卷章节目录（相对 project_root），整棵子树从该路径消失。
    pub(crate) source_chapter_dir_rel: String,
    /// 新卷章节目录里由本次事务写入的每个文件（相对 project_root）。
    pub(crate) target_file_rels: Vec<String>,
    /// 两卷中 order 实际变化、被事务重写的 `chapter.meta.json`（相对 project_root）。
    pub(crate) reordered_meta_rels: Vec<String>,
}

/// 计算并原子提交跨卷移动的完整事务，返回已提交的计划事实。
///
/// Issue #1051：章节的卷归属由磁盘目录表达
/// （`volumes/{volume_id}/chapters/{chapter_id}`），一次跨卷移动同时改变
/// "章节目录位置"和"两卷 order"两类持久事实。这里先把两类事实算成一个完整
/// 变更计划，装进同一个 [`crate::storage::transaction::SaveTransaction`] 一次提交：
///
/// 1. 新卷章节目录里的每个文件（`chapter.meta.json` 带移动后的 order，其余原样）；
/// 2. 源卷剩余章节中 order 发生变化的 `chapter.meta.json`；
/// 3. 目标卷原有章节中 order 发生变化的 `chapter.meta.json`；
/// 4. 旧卷章节目录里每个文件的删除条目（排在所有写入之后）。
///
/// 事务 manifest 在任何 live 文件被改动之前落盘；崩溃或提交中途失败后，
/// 恢复流程按同一份 manifest 重放，收敛到"新卷文件齐全、旧卷文件删除、
/// 两卷 order 一致"。不再先裸 `fs::rename` 目录、再执行两个可能各自失败的
/// `reorder_chapters`。
///
/// 提交成功后旧章节目录只剩空壳；删除空目录是 best-effort——失败也只留下
/// 不参与同步/Git 的空目录，不影响已提交的业务事实。
#[allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap
)]
pub(super) fn commit_move_chapter_to_volume(
    project_root: &Path,
    source_volume_id: &str,
    target_volume_id: &str,
    chapter_id: &str,
    target_index: usize,
) -> Result<CommittedChapterMove> {
    let source_volume_id = crate::delete_guard::validate_id_segment(source_volume_id)?;
    let target_volume_id = crate::delete_guard::validate_id_segment(target_volume_id)?;
    let chapter_id = crate::delete_guard::validate_id_segment(chapter_id)?;

    if source_volume_id == target_volume_id {
        return Err(crate::error::Error::Other(
            "move_chapter_to_volume: source and target volume are the same".to_string(),
        ));
    }

    let volumes_dir = project_root.join("volumes");
    if !volumes_dir
        .join(source_volume_id)
        .join("volume.json")
        .exists()
    {
        return Err(crate::error::Error::VolumeNotFound);
    }
    if !volumes_dir
        .join(target_volume_id)
        .join("volume.json")
        .exists()
    {
        return Err(crate::error::Error::VolumeNotFound);
    }

    let source_chapters = list_chapters(project_root, source_volume_id)?;
    let target_chapters = list_chapters(project_root, target_volume_id)?;
    if !source_chapters.iter().any(|c| c.id == chapter_id) {
        return Err(crate::error::Error::ChapterNotFound);
    }

    let source_dir = volumes_dir
        .join(source_volume_id)
        .join("chapters")
        .join(chapter_id);
    let target_dir = volumes_dir
        .join(target_volume_id)
        .join("chapters")
        .join(chapter_id);
    if target_dir.exists() {
        return Err(crate::error::Error::Other(format!(
            "move_chapter_to_volume: target already exists: {chapter_id}"
        )));
    }

    // 章节目录内可能不止 chapter.md / chapter.meta.json：同步冲突副本
    // （chapter.md.remote-conflict-*）等同样是章节数据，必须整体迁移。
    let source_files = collect_chapter_dir_files(&source_dir)?;

    let mut target_ids: Vec<String> = target_chapters.iter().map(|c| c.id.clone()).collect();
    let insert_at = target_index.min(target_ids.len());
    target_ids.insert(insert_at, chapter_id.to_string());

    let now_str = Utc::now().to_rfc3339();
    let mut tx = crate::storage::transaction::SaveTransaction::new(project_root);
    let mut target_file_rels = Vec::with_capacity(source_files.len());

    // 1) 新卷章节目录：先写入全部文件，任何删除都排在这些写入之后。
    for (rel_in_chapter, source_path) in &source_files {
        let target_path = target_dir.join(rel_in_chapter);
        let target_rel = project_relative_string(&target_path, project_root);
        if rel_in_chapter == Path::new("chapter.meta.json") {
            let meta_str = fs::read_to_string(source_path)?;
            let mut meta: Chapter = serde_json::from_str(&meta_str)?;
            if meta.order == insert_at as i32 {
                // order 不变时原样搬运，不额外改写 meta 字节。
                tx.add_file(&target_rel, &meta_str)?;
            } else {
                meta.order = insert_at as i32;
                meta.updated_at = now_str.clone();
                tx.add_file(&target_rel, &serde_json::to_string_pretty(&meta)?)?;
            }
        } else {
            tx.add_bytes(&target_rel, &fs::read(source_path)?)?;
        }
        target_file_rels.push(target_rel);
    }

    // 2) 源卷剩余章节：与 `reorder_chapters` 一样按磁盘当前 meta 判断，
    //    只重写 order 实际变化的章节。
    let mut reordered_meta_rels = Vec::new();
    for (index, chapter) in source_chapters
        .iter()
        .filter(|c| c.id != chapter_id)
        .enumerate()
    {
        let meta_rel = chapter_meta_relative(project_root, source_volume_id, &chapter.id);
        let meta_path = project_root.join(&meta_rel);
        let mut meta: Chapter = serde_json::from_str(&fs::read_to_string(&meta_path)?)?;
        if meta.order == index as i32 {
            continue;
        }
        meta.order = index as i32;
        meta.updated_at = now_str.clone();
        tx.add_file(&meta_rel, &serde_json::to_string_pretty(&meta)?)?;
        reordered_meta_rels.push(meta_rel);
    }

    // 3) 目标卷原有章节：被移动章节的 order 已在第 1 步随 meta 一起写入。
    for (index, id) in target_ids.iter().enumerate() {
        if id == chapter_id {
            continue;
        }
        let meta_rel = chapter_meta_relative(project_root, target_volume_id, id);
        let meta_path = project_root.join(&meta_rel);
        let mut meta: Chapter = serde_json::from_str(&fs::read_to_string(&meta_path)?)?;
        if meta.order == index as i32 {
            continue;
        }
        meta.order = index as i32;
        meta.updated_at = now_str.clone();
        tx.add_file(&meta_rel, &serde_json::to_string_pretty(&meta)?)?;
        reordered_meta_rels.push(meta_rel);
    }

    // 4) 旧卷章节目录的删除条目放在所有写入之后：崩溃或中途失败时最多出现
    //    "新卷文件已就位、旧卷仍有副本"，不会出现两卷都缺文件的窗口。
    for (rel_in_chapter, _) in &source_files {
        let source_rel = project_relative_string(&source_dir.join(rel_in_chapter), project_root);
        tx.add_delete(&source_rel);
    }

    // 一次提交：manifest 先落盘，再写新卷文件、最后删旧卷文件。
    // 提交中途失败时 manifest 留在 `app-meta/transactions/<id>/`（Prepared），
    // 恢复扫描（`storage::transaction::scan_pending_transactions`）按它重放
    // 剩余条目即可收敛；失败前新卷文件已全部 staged，因此任何失败都不会丢数据。
    if let Err(error) = tx.commit() {
        log::warn!(
            "move_chapter_to_volume: transaction {} commit failed, \
             prepared manifest kept under app-meta/transactions for recovery: {}",
            tx.transaction_id(),
            error
        );
        return Err(error);
    }

    // 业务事实已提交。旧目录内容已被逐文件删除，这里只尽力移除空目录：
    // 失败（例如有并发写入留下的文件）只记 warning，不回滚已提交的事务。
    match fs::remove_dir(&source_dir) {
        Ok(()) => {
            if let Err(error) = crate::storage::sync_parent(&source_dir) {
                log::warn!(
                    "move_chapter_to_volume: fsync stale chapter dir parent failed: {}: {}",
                    source_dir.display(),
                    error
                );
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => log::warn!(
            "move_chapter_to_volume: committed but failed to remove empty old chapter dir {}: {}",
            source_dir.display(),
            error
        ),
    }

    Ok(CommittedChapterMove {
        source_chapter_dir_rel: project_relative_string(&source_dir, project_root),
        target_file_rels,
        reordered_meta_rels,
    })
}

/// 递归列出章节目录下的所有文件，返回 `(章节目录内相对路径, 绝对路径)`。
///
/// 结果按相对路径排序，保证同一个目录快照生成的事务条目顺序稳定。
/// 非目录项一律按文件处理：符号链接等读取失败时显式返回错误，
/// 不静默跳过——跳过会让该文件在移动后从两卷都消失。
fn collect_chapter_dir_files(
    chapter_dir: &Path,
) -> Result<Vec<(std::path::PathBuf, std::path::PathBuf)>> {
    let mut files = Vec::new();
    let mut pending_dirs = vec![std::path::PathBuf::new()];
    while let Some(rel_dir) = pending_dirs.pop() {
        let abs_dir = chapter_dir.join(&rel_dir);
        for entry in fs::read_dir(&abs_dir)? {
            let entry = entry?;
            let rel_path = rel_dir.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                pending_dirs.push(rel_path);
            } else {
                files.push((rel_path, entry.path()));
            }
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(files)
}

/// 把绝对路径转成相对 `project_root`、以 `/` 分隔的字符串。
///
/// `SaveTransaction` 的条目路径与包内其它调用方一致，都是相对事务根的字符串。
fn project_relative_string(path: &Path, project_root: &Path) -> String {
    path.strip_prefix(project_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// 章节 `chapter.meta.json` 相对 project_root 的路径。
fn chapter_meta_relative(project_root: &Path, volume_id: &str, chapter_id: &str) -> String {
    project_relative_string(
        &project_root
            .join("volumes")
            .join(volume_id)
            .join("chapters")
            .join(chapter_id)
            .join("chapter.meta.json"),
        project_root,
    )
}

/// 跨卷移动章节并返回变更集。
///
/// Issue #1051：移动现在把"新卷章节目录文件、旧卷目录文件删除、两卷 order
/// 重排"算成一个完整计划，由同一个持久事务一次提交。变更集只从**已提交的
/// 事务计划**生成，提交失败时不会返回任何 change set：
///
/// - 旧卷章节目录整棵子树 `DeleteTree`；
/// - 新卷章节目录里事务真正写入的每个文件 `Upsert`（`chapter.meta.json` /
///   `chapter.md` / 同步冲突副本等）；
/// - 两卷中 order 实际被重写的 `chapter.meta.json` `Upsert`。
///
/// 不再在移动成功后重新扫盘推路径，因此 change set 与磁盘事实一一对应。
pub fn move_chapter_to_volume_with_changes(
    project_root: &Path,
    source_volume_id: &str,
    target_volume_id: &str,
    chapter_id: &str,
    target_index: usize,
    workspace_root: &Path,
) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
    let committed = commit_move_chapter_to_volume(
        project_root,
        source_volume_id,
        target_volume_id,
        chapter_id,
        target_index,
    )?;

    // 事务条目路径相对 project_root；workspace Git 需要 workspace-relative。
    let workspace_rel = |rel: &str| {
        let absolute = project_root.join(rel);
        std::path::PathBuf::from(
            absolute
                .strip_prefix(workspace_root)
                .unwrap_or(&absolute)
                .to_string_lossy()
                .replace('\\', "/"),
        )
    };

    let mut change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
        .add_delete_tree(workspace_rel(&committed.source_chapter_dir_rel));
    for rel in &committed.target_file_rels {
        change_set = change_set.add_upsert(workspace_rel(rel));
    }
    for rel in &committed.reordered_meta_rels {
        change_set = change_set.add_upsert(workspace_rel(rel));
    }
    Ok(change_set)
}

/// 更新章节备注并返回变更集。
///
/// 返回 `(Chapter, WorkspaceChangeSet)`，变更集包含 `Upsert(chapter.meta.json)`。
pub fn update_chapter_note_with_changes(
    project_root: &Path,
    volume_id: &str,
    chapter_id: &str,
    note: &str,
    workspace_root: &Path,
) -> Result<(Chapter, crate::storage::workspace_git::WorkspaceChangeSet)> {
    update_chapter_note(project_root, volume_id, chapter_id, note)?;

    // 重新读取以获取更新后的章节信息
    let chapter_dir = project_root
        .join("volumes")
        .join(volume_id)
        .join("chapters")
        .join(chapter_id);
    let meta_path = chapter_dir.join("chapter.meta.json");
    let meta_str = fs::read_to_string(&meta_path)?;
    let chapter: Chapter = serde_json::from_str(&meta_str)?;

    let meta_rel_path = meta_path
        .strip_prefix(workspace_root)
        .unwrap_or(&meta_path)
        .to_string_lossy()
        .replace('\\', "/");

    let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
        .add_upsert(std::path::PathBuf::from(meta_rel_path));

    Ok((chapter, change_set))
}
