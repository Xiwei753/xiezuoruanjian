//! # 星图模块 (StarMap Module)
//!
//! 本模块实现了星图（StarMap）功能，用于可视化管理写作项目中的世界观元素、
//! 角色关系、情节线索等创作要素。星图是一种图形化的思维导图工具，
//! 帮助作者组织和展示复杂的故事元素之间的关系。
//!
//! ## 主要功能
//! - **星图元数据管理**：创建、读取、更新、删除星图的基本信息
//! - **星图索引管理**：维护工作区中所有星图的索引，支持快速查询
//! - **项目关联**：将星图绑定到特定项目，支持设置项目主星图
//! - **父子关系**：支持创建子星图，形成层次化的星图结构
//! - **统计信息**：自动更新星图的节点数、边数和关联章节数
//!
//! ## 依赖关系
//! - `crate::error`：错误处理模块
//! - `crate::storage`：原子写入功能
//! - `serde`：JSON序列化/反序列化
//! - `uuid`：生成唯一标识符
//!
//! ## 使用场景
//! - 为小说项目创建世界观设定图
//! - 管理角色关系网络
//! - 组织情节线索和故事结构
//! - 支持多层级的创作要素分类

pub mod graph;
pub mod types;
use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapIndex {
    pub schema_version: u32,
    pub starmaps: Vec<StarMapMeta>,
    pub updated_at: u64,
}

fn now_epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn starmaps_dir(workspace: &Path) -> std::path::PathBuf {
    workspace.join("app-meta").join("starmaps")
}

fn index_path(workspace: &Path) -> std::path::PathBuf {
    starmaps_dir(workspace).join("index.json")
}

fn starmap_meta_path(workspace: &Path, starmap_id: &str) -> std::path::PathBuf {
    starmaps_dir(workspace).join(format!("{}.meta.json", starmap_id))
}

fn load_index(workspace: &Path) -> Result<StarMapIndex> {
    let path = index_path(workspace);
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

fn save_index(workspace: &Path, idx: &StarMapIndex) -> Result<()> {
    let dir = starmaps_dir(workspace);
    fs::create_dir_all(&dir)?;
    let content = serde_json::to_string_pretty(idx)?;
    crate::storage::atomic_write_string(&index_path(workspace), &content)
}

fn save_starmap_meta(workspace: &Path, meta: &StarMapMeta) -> Result<()> {
    let dir = starmaps_dir(workspace);
    fs::create_dir_all(&dir)?;
    let content = serde_json::to_string_pretty(meta)?;
    crate::storage::atomic_write_string(&starmap_meta_path(workspace, &meta.starmap_id), &content)
}

fn load_starmap_meta(workspace: &Path, starmap_id: &str) -> Result<StarMapMeta> {
    let path = starmap_meta_path(workspace, starmap_id);
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

fn delete_starmap_meta(workspace: &Path, starmap_id: &str) -> Result<()> {
    let path = starmap_meta_path(workspace, starmap_id);
    if path.exists() {
        fs::remove_file(&path)?;
    }
    Ok(())
}

pub fn starmap_graph_path(workspace: &Path, starmap_id: &str) -> std::path::PathBuf {
    starmaps_dir(workspace).join(starmap_id).join("graph.json")
}

pub fn list_starmaps(workspace: &Path) -> Result<Vec<StarMapMeta>> {
    let idx = load_index(workspace)?;
    Ok(idx.starmaps)
}

pub fn list_starmaps_for_project(workspace: &Path, project_id: &str) -> Result<Vec<StarMapMeta>> {
    let all = list_starmaps(workspace)?;
    Ok(all
        .into_iter()
        .filter(|m| m.project_id.as_deref() == Some(project_id) || m.project_id.is_none())
        .collect())
}

pub fn get_starmap(workspace: &Path, starmap_id: &str) -> Result<StarMapMeta> {
    load_starmap_meta(workspace, starmap_id)
}

pub fn create_starmap(
    workspace: &Path,
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
    save_starmap_meta(workspace, &meta)?;
    let mut idx = load_index(workspace)?;
    idx.starmaps.push(meta.clone());
    idx.updated_at = now;
    save_index(workspace, &idx)?;
    Ok(meta)
}

pub fn create_child_starmap(
    workspace: &Path,
    parent_id: &str,
    title: &str,
    description: &str,
    accent_color: Option<&str>,
) -> Result<StarMapMeta> {
    let parent = load_starmap_meta(workspace, parent_id)?;
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
    save_starmap_meta(workspace, &meta)?;

    let mut idx = load_index(workspace)?;
    idx.starmaps.push(meta.clone());
    idx.updated_at = now;
    save_index(workspace, &idx)?;

    // Update parent child count
    let mut updated_parent = parent;
    updated_parent.child_starmap_count += 1;
    updated_parent.updated_at = now;
    save_starmap_meta(workspace, &updated_parent)?;

    Ok(meta)
}

pub fn rename_starmap(workspace: &Path, starmap_id: &str, new_title: &str) -> Result<StarMapMeta> {
    let mut meta = load_starmap_meta(workspace, starmap_id)?;
    meta.title = new_title.to_string();
    meta.updated_at = now_epoch();
    save_starmap_meta(workspace, &meta)?;

    let mut idx = load_index(workspace)?;
    if let Some(entry) = idx.starmaps.iter_mut().find(|m| m.starmap_id == starmap_id) {
        entry.title = new_title.to_string();
        entry.updated_at = meta.updated_at;
    }
    idx.updated_at = meta.updated_at;
    save_index(workspace, &idx)?;
    Ok(meta)
}

pub fn delete_starmap(workspace: &Path, starmap_id: &str) -> Result<()> {
    let meta = load_starmap_meta(workspace, starmap_id)?;

    // Delete children first
    let idx = load_index(workspace)?;
    for child in &idx.starmaps {
        if child.parent_starmap_id.as_deref() == Some(starmap_id) {
            let _ = delete_starmap(workspace, &child.starmap_id);
        }
    }

    // Remove from parent's child count
    if let Some(ref parent_id) = meta.parent_starmap_id {
        if let Ok(mut parent_meta) = load_starmap_meta(workspace, parent_id) {
            if parent_meta.child_starmap_count > 0 {
                parent_meta.child_starmap_count -= 1;
                parent_meta.updated_at = now_epoch();
                let _ = save_starmap_meta(workspace, &parent_meta);
            }
        }
    }

    delete_starmap_meta(workspace, starmap_id)?;

    let mut idx = load_index(workspace)?;
    idx.starmaps.retain(|m| m.starmap_id != starmap_id);
    idx.updated_at = now_epoch();
    save_index(workspace, &idx)?;

    // Clean up the underlying mind_map graph if it exists
    let graph_dir = starmaps_dir(workspace).join(starmap_id);
    if graph_dir.exists() {
        let _ = fs::remove_dir_all(&graph_dir);
    }

    Ok(())
}

pub fn bind_starmap_to_project(workspace: &Path, starmap_id: &str, project_id: &str) -> Result<()> {
    let mut meta = load_starmap_meta(workspace, starmap_id)?;
    meta.project_id = Some(project_id.to_string());
    meta.updated_at = now_epoch();
    save_starmap_meta(workspace, &meta)?;

    let mut idx = load_index(workspace)?;
    if let Some(entry) = idx.starmaps.iter_mut().find(|m| m.starmap_id == starmap_id) {
        entry.project_id = Some(project_id.to_string());
        entry.updated_at = meta.updated_at;
    }
    idx.updated_at = meta.updated_at;
    save_index(workspace, &idx)?;
    Ok(())
}

pub fn set_main_starmap_for_project(
    workspace: &Path,
    starmap_id: &str,
    project_id: &str,
) -> Result<()> {
    // Clear previous main
    let mut idx = load_index(workspace)?;
    for entry in &mut idx.starmaps {
        if entry.project_id.as_deref() == Some(project_id) && entry.is_main_for_project {
            entry.is_main_for_project = false;
            entry.updated_at = now_epoch();
            let _ = save_starmap_meta(workspace, entry);
        }
    }

    // Set new main
    if let Some(entry) = idx.starmaps.iter_mut().find(|m| m.starmap_id == starmap_id) {
        entry.is_main_for_project = true;
        entry.project_id = Some(project_id.to_string());
        entry.updated_at = now_epoch();
        let _ = save_starmap_meta(workspace, entry);
    }
    idx.updated_at = now_epoch();
    save_index(workspace, &idx)?;

    // Also update the meta file
    let mut meta = load_starmap_meta(workspace, starmap_id)?;
    meta.is_main_for_project = true;
    meta.project_id = Some(project_id.to_string());
    meta.updated_at = now_epoch();
    save_starmap_meta(workspace, &meta)?;

    Ok(())
}

pub fn get_main_starmap_for_project(
    workspace: &Path,
    project_id: &str,
) -> Result<Option<StarMapMeta>> {
    let idx = load_index(workspace)?;
    for entry in &idx.starmaps {
        if entry.project_id.as_deref() == Some(project_id) && entry.is_main_for_project {
            return Ok(Some(load_starmap_meta(workspace, &entry.starmap_id)?));
        }
    }
    Ok(None)
}

pub fn unbind_starmap_from_project(workspace: &Path, starmap_id: &str) -> Result<()> {
    let mut meta = load_starmap_meta(workspace, starmap_id)?;
    meta.project_id = None;
    meta.is_main_for_project = false;
    meta.updated_at = now_epoch();
    save_starmap_meta(workspace, &meta)?;

    let mut idx = load_index(workspace)?;
    if let Some(entry) = idx.starmaps.iter_mut().find(|m| m.starmap_id == starmap_id) {
        entry.project_id = None;
        entry.is_main_for_project = false;
        entry.updated_at = meta.updated_at;
    }
    idx.updated_at = meta.updated_at;
    save_index(workspace, &idx)?;
    Ok(())
}

pub fn update_starmap_stats(
    workspace: &Path,
    starmap_id: &str,
    node_count: u32,
    edge_count: u32,
    linked_chapter_count: u32,
) -> Result<()> {
    let mut meta = load_starmap_meta(workspace, starmap_id)?;
    meta.node_count = node_count;
    meta.edge_count = edge_count;
    meta.linked_chapter_count = linked_chapter_count;
    meta.updated_at = now_epoch();
    save_starmap_meta(workspace, &meta)?;

    let mut idx = load_index(workspace)?;
    if let Some(entry) = idx.starmaps.iter_mut().find(|m| m.starmap_id == starmap_id) {
        entry.node_count = node_count;
        entry.edge_count = edge_count;
        entry.linked_chapter_count = linked_chapter_count;
        entry.updated_at = meta.updated_at;
    }
    idx.updated_at = meta.updated_at;
    save_index(workspace, &idx)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup_workspace() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        dir
    }

    #[test]
    fn test_create_and_list_starmaps() {
        let dir = setup_workspace();
        let _meta1 = create_starmap(dir.path(), "Star Map 1", "desc1", None).unwrap();
        let _meta2 = create_starmap(dir.path(), "Star Map 2", "desc2", Some("#FF0000")).unwrap();

        let all = list_starmaps(dir.path()).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].title, "Star Map 1");
        assert_eq!(all[1].title, "Star Map 2");
        assert_eq!(all[1].accent_color, "#FF0000");
    }

    #[test]
    fn test_create_child_starmap() {
        let dir = setup_workspace();
        let parent = create_starmap(dir.path(), "Parent", "", None).unwrap();
        let child =
            create_child_starmap(dir.path(), &parent.starmap_id, "Child 1", "", None).unwrap();

        assert_eq!(
            child.parent_starmap_id.as_deref(),
            Some(parent.starmap_id.as_str())
        );
        assert_eq!(child.project_id, None);

        let refreshed_parent = get_starmap(dir.path(), &parent.starmap_id).unwrap();
        assert_eq!(refreshed_parent.child_starmap_count, 1);
    }

    #[test]
    fn test_bind_and_get_main_starmap() {
        let dir = setup_workspace();
        let sm = create_starmap(dir.path(), "My Map", "", None).unwrap();

        bind_starmap_to_project(dir.path(), &sm.starmap_id, "proj1").unwrap();
        set_main_starmap_for_project(dir.path(), &sm.starmap_id, "proj1").unwrap();

        let main = get_main_starmap_for_project(dir.path(), "proj1").unwrap();
        assert!(main.is_some());
        assert_eq!(main.unwrap().starmap_id, sm.starmap_id);
    }

    #[test]
    fn test_delete_cascades_children() {
        let dir = setup_workspace();
        let parent = create_starmap(dir.path(), "Parent", "", None).unwrap();
        let _child1 =
            create_child_starmap(dir.path(), &parent.starmap_id, "Child 1", "", None).unwrap();
        let _child2 =
            create_child_starmap(dir.path(), &parent.starmap_id, "Child 2", "", None).unwrap();

        delete_starmap(dir.path(), &parent.starmap_id).unwrap();

        let all = list_starmaps(dir.path()).unwrap();
        assert_eq!(all.len(), 0);
    }

    #[test]
    fn test_rename_starmap() {
        let dir = setup_workspace();
        let sm = create_starmap(dir.path(), "Old Name", "", None).unwrap();
        let renamed = rename_starmap(dir.path(), &sm.starmap_id, "New Name").unwrap();
        assert_eq!(renamed.title, "New Name");

        let from_index = list_starmaps(dir.path()).unwrap();
        assert_eq!(from_index[0].title, "New Name");
    }

    #[test]
    fn test_unbind_starmap() {
        let dir = setup_workspace();
        let sm = create_starmap(dir.path(), "Map", "", None).unwrap();
        bind_starmap_to_project(dir.path(), &sm.starmap_id, "proj1").unwrap();
        set_main_starmap_for_project(dir.path(), &sm.starmap_id, "proj1").unwrap();

        unbind_starmap_from_project(dir.path(), &sm.starmap_id).unwrap();

        let main = get_main_starmap_for_project(dir.path(), "proj1").unwrap();
        assert!(main.is_none());
    }
}
