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

/// 跨卷移动的完整计划（不触碰 live 文件）。
///
/// 在 `SaveTransaction` 建事务之前算好所有目标文件内容、两卷 order 重写和
/// 旧卷删除条目，供 live 前的 journal 落盘和随后的原子提交共用。
struct ChapterMovePlan {
    /// 旧卷章节目录（相对 project_root），整棵子树从该路径消失。
    source_chapter_dir_rel: String,
    /// 旧卷章节目录绝对路径，提交后用于 best-effort 删除空壳。
    source_dir: std::path::PathBuf,
    /// 新卷章节目录里由本次事务写入的文件 `(project_root 相对路径, 内容)`。
    target_files: Vec<(String, Vec<u8>)>,
    /// 新卷章节目录里由本次事务写入的每个文件（相对 project_root）。
    target_file_rels: Vec<String>,
    /// 两卷中 order 实际变化、被事务重写的 meta `(相对路径, 内容)`。
    reordered_meta_files: Vec<(String, String)>,
    /// 两卷中 order 实际变化、被事务重写的 `chapter.meta.json`（相对 project_root）。
    reordered_meta_rels: Vec<String>,
    /// 旧卷章节目录里每个文件的删除条目（相对 project_root）。
    source_delete_rels: Vec<String>,
}

/// 计算跨卷移动的完整计划，不做任何 live 文件写入。
///
/// 校验与文件内容读取都在这里完成：
/// - 源卷/目标卷存在且不是同一卷；
/// - 源卷确实包含该章节；
/// - 目标卷章节目录不存在（`allow_existing_target` 为恢复路径的重放开关）；
/// - 新卷每个章节文件的内容（`chapter.meta.json` 改写为目标 order）；
/// - 两卷 order 实际变化的 `chapter.meta.json` 内容；
/// - 旧卷章节文件的删除条目。
#[allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap
)]
fn plan_move_chapter(
    project_root: &Path,
    source_volume_id: &str,
    target_volume_id: &str,
    chapter_id: &str,
    target_index: usize,
    allow_existing_target: bool,
) -> Result<ChapterMovePlan> {
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
    if target_dir.exists() && !allow_existing_target {
        return Err(crate::error::Error::Other(format!(
            "move_chapter_to_volume: target already exists: {chapter_id}"
        )));
    }

    // 章节目录内可能不止 chapter.md / chapter.meta.json：同步冲突副本
    // （chapter.md.remote-conflict-*）等同样是章节数据，必须整体迁移。
    let source_files = collect_chapter_dir_files(&source_dir)?;

    // 目标卷里若已经出现该章节（恢复路径的部分提交），先排除再按目标下标插入，
    // 保证重算出的目标顺序与首次计划一致，不产生重复章节。
    let mut target_ids: Vec<String> = target_chapters
        .iter()
        .map(|c| c.id.clone())
        .filter(|id| id != chapter_id)
        .collect();
    let insert_at = target_index.min(target_ids.len());
    target_ids.insert(insert_at, chapter_id.to_string());

    let now_str = Utc::now().to_rfc3339();
    let mut target_files = Vec::with_capacity(source_files.len());
    let mut target_file_rels = Vec::with_capacity(source_files.len());

    // 1) 新卷章节目录：先准备全部文件，任何删除都排在这些写入之后。
    for (rel_in_chapter, source_path) in &source_files {
        let target_path = target_dir.join(rel_in_chapter);
        let target_rel = project_relative_string(&target_path, project_root);
        let bytes = if rel_in_chapter == Path::new("chapter.meta.json") {
            let meta_str = fs::read_to_string(source_path)?;
            let mut meta: Chapter = serde_json::from_str(&meta_str)?;
            if meta.order == insert_at as i32 {
                // order 不变时原样搬运，不额外改写 meta 字节。
                meta_str.into_bytes()
            } else {
                meta.order = insert_at as i32;
                meta.updated_at = now_str.clone();
                serde_json::to_string_pretty(&meta)?.into_bytes()
            }
        } else {
            fs::read(source_path)?
        };
        target_file_rels.push(target_rel.clone());
        target_files.push((target_rel, bytes));
    }

    // 2) 源卷剩余章节：与 `reorder_chapters` 一样按磁盘当前 meta 判断，
    //    只重写 order 实际变化的章节。
    let mut reordered_meta_files = Vec::new();
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
        reordered_meta_rels.push(meta_rel.clone());
        reordered_meta_files.push((meta_rel, serde_json::to_string_pretty(&meta)?));
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
        reordered_meta_rels.push(meta_rel.clone());
        reordered_meta_files.push((meta_rel, serde_json::to_string_pretty(&meta)?));
    }

    // 4) 旧卷章节目录的删除条目放在所有写入之后：崩溃或中途失败时最多出现
    //    "新卷文件已就位、旧卷仍有副本"，不会出现两卷都缺文件的窗口。
    let source_delete_rels = source_files
        .iter()
        .map(|(rel_in_chapter, _)| {
            project_relative_string(&source_dir.join(rel_in_chapter), project_root)
        })
        .collect();

    Ok(ChapterMovePlan {
        source_chapter_dir_rel: project_relative_string(&source_dir, project_root),
        source_dir,
        target_files,
        target_file_rels,
        reordered_meta_files,
        reordered_meta_rels,
        source_delete_rels,
    })
}

/// 从已完成的计划构造 workspace-relative 变更集。
fn build_move_change_set(
    plan: &ChapterMovePlan,
    project_root: &Path,
    workspace_root: &Path,
) -> crate::storage::workspace_git::WorkspaceChangeSet {
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
        .add_delete_tree(workspace_rel(&plan.source_chapter_dir_rel));
    for rel in &plan.target_file_rels {
        change_set = change_set.add_upsert(workspace_rel(rel));
    }
    for rel in &plan.reordered_meta_rels {
        change_set = change_set.add_upsert(workspace_rel(rel));
    }
    change_set
}

/// 用同一个 `SaveTransaction` 提交计划：先写新卷文件与两卷 order，再删旧卷文件。
///
/// manifest 在 live 文件改动前落盘；提交失败时 manifest 留在
/// `app-meta/transactions/<id>/`（Prepared），恢复扫描按它重放剩余条目。
fn apply_move_chapter_plan(project_root: &Path, plan: &ChapterMovePlan) -> Result<()> {
    let mut tx = crate::storage::transaction::SaveTransaction::new(project_root);
    for (rel, bytes) in &plan.target_files {
        tx.add_bytes(rel, bytes)?;
    }
    for (rel, content) in &plan.reordered_meta_files {
        tx.add_file(rel, content)?;
    }
    for rel in &plan.source_delete_rels {
        tx.add_delete(rel);
    }

    // 一次提交：manifest 先落盘，再写新卷文件、最后删旧卷文件。
    // 提交中途失败时 manifest 保留，恢复扫描（`scan_pending_transactions`）
    // 按它重放剩余条目即可收敛；失败前新卷文件已全部 staged，不会丢数据。
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
    match fs::remove_dir(&plan.source_dir) {
        Ok(()) => {
            if let Err(error) = crate::storage::sync_parent(&plan.source_dir) {
                log::warn!(
                    "move_chapter_to_volume: fsync stale chapter dir parent failed: {}: {}",
                    plan.source_dir.display(),
                    error
                );
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => log::warn!(
            "move_chapter_to_volume: committed but failed to remove empty old chapter dir {}: {}",
            plan.source_dir.display(),
            error
        ),
    }
    Ok(())
}

/// 从 project_root 提取作品 ID（与 `plan_delete_chapter` 一致）。
fn project_id_from_root(project_root: &Path) -> Result<String> {
    project_root
        .file_name()
        .and_then(|n| n.to_str())
        .map(ToString::to_string)
        .ok_or_else(|| {
            crate::error::Error::Other(format!(
                "plan_move_chapter: cannot extract project_id from project_root {}",
                project_root.display()
            ))
        })
}

/// 跨卷移动章节并返回变更集与 journal token。
///
/// Issue #1051：移动现在把"新卷章节目录文件、旧卷目录文件删除、两卷 order
/// 重排"算成一个完整计划，并且**在触碰 live 文件之前**把计划写入
/// workspace change journal：
///
/// 1. 纯计算计划（校验 + 目标文件内容 + 两卷 order + 删除条目）；
/// 2. `WorkspaceChangeJournal::save_pending_move_with_token` 落盘 Pending
///    journal（含完整 `change_set` 与 `planned_move`）；
/// 3. 用同一个 `SaveTransaction` 提交文件移动；
/// 4. 提交成功后 journal 保持 `Pending`，由 API 层推进到 `LocalApplied`，
///    并在 workspace history 成功后再 `mark_history_recorded` + 清理。
///
/// 变更集只从**已计算的计划**生成，不在移动成功后重新扫盘：
///
/// - 旧卷章节目录整棵子树 `DeleteTree`；
/// - 新卷章节目录里事务真正写入的每个文件 `Upsert`（`chapter.meta.json` /
///   `chapter.md` / 同步冲突副本等）；
/// - 两卷中 order 实际被重写的 `chapter.meta.json` `Upsert`。
///
/// 提交失败时返回错误，Pending journal 与 SaveTransaction manifest 都保留，
/// 供 bootstrap 的恢复流程重放。
pub fn move_chapter_to_volume_with_changes(
    project_root: &Path,
    source_volume_id: &str,
    target_volume_id: &str,
    chapter_id: &str,
    target_index: usize,
    workspace_root: &Path,
) -> Result<(crate::storage::workspace_git::WorkspaceChangeSet, String)> {
    use crate::storage::journal::workspace_change::{
        new_token, PlannedChapterMove, WorkspaceChangeJournal,
    };

    let plan = plan_move_chapter(
        project_root,
        source_volume_id,
        target_volume_id,
        chapter_id,
        target_index,
        false,
    )?;
    let change_set = build_move_change_set(&plan, project_root, workspace_root);
    let project_id = project_id_from_root(project_root)?;
    let planned_move = PlannedChapterMove {
        project_id,
        source_volume_id: source_volume_id.to_string(),
        target_volume_id: target_volume_id.to_string(),
        chapter_id: chapter_id.to_string(),
        target_index,
    };

    let token = new_token();
    let device_id = crate::settings::load_device_info(workspace_root)
        .map(|info| info.device_id)
        .unwrap_or_default();
    // live 前置：journal 必须早于 SaveTransaction 的第一次文件写入。
    let journal = WorkspaceChangeJournal::save_pending_move_with_token(
        workspace_root,
        &token,
        &change_set,
        &device_id,
        planned_move,
    )?;

    if let Err(error) = apply_move_chapter_plan(project_root, &plan) {
        log::warn!(
            "move_chapter_to_volume: file transaction failed after journal {} (Pending) \
             was written — journal and transaction manifest retained for recovery: {}",
            journal.token,
            error
        );
        return Err(error);
    }

    Ok((change_set, token))
}

/// 恢复 Pending 的章节跨卷移动：按 journal 的 `planned_move` 重新执行文件事务。
///
/// 只允许在“源卷章节 meta 仍在、目标卷章节 meta 尚未出现”时调用。
/// 计划按当前磁盘快照重新计算，目标目录已存在时允许覆盖（部分提交后重放）；
/// 旧卷删除条目仍排在所有写入之后，重复执行收敛到同一结果。
pub fn apply_planned_move_chapter(
    project_root: &Path,
    planned: &crate::storage::journal::workspace_change::PlannedChapterMove,
) -> Result<()> {
    let plan = plan_move_chapter(
        project_root,
        &planned.source_volume_id,
        &planned.target_volume_id,
        &planned.chapter_id,
        planned.target_index,
        true,
    )?;
    apply_move_chapter_plan(project_root, &plan)
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
