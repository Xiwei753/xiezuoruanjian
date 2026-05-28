//! # 思维导图模块 (MindMap Module)
//!
//! 本模块实现了思维导图（MindMap）功能，用于可视化管理写作项目中的故事结构、
//! 角色关系、情节发展等创作要素。思维导图是一种图形化的组织工具，
//! 帮助作者构建和展示复杂的故事元素之间的层次和关联关系。
//!
//! ## 主要功能
//! - **图形管理**：创建、读取、更新、删除思维导图图形数据
//! - **节点管理**：管理思维导图中的节点，支持多种节点类型
//! - **边管理**：管理节点之间的关系边，支持多种关系类型
//! - **布局计算**：自动计算节点的布局位置，支持多种布局算法
//! - **快照生成**：生成思维导图的只读快照，用于前端渲染
//! - **数据验证**：验证思维导图数据的完整性和一致性
//! - **数据迁移**：支持从旧版本格式迁移到新版本
//! - **锚点管理**：管理思维导图节点与章节内容的关联锚点
//! - **存储管理**：处理思维导图数据的持久化存储
//!
//! ## 子模块
//! - `anchor`：锚点和链接管理
//! - `edit`：图形编辑操作
//! - `graph`：数据类型定义
//! - `layout`：布局算法
//! - `migration`：数据迁移
//! - `snapshot`：快照生成
//! - `storage`：存储管理
//! - `validation`：数据验证
//!
//! ## 依赖关系
//! - `crate::facade::WriterCore`：核心门面，提供项目和章节管理功能
//! - `crate::error`：错误处理模块
//! - `serde`：JSON序列化/反序列化
//! - `uuid`：生成唯一标识符
//!
//! ## 使用场景
//! - 为小说项目创建故事结构图
//! - 管理角色关系网络
//! - 组织情节发展线索
//! - 支持多种布局算法的可视化展示
//! - 与章节内容建立关联锚点

pub mod anchor;
pub mod edit;
pub mod graph;
pub mod layout;
pub mod migration;
pub mod snapshot;
pub mod storage;
pub mod validation;

pub use anchor::{MindMapAnchor, MindMapLink};
pub use graph::{
    MindMapEdgeKind, MindMapGraph, MindMapGraphEdge, MindMapGraphNode, MindMapNodeKind,
};
pub use layout::{LayoutKind, MindMapLayout, MindMapLayoutNode};
pub use snapshot::{MindMapBounds, MindMapSnapshot, MindMapSnapshotEdge, MindMapSnapshotNode};

pub fn generate_snapshot(
    core: &crate::facade::WriterCore,
    project_id: &str,
) -> crate::error::Result<MindMapSnapshot> {
    // 1. Check if project exists
    let project = core
        .list_projects()?
        .into_iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Project not found",
            ))
        })?;

    // 2. Try to load an existing graph
    match storage::load_mind_map_graph(core, project_id, None) {
        Ok(graph) => {
            // Found a custom graph (or migrated V1).
            // Must validate before generating snapshot.
            validation::validate_graph(&graph, core).map_err(|e| {
                crate::error::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("{:?}", e),
                ))
            })?;

            // Try to load layout, fallback to calculate_layout if not found
            let layout = match storage::load_mind_map_layout(core, project_id, &graph.id) {
                Ok(layout) => layout,
                Err(_) => layout::calculate_layout(&graph, LayoutKind::Freeform),
            };
            Ok(snapshot::generate_snapshot(&graph, &layout))
        }
        Err(e) => {
            // Check if error was just not found, if so fallback to auto graph
            if let crate::error::Error::Io(io_err) = &e {
                if io_err.kind() == std::io::ErrorKind::NotFound {
                    return generate_auto_graph_snapshot(core, project_id, &project.title);
                }
            }
            // Some other error like JSON parsing or unsupported schema, return it
            Err(e)
        }
    }
}

fn generate_auto_graph_snapshot(
    core: &crate::facade::WriterCore,
    project_id: &str,
    project_title: &str,
) -> crate::error::Result<MindMapSnapshot> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    // Add project root node
    nodes.push(MindMapGraphNode {
        id: project_id.to_string(),
        title: project_title.to_string(),
        kind: MindMapNodeKind::Project,
        payload: None,
        tags: vec![],
        created_at: now,
        updated_at: now,
    });

    let volumes = core.list_volumes(project_id)?;

    for vol in volumes {
        nodes.push(MindMapGraphNode {
            id: vol.id.clone(),
            title: vol.title.clone(),
            kind: MindMapNodeKind::Volume,
            payload: None,
            tags: vec![],
            created_at: now,
            updated_at: now,
        });

        edges.push(MindMapGraphEdge {
            id: format!("edge_{}_{}", project_id, vol.id),
            from: project_id.to_string(),
            to: vol.id.clone(),
            kind: MindMapEdgeKind::Contains,
            label: None,
            payload: None,
            created_at: now,
            updated_at: now,
        });

        let chapters = core.list_chapters(project_id, &vol.id)?;
        for chap in chapters {
            nodes.push(MindMapGraphNode {
                id: chap.id.clone(),
                title: chap.title.clone(),
                kind: MindMapNodeKind::Chapter,
                payload: None,
                tags: vec![],
                created_at: now,
                updated_at: now,
            });

            edges.push(MindMapGraphEdge {
                id: format!("edge_{}_{}", vol.id, chap.id),
                from: vol.id.clone(),
                to: chap.id.clone(),
                kind: MindMapEdgeKind::Contains,
                label: None,
                payload: None,
                created_at: now,
                updated_at: now,
            });
        }
    }

    let graph = MindMapGraph {
        schema_version: 2,
        id: format!("auto_graph_{}", project_id),
        project_id: project_id.to_string(),
        title: "Auto Graph".to_string(),
        nodes,
        edges,
        anchors: vec![],
        links: vec![],
        created_at: now,
        updated_at: now,
    };

    let layout = layout::calculate_layout(&graph, LayoutKind::AutoRadial);
    Ok(snapshot::generate_snapshot(&graph, &layout))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facade::WriterCore;
    use tempfile::tempdir;

    #[test]
    fn test_generate_auto_graph() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let core = WriterCore::new(temp_dir.path());
        let proj = core.create_project("Test Project").unwrap();
        let vol = core.create_volume(&proj.id, "Test Volume").unwrap();
        core.create_chapter(&proj.id, &vol.id, "Test Chapter 1")
            .unwrap();

        let snapshot = generate_snapshot(&core, &proj.id).unwrap();

        // 1 proj + 1 default vol + 1 custom vol + 1 chap
        assert!(snapshot.nodes.len() >= 4);
        assert_eq!(snapshot.project_id, proj.id);

        // Auto graph falls back to radial
        assert_eq!(snapshot.layout_kind, "AutoRadial");
    }

    #[test]
    fn test_custom_graph_generation() {
        let temp_dir = tempdir().unwrap();
        crate::workspace::create_workspace(temp_dir.path()).unwrap();
        let core = WriterCore::new(temp_dir.path());
        let proj = core.create_project("Test Project").unwrap();

        let custom_graph = MindMapGraph {
            schema_version: 2,
            id: "g1".into(),
            project_id: proj.id.clone(),
            title: "Custom".into(),
            nodes: vec![MindMapGraphNode {
                id: "n1".into(),
                title: "Custom Node".into(),
                kind: MindMapNodeKind::Character,
                payload: None,
                tags: vec![],
                created_at: 0,
                updated_at: 0,
            }],
            edges: vec![],
            anchors: vec![],
            links: vec![],
            created_at: 0,
            updated_at: 0,
        };

        crate::mind_map::storage::save_mind_map_graph(&core, &custom_graph).unwrap();

        let snapshot = generate_snapshot(&core, &proj.id).unwrap();

        assert_eq!(snapshot.layout_kind, "Freeform");
        assert_eq!(snapshot.nodes.len(), 1);
        assert_eq!(snapshot.nodes[0].title, "Custom Node");
    }
}
