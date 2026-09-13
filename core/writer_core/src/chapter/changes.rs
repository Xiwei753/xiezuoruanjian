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
    workspace_root: &Path,
) -> Result<(
    ChapterSaveReceipt,
    crate::storage::workspace_git::WorkspaceChangeSet,
)> {
    let receipt = save_chapter_verified_with_options(
        project_root,
        volume_id,
        chapter_id,
        content,
        allow_empty_overwrite,
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

    let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
        .add_upsert(std::path::PathBuf::from(meta_rel_path))
        .add_upsert(std::path::PathBuf::from(md_rel_path));

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
