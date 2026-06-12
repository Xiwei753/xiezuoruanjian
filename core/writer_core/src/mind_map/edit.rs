//! # 思维导图编辑操作模块
//!
//! 本模块提供了思维导图的高级编辑操作，包括图形的创建、节点和边的增删改查、
//! 锚点管理以及布局保存等功能。所有编辑操作都包含数据验证，确保数据一致性。
//!
//! ## 主要功能
//! - **图形管理**：创建新的思维导图图形，列出项目中的所有图形
//! - **节点操作**：创建、更新、删除思维导图节点
//! - **边操作**：创建、更新、删除思维导图边
//! - **锚点操作**：创建锚点，将节点绑定到锚点
//! - **布局保存**：保存思维导图的布局信息
//! - **默认图形管理**：设置和获取项目的默认思维导图
//!
//! ## 核心函数
//! - `create_mind_map_graph`：创建新的思维导图图形
//! - `list_mind_map_graphs`：列出项目中的所有思维导图
//! - `create_mind_map_node`：创建思维导图节点
//! - `update_mind_map_node`：更新思维导图节点
//! - `delete_mind_map_node`：删除思维导图节点（支持级联删除）
//! - `create_mind_map_edge`：创建思维导图边
//! - `update_mind_map_edge`：更新思维导图边
//! - `delete_mind_map_edge`：删除思维导图边
//! - `create_mind_map_anchor`：创建文本锚点
//! - `bind_mind_map_node_to_anchor`：将节点绑定到锚点
//! - `save_mind_map_layout`：保存布局信息
//!
//! ## 数据验证
//! - 所有编辑操作都会在修改前后验证数据完整性
//! - 节点删除时检查是否被边或链接引用
//! - 边创建时验证起始和结束节点是否存在
//! - 锚点绑定时验证节点和锚点是否存在
//!
//! ## 依赖关系
//! - `crate::mind_map::graph`：思维导图数据类型
//! - `crate::mind_map::anchor`：锚点和链接类型
//! - `crate::mind_map::layout`：布局类型
//! - `crate::mind_map::storage`：存储操作
//! - `crate::mind_map::validation`：数据验证
//! - `crate::facade::WriterCore`：核心门面
//! - `serde`：JSON序列化/反序列化
//! - `uuid`：生成唯一标识符
//!
//! ## 使用场景
//! - 思维导图编辑器的后端逻辑
//! - 图形数据的CRUD操作
//! - 数据完整性保护

use crate::mind_map::anchor::{MindMapAnchor, MindMapLink};
use crate::mind_map::graph::{
    MindMapEdgeKind, MindMapGraph, MindMapGraphEdge, MindMapGraphNode, MindMapNodeKind,
};
use crate::mind_map::layout::MindMapLayout;
use crate::mind_map::storage::{self, MindMapIndex};
use crate::mind_map::validation;
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MindMapGraphMetadata {
    pub id: String,
    pub title: String,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MindMapGraphsList {
    pub default_graph_id: Option<String>,
    pub graphs: Vec<MindMapGraphMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MindMapGraphNodePatch {
    pub title: Option<String>,
    pub kind: Option<MindMapNodeKind>,
    pub payload: Option<Option<serde_json::Value>>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MindMapGraphEdgePatch {
    pub kind: Option<MindMapEdgeKind>,
    pub label: Option<Option<String>>,
    pub payload: Option<Option<serde_json::Value>>,
}

fn mutate_mind_map_graph<F, R>(
    core: &crate::facade::WriterCore,
    project_id: &str,
    graph_id: &str,
    mutator: F,
) -> crate::error::Result<R>
where
    F: FnOnce(&mut MindMapGraph) -> crate::error::Result<R>,
{
    // 1. Load graph
    let mut graph = storage::load_mind_map_graph(core, project_id, Some(graph_id))?;

    // 2. Validate loaded graph (before mutation)
    validation::validate_graph(&graph, core).map_err(|e| {
        crate::error::Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Invalid initial graph state: {:?}", e),
        ))
    })?;

    // 3. Apply mutation
    let result = mutator(&mut graph)?;

    // 4. Update timestamp
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    graph.updated_at = now;

    // 5. Validate mutated graph (after mutation)
    validation::validate_graph(&graph, core).map_err(|e| {
        crate::error::Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Invalid mutated graph state: {:?}", e),
        ))
    })?;

    // 6. Save graph
    storage::save_mind_map_graph(core, &graph)?;

    Ok(result)
}

pub fn create_mind_map_graph(
    core: &crate::facade::WriterCore,
    project_id: &str,
    title: &str,
) -> crate::error::Result<MindMapGraph> {
    let project_path = core.workspace_path().join("projects").join(project_id);
    if !project_path.exists() {
        return Err(crate::error::Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Project not found",
        )));
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let graph = MindMapGraph {
        schema_version: 2,
        id: format!("graph_{}", uuid::Uuid::new_v4()),
        project_id: project_id.to_string(),
        title: title.to_string(),
        nodes: vec![],
        edges: vec![],
        anchors: vec![],
        links: vec![],
        created_at: now,
        updated_at: now,
    };

    storage::save_mind_map_graph(core, &graph)?;
    Ok(graph)
}

pub fn list_mind_map_graphs(
    core: &crate::facade::WriterCore,
    project_id: &str,
) -> crate::error::Result<MindMapGraphsList> {
    let project_path = core.workspace_path().join("projects").join(project_id);
    if !project_path.exists() {
        return Err(crate::error::Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Project not found",
        )));
    }
    let mind_map_dir = project_path.join("mind_map");
    let index_path = mind_map_dir.join("index.json");

    let mut default_graph_id = None;
    if index_path.exists() {
        if let Ok(index_str) = fs::read_to_string(&index_path) {
            if let Ok(idx) = serde_json::from_str::<MindMapIndex>(&index_str) {
                default_graph_id = Some(idx.default_graph_id);
            }
        }
    }

    let graphs_dir = mind_map_dir.join("graphs");
    let mut graphs = Vec::new();
    if graphs_dir.exists() {
        for entry in fs::read_dir(graphs_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(json_str) = fs::read_to_string(&path) {
                    if let Ok(graph) = serde_json::from_str::<MindMapGraph>(&json_str) {
                        graphs.push(MindMapGraphMetadata {
                            id: graph.id,
                            title: graph.title,
                            created_at: graph.created_at,
                            updated_at: graph.updated_at,
                        });
                    }
                }
            }
        }
    }

    Ok(MindMapGraphsList {
        default_graph_id,
        graphs,
    })
}

pub fn set_default_mind_map_graph(
    core: &crate::facade::WriterCore,
    project_id: &str,
    graph_id: &str,
) -> crate::error::Result<()> {
    let project_path = core.workspace_path().join("projects").join(project_id);
    let mind_map_dir = project_path.join("mind_map");
    let graphs_dir = mind_map_dir.join("graphs");
    let graph_path = graphs_dir.join(format!("{}.json", graph_id));
    if !graph_path.exists() {
        return Err(crate::error::Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Graph not found: {}", graph_id),
        )));
    }
    let index_path = mind_map_dir.join("index.json");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let mut index = if index_path.exists() {
        let index_str = fs::read_to_string(&index_path)?;
        serde_json::from_str::<MindMapIndex>(&index_str).map_err(|e| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Index file corrupted: {}", e),
            ))
        })?
    } else {
        MindMapIndex {
            schema_version: 2,
            default_graph_id: graph_id.to_string(),
            graph_ids: vec![graph_id.to_string()],
            updated_at: now,
        }
    };

    index.default_graph_id = graph_id.to_string();
    index.updated_at = now;
    if !index.graph_ids.contains(&graph_id.to_string()) {
        index.graph_ids.push(graph_id.to_string());
    }

    fs::create_dir_all(&mind_map_dir)?;
    let index_str = serde_json::to_string_pretty(&index)?;
    fs::write(index_path, index_str)?;
    Ok(())
}

pub fn create_mind_map_node(
    core: &crate::facade::WriterCore,
    project_id: &str,
    graph_id: &str,
    mut node: MindMapGraphNode,
) -> crate::error::Result<MindMapGraphNode> {
    if node.id.is_empty() {
        node.id = format!("node_{}", uuid::Uuid::new_v4());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    node.created_at = now;
    node.updated_at = now;

    let node_clone = node.clone();
    mutate_mind_map_graph(core, project_id, graph_id, move |graph| {
        if graph.nodes.iter().any(|n| n.id == node.id) {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("Node already exists: {}", node.id),
            )));
        }
        graph.nodes.push(node);
        Ok(())
    })?;
    Ok(node_clone)
}

pub fn update_mind_map_node(
    core: &crate::facade::WriterCore,
    project_id: &str,
    graph_id: &str,
    node_id: &str,
    patch: MindMapGraphNodePatch,
) -> crate::error::Result<MindMapGraphNode> {
    let mut updated_node = None;
    mutate_mind_map_graph(core, project_id, graph_id, |graph| {
        let node = graph
            .nodes
            .iter_mut()
            .find(|n| n.id == node_id)
            .ok_or_else(|| {
                crate::error::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Node not found: {}", node_id),
                ))
            })?;

        if let Some(t) = patch.title {
            node.title = t;
        }
        if let Some(k) = patch.kind {
            node.kind = k;
        }
        if let Some(p) = patch.payload {
            node.payload = p;
        }
        if let Some(tg) = patch.tags {
            node.tags = tg;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        node.updated_at = now;
        updated_node = Some(node.clone());
        Ok(())
    })?;
    updated_node.ok_or_else(|| {
        crate::error::Error::Io(std::io::Error::other("Failed to retrieve updated node"))
    })
}

pub fn delete_mind_map_node(
    core: &crate::facade::WriterCore,
    project_id: &str,
    graph_id: &str,
    node_id: &str,
    cascade: bool,
) -> crate::error::Result<()> {
    mutate_mind_map_graph(core, project_id, graph_id, |graph| {
        // Check if node exists
        let node_index = graph
            .nodes
            .iter()
            .position(|n| n.id == node_id)
            .ok_or_else(|| {
                crate::error::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Node not found: {}", node_id),
                ))
            })?;

        // Check references
        let is_referenced_by_edge = graph
            .edges
            .iter()
            .any(|e| e.from == node_id || e.to == node_id);
        let is_referenced_by_link = graph.links.iter().any(|l| l.node_id == node_id);

        if is_referenced_by_edge || is_referenced_by_link {
            if !cascade {
                return Err(crate::error::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "Node {} is referenced by edges or links, cannot delete without cascade",
                        node_id
                    ),
                )));
            } else {
                // Cascade delete: remove referencing edges and links
                graph.edges.retain(|e| e.from != node_id && e.to != node_id);
                graph.links.retain(|l| l.node_id != node_id);
            }
        }

        graph.nodes.remove(node_index);
        Ok(())
    })
}

pub fn create_mind_map_edge(
    core: &crate::facade::WriterCore,
    project_id: &str,
    graph_id: &str,
    mut edge: MindMapGraphEdge,
) -> crate::error::Result<MindMapGraphEdge> {
    if edge.id.is_empty() {
        edge.id = format!("edge_{}", uuid::Uuid::new_v4());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    edge.created_at = now;
    edge.updated_at = now;

    let edge_clone = edge.clone();
    mutate_mind_map_graph(core, project_id, graph_id, move |graph| {
        if graph.edges.iter().any(|e| e.id == edge.id) {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("Edge already exists: {}", edge.id),
            )));
        }
        // Validate from/to node existence
        if !graph.nodes.iter().any(|n| n.id == edge.from) {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("From node not found: {}", edge.from),
            )));
        }
        if !graph.nodes.iter().any(|n| n.id == edge.to) {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("To node not found: {}", edge.to),
            )));
        }
        graph.edges.push(edge);
        Ok(())
    })?;
    Ok(edge_clone)
}

pub fn update_mind_map_edge(
    core: &crate::facade::WriterCore,
    project_id: &str,
    graph_id: &str,
    edge_id: &str,
    patch: MindMapGraphEdgePatch,
) -> crate::error::Result<MindMapGraphEdge> {
    let mut updated_edge = None;
    mutate_mind_map_graph(core, project_id, graph_id, |graph| {
        let edge = graph
            .edges
            .iter_mut()
            .find(|e| e.id == edge_id)
            .ok_or_else(|| {
                crate::error::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Edge not found: {}", edge_id),
                ))
            })?;

        if let Some(k) = patch.kind {
            edge.kind = k;
        }
        if let Some(l) = patch.label {
            edge.label = l;
        }
        if let Some(p) = patch.payload {
            edge.payload = p;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        edge.updated_at = now;
        updated_edge = Some(edge.clone());
        Ok(())
    })?;
    updated_edge.ok_or_else(|| {
        crate::error::Error::Io(std::io::Error::other("Failed to retrieve updated edge"))
    })
}

pub fn delete_mind_map_edge(
    core: &crate::facade::WriterCore,
    project_id: &str,
    graph_id: &str,
    edge_id: &str,
) -> crate::error::Result<()> {
    mutate_mind_map_graph(core, project_id, graph_id, |graph| {
        let edge_index = graph
            .edges
            .iter()
            .position(|e| e.id == edge_id)
            .ok_or_else(|| {
                crate::error::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Edge not found: {}", edge_id),
                ))
            })?;
        graph.edges.remove(edge_index);
        Ok(())
    })
}

pub fn create_mind_map_anchor(
    core: &crate::facade::WriterCore,
    project_id: &str,
    graph_id: &str,
    mut anchor: MindMapAnchor,
) -> crate::error::Result<MindMapAnchor> {
    if anchor.id.is_empty() {
        anchor.id = format!("anchor_{}", uuid::Uuid::new_v4());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    anchor.created_at = now;
    anchor.updated_at = now;

    if anchor.checksum.is_empty() {
        anchor.checksum = format!("{:x}", md5::compute(anchor.selected_text.as_bytes()));
    }

    let anchor_clone = anchor.clone();
    mutate_mind_map_graph(core, project_id, graph_id, move |graph| {
        if graph.anchors.iter().any(|a| a.id == anchor.id) {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("Anchor already exists: {}", anchor.id),
            )));
        }
        graph.anchors.push(anchor);
        Ok(())
    })?;
    Ok(anchor_clone)
}

pub fn bind_mind_map_node_to_anchor(
    core: &crate::facade::WriterCore,
    project_id: &str,
    graph_id: &str,
    node_id: &str,
    anchor_id: &str,
    link_kind: &str,
) -> crate::error::Result<MindMapLink> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let link = MindMapLink {
        id: format!("link_{}", uuid::Uuid::new_v4()),
        node_id: node_id.to_string(),
        anchor_id: anchor_id.to_string(),
        kind: link_kind.to_string(),
        created_at: now,
        updated_at: now,
    };
    let link_clone = link.clone();
    mutate_mind_map_graph(core, project_id, graph_id, move |graph| {
        if !graph.nodes.iter().any(|n| n.id == link.node_id) {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Node not found: {}", link.node_id),
            )));
        }
        if !graph.anchors.iter().any(|a| a.id == link.anchor_id) {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Anchor not found: {}", link.anchor_id),
            )));
        }
        if graph
            .links
            .iter()
            .any(|l| l.node_id == link.node_id && l.anchor_id == link.anchor_id)
        {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!(
                    "Link already exists between node {} and anchor {}",
                    link.node_id, link.anchor_id
                ),
            )));
        }
        graph.links.push(link);
        Ok(())
    })?;
    Ok(link_clone)
}

pub fn save_mind_map_layout(
    core: &crate::facade::WriterCore,
    project_id: &str,
    graph_id: &str,
    layout: MindMapLayout,
) -> crate::error::Result<()> {
    // 1. Load graph
    let graph = storage::load_mind_map_graph(core, project_id, Some(graph_id))?;

    // 2. Validate loaded graph
    validation::validate_graph(&graph, core).map_err(|e| {
        crate::error::Error::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Invalid graph state: {:?}", e),
        ))
    })?;

    // 3. Optional layout validation (ensure layout nodes exist in graph)
    for ln in &layout.nodes {
        if !graph.nodes.iter().any(|gn| gn.id == ln.node_id) {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Layout references nonexistent node: {}", ln.node_id),
            )));
        }
    }

    // 4. Save layout
    storage::save_mind_map_layout(core, project_id, graph_id, &layout)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facade::WriterCore;
    use crate::mind_map::graph::{
        MindMapEdgeKind, MindMapGraphEdge, MindMapGraphNode, MindMapNodeKind,
    };
    use crate::mind_map::layout::MindMapLayoutNode;
    use tempfile::tempdir;

    #[test]
    fn test_mind_map_edit_flow() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let core = WriterCore::new(temp_dir.path());
        let proj = core.create_project("Test Project").unwrap();
        let vol = core.create_volume(&proj.id, "Vol").unwrap();
        let chap = core.create_chapter(&proj.id, &vol.id, "Chap").unwrap();

        // 1. Create Graph
        let graph = create_mind_map_graph(&core, &proj.id, "Main Graph").unwrap();
        assert_eq!(graph.title, "Main Graph");

        // 2. List Graphs
        let list = list_mind_map_graphs(&core, &proj.id).unwrap();
        assert_eq!(list.default_graph_id.unwrap(), graph.id);
        assert_eq!(list.graphs.len(), 1);
        assert_eq!(list.graphs[0].title, "Main Graph");

        // 3. Create Node
        let node = MindMapGraphNode {
            id: "node_1".to_string(),
            title: "Character A".to_string(),
            kind: MindMapNodeKind::Character,
            payload: None,
            tags: vec![],
            created_at: 0,
            updated_at: 0,
        };
        let created_node = create_mind_map_node(&core, &proj.id, &graph.id, node).unwrap();
        assert_eq!(created_node.id, "node_1");

        // 4. Update Node
        let patch = MindMapGraphNodePatch {
            title: Some("Character A Updated".to_string()),
            kind: None,
            payload: None,
            tags: Some(vec!["main".to_string()]),
        };
        let updated_node =
            update_mind_map_node(&core, &proj.id, &graph.id, "node_1", patch).unwrap();
        assert_eq!(updated_node.title, "Character A Updated");
        assert_eq!(updated_node.tags, vec!["main"]);

        // 5. Create Another Node
        let node2 = MindMapGraphNode {
            id: "node_2".to_string(),
            title: "Concept B".to_string(),
            kind: MindMapNodeKind::Concept,
            payload: None,
            tags: vec![],
            created_at: 0,
            updated_at: 0,
        };
        create_mind_map_node(&core, &proj.id, &graph.id, node2).unwrap();

        // 6. Create Edge (referencing existing node_1 and node_2)
        let edge = MindMapGraphEdge {
            id: "edge_1".to_string(),
            from: "node_1".to_string(),
            to: "node_2".to_string(),
            kind: MindMapEdgeKind::RelatedTo,
            label: Some("Friend".to_string()),
            payload: None,
            created_at: 0,
            updated_at: 0,
        };
        let created_edge = create_mind_map_edge(&core, &proj.id, &graph.id, edge).unwrap();
        assert_eq!(created_edge.id, "edge_1");

        // 7. Try creating edge referencing missing node, should fail
        let bad_edge = MindMapGraphEdge {
            id: "edge_2".to_string(),
            from: "node_1".to_string(),
            to: "missing_node".to_string(),
            kind: MindMapEdgeKind::RelatedTo,
            label: None,
            payload: None,
            created_at: 0,
            updated_at: 0,
        };
        assert!(create_mind_map_edge(&core, &proj.id, &graph.id, bad_edge).is_err());

        // 8. Test delete node (referenced by edge_1), should fail without cascade
        assert!(delete_mind_map_node(&core, &proj.id, &graph.id, "node_1", false).is_err());

        // 9. Create Anchor and Bind
        let anchor = MindMapAnchor {
            id: "anchor_1".to_string(),
            project_id: proj.id.clone(),
            chapter_id: chap.id.clone(),
            start_offset: 0,
            end_offset: 10,
            selected_text: "some text".to_string(),
            prefix_text: "".to_string(),
            suffix_text: "".to_string(),
            checksum: "".to_string(),
            created_at: 0,
            updated_at: 0,
        };
        let created_anchor = create_mind_map_anchor(&core, &proj.id, &graph.id, anchor).unwrap();
        assert_eq!(created_anchor.id, "anchor_1");

        let link = bind_mind_map_node_to_anchor(
            &core, &proj.id, &graph.id, "node_1", "anchor_1", "Primary",
        )
        .unwrap();
        assert_eq!(link.node_id, "node_1");
        assert_eq!(link.anchor_id, "anchor_1");

        // 10. Generate snapshot and verify anchorCount
        let snapshot = crate::mind_map::generate_snapshot(&core, &proj.id).unwrap();
        let sn1 = snapshot.nodes.iter().find(|n| n.id == "node_1").unwrap();
        assert_eq!(sn1.anchor_count, 1);

        // 11. Test delete node with cascade (should delete edge_1 and link)
        delete_mind_map_node(&core, &proj.id, &graph.id, "node_1", true).unwrap();

        let loaded_graph = storage::load_mind_map_graph(&core, &proj.id, Some(&graph.id)).unwrap();
        assert!(!loaded_graph.nodes.iter().any(|n| n.id == "node_1"));
        assert!(!loaded_graph.edges.iter().any(|e| e.id == "edge_1"));
        assert!(!loaded_graph.links.iter().any(|l| l.node_id == "node_1"));

        // 12. Save layout and verify it is retrieved by generate_snapshot
        let layout = MindMapLayout {
            kind: crate::mind_map::layout::LayoutKind::Freeform,
            nodes: vec![MindMapLayoutNode {
                node_id: "node_2".to_string(),
                x: 150.0,
                y: 250.0,
                width: 100.0,
                height: 50.0,
                radius: 25.0,
                collapsed: true,
                z_index: 1,
            }],
        };
        save_mind_map_layout(&core, &proj.id, &graph.id, layout).unwrap();

        let snapshot2 = crate::mind_map::generate_snapshot(&core, &proj.id).unwrap();
        let sn2 = snapshot2.nodes.iter().find(|n| n.id == "node_2").unwrap();
        assert_eq!(sn2.x, 150.0);
        assert_eq!(sn2.y, 250.0);
        assert!(sn2.collapsed);

        // 13. Create another graph and set default
        let graph2 = create_mind_map_graph(&core, &proj.id, "Graph 2").unwrap();
        set_default_mind_map_graph(&core, &proj.id, &graph2.id).unwrap();

        let list2 = list_mind_map_graphs(&core, &proj.id).unwrap();
        assert_eq!(list2.default_graph_id.unwrap(), graph2.id);

        // Loading None should retrieve graph2 stably
        let default_graph = storage::load_mind_map_graph(&core, &proj.id, None).unwrap();
        assert_eq!(default_graph.id, graph2.id);
    }
}
