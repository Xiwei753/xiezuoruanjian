//! # 星图图形数据管理模块
//!
//! 本模块负责星图图形数据的持久化存储和操作，包括节点（Node）和边（Edge）的
//! 增删改查操作，以及布局（Layout）信息的管理。
//!
//! ## 主要功能
//! - **图形数据存储**：将星图的节点和边数据序列化为JSON格式存储
//! - **节点管理**：添加、更新、删除星图节点，自动同步布局信息
//! - **边管理**：添加、更新、删除星图边，验证节点存在性
//! - **布局管理**：存储和读取节点的位置、大小等布局信息
//! - **统计更新**：自动更新星图的节点数、边数和关联章节数
//!
//! ## 数据结构
//! - `StarMapGraph`：包含节点和边的图形数据结构
//! - `StarMapLayout`：包含节点布局信息的数据结构
//! - `StarMapNode`：星图节点，表示故事元素
//! - `StarMapEdge`：星图边，表示元素之间的关系
//!
//! ## 依赖关系
//! - `crate::starmap::types`：星图数据类型定义
//! - `crate::starmap`：星图元数据管理功能
//! - `crate::storage`：原子写入功能
//! - `crate::error`：错误处理
//!
//! ## 使用场景
//! - 可视化编辑器中的图形数据持久化
//! - 节点和边的CRUD操作
//! - 布局信息的保存和恢复

use crate::error::{Error, Result};
use crate::starmap::types::*;
use crate::starmap::{load_starmap_meta, now_epoch, starmaps_dir, update_starmap_stats};
use crate::storage::atomic_write_string;
use std::fs;
use std::path::Path;

fn graph_path(workspace: &Path, starmap_id: &str) -> std::path::PathBuf {
    starmaps_dir(workspace).join(starmap_id).join("graph.json")
}

fn layout_path(workspace: &Path, starmap_id: &str) -> std::path::PathBuf {
    starmaps_dir(workspace).join(starmap_id).join("layout.json")
}

pub fn get_starmap_graph(workspace: &Path, starmap_id: &str) -> Result<StarMapGraph> {
    let meta = load_starmap_meta(workspace, starmap_id)?;

    let path = graph_path(workspace, starmap_id);
    if !path.exists() {
        return Ok(StarMapGraph {
            schema_version: 1,
            id: starmap_id.to_string(),
            starmap_id: starmap_id.to_string(),
            title: meta.title.clone(),
            nodes: vec![],
            edges: vec![],
            created_at: now_epoch(),
            updated_at: now_epoch(),
        });
    }

    let json_str = fs::read_to_string(&path)?;
    let graph: StarMapGraph = serde_json::from_str(&json_str)?;
    Ok(graph)
}

pub fn save_starmap_graph(workspace: &Path, starmap_id: &str, graph: &StarMapGraph) -> Result<()> {
    validate_graph(workspace, graph)?;

    let starmap_dir = starmaps_dir(workspace).join(starmap_id);
    fs::create_dir_all(&starmap_dir)?;

    let json_str = serde_json::to_string_pretty(graph)?;
    atomic_write_string(&graph_path(workspace, starmap_id), &json_str)?;

    let node_count = graph.nodes.len() as u32;
    let edge_count = graph.edges.len() as u32;

    let mut linked_chapters = 0u32;
    for node in &graph.nodes {
        if node.kind == StarMapNodeKind::Chapter {
            linked_chapters += 1;
        }
    }

    update_starmap_stats(
        workspace,
        starmap_id,
        node_count,
        edge_count,
        linked_chapters,
    )?;
    Ok(())
}

pub fn get_starmap_layout(workspace: &Path, starmap_id: &str) -> Result<StarMapLayout> {
    let path = layout_path(workspace, starmap_id);
    if !path.exists() {
        return Ok(StarMapLayout::default());
    }

    let json_str = fs::read_to_string(&path)?;
    let layout: StarMapLayout = serde_json::from_str(&json_str)?;
    Ok(layout)
}

pub fn save_starmap_layout(
    workspace: &Path,
    starmap_id: &str,
    layout: &StarMapLayout,
) -> Result<()> {
    validate_layout(layout)?;

    let starmap_dir = starmaps_dir(workspace).join(starmap_id);
    fs::create_dir_all(&starmap_dir)?;

    let json_str = serde_json::to_string_pretty(layout)?;
    atomic_write_string(&layout_path(workspace, starmap_id), &json_str)?;
    Ok(())
}

pub fn add_starmap_node(
    workspace: &Path,
    starmap_id: &str,
    node: StarMapNode,
    default_x: f32,
    default_y: f32,
) -> Result<StarMapNode> {
    let mut graph = get_starmap_graph(workspace, starmap_id)?;
    let new_node = node.clone();
    graph.nodes.push(new_node.clone());
    graph.updated_at = now_epoch();
    save_starmap_graph(workspace, starmap_id, &graph)?;

    if let Ok(mut layout) = get_starmap_layout(workspace, starmap_id) {
        layout.nodes.push(crate::starmap::types::StarMapLayoutNode {
            node_id: new_node.id.clone(),
            x: default_x,
            y: default_y,
            width: 150.0,
            height: 60.0,
            radius: 30.0,
            collapsed: false,
            z_index: 0,
            scale: 1.0,
            depth: 0.0,
            focus_weight: 0.0,
            orbit_group: None,
        });
        let _ = save_starmap_layout(workspace, starmap_id, &layout);
    }

    Ok(new_node)
}

pub fn update_starmap_node(
    workspace: &Path,
    starmap_id: &str,
    node_id: &str,
    patch: StarMapNodePatch,
) -> Result<StarMapNode> {
    let mut graph = get_starmap_graph(workspace, starmap_id)?;
    if let Some(node) = graph.nodes.iter_mut().find(|n| n.id == node_id) {
        if let Some(t) = patch.title {
            node.title = t;
        }
        if let Some(k) = patch.kind {
            node.kind = k;
        }
        if let Some(p) = patch.payload {
            node.payload = p;
        }
        if let Some(t) = patch.tags {
            node.tags = t;
        }
        if let Some(c) = patch.content {
            node.content = c;
        }
        if let Some(a) = patch.anchors {
            node.anchors = a;
        }
        if let Some(p) = patch.portal {
            node.portal = p;
        }
        if let Some(dp) = patch.display_policy {
            node.display_policy = dp;
        }
        if let Some(ob) = patch.open_behavior {
            node.open_behavior = ob;
        }
        if let Some(p) = patch.provenance {
            node.provenance = p;
        }
        node.updated_at = now_epoch();
        let updated_node = node.clone();
        graph.updated_at = now_epoch();
        save_starmap_graph(workspace, starmap_id, &graph)?;
        Ok(updated_node)
    } else {
        Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Node not found",
        )))
    }
}

pub fn delete_starmap_node(workspace: &Path, starmap_id: &str, node_id: &str) -> Result<()> {
    let mut graph = get_starmap_graph(workspace, starmap_id)?;
    let initial_count = graph.nodes.len();
    graph.nodes.retain(|n| n.id != node_id);
    if graph.nodes.len() == initial_count {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Node not found",
        )));
    }

    graph.edges.retain(|e| e.from != node_id && e.to != node_id);

    if let Ok(mut layout) = get_starmap_layout(workspace, starmap_id) {
        layout.nodes.retain(|n| n.node_id != node_id);
        let _ = save_starmap_layout(workspace, starmap_id, &layout);
    }

    graph.updated_at = now_epoch();
    save_starmap_graph(workspace, starmap_id, &graph)?;
    Ok(())
}

pub fn add_starmap_edge(
    workspace: &Path,
    starmap_id: &str,
    edge: StarMapEdge,
) -> Result<StarMapEdge> {
    let mut graph = get_starmap_graph(workspace, starmap_id)?;

    if !graph.nodes.iter().any(|n| n.id == edge.from)
        || !graph.nodes.iter().any(|n| n.id == edge.to)
    {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Edge nodes do not exist",
        )));
    }

    let new_edge = edge.clone();
    graph.edges.push(new_edge.clone());
    graph.updated_at = now_epoch();
    save_starmap_graph(workspace, starmap_id, &graph)?;
    Ok(new_edge)
}

pub fn update_starmap_edge(
    workspace: &Path,
    starmap_id: &str,
    edge_id: &str,
    patch: StarMapEdgePatch,
) -> Result<StarMapEdge> {
    let mut graph = get_starmap_graph(workspace, starmap_id)?;
    if let Some(edge) = graph.edges.iter_mut().find(|e| e.id == edge_id) {
        if let Some(k) = patch.kind {
            edge.kind = k;
        }
        if let Some(l) = patch.label {
            edge.label = l;
        }
        if let Some(p) = patch.payload {
            edge.payload = p;
        }
        edge.updated_at = now_epoch();
        let updated_edge = edge.clone();
        graph.updated_at = now_epoch();
        save_starmap_graph(workspace, starmap_id, &graph)?;
        Ok(updated_edge)
    } else {
        Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Edge not found",
        )))
    }
}

pub fn delete_starmap_edge(workspace: &Path, starmap_id: &str, edge_id: &str) -> Result<()> {
    let mut graph = get_starmap_graph(workspace, starmap_id)?;
    let initial_count = graph.edges.len();
    graph.edges.retain(|e| e.id != edge_id);
    if graph.edges.len() == initial_count {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Edge not found",
        )));
    }
    graph.updated_at = now_epoch();
    save_starmap_graph(workspace, starmap_id, &graph)?;
    Ok(())
}

fn validate_graph(workspace: &Path, graph: &StarMapGraph) -> Result<()> {
    let mut node_ids = std::collections::HashSet::new();
    for node in &graph.nodes {
        if !node_ids.insert(&node.id) {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Duplicate node ID",
            )));
        }

        let mut anchor_ids = std::collections::HashSet::new();
        for anchor in &node.anchors {
            if !anchor_ids.insert(&anchor.anchor_id) {
                return Err(Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Duplicate anchor ID in node",
                )));
            }
        }

        if let Some(portal) = &node.portal {
            if portal.mode == crate::starmap::semantic::StarMapPortalMode::EnterChild {
                // To avoid recursive locking or parsing, just check existence of the starmap dir
                let target_dir = starmaps_dir(workspace).join(&portal.target_starmap_id);
                if !target_dir.exists() {
                    return Err(Error::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "Portal target starmap does not exist",
                    )));
                }
            }
        }

        let dp = &node.display_policy;
        if dp.importance.is_nan()
            || dp.importance < 0.0
            || dp.min_visible_scale.is_nan()
            || dp.min_visible_scale < 0.0
            || dp.title_scale.is_nan()
            || dp.title_scale < 0.0
            || dp.summary_scale.is_nan()
            || dp.summary_scale < 0.0
            || dp.detail_scale.is_nan()
            || dp.detail_scale < 0.0
            || dp.min_readable_px.is_nan()
            || dp.min_readable_px < 0.0
        {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Invalid display policy values",
            )));
        }
    }

    for edge in &graph.edges {
        if !node_ids.contains(&edge.from) || !node_ids.contains(&edge.to) {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Edge references non-existent node",
            )));
        }
    }

    Ok(())
}

fn validate_layout(layout: &StarMapLayout) -> Result<()> {
    for node in &layout.nodes {
        if node.scale <= 0.0 || node.scale.is_nan() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Layout scale must be > 0",
            )));
        }
        if node.depth.is_nan() || node.focus_weight.is_nan() {
            return Err(Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Layout depth or focus_weight cannot be NaN",
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::starmap::create_starmap;
    use crate::workspace::create_workspace;
    use tempfile::tempdir;

    fn setup_workspace() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        create_workspace(dir.path()).unwrap();
        dir
    }

    #[test]
    fn test_starmap_graph_crud() {
        let dir = setup_workspace();
        let meta = create_starmap(dir.path(), "Test Map", "desc", None).unwrap();
        crate::starmap::bind_starmap_to_project(dir.path(), &meta.starmap_id, "test_proj").unwrap();

        let mut graph = get_starmap_graph(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(graph.nodes.len(), 0);
        assert_eq!(graph.edges.len(), 0);

        let node1 = StarMapNode {
            id: "n1".to_string(),
            title: "Node 1".to_string(),
            kind: StarMapNodeKind::Note,
            payload: None,
            tags: vec![],
            content: Default::default(),
            anchors: vec![],
            portal: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            provenance: Default::default(),
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        add_starmap_node(dir.path(), &meta.starmap_id, node1.clone(), 0.0, 0.0).unwrap();

        let node2 = StarMapNode {
            id: "n2".to_string(),
            title: "Node 2".to_string(),
            kind: StarMapNodeKind::Concept,
            payload: None,
            tags: vec![],
            content: Default::default(),
            anchors: vec![],
            portal: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            provenance: Default::default(),
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        add_starmap_node(dir.path(), &meta.starmap_id, node2.clone(), 0.0, 0.0).unwrap();

        graph = get_starmap_graph(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(graph.nodes.len(), 2);

        let refreshed_meta = load_starmap_meta(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(refreshed_meta.node_count, 2);

        update_starmap_node(
            dir.path(),
            &meta.starmap_id,
            "n1",
            StarMapNodePatch {
                title: Some("Updated N1".to_string()),
                kind: Some(StarMapNodeKind::Chapter),
                payload: None,
                tags: None,
                content: None,
                anchors: None,
                portal: None,
                display_policy: None,
                open_behavior: None,
                provenance: None,
            },
        )
        .unwrap();
        graph = get_starmap_graph(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(
            graph.nodes.iter().find(|n| n.id == "n1").unwrap().title,
            "Updated N1"
        );

        let refreshed_meta2 = load_starmap_meta(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(refreshed_meta2.linked_chapter_count, 1);

        let edge = StarMapEdge {
            id: "e1".to_string(),
            from: "n1".to_string(),
            to: "n2".to_string(),
            kind: StarMapEdgeKind::RelatedTo,
            label: Some("relates".to_string()),
            payload: None,
            created_at: now_epoch(),
            updated_at: now_epoch(),
        };
        add_starmap_edge(dir.path(), &meta.starmap_id, edge.clone()).unwrap();

        graph = get_starmap_graph(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(graph.edges.len(), 1);

        update_starmap_edge(
            dir.path(),
            &meta.starmap_id,
            "e1",
            StarMapEdgePatch {
                kind: Some(StarMapEdgeKind::Causes),
                label: Some(Some("causes".to_string())),
                payload: None,
            },
        )
        .unwrap();
        graph = get_starmap_graph(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(graph.edges[0].label.as_deref(), Some("causes"));

        delete_starmap_node(dir.path(), &meta.starmap_id, "n1").unwrap();
        graph = get_starmap_graph(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(graph.nodes.len(), 1);
        assert_eq!(graph.edges.len(), 0);

        let refreshed_meta3 = load_starmap_meta(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(refreshed_meta3.node_count, 1);
        assert_eq!(refreshed_meta3.edge_count, 0);
        assert_eq!(refreshed_meta3.linked_chapter_count, 0);
    }

    #[test]
    fn test_starmap_layout_crud() {
        let dir = setup_workspace();
        let meta = create_starmap(dir.path(), "Layout Map", "desc", None).unwrap();
        crate::starmap::bind_starmap_to_project(dir.path(), &meta.starmap_id, "test_proj").unwrap();

        let mut layout = get_starmap_layout(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(layout.nodes.len(), 0);

        layout.nodes.push(StarMapLayoutNode {
            node_id: "n1".to_string(),
            x: 100.0,
            y: 200.0,
            width: 50.0,
            height: 50.0,
            radius: 25.0,
            collapsed: false,
            z_index: 0,
            scale: 1.0,
            depth: 0.0,
            focus_weight: 0.0,
            orbit_group: None,
        });

        save_starmap_layout(dir.path(), &meta.starmap_id, &layout).unwrap();

        let loaded = get_starmap_layout(dir.path(), &meta.starmap_id).unwrap();
        assert_eq!(loaded.nodes.len(), 1);
        assert_eq!(loaded.nodes[0].x, 100.0);
    }
}
