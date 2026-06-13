//! # 星图模块 (StarMap Module)
//!
//! 本模块实现了星图（StarMap）功能，用于可视化管理写作项目中的世界观元素、
//! 角色关系、情节线索等创作要素。星图是一种结构化的知识图谱工具，
//! 帮助作者组织和展示复杂的故事元素之间的关系。
//!
//! ## 主要功能
//! - **星图元数据管理**：创建、读取、更新、删除星图的基本信息
//! - **星图索引管理**：维护工作区中所有星图的索引，支持快速查询
//! - **项目关联**：将星图绑定到特定项目，支持设置项目主星图
//! - **布局算法**：grid / radial 自动布局（Core 层，跨端共享）
//! - **命中测试**：节点 AABB / 边线段距离（Core 层，跨端共享）

pub mod graph;
pub mod hittest;
pub mod layout;
pub mod legacy_migration;
pub mod package_storage;
pub mod render;
pub mod semantic;
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

pub(crate) fn now_epoch() -> u64 {
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

#[cfg(test)]
pub fn create_child_starmap_legacy(
    workspace: &Path,
    parent_id: &str,
    title: &str,
    description: &str,
    accent_color: Option<&str>,
) -> Result<StarMapMeta> {
    create_child_starmap(workspace, parent_id, title, description, accent_color)
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
    // Before deleting, check if it's referenced by any EXTERNAL StarMap.
    let refs = find_starmap_references(workspace, starmap_id)?;
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

    let meta = load_starmap_meta(workspace, starmap_id)?;

    // Remove from parent's child count (if parent exists)
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

    let graph_dir = starmaps_dir(workspace).join(starmap_id);
    if graph_dir.exists() {
        let _ = fs::remove_dir_all(&graph_dir);
    }

    Ok(())
}

#[cfg(test)]
pub fn delete_starmap_cascade_legacy(workspace: &Path, starmap_id: &str) -> Result<()> {
    // Delete children first
    let idx = load_index(workspace)?;
    for child in &idx.starmaps {
        if child.parent_starmap_id.as_deref() == Some(starmap_id) {
            let _ = delete_starmap_cascade_legacy(workspace, &child.starmap_id);
        }
    }

    delete_starmap_meta(workspace, starmap_id)?;

    let mut idx = load_index(workspace)?;
    idx.starmaps.retain(|m| m.starmap_id != starmap_id);
    idx.updated_at = now_epoch();
    save_index(workspace, &idx)?;

    // Clean up the underlying starmap graph directory
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
        _ => false,
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

pub fn find_starmap_references(
    workspace: &Path,
    target_starmap_id: &str,
) -> Result<Vec<StarMapReference>> {
    let mut refs = Vec::new();
    let idx = load_index(workspace)?;

    for m in &idx.starmaps {
        if let Ok(graph) = crate::starmap::graph::get_starmap_graph(workspace, &m.starmap_id) {
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
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn setup_workspace() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        dir
    }

    #[test]
    fn test_starmap_graph_path() {
        let workspace = std::path::Path::new("/dummy/workspace");
        let starmap_id = "test_starmap_id";

        let path = starmap_graph_path(workspace, starmap_id);

        let expected = std::path::PathBuf::from("/dummy/workspace/app-meta/starmaps/test_starmap_id/graph.json");
        assert_eq!(path, expected);
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
    fn test_create_child_starmap_legacy() {
        let dir = setup_workspace();
        let parent = create_starmap(dir.path(), "Parent", "", None).unwrap();
        let child =
            create_child_starmap_legacy(dir.path(), &parent.starmap_id, "Child 1", "", None)
                .unwrap();

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
    fn test_delete_cascades_children_legacy() {
        let dir = setup_workspace();
        let parent = create_starmap(dir.path(), "Parent", "", None).unwrap();
        let _child1 =
            create_child_starmap_legacy(dir.path(), &parent.starmap_id, "Child 1", "", None)
                .unwrap();
        let _child2 =
            create_child_starmap_legacy(dir.path(), &parent.starmap_id, "Child 2", "", None)
                .unwrap();

        delete_starmap_cascade_legacy(dir.path(), &parent.starmap_id).unwrap();

        let all = list_starmaps(dir.path()).unwrap();
        assert_eq!(all.len(), 0);
    }

    #[test]
    fn test_delete_starmap_no_cascade() {
        let dir = setup_workspace();
        let parent = create_starmap(dir.path(), "Parent", "", None).unwrap();
        let child1 =
            create_child_starmap_legacy(dir.path(), &parent.starmap_id, "Child 1", "", None)
                .unwrap();
        let child2 =
            create_child_starmap_legacy(dir.path(), &parent.starmap_id, "Child 2", "", None)
                .unwrap();

        delete_starmap(dir.path(), &parent.starmap_id).unwrap();

        let all = list_starmaps(dir.path()).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all.iter().any(|m| m.starmap_id == child1.starmap_id));
        assert!(all.iter().any(|m| m.starmap_id == child2.starmap_id));
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

    #[test]
    fn test_delete_starmap_edge_protection() {
        let dir = setup_workspace();
        let parent = create_starmap(dir.path(), "Parent", "", None).unwrap();
        let child = create_starmap(dir.path(), "Child", "", None).unwrap();

        // 1. Add internal edge -> shouldn't block delete
        let mut parent_graph =
            crate::starmap::graph::get_starmap_graph(dir.path(), &parent.starmap_id).unwrap();
        let internal_edge = crate::starmap::types::StarMapEdge {
            id: "internal_e".to_string(),
            from: None,
            to: None,
            kind: crate::starmap::types::StarMapEdgeKind::RelatedTo,
            label: None,
            payload: None,
            from_target: None,
            to_target: None,
            from_endpoint: Some(crate::starmap::types::StarMapEdgeEndpoint::Starmap),
            to_endpoint: Some(crate::starmap::types::StarMapEdgeEndpoint::DeepTarget {
                target: crate::starmap::semantic::StarMapDeepTarget {
                    starmap_id: parent.starmap_id.clone(),
                    path: vec![],
                    target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
                },
            }),
            from_endpoint_path: None,
            to_endpoint_path: None,
            created_at: 0,
            updated_at: 0,
        };
        parent_graph.edges.push(internal_edge);
        crate::starmap::graph::save_starmap_graph(dir.path(), &parent.starmap_id, &parent_graph)
            .unwrap();

        // Deleting parent should still work since it's an internal reference,
        // but we won't delete parent yet, we need it.

        // 2. Add external edge in parent pointing to child
        let mut parent_graph =
            crate::starmap::graph::get_starmap_graph(dir.path(), &parent.starmap_id).unwrap();
        let external_edge = crate::starmap::types::StarMapEdge {
            id: "external_e".to_string(),
            from: None,
            to: None,
            kind: crate::starmap::types::StarMapEdgeKind::RelatedTo,
            label: None,
            payload: None,
            from_target: None,
            to_target: None,
            from_endpoint: None,
            to_endpoint: Some(crate::starmap::types::StarMapEdgeEndpoint::DeepTarget {
                target: crate::starmap::semantic::StarMapDeepTarget {
                    starmap_id: child.starmap_id.clone(),
                    path: vec![],
                    target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
                },
            }),
            from_endpoint_path: None,
            to_endpoint_path: None,
            created_at: 0,
            updated_at: 0,
        };
        parent_graph.edges.push(external_edge);
        crate::starmap::graph::save_starmap_graph(dir.path(), &parent.starmap_id, &parent_graph)
            .unwrap();

        // Check find_starmap_references discovers it
        let refs = find_starmap_references(dir.path(), &child.starmap_id).unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].ref_type, "edge");
        assert_eq!(refs[0].ref_id, "external_e");

        // Try deleting child -> should fail
        assert!(delete_starmap(dir.path(), &child.starmap_id).is_err());

        // Delete the external edge
        crate::starmap::graph::delete_starmap_edge(dir.path(), &parent.starmap_id, "external_e")
            .unwrap();

        // 3. Add external edge with from_endpoint and EnterChild path
        let mut parent_graph =
            crate::starmap::graph::get_starmap_graph(dir.path(), &parent.starmap_id).unwrap();
        let external_edge_2 = crate::starmap::types::StarMapEdge {
            id: "external_e2".to_string(),
            from: None,
            to: None,
            kind: crate::starmap::types::StarMapEdgeKind::RelatedTo,
            label: None,
            payload: None,
            from_target: None,
            to_target: None,
            from_endpoint: Some(crate::starmap::types::StarMapEdgeEndpoint::DeepTarget {
                target: crate::starmap::semantic::StarMapDeepTarget {
                    starmap_id: parent.starmap_id.clone(),
                    path: vec![crate::starmap::semantic::StarMapPathSegment::EnterChild {
                        starmap_id: child.starmap_id.clone(),
                    }],
                    target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
                },
            }),
            to_endpoint: None,
            from_endpoint_path: None,
            to_endpoint_path: None,
            created_at: 0,
            updated_at: 0,
        };
        parent_graph.edges.push(external_edge_2);
        crate::starmap::graph::save_starmap_graph(dir.path(), &parent.starmap_id, &parent_graph)
            .unwrap();

        // Check find_starmap_references discovers the EnterChild path from from_endpoint
        let refs2 = find_starmap_references(dir.path(), &child.starmap_id).unwrap();
        assert_eq!(refs2.len(), 1);
        assert_eq!(refs2[0].ref_type, "edge");
        assert_eq!(refs2[0].ref_id, "external_e2");

        // Try deleting child -> should fail
        assert!(delete_starmap(dir.path(), &child.starmap_id).is_err());

        // Delete external edge 2
        crate::starmap::graph::delete_starmap_edge(dir.path(), &parent.starmap_id, "external_e2")
            .unwrap();

        // Try deleting child again -> should succeed
        assert!(delete_starmap(dir.path(), &child.starmap_id).is_ok());

        // Try deleting parent -> should succeed (internal edge shouldn't block)
        assert!(delete_starmap(dir.path(), &parent.starmap_id).is_ok());
    }
}
