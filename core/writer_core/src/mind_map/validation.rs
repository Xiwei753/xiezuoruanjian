//! # 思维导图数据验证模块
//!
//! 本模块负责验证思维导图数据的完整性和一致性，确保图形数据符合业务规则。
//! 验证操作在图形数据的读取和保存前执行，防止无效数据进入系统。
//!
//! ## 验证规则
//!
//! ### 基本验证
//! - 图形ID不能为空
//! - 项目ID不能为空
//! - schema版本必须为2
//! - 项目必须存在
//!
//! ### 节点验证
//! - 节点ID不能为空
//! - 节点ID不能重复
//!
//! ### 边验证
//! - 边ID不能为空
//! - 边ID不能重复
//! - 边的起始节点必须存在
//! - 边的结束节点必须存在
//!
//! ### 锚点验证
//! - 锚点ID不能为空
//! - 锚点ID不能重复
//! - 锚点关联的章节必须属于该项目
//!
//! ### 链接验证
//! - 链接ID不能为空
//! - 链接ID不能重复
//! - 链接关联的节点必须存在
//! - 链接关联的锚点必须存在
//!
//! ## 错误类型
//! `ValidationError`枚举定义了所有可能的验证错误：
//! - `EmptyGraphId`：图形ID为空
//! - `EmptyProjectId`：项目ID为空
//! - `UnsupportedSchemaVersion`：不支持的schema版本
//! - `EmptyNodeId`：节点ID为空
//! - `DuplicateNodeId`：节点ID重复
//! - `EmptyEdgeId`：边ID为空
//! - `DuplicateEdgeId`：边ID重复
//! - `EdgeReferencesMissingNode`：边引用了不存在的节点
//! - `LinkReferencesMissingNode`：链接引用了不存在的节点
//! - `LinkReferencesMissingAnchor`：链接引用了不存在的锚点
//! - `AnchorChapterNotInProject`：锚点章节不属于项目
//! - `ProjectNotFound`：项目不存在
//! - `EmptyAnchorId`：锚点ID为空
//! - `DuplicateAnchorId`：锚点ID重复
//! - `EmptyLinkId`：链接ID为空
//! - `DuplicateLinkId`：链接ID重复
//!
//! ## 核心函数
//! - `validate_graph`：验证思维导图图形数据
//!
//! ## 依赖关系
//! - `crate::mind_map::graph`：思维导图数据类型
//! - `crate::facade::WriterCore`：核心门面，用于查询项目和章节信息
//! - `std::collections::HashSet`：用于检测重复ID
//!
//! ## 使用场景
//! - 数据保存前的验证
//! - 数据加载后的验证
//! - 防止无效数据进入系统
//! - 确保数据引用的完整性

use crate::mind_map::graph::MindMapGraph;
use std::collections::HashSet;

#[derive(Debug, PartialEq)]
pub enum ValidationError {
    EmptyGraphId,
    EmptyProjectId,
    UnsupportedSchemaVersion(u32),
    EmptyNodeId,
    DuplicateNodeId(String),
    EmptyEdgeId,
    DuplicateEdgeId(String),
    EdgeReferencesMissingNode(String, String), // edge_id, node_id
    LinkReferencesMissingNode(String, String), // link_id, node_id
    LinkReferencesMissingAnchor(String, String), // link_id, anchor_id
    AnchorChapterNotInProject(String, String), // anchor_id, chapter_id
    ProjectNotFound(String),
    EmptyAnchorId,
    DuplicateAnchorId(String),
    EmptyLinkId,
    DuplicateLinkId(String),
}

pub fn validate_graph(
    graph: &MindMapGraph,
    core: &crate::facade::WriterCore,
) -> Result<(), ValidationError> {
    if graph.id.is_empty() {
        return Err(ValidationError::EmptyGraphId);
    }
    if graph.project_id.is_empty() {
        return Err(ValidationError::EmptyProjectId);
    }
    if graph.schema_version != 2 {
        return Err(ValidationError::UnsupportedSchemaVersion(
            graph.schema_version,
        ));
    }

    // Validate project existence
    match core.list_projects() {
        Ok(projects) => {
            if !projects.into_iter().any(|p| p.id == graph.project_id) {
                return Err(ValidationError::ProjectNotFound(graph.project_id.clone()));
            }
        }
        Err(_) => {
            // Other errors treated as missing or invalid project for validation
            return Err(ValidationError::ProjectNotFound(graph.project_id.clone()));
        }
    }

    // Gather all valid node ids
    let mut node_ids = HashSet::new();
    for node in &graph.nodes {
        if node.id.is_empty() {
            return Err(ValidationError::EmptyNodeId);
        }
        if !node_ids.insert(node.id.clone()) {
            return Err(ValidationError::DuplicateNodeId(node.id.clone()));
        }
    }

    // Validate edges
    let mut edge_ids = HashSet::new();
    for edge in &graph.edges {
        if edge.id.is_empty() {
            return Err(ValidationError::EmptyEdgeId);
        }
        if !edge_ids.insert(edge.id.clone()) {
            return Err(ValidationError::DuplicateEdgeId(edge.id.clone()));
        }
        if !node_ids.contains(&edge.from) {
            return Err(ValidationError::EdgeReferencesMissingNode(
                edge.id.clone(),
                edge.from.clone(),
            ));
        }
        if !node_ids.contains(&edge.to) {
            return Err(ValidationError::EdgeReferencesMissingNode(
                edge.id.clone(),
                edge.to.clone(),
            ));
        }
    }

    // Pre-build the set of valid chapter ids for this project
    let valid_chapter_ids = core.list_valid_chapter_ids(&graph.project_id).unwrap_or_default();

    // Validate anchors
    let mut anchor_ids = HashSet::new();
    for anchor in &graph.anchors {
        if anchor.id.is_empty() {
            return Err(ValidationError::EmptyAnchorId);
        }
        if !anchor_ids.insert(anchor.id.clone()) {
            return Err(ValidationError::DuplicateAnchorId(anchor.id.clone()));
        }
        if !valid_chapter_ids.contains(&anchor.chapter_id) {
            return Err(ValidationError::AnchorChapterNotInProject(
                anchor.id.clone(),
                anchor.chapter_id.clone(),
            ));
        }
    }

    // Validate links
    let mut link_ids = HashSet::new();
    for link in &graph.links {
        if link.id.is_empty() {
            return Err(ValidationError::EmptyLinkId);
        }
        if !link_ids.insert(link.id.clone()) {
            return Err(ValidationError::DuplicateLinkId(link.id.clone()));
        }
        if !node_ids.contains(&link.node_id) {
            return Err(ValidationError::LinkReferencesMissingNode(
                link.id.clone(),
                link.node_id.clone(),
            ));
        }
        if !anchor_ids.contains(&link.anchor_id) {
            return Err(ValidationError::LinkReferencesMissingAnchor(
                link.id.clone(),
                link.anchor_id.clone(),
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facade::WriterCore;
    use crate::mind_map::graph::{
        MindMapEdgeKind, MindMapGraphEdge, MindMapGraphNode, MindMapNodeKind,
    };
    use tempfile::tempdir;

    #[test]
    fn test_validate_graph_missing_nodes() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let core = WriterCore::new(temp_dir.path());
        let proj = core.create_project("Test Project").unwrap();

        let graph = MindMapGraph {
            schema_version: 2,
            id: "g1".into(),
            project_id: proj.id.clone(),
            title: "Test".into(),
            nodes: vec![MindMapGraphNode {
                id: "n1".into(),
                title: "Node 1".into(),
                kind: MindMapNodeKind::Note,
                payload: None,
                tags: vec![],
                created_at: 0,
                updated_at: 0,
            }],
            edges: vec![MindMapGraphEdge {
                id: "e1".into(),
                from: "n1".into(),
                to: "n2".into(), // n2 is missing
                kind: MindMapEdgeKind::Custom,
                label: None,
                payload: None,
                created_at: 0,
                updated_at: 0,
            }],
            anchors: vec![],
            links: vec![],
            created_at: 0,
            updated_at: 0,
        };

        let result = validate_graph(&graph, &core);
        assert_eq!(
            result,
            Err(ValidationError::EdgeReferencesMissingNode(
                "e1".into(),
                "n2".into()
            ))
        );
    }

    #[test]
    fn test_validate_graph_duplicate_nodes() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let core = WriterCore::new(temp_dir.path());
        let proj = core.create_project("Test Project").unwrap();

        let graph = MindMapGraph {
            schema_version: 2,
            id: "g1".into(),
            project_id: proj.id.clone(),
            title: "Test".into(),
            nodes: vec![
                MindMapGraphNode {
                    id: "n1".into(),
                    title: "Node 1".into(),
                    kind: MindMapNodeKind::Note,
                    payload: None,
                    tags: vec![],
                    created_at: 0,
                    updated_at: 0,
                },
                MindMapGraphNode {
                    id: "n1".into(), // Duplicate
                    title: "Node 1".into(),
                    kind: MindMapNodeKind::Note,
                    payload: None,
                    tags: vec![],
                    created_at: 0,
                    updated_at: 0,
                },
            ],
            edges: vec![],
            anchors: vec![],
            links: vec![],
            created_at: 0,
            updated_at: 0,
        };

        let result = validate_graph(&graph, &core);
        assert_eq!(result, Err(ValidationError::DuplicateNodeId("n1".into())));
    }

    #[test]
    fn test_validate_graph_duplicate_anchor_id() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let core = WriterCore::new(temp_dir.path());
        let proj = core.create_project("Test Project").unwrap();
        let vol = core.create_volume(&proj.id, "Vol 1").unwrap();
        let chap = core.create_chapter(&proj.id, &vol.id, "Chap 1").unwrap();

        let graph = MindMapGraph {
            schema_version: 2,
            id: "g1".into(),
            project_id: proj.id.clone(),
            title: "Test".into(),
            nodes: vec![MindMapGraphNode {
                id: "n1".into(),
                title: "Node 1".into(),
                kind: MindMapNodeKind::Note,
                payload: None,
                tags: vec![],
                created_at: 0,
                updated_at: 0,
            }],
            edges: vec![],
            anchors: vec![
                crate::mind_map::anchor::MindMapAnchor {
                    id: "a1".into(),
                    project_id: proj.id.clone(),
                    chapter_id: chap.id.clone(),
                    start_offset: 0,
                    end_offset: 0,
                    selected_text: "".into(),
                    prefix_text: "".into(),
                    suffix_text: "".into(),
                    checksum: "".into(),
                    created_at: 0,
                    updated_at: 0,
                },
                crate::mind_map::anchor::MindMapAnchor {
                    id: "a1".into(), // Duplicate
                    project_id: proj.id.clone(),
                    chapter_id: chap.id.clone(),
                    start_offset: 0,
                    end_offset: 0,
                    selected_text: "".into(),
                    prefix_text: "".into(),
                    suffix_text: "".into(),
                    checksum: "".into(),
                    created_at: 0,
                    updated_at: 0,
                },
            ],
            links: vec![],
            created_at: 0,
            updated_at: 0,
        };

        let result = validate_graph(&graph, &core);
        assert_eq!(result, Err(ValidationError::DuplicateAnchorId("a1".into())));
    }

    #[test]
    fn test_validate_graph_empty_link_id() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let core = WriterCore::new(temp_dir.path());
        let proj = core.create_project("Test Project").unwrap();

        let graph = MindMapGraph {
            schema_version: 2,
            id: "g1".into(),
            project_id: proj.id.clone(),
            title: "Test".into(),
            nodes: vec![MindMapGraphNode {
                id: "n1".into(),
                title: "Node 1".into(),
                kind: MindMapNodeKind::Note,
                payload: None,
                tags: vec![],
                created_at: 0,
                updated_at: 0,
            }],
            edges: vec![],
            anchors: vec![],
            links: vec![crate::mind_map::anchor::MindMapLink {
                id: "".into(), // Empty
                node_id: "n1".into(),
                anchor_id: "a1".into(),
                kind: "Primary".into(),
                created_at: 0,
                updated_at: 0,
            }],
            created_at: 0,
            updated_at: 0,
        };

        let result = validate_graph(&graph, &core);
        assert_eq!(result, Err(ValidationError::EmptyLinkId));
    }

    #[test]
    fn test_validate_graph_duplicate_link_id() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let core = WriterCore::new(temp_dir.path());
        let proj = core.create_project("Test Project").unwrap();
        let vol = core.create_volume(&proj.id, "Vol 1").unwrap();
        let chap = core.create_chapter(&proj.id, &vol.id, "Chap 1").unwrap();

        let graph = MindMapGraph {
            schema_version: 2,
            id: "g1".into(),
            project_id: proj.id.clone(),
            title: "Test".into(),
            nodes: vec![MindMapGraphNode {
                id: "n1".into(),
                title: "Node 1".into(),
                kind: MindMapNodeKind::Note,
                payload: None,
                tags: vec![],
                created_at: 0,
                updated_at: 0,
            }],
            edges: vec![],
            anchors: vec![crate::mind_map::anchor::MindMapAnchor {
                id: "a1".into(),
                project_id: proj.id.clone(),
                chapter_id: chap.id.clone(),
                start_offset: 0,
                end_offset: 0,
                selected_text: "".into(),
                prefix_text: "".into(),
                suffix_text: "".into(),
                checksum: "".into(),
                created_at: 0,
                updated_at: 0,
            }],
            links: vec![
                crate::mind_map::anchor::MindMapLink {
                    id: "l1".into(),
                    node_id: "n1".into(),
                    anchor_id: "a1".into(),
                    kind: "Primary".into(),
                    created_at: 0,
                    updated_at: 0,
                },
                crate::mind_map::anchor::MindMapLink {
                    id: "l1".into(), // Duplicate
                    node_id: "n1".into(),
                    anchor_id: "a1".into(),
                    kind: "Primary".into(),
                    created_at: 0,
                    updated_at: 0,
                },
            ],
            created_at: 0,
            updated_at: 0,
        };

        let result = validate_graph(&graph, &core);
        assert_eq!(result, Err(ValidationError::DuplicateLinkId("l1".into())));
    }

    #[test]
    fn test_validate_graph_empty_anchor_id() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let core = WriterCore::new(temp_dir.path());
        let proj = core.create_project("Test Project").unwrap();
        let vol = core.create_volume(&proj.id, "Vol 1").unwrap();
        let chap = core.create_chapter(&proj.id, &vol.id, "Chap 1").unwrap();

        let graph = MindMapGraph {
            schema_version: 2,
            id: "g1".into(),
            project_id: proj.id.clone(),
            title: "Test".into(),
            nodes: vec![MindMapGraphNode {
                id: "n1".into(),
                title: "Node 1".into(),
                kind: MindMapNodeKind::Note,
                payload: None,
                tags: vec![],
                created_at: 0,
                updated_at: 0,
            }],
            edges: vec![],
            anchors: vec![crate::mind_map::anchor::MindMapAnchor {
                id: "".into(), // Empty
                project_id: proj.id.clone(),
                chapter_id: chap.id.clone(),
                start_offset: 0,
                end_offset: 0,
                selected_text: "".into(),
                prefix_text: "".into(),
                suffix_text: "".into(),
                checksum: "".into(),
                created_at: 0,
                updated_at: 0,
            }],
            links: vec![],
            created_at: 0,
            updated_at: 0,
        };

        let result = validate_graph(&graph, &core);
        assert_eq!(result, Err(ValidationError::EmptyAnchorId));
    }
}
