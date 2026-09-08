//! # 星图模块 (StarMap Module)
//!
//! 本模块实现了星图（StarMap）功能，用于可视化管理写作项目中的世界观元素、
//! 角色关系、情节线索等创作要素。星图是一种结构化的知识图谱工具，
//! 帮助作者组织和展示复杂的故事元素之间的关系。
//!
//! ## 主要功能
//! - **星图元数据管理**：创建、读取、更新、删除星图的基本信息
//! - **星图索引管理**：维护数据根中所有星图的索引，支持快速查询
//! - **项目关联**：将星图绑定到特定项目，支持设置项目主星图
//! - **布局算法**：grid / radial 自动布局（Core 层，跨端共享）
//! - **命中测试**：节点 AABB / 边线段距离（Core 层，跨端共享）

pub mod graph;
pub mod hittest;
pub mod layout;
pub mod package_storage;
pub mod render;
pub mod semantic;
pub mod store;
pub mod types;

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

/// 星图元数据。
///
/// 每个星图同时维护两份持久化：
/// 1. 独立元数据文件 `app-meta/starmaps/{id}.meta.json`
/// 2. 全局索引 `app-meta/starmaps/index.json` 中的条目
///
/// 两份数据必须保持一致（双写），修改时需同时更新两者。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapMeta {
    pub starmap_id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub parent_starmap_id: Option<String>,
    #[serde(default)]
    pub is_main_for_project: bool,
    #[serde(default = "default_accent_color")]
    pub accent_color: String,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub node_count: u32,
    #[serde(default)]
    pub edge_count: u32,
    #[serde(default)]
    pub linked_chapter_count: u32,
    #[serde(default)]
    pub child_starmap_count: u32,
}

fn default_accent_color() -> String {
    "#7B8CDE".to_string()
}

/// 星图全局索引。
///
/// 存储于 `app-meta/starmaps/index.json`，包含所有星图的元数据摘要。
/// 与各星图独立元数据文件构成双写关系，修改时需同步更新。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapIndex {
    pub schema_version: u32,
    pub starmaps: Vec<StarMapMeta>,
    pub updated_at: u64,
}

#[allow(clippy::cast_possible_truncation)]
pub(crate) fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn starmaps_dir(app_data_root: &Path) -> std::path::PathBuf {
    app_data_root.join("starmaps")
}

fn index_path(app_data_root: &Path) -> std::path::PathBuf {
    starmaps_dir(app_data_root).join("index.json")
}

fn starmap_meta_path(app_data_root: &Path, starmap_id: &str) -> std::path::PathBuf {
    starmaps_dir(app_data_root).join(format!("{}.meta.json", starmap_id))
}

/// #645 评论 5504296097 问题3：starmap meta 的 workspace-relative 路径。
fn starmap_meta_rel_path(starmap_id: &str) -> std::path::PathBuf {
    std::path::PathBuf::from("starmaps").join(format!("{}.meta.json", starmap_id))
}

/// #645 评论 5504296097 问题3：starmaps/index.json 的 workspace-relative 路径。
fn starmaps_index_rel_path() -> std::path::PathBuf {
    std::path::PathBuf::from("starmaps").join("index.json")
}

/// #645 评论 5504296097 问题3：starmaps/{id}/ 目录的 workspace-relative 路径。
fn starmap_dir_rel_path(starmap_id: &str) -> std::path::PathBuf {
    std::path::PathBuf::from("starmaps").join(starmap_id)
}

/// #645 评论 5504296097 问题3：构造单个 starmap meta + index 的变更集。
fn change_set_for_meta_and_index(
    starmap_id: &str,
) -> crate::storage::workspace_git::WorkspaceChangeSet {
    crate::storage::workspace_git::WorkspaceChangeSet::new()
        .add_upsert(starmap_meta_rel_path(starmap_id))
        .add_upsert(starmaps_index_rel_path())
}

fn load_index(app_data_root: &Path) -> Result<StarMapIndex> {
    let path = index_path(app_data_root);
    if !path.exists() {
        return Ok(StarMapIndex {
            schema_version: 1,
            starmaps: vec![],
            updated_at: now_epoch(),
        });
    }
    let content = fs::read_to_string(&path)?;
    let idx: StarMapIndex = serde_json::from_str(&content)?;
    Ok(idx)
}

fn save_index(app_data_root: &Path, idx: &StarMapIndex) -> Result<()> {
    let dir = starmaps_dir(app_data_root);
    fs::create_dir_all(&dir)?;
    let content = serde_json::to_string_pretty(idx)?;
    crate::storage::atomic_write_string(&index_path(app_data_root), &content)
}

fn save_starmap_meta(app_data_root: &Path, meta: &StarMapMeta) -> Result<()> {
    let dir = starmaps_dir(app_data_root);
    fs::create_dir_all(&dir)?;
    let content = serde_json::to_string_pretty(meta)?;
    crate::storage::atomic_write_string(
        &starmap_meta_path(app_data_root, &meta.starmap_id),
        &content,
    )
}

fn load_starmap_meta(app_data_root: &Path, starmap_id: &str) -> Result<StarMapMeta> {
    let path = starmap_meta_path(app_data_root, starmap_id);
    if !path.exists() {
        return Err(crate::error::Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("StarMap not found: {}", starmap_id),
        )));
    }
    let content = fs::read_to_string(&path)?;
    let meta: StarMapMeta = serde_json::from_str(&content)?;
    Ok(meta)
}

fn delete_starmap_meta(app_data_root: &Path, starmap_id: &str) -> Result<()> {
    let path = starmap_meta_path(app_data_root, starmap_id);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

pub fn starmap_graph_path(app_data_root: &Path, starmap_id: &str) -> std::path::PathBuf {
    starmaps_dir(app_data_root)
        .join(starmap_id)
        .join("graph.json")
}

pub fn list_starmaps(app_data_root: &Path) -> Result<Vec<StarMapMeta>> {
    let idx = load_index(app_data_root)?;
    Ok(idx.starmaps)
}

pub fn list_starmaps_for_project(
    app_data_root: &Path,
    project_id: &str,
) -> Result<Vec<StarMapMeta>> {
    list_starmaps_bound_to_project(app_data_root, project_id)
}

pub fn list_starmaps_bound_to_project(
    app_data_root: &Path,
    project_id: &str,
) -> Result<Vec<StarMapMeta>> {
    let all = list_starmaps(app_data_root)?;
    Ok(all
        .into_iter()
        .filter(|m| m.project_id.as_deref() == Some(project_id))
        .collect())
}

pub fn get_starmap(app_data_root: &Path, starmap_id: &str) -> Result<StarMapMeta> {
    load_starmap_meta(app_data_root, starmap_id)
}

pub fn create_starmap(
    app_data_root: &Path,
    title: &str,
    description: &str,
    accent_color: Option<&str>,
) -> Result<StarMapMeta> {
    let now = now_epoch();
    let meta = StarMapMeta {
        starmap_id: format!("sm_{}", uuid::Uuid::new_v4()),
        title: title.to_string(),
        description: description.to_string(),
        project_id: None,
        parent_starmap_id: None,
        is_main_for_project: false,
        accent_color: accent_color.unwrap_or(&default_accent_color()).to_string(),
        created_at: now,
        updated_at: now,
        node_count: 0,
        edge_count: 0,
        linked_chapter_count: 0,
        child_starmap_count: 0,
    };
    save_starmap_meta(app_data_root, &meta)?;
    let mut idx = load_index(app_data_root)?;
    idx.starmaps.push(meta.clone());
    idx.updated_at = now;
    save_index(app_data_root, &idx)?;
    Ok(meta)
}

/// #645 评论 5504296097 问题3：create_starmap 的变更集版本。
///
/// 返回 `(StarMapMeta, WorkspaceChangeSet)`，变更集包含
/// `Upsert(starmaps/{id}.meta.json) + Upsert(starmaps/index.json)`。
pub fn create_starmap_with_changes(
    app_data_root: &Path,
    title: &str,
    description: &str,
    accent_color: Option<&str>,
) -> Result<(
    StarMapMeta,
    crate::storage::workspace_git::WorkspaceChangeSet,
)> {
    let meta = create_starmap(app_data_root, title, description, accent_color)?;
    let change_set = change_set_for_meta_and_index(&meta.starmap_id);
    Ok((meta, change_set))
}

pub fn create_child_starmap(
    app_data_root: &Path,
    parent_id: &str,
    title: &str,
    description: &str,
    accent_color: Option<&str>,
) -> Result<StarMapMeta> {
    let parent = load_starmap_meta(app_data_root, parent_id)?;
    let now = now_epoch();
    let meta = StarMapMeta {
        starmap_id: format!("sm_{}", uuid::Uuid::new_v4()),
        title: title.to_string(),
        description: description.to_string(),
        project_id: parent.project_id.clone(),
        parent_starmap_id: Some(parent_id.to_string()),
        is_main_for_project: false,
        accent_color: accent_color.unwrap_or(&parent.accent_color).to_string(),
        created_at: now,
        updated_at: now,
        node_count: 0,
        edge_count: 0,
        linked_chapter_count: 0,
        child_starmap_count: 0,
    };
    save_starmap_meta(app_data_root, &meta)?;

    let mut idx = load_index(app_data_root)?;
    idx.starmaps.push(meta.clone());
    idx.updated_at = now;
    save_index(app_data_root, &idx)?;

    // Update parent child count
    let mut updated_parent = parent;
    updated_parent.child_starmap_count += 1;
    updated_parent.updated_at = now;
    save_starmap_meta(app_data_root, &updated_parent)?;

    Ok(meta)
}

/// #645 评论 5504296097 问题3：create_child_starmap 的变更集版本。
///
/// 返回 `(StarMapMeta, WorkspaceChangeSet)`，变更集包含
/// `Upsert(child meta) + Upsert(parent meta) + Upsert(index.json)`。
pub fn create_child_starmap_with_changes(
    app_data_root: &Path,
    parent_id: &str,
    title: &str,
    description: &str,
    accent_color: Option<&str>,
) -> Result<(
    StarMapMeta,
    crate::storage::workspace_git::WorkspaceChangeSet,
)> {
    let meta = create_child_starmap(app_data_root, parent_id, title, description, accent_color)?;
    let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
        .add_upsert(starmap_meta_rel_path(&meta.starmap_id))
        .add_upsert(starmap_meta_rel_path(parent_id))
        .add_upsert(starmaps_index_rel_path());
    Ok((meta, change_set))
}

pub fn rename_starmap(
    app_data_root: &Path,
    starmap_id: &str,
    new_title: &str,
) -> Result<StarMapMeta> {
    let mut meta = load_starmap_meta(app_data_root, starmap_id)?;
    meta.title = new_title.to_string();
    meta.updated_at = now_epoch();
    save_starmap_meta(app_data_root, &meta)?;

    let mut idx = load_index(app_data_root)?;
    if let Some(entry) = idx.starmaps.iter_mut().find(|m| m.starmap_id == starmap_id) {
        entry.title = new_title.to_string();
        entry.updated_at = meta.updated_at;
    }
    idx.updated_at = meta.updated_at;
    save_index(app_data_root, &idx)?;
    Ok(meta)
}

/// #645 评论 5504296097 问题3：rename_starmap 的变更集版本。
///
/// 变更集：`Upsert(starmaps/{id}.meta.json) + Upsert(starmaps/index.json)`。
pub fn rename_starmap_with_changes(
    app_data_root: &Path,
    starmap_id: &str,
    new_title: &str,
) -> Result<(
    StarMapMeta,
    crate::storage::workspace_git::WorkspaceChangeSet,
)> {
    let meta = rename_starmap(app_data_root, starmap_id, new_title)?;
    Ok((meta, change_set_for_meta_and_index(starmap_id)))
}

/// 删除星图。
///
/// 先检查是否有外部引用（embed/link/edge 指向此星图），有则拒绝删除。
/// 自引用（星图内部的边/嵌入指向自身）不阻止删除。
/// 删除后同步更新父星图的 `child_starmap_count` 和全局索引。
pub fn delete_starmap(app_data_root: &Path, starmap_id: &str) -> Result<()> {
    // Before deleting, check if it's referenced by any EXTERNAL StarMap.
    let refs = find_starmap_references(app_data_root, starmap_id)?;
    let external_refs: Vec<_> = refs
        .into_iter()
        .filter(|r| r.host_starmap_id != starmap_id)
        .collect();
    if !external_refs.is_empty() {
        return Err(crate::error::Error::Io(std::io::Error::other(format!(
            "Cannot delete StarMap because it is referenced by {} external places.",
            external_refs.len()
        ))));
    }

    let meta = load_starmap_meta(app_data_root, starmap_id)?;

    // Remove from parent's child count (if parent exists)
    if let Some(ref parent_id) = meta.parent_starmap_id {
        if let Ok(mut parent_meta) = load_starmap_meta(app_data_root, parent_id) {
            if parent_meta.child_starmap_count > 0 {
                parent_meta.child_starmap_count -= 1;
                parent_meta.updated_at = now_epoch();
                let _ = save_starmap_meta(app_data_root, &parent_meta);
            }
        }
    }

    delete_starmap_meta(app_data_root, starmap_id)?;

    let mut idx = load_index(app_data_root)?;
    idx.starmaps.retain(|m| m.starmap_id != starmap_id);
    idx.updated_at = now_epoch();
    save_index(app_data_root, &idx)?;

    let graph_dir = starmaps_dir(app_data_root).join(starmap_id);
    if graph_dir.exists() {
        let _ = fs::remove_dir_all(&graph_dir);
    }

    Ok(())
}

/// #645 评论 5504296097 问题3：delete_starmap 的变更集版本。
///
/// 变更集：`Delete(starmaps/{id}.meta.json) + DeleteTree(starmaps/{id}) +
/// Upsert(starmaps/index.json) + 可选 Upsert(parent meta)`。
pub fn delete_starmap_with_changes(
    app_data_root: &Path,
    starmap_id: &str,
) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
    // 先读取 parent_id（删除前），用于构造变更集。
    let meta = load_starmap_meta(app_data_root, starmap_id).ok();
    delete_starmap(app_data_root, starmap_id)?;
    let mut change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
        .add_delete(starmap_meta_rel_path(starmap_id))
        .add_delete_tree(starmap_dir_rel_path(starmap_id))
        .add_upsert(starmaps_index_rel_path());
    if let Some(m) = meta {
        if let Some(ref parent_id) = m.parent_starmap_id {
            change_set = change_set.add_upsert(starmap_meta_rel_path(parent_id));
        }
    }
    Ok(change_set)
}

pub fn bind_starmap_to_project(
    app_data_root: &Path,
    starmap_id: &str,
    project_id: &str,
) -> Result<()> {
    let mut meta = load_starmap_meta(app_data_root, starmap_id)?;
    meta.project_id = Some(project_id.to_string());
    meta.updated_at = now_epoch();
    save_starmap_meta(app_data_root, &meta)?;

    let mut idx = load_index(app_data_root)?;
    if let Some(entry) = idx.starmaps.iter_mut().find(|m| m.starmap_id == starmap_id) {
        entry.project_id = Some(project_id.to_string());
        entry.updated_at = meta.updated_at;
    }
    idx.updated_at = meta.updated_at;
    save_index(app_data_root, &idx)?;
    Ok(())
}

/// #645 评论 5504296097 问题3：bind_starmap_to_project 的变更集版本。
///
/// 变更集：`Upsert(starmaps/{id}.meta.json) + Upsert(starmaps/index.json)`。
pub fn bind_starmap_to_project_with_changes(
    app_data_root: &Path,
    starmap_id: &str,
    project_id: &str,
) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
    bind_starmap_to_project(app_data_root, starmap_id, project_id)?;
    Ok(change_set_for_meta_and_index(starmap_id))
}

/// 设置项目的主星图。
///
/// 先清除该项目下所有星图的 `is_main_for_project` 标记，再设置目标星图。
/// 清除和设置之间不是原子的，崩溃可能导致无主星图状态，但不会导致多主星图。
pub fn set_main_starmap_for_project(
    app_data_root: &Path,
    starmap_id: &str,
    project_id: &str,
) -> Result<()> {
    // Clear previous main
    let mut idx = load_index(app_data_root)?;
    for entry in &mut idx.starmaps {
        if entry.project_id.as_deref() == Some(project_id) && entry.is_main_for_project {
            entry.is_main_for_project = false;
            entry.updated_at = now_epoch();
            let _ = save_starmap_meta(app_data_root, entry);
        }
    }

    // Set new main
    if let Some(entry) = idx.starmaps.iter_mut().find(|m| m.starmap_id == starmap_id) {
        entry.is_main_for_project = true;
        entry.project_id = Some(project_id.to_string());
        entry.updated_at = now_epoch();
        let _ = save_starmap_meta(app_data_root, entry);
    }
    idx.updated_at = now_epoch();
    save_index(app_data_root, &idx)?;

    // Also update the meta file
    let mut meta = load_starmap_meta(app_data_root, starmap_id)?;
    meta.is_main_for_project = true;
    meta.project_id = Some(project_id.to_string());
    meta.updated_at = now_epoch();
    save_starmap_meta(app_data_root, &meta)?;

    Ok(())
}

/// #645 评论 5504296097 问题3：set_main_starmap_for_project 的变更集版本。
///
/// 变更集：所有本次实际改过的 meta + `Upsert(starmaps/index.json)`。
/// 包含被清除 main 标记的旧主星图 meta、新主星图 meta、index.json。
pub fn set_main_starmap_for_project_with_changes(
    app_data_root: &Path,
    starmap_id: &str,
    project_id: &str,
) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
    // 先收集本次会被改的 meta：当前 project 下所有 is_main 的 + 目标 starmap。
    let idx = load_index(app_data_root)?;
    let mut changed_metas: Vec<String> = idx
        .starmaps
        .iter()
        .filter(|m| {
            (m.project_id.as_deref() == Some(project_id) && m.is_main_for_project)
                || m.starmap_id == starmap_id
        })
        .map(|m| m.starmap_id.clone())
        .collect();
    // 去重（目标 starmap 可能本身就是旧 main）。
    changed_metas.sort();
    changed_metas.dedup();

    set_main_starmap_for_project(app_data_root, starmap_id, project_id)?;

    let mut change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
        .add_upsert(starmaps_index_rel_path());
    for id in &changed_metas {
        change_set = change_set.add_upsert(starmap_meta_rel_path(id));
    }
    Ok(change_set)
}

pub fn get_main_starmap_for_project(
    app_data_root: &Path,
    project_id: &str,
) -> Result<Option<StarMapMeta>> {
    let idx = load_index(app_data_root)?;
    for entry in &idx.starmaps {
        if entry.project_id.as_deref() == Some(project_id) && entry.is_main_for_project {
            return Ok(Some(load_starmap_meta(app_data_root, &entry.starmap_id)?));
        }
    }
    Ok(None)
}

pub fn unbind_starmap_from_project(app_data_root: &Path, starmap_id: &str) -> Result<()> {
    let mut meta = load_starmap_meta(app_data_root, starmap_id)?;
    meta.project_id = None;
    meta.is_main_for_project = false;
    meta.updated_at = now_epoch();
    save_starmap_meta(app_data_root, &meta)?;

    let mut idx = load_index(app_data_root)?;
    if let Some(entry) = idx.starmaps.iter_mut().find(|m| m.starmap_id == starmap_id) {
        entry.project_id = None;
        entry.is_main_for_project = false;
        entry.updated_at = meta.updated_at;
    }
    idx.updated_at = meta.updated_at;
    save_index(app_data_root, &idx)?;
    Ok(())
}

/// #645 评论 5504296097 问题3：unbind_starmap_from_project 的变更集版本。
///
/// 变更集：`Upsert(starmaps/{id}.meta.json) + Upsert(starmaps/index.json)`。
pub fn unbind_starmap_from_project_with_changes(
    app_data_root: &Path,
    starmap_id: &str,
) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
    unbind_starmap_from_project(app_data_root, starmap_id)?;
    Ok(change_set_for_meta_and_index(starmap_id))
}

pub fn get_motion_policy(
    _app_data_root: &Path,
) -> Result<crate::starmap::types::StarMapMotionPolicyDto> {
    Ok(crate::starmap::types::StarMapMotionPolicyDto::default())
}

pub fn update_starmap_stats(
    app_data_root: &Path,
    starmap_id: &str,
    node_count: u32,
    edge_count: u32,
    linked_chapter_count: u32,
) -> Result<Vec<std::path::PathBuf>> {
    let mut meta = load_starmap_meta(app_data_root, starmap_id)?;
    meta.node_count = node_count;
    meta.edge_count = edge_count;
    meta.linked_chapter_count = linked_chapter_count;
    meta.updated_at = now_epoch();
    save_starmap_meta(app_data_root, &meta)?;

    let mut idx = load_index(app_data_root)?;
    if let Some(entry) = idx.starmaps.iter_mut().find(|m| m.starmap_id == starmap_id) {
        entry.node_count = node_count;
        entry.edge_count = edge_count;
        entry.linked_chapter_count = linked_chapter_count;
        entry.updated_at = meta.updated_at;
    }
    idx.updated_at = meta.updated_at;
    save_index(app_data_root, &idx)?;

    Ok(vec![
        std::path::PathBuf::from("starmaps").join(format!("{}.meta.json", starmap_id)),
        std::path::PathBuf::from("starmaps").join("index.json"),
    ])
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StarMapReference {
    pub host_starmap_id: String,
    pub host_title: String,
    pub ref_type: String, // "embed", "link", "portal", "edge"
    pub ref_id: String,
    pub target_starmap_id: String,
}

fn deep_target_references_starmap(
    target: &crate::starmap::semantic::StarMapDeepTarget,
    target_starmap_id: &str,
) -> bool {
    if target.starmap_id == target_starmap_id {
        return true;
    }
    target.path.iter().any(|p| match p {
        crate::starmap::semantic::StarMapPathSegment::EnterChild { starmap_id: s } => {
            s == target_starmap_id
        }
    })
}

fn edge_endpoint_references_starmap(
    endpoint: &crate::starmap::types::StarMapEdgeEndpoint,
    target_starmap_id: &str,
) -> bool {
    match endpoint {
        crate::starmap::types::StarMapEdgeEndpoint::DeepTarget { target } => {
            deep_target_references_starmap(target, target_starmap_id)
        }
        _ => false,
    }
}

#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::excessive_nesting,
    clippy::too_many_arguments,
    clippy::type_complexity
)]
pub fn find_starmap_references(
    app_data_root: &Path,
    target_starmap_id: &str,
) -> Result<Vec<StarMapReference>> {
    let mut refs = Vec::new();
    let idx = load_index(app_data_root)?;

    for m in &idx.starmaps {
        let mut store = crate::starmap::store::StarMapStore::new(app_data_root, &m.starmap_id);
        if store.load_full().is_ok() {
            let graph = store.to_starmap_graph();
            // 1. Check embeds
            for embed in &graph.embeds {
                if embed.target_starmap_id == target_starmap_id {
                    refs.push(StarMapReference {
                        host_starmap_id: m.starmap_id.clone(),
                        host_title: m.title.clone(),
                        ref_type: "embed".to_string(),
                        ref_id: embed.instance_id.clone(),
                        target_starmap_id: target_starmap_id.to_string(),
                    });
                }
            }

            // 2. Check links
            for link in &graph.links {
                if deep_target_references_starmap(&link.target, target_starmap_id) {
                    refs.push(StarMapReference {
                        host_starmap_id: m.starmap_id.clone(),
                        host_title: m.title.clone(),
                        ref_type: "link".to_string(),
                        ref_id: link.link_id.clone(),
                        target_starmap_id: target_starmap_id.to_string(),
                    });
                }
            }

            // 3. Check edges
            for edge in &graph.edges {
                let mut matches = false;
                if let Some(ft) = &edge.from_target {
                    if deep_target_references_starmap(ft, target_starmap_id) {
                        matches = true;
                    }
                }
                if let Some(tt) = &edge.to_target {
                    if deep_target_references_starmap(tt, target_starmap_id) {
                        matches = true;
                    }
                }
                if let Some(fe) = &edge.from_endpoint {
                    if edge_endpoint_references_starmap(fe, target_starmap_id) {
                        matches = true;
                    }
                }
                if let Some(te) = &edge.to_endpoint {
                    if edge_endpoint_references_starmap(te, target_starmap_id) {
                        matches = true;
                    }
                }

                if matches {
                    refs.push(StarMapReference {
                        host_starmap_id: m.starmap_id.clone(),
                        host_title: m.title.clone(),
                        ref_type: "edge".to_string(),
                        ref_id: edge.id.clone(),
                        target_starmap_id: target_starmap_id.to_string(),
                    });
                }
            }

            // 4. Check portals
            for node in &graph.nodes {
                if let Some(portal) = &node.portal {
                    let mut matches = false;
                    if portal.target_starmap_id == target_starmap_id {
                        matches = true;
                    }
                    if let Some(dt) = &portal.deep_target {
                        if deep_target_references_starmap(dt, target_starmap_id) {
                            matches = true;
                        }
                    }

                    if matches {
                        refs.push(StarMapReference {
                            host_starmap_id: m.starmap_id.clone(),
                            host_title: m.title.clone(),
                            ref_type: "portal".to_string(),
                            ref_id: node.id.clone(),
                            target_starmap_id: target_starmap_id.to_string(),
                        });
                    }
                }
            }
        }
    }

    Ok(refs)
}

#[cfg(test)]
mod tests;
