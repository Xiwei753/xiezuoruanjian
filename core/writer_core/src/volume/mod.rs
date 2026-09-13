//! # 卷管理（Core 层）
//!
//! 负责卷（Volume）的 CRUD、排序、重命名、删除。
//!
//! ## 职责边界
//!
//! - **做**：卷创建/列表/重命名/删除/排序
//! - **不做**：章节管理（由 `chapter.rs` 负责）
//! - **删除安全**：所有删除操作经过 `delete_guard` 验证，删除后移入 trash 目录并记录 tombstone
//!
//! ## 目录结构
//!
//! ```text
//! projects/{project_id}/volumes/
//!   {volume_id}/
//!     volume.json           # 卷元数据（id、title、order、时间戳）
//!     chapters/             # 所有章节
//! ```

use crate::error::Result;
use chrono::Utc;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use uuid::Uuid;

/// 将路径归一化为相对于 base 的正斜杠字符串。
///
/// 用于 tombstone 和同步协议中的路径表示（Git/远端约定正斜杠）。
/// 如果 path 不以 base 为前缀，返回 path 本身的归一化形式。
pub(crate) fn normalize_rel_path(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .replace("\\", "/")
}

/// 卷元数据结构体。
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Volume {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub order: i32,
}

/// 列出指定项目下所有卷。
///
/// 使用 `rayon` 并行读取各卷的 `volume.json`，解析失败的卷静默跳过。
/// 结果按 `order` 字段升序排列。
pub fn list_volumes(project_root: &Path) -> Result<Vec<Volume>> {
    let volumes_dir = project_root.join("volumes");
    if !volumes_dir.exists() {
        return Ok(Vec::new());
    }

    let mut dir_paths = Vec::new();
    for entry in fs::read_dir(volumes_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            dir_paths.push(path);
        }
    }

    let volumes_result: Result<Vec<Option<Volume>>> = dir_paths
        .into_par_iter()
        .map(|path| {
            let meta_path = path.join("volume.json");
            if meta_path.exists() {
                let content = fs::read_to_string(&meta_path)?;
                if let Ok(volume) = serde_json::from_str::<Volume>(&content) {
                    Ok(Some(volume))
                } else {
                    Ok(None)
                }
            } else {
                Ok(None)
            }
        })
        .collect();

    let mut volumes: Vec<Volume> = volumes_result?.into_iter().flatten().collect();
    volumes.sort_by_key(|v| v.order);
    Ok(volumes)
}

/// 创建卷。
///
/// `order` 字段取当前项目下最大 order + 1，保证新卷排在最后。
/// 同时创建 `chapters/` 子目录和 `volume.json` 元数据文件。
pub fn create_volume(project_root: &Path, title: &str) -> Result<Volume> {
    let volumes = list_volumes(project_root)?;
    let order = volumes
        .iter()
        .map(|v| v.order)
        .max()
        .map(|m| m + 1)
        .unwrap_or(0);

    create_volume_with_id_and_order(project_root, title, None, order)
}

/// 恢复/导入卷——使用 manifest 中的稳定 ID、标题和 order。
///
/// 用于镜像恢复、导入等场景，调用方传入 manifest 中保存的稳定 ID。
///
/// Core 在私有真相源里按原 ID 重建，
/// Android Restorer 只把 manifest 转成 DTO 调此入口。
pub fn create_volume_with_id(
    project_root: &Path,
    id: &str,
    title: &str,
    order: i32,
) -> Result<Volume> {
    create_volume_with_id_and_order(project_root, title, Some(id), order)
}

/// 内部共享实现：带可选 ID 的卷创建。
/// `id` 为 `None` 时自动生成 UUID；为 `Some(id)` 时使用传入的稳定 ID。
fn create_volume_with_id_and_order(
    project_root: &Path,
    title: &str,
    id_opt: Option<&str>,
    order: i32,
) -> Result<Volume> {
    let id = id_opt
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    // 验证 ID 格式：必须是合法 UUID 字符串，避免磁盘路径注入
    if Uuid::parse_str(&id).is_err() {
        return Err(crate::error::Error::Other(format!(
            "Invalid volume ID format: {id}"
        )));
    }

    let now = Utc::now().to_rfc3339();
    let volume = Volume {
        id: id.clone(),
        title: title.to_string(),
        created_at: now.clone(),
        updated_at: now,
        order,
    };

    let volume_dir = project_root.join("volumes").join(&id);
    fs::create_dir_all(&volume_dir)?;
    fs::create_dir_all(volume_dir.join("chapters"))?;

    let meta_path = volume_dir.join("volume.json");
    let content = serde_json::to_string_pretty(&volume)?;
    crate::storage::atomic_write_string(&meta_path, &content)?;

    Ok(volume)
}

pub fn rename_volume(project_root: &Path, volume_id: &str, new_title: &str) -> Result<()> {
    let volume_dir = project_root.join("volumes").join(volume_id);
    let meta_path = volume_dir.join("volume.json");

    if !meta_path.exists() {
        return Err(crate::error::Error::VolumeNotFound);
    }

    let meta_str = fs::read_to_string(&meta_path)?;
    let mut meta: Volume = serde_json::from_str(&meta_str)?;

    meta.title = new_title.to_string();
    meta.updated_at = Utc::now().to_rfc3339();

    let updated_meta_str = serde_json::to_string_pretty(&meta)?;
    crate::storage::atomic_write_string(&meta_path, &updated_meta_str)?;

    Ok(())
}

/// 删除卷。
///
/// 经过 `delete_guard` 双重验证后，将卷目录移入 `app-meta/sync/trash/`，
/// 命名格式为 `{timestamp}_{uuid}_{volume_id}`，确保唯一且可溯源。
/// 同时生成 tombstone 记录供同步使用。
///
///   ：tombstone 持久化到 `project_root` 的 SyncState（作品同步真正的
/// sync_root 是 `projects_root/<project_id>`），不再写到 `app_data_root`。
/// 不再吞 load/save 错误：tombstone 没真正落盘，删除事务就不能成功。
pub fn delete_volume(project_root: &Path, volume_id: &str, app_data_root: &Path) -> Result<()> {
    let volume_id = crate::delete_guard::validate_id_segment(volume_id)?;
    let volume_dir = project_root.join("volumes").join(volume_id);
    let target_canon =
        crate::delete_guard::validate_delete_target(project_root, &volume_dir, "volume.json")?;

    let trash_dir = app_data_root.join("sync/trash");
    let _ = fs::create_dir_all(&trash_dir);
    let trash_path = trash_dir.join(format!(
        "{}_{}_{}",
        chrono::Utc::now().timestamp_millis(),
        uuid::Uuid::new_v4(),
        volume_id
    ));
    fs::rename(&target_canon, &trash_path)?;

    // 持久化 tombstone 到 project_root 的 SyncState（作品同步真正的 sync_root）。
    // 不再吞 load/save 错误：tombstone 没真正落盘，事务就不能成功。
    let mut state = crate::sync::SyncService::load_sync_state(project_root)?;
    let rel_volume_dir = normalize_rel_path(&volume_dir, project_root);
    let rel_trash_path = normalize_rel_path(&trash_path, app_data_root);

    crate::trash::generate_tombstones(&mut state, &trash_path, &rel_volume_dir, &rel_trash_path);
    crate::sync::SyncService::save_sync_state(project_root, &state)?;
    Ok(())
}

/// 重排卷顺序。
///
/// `ordered_ids` 必须是当前项目所有卷 ID 的精确排列（集合完全一致，无遗漏无多余），
/// 否则返回错误。每个卷的 `order` 字段被设置为该 ID 在列表中的索引。
///
///   本函数是 `reorder_volumes_with_changes` 的薄包装。
/// 真正的写循环在 `reorder_volumes_with_changes` 里——只对 order 实际变化的卷
/// 重写 `volume.json`，避免磁盘事实（N 个文件变了）和 change_set（M < N 个）漂移。
/// 旧接口不返回 change_set，`app_data_root` 用 `project_root` 作占位
/// （`workspace_rel` 会 `strip_prefix`，结果不影响旧接口正确性）。
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
pub fn reorder_volumes(project_root: &Path, ordered_ids: &[String]) -> Result<()> {
    reorder_volumes_with_changes(project_root, ordered_ids, project_root).map(|_| ())
}

// ──   volume 的 *_with_changes 入口 ──
//
// 模式参考 `chapter::save_chapter_verified_with_changes` 和
// `project::create_project_with_changes`：先调原函数落盘，再根据真实写入的
// 文件构造 `WorkspaceChangeSet`。volume 的磁盘路径是
// `projects/{project_id}/volumes/{volume_id}/volume.json`，底层函数接收
// `project_root`（绝对路径），用 `strip_prefix(app_data_root)` 转成
// workspace-relative。

/// 把绝对路径转成 workspace-relative 的正斜杠字符串。
fn workspace_rel(path: &Path, workspace_root: &Path) -> String {
    path.strip_prefix(workspace_root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

///   create_volume 的变更集版本。
///
/// 返回 `(Volume, WorkspaceChangeSet)`，变更集包含
/// `Upsert(projects/{project_id}/volumes/{volume_id}/volume.json)`。
pub fn create_volume_with_changes(
    project_root: &Path,
    title: &str,
    app_data_root: &Path,
) -> Result<(Volume, crate::storage::workspace_git::WorkspaceChangeSet)> {
    let volume = create_volume(project_root, title)?;
    let meta_path = project_root
        .join("volumes")
        .join(&volume.id)
        .join("volume.json");
    let rel = workspace_rel(&meta_path, app_data_root);
    let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
        .add_upsert(std::path::PathBuf::from(rel));
    Ok((volume, change_set))
}

///   rename_volume 的变更集版本。
///
/// 返回 `WorkspaceChangeSet`，变更集包含
/// `Upsert(projects/{project_id}/volumes/{volume_id}/volume.json)`。
pub fn rename_volume_with_changes(
    project_root: &Path,
    volume_id: &str,
    new_title: &str,
    app_data_root: &Path,
) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
    rename_volume(project_root, volume_id, new_title)?;
    let meta_path = project_root
        .join("volumes")
        .join(volume_id)
        .join("volume.json");
    let rel = workspace_rel(&meta_path, app_data_root);
    let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
        .add_upsert(std::path::PathBuf::from(rel));
    Ok(change_set)
}

///   delete_volume 的变更集版本。
///
/// 返回 `WorkspaceChangeSet`，变更集包含
/// `DeleteTree(projects/{project_id}/volumes/{volume_id})`。
/// 用 DeleteTree 在 Git index 层按 prefix 删除所有 tracked entries，
/// 不再只传 volume.json 导致卷下章节残留。
///
/// 注意：本函数会先执行物理删除再返回 change_set。新的 durable 删除事务
/// （先 save_pending 落盘 journal 再物理删除）应改用
/// [`plan_delete_volume_changes`] + [`delete_volume`] 两步，不要再用本函数
/// 作为 durable 删除事务起点。本函数保留供非事务场景使用。
pub fn delete_volume_with_changes(
    project_root: &Path,
    volume_id: &str,
    app_data_root: &Path,
) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
    delete_volume(project_root, volume_id, app_data_root)?;
    let volume_dir = project_root.join("volumes").join(volume_id);
    let rel = workspace_rel(&volume_dir, app_data_root);
    let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
        .add_delete_tree(std::path::PathBuf::from(rel));
    Ok(change_set)
}

///   delete_volume 的"先计划"版本（不修改磁盘）。
///
/// 只校验目标存在并构造 `WorkspaceChangeSet`，包含
/// `DeleteTree(projects/{project_id}/volumes/{volume_id})`。
/// 不执行物理删除，供 durable 删除事务在 `save_pending` 落盘 journal 前调用。
/// 真正的物理删除继续由 [`delete_volume`] 完成。
///
/// 这样保证顺序为：plan change_set -> save_pending -> 本地删除 ->
/// mark_local_applied -> record history -> mark_history_recorded -> clear_journal。
pub fn plan_delete_volume_changes(
    project_root: &Path,
    volume_id: &str,
    app_data_root: &Path,
) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
    // 校验目标存在（与 delete_volume 相同的校验，但不执行移动）。
    let volume_id = crate::delete_guard::validate_id_segment(volume_id)?;
    let volume_dir = project_root.join("volumes").join(volume_id);
    // 校验目标是合法删除目标（存在 + 在 project_root 下）。
    crate::delete_guard::validate_delete_target(project_root, &volume_dir, "volume.json")?;
    let rel = workspace_rel(&volume_dir, app_data_root);
    let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
        .add_delete_tree(std::path::PathBuf::from(rel));
    Ok(change_set)
}

/// 构造完整的卷删除计划（不修改磁盘），包含固定的 trash 路径和完整 sync_delete_facts。
///
/// 在任何 `rename()` 之前生成固定的 trash token，并遍历源目录所有文件
/// 提前构造每个 `SyncDeleteFact`。供 durable 删除事务 `save_pending` 使用。
///
/// `device_id` 用真实设备 ID，不写固定 `"local"`。
/// `project_id` 从 `project_root.file_name()` 提取。
pub fn plan_delete_volume(
    project_root: &Path,
    volume_id: &str,
    app_data_root: &Path,
    device_id: &str,
) -> Result<(
    crate::storage::workspace_git::WorkspaceChangeSet,
    crate::storage::journal::workspace_change::PlannedWorkspaceDelete,
)> {
    use crate::storage::journal::workspace_change::{DeleteTarget, PlannedWorkspaceDelete};

    let volume_id = crate::delete_guard::validate_id_segment(volume_id)?;
    let volume_dir = project_root.join("volumes").join(&volume_id);
    crate::delete_guard::validate_delete_target(project_root, &volume_dir, "volume.json")?;

    let rel = workspace_rel(&volume_dir, app_data_root);
    let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
        .add_delete_tree(std::path::PathBuf::from(rel));

    let project_id = project_root
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            crate::error::Error::Other(format!(
                "plan_delete_volume: cannot extract project_id from project_root {}",
                project_root.display()
            ))
        })?
        .to_string();

    // 在任何 rename 之前生成固定 trash token。
    let trash_token = format!(
        "{}_{}_{}",
        chrono::Utc::now().timestamp_millis(),
        uuid::Uuid::new_v4(),
        volume_id
    );
    let trash_rel_path = format!("sync/trash/{trash_token}");

    let facts = PlannedWorkspaceDelete::build_facts(
        project_root,
        &volume_dir,
        &trash_rel_path,
        device_id,
    )?;
    if facts.is_empty() {
        return Err(crate::error::Error::Other(format!(
            "plan_delete_volume: no sync_delete_facts generated for volume {volume_id} \
             — directory may be empty or contain only app-meta files"
        )));
    }

    let planned = PlannedWorkspaceDelete {
        delete_target: DeleteTarget::Volume {
            project_id,
            volume_id: volume_id.to_string(),
        },
        trash_rel_path,
        sync_delete_facts: facts,
    };
    Ok((change_set, planned))
}

/// 消费 `PlannedWorkspaceDelete` 执行卷删除：rename 源目录到固定 trash 路径，
/// 再按 journal facts 幂等写入 project_root SyncState tombstone。
///
/// apply 顺序固定为：rename -> 写 tombstone。两步都成功后才算本地删除完成。
/// 不在 apply 阶段重新生成 trash 路径或重新扫描磁盘——事实来自 plan 阶段。
pub fn apply_planned_delete_volume(
    project_root: &Path,
    volume_id: &str,
    app_data_root: &Path,
    planned: &crate::storage::journal::workspace_change::PlannedWorkspaceDelete,
) -> Result<()> {
    use crate::storage::journal::workspace_change::DeleteTarget;

    let volume_id = crate::delete_guard::validate_id_segment(volume_id)?;
    let volume_dir = project_root.join("volumes").join(&volume_id);
    let target_canon =
        crate::delete_guard::validate_delete_target(project_root, &volume_dir, "volume.json")?;

    // 校验 planned_delete 的 delete_target 匹配。
    match &planned.delete_target {
        DeleteTarget::Volume {
            volume_id: planned_vid,
            ..
        } if planned_vid == &volume_id => {}
        other => {
            return Err(crate::error::Error::Other(format!(
                "apply_planned_delete_volume: delete_target mismatch — expected Volume({volume_id}), got {other:?}"
            )));
        }
    }

    let trash_dir = app_data_root.join("sync/trash");
    let _ = fs::create_dir_all(&trash_dir);
    let trash_path = app_data_root.join(&planned.trash_rel_path);
    fs::rename(&target_canon, &trash_path)?;

    // 按 journal facts 幂等写入 project_root SyncState tombstone。
    ensure_tombstones_from_facts(project_root, &planned.sync_delete_facts)?;
    Ok(())
}

/// 根据 sync_delete_facts 幂等补齐 project_root 的 SyncState tombstone。
///
/// 已存在的 tombstone（按 original_path + trash_path 匹配）跳过，保证幂等。
pub(crate) fn ensure_tombstones_from_facts(
    project_root: &Path,
    facts: &[crate::storage::journal::workspace_change::SyncDeleteFact],
) -> Result<()> {
    if facts.is_empty() {
        return Ok(());
    }
    let mut state = crate::sync::SyncService::load_sync_state(project_root)?;
    let mut changed = false;
    for fact in facts {
        let exists = state
            .tombstones
            .iter()
            .any(|t| t.original_path == fact.original_path && t.trash_path == fact.trash_path);
        if exists {
            continue;
        }
        state.tombstones.push(crate::sync::Tombstone {
            original_path: fact.original_path.clone(),
            trash_path: fact.trash_path.clone(),
            deleted_at: fact.deleted_at,
            purge_after: fact.deleted_at + 30 * 24 * 3600,
            deleted_by: if fact.deleted_by.is_empty() {
                state.device_id.clone()
            } else {
                fact.deleted_by.clone()
            },
            original_hash: fact.original_hash.clone(),
            kind: "local_delete".to_string(),
        });
        changed = true;
    }
    if changed {
        crate::sync::SyncService::save_sync_state(project_root, &state)?;
    }
    Ok(())
}

///   reorder_volumes 的变更集版本。
///
/// 返回 `WorkspaceChangeSet`，变更集包含所有被改 order 的 volume.json 的 Upsert 路径。
///
///   把真正的写循环收进本函数，只对 order 实际变化的卷
/// 重写 `volume.json`。这样"实际写了什么"和"history 记录什么"来自同一个循环，
/// 不会再出现磁盘改了 N 个 volume.json 但 change_set 只有 M 个 (M < N) 的漂移。
#[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
pub fn reorder_volumes_with_changes(
    project_root: &Path,
    ordered_ids: &[String],
    app_data_root: &Path,
) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
    // 校验 ordered_ids 是当前所有卷 ID 的精确排列。
    let volumes = list_volumes(project_root)?;
    let existing_ids: std::collections::HashSet<_> = volumes.iter().map(|v| v.id.clone()).collect();
    let new_ids: std::collections::HashSet<_> = ordered_ids.iter().cloned().collect();
    if existing_ids.len() != new_ids.len()
        || existing_ids != new_ids
        || ordered_ids.len() != new_ids.len()
    {
        return Err(crate::error::Error::Other(
            "Invalid ordered_ids for reorder".to_string(),
        ));
    }

    let mut changes = crate::storage::workspace_git::WorkspaceChangeSet::new();
    for (index, id) in ordered_ids.iter().enumerate() {
        let volume_dir = project_root.join("volumes").join(id);
        let meta_path = volume_dir.join("volume.json");
        if !meta_path.exists() {
            return Err(crate::error::Error::VolumeNotFound);
        }
        let meta_str = fs::read_to_string(&meta_path)?;
        let mut meta = serde_json::from_str::<Volume>(&meta_str)?;
        let new_order = index as i32;
        if meta.order == new_order {
            // 不变则不写——磁盘事实和 change_set 一致。
            continue;
        }
        meta.order = new_order;
        meta.updated_at = Utc::now().to_rfc3339();
        let updated_meta_str = serde_json::to_string_pretty(&meta)?;
        crate::storage::atomic_write_string(&meta_path, &updated_meta_str)?;
        let rel = workspace_rel(&meta_path, app_data_root);
        changes = changes.add_upsert(std::path::PathBuf::from(rel));
    }
    Ok(changes)
}

#[cfg(test)]
mod tests;
