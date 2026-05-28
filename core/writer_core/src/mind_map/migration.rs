//! # 思维导图数据迁移模块
//!
//! 本模块负责将旧版本（V1）的思维导图数据迁移到新版本（V2）格式。
//! 支持自动检测和转换，确保数据兼容性。
//!
//! ## V1格式特点
//! - 单个`mind_map.json`文件包含所有数据
//! - 节点类型为字符串格式
//! - 布局信息混合在节点数据中
//! - 没有独立的索引文件
//!
//! ## V2格式特点
//! - 分离的图形数据和布局数据
//! - 独立的索引文件管理多个图形
//! - 节点类型为枚举格式
//! - 支持锚点和链接
//! - 更好的数据组织结构
//!
//! ## 迁移流程
//! 1. 尝试解析为V2格式，如果成功且版本正确则直接返回
//! 2. 尝试解析为V1格式
//! 3. 将V1节点转换为V2节点：
//!    - 转换节点类型字符串为枚举值
//!    - 移除布局相关字段
//!    - 添加时间戳
//! 4. 将V1边转换为V2边：
//!    - 转换边类型字符串为枚举值
//!    - 生成唯一ID
//!    - 添加时间戳
//! 5. 保留原有的锚点和链接数据
//! 6. 设置schema版本为2
//!
//! ## 错误处理
//! - 不支持的schema版本会返回错误
//! - JSON解析失败会返回错误
//! - 迁移失败时原数据保持不变
//!
//! ## 核心函数
//! - `migrate_graph_schema`：迁移思维导图数据格式
//!
//! ## 依赖关系
//! - `crate::mind_map::graph`：V2数据类型
//! - `crate::mind_map::anchor`：锚点和链接类型
//! - `crate::error`：错误处理
//! - `serde`：JSON序列化/反序列化
//!
//! ## 使用场景
//! - 自动迁移旧版本数据
//! - 支持多版本兼容
//! - 平滑升级用户数据

use crate::mind_map::anchor::{MindMapAnchor, MindMapLink};
use crate::mind_map::graph::{
    MindMapEdgeKind, MindMapGraph, MindMapGraphEdge, MindMapGraphNode, MindMapNodeKind,
};

// Represents V1 structures for parsing old files
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct V1MindMapNode {
    pub id: String,
    pub title: String,
    pub kind: String, // String in V1
    // Layout and style were mixed in
    #[allow(dead_code)]
    pub parent_id: Option<String>,
    #[allow(dead_code)]
    pub depth: i32,
    #[allow(dead_code)]
    pub x: f32,
    #[allow(dead_code)]
    pub y: f32,
    #[allow(dead_code)]
    pub radius: f32,
    #[allow(dead_code)]
    pub width: f32,
    #[allow(dead_code)]
    pub height: f32,
    #[allow(dead_code)]
    pub collapsed: bool,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct V1MindMapEdge {
    pub from: String,
    pub to: String,
    pub kind: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct V1MindMapGraph {
    pub id: String,
    pub nodes: Vec<V1MindMapNode>,
    pub edges: Vec<V1MindMapEdge>,
    #[serde(default)]
    pub anchors: Vec<MindMapAnchor>,
    #[serde(default)]
    pub links: Vec<MindMapLink>,
}

pub fn migrate_graph_schema(
    json_str: &str,
    project_id: &str,
) -> Result<MindMapGraph, crate::error::Error> {
    // Try to parse as V2 first
    if let Ok(v2_graph) = serde_json::from_str::<MindMapGraph>(json_str) {
        if v2_graph.schema_version == 2 {
            return Ok(v2_graph);
        } else {
            return Err(crate::error::Error::Json(serde::de::Error::custom(
                "Unsupported schema version",
            )));
        }
    }

    // Try parsing as V1
    let v1_graph: V1MindMapGraph = serde_json::from_str(json_str)?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let v2_nodes = v1_graph
        .nodes
        .into_iter()
        .map(|n| {
            let kind = match n.kind.as_str() {
                "Project" => MindMapNodeKind::Project,
                "Volume" => MindMapNodeKind::Volume,
                "Chapter" => MindMapNodeKind::Chapter,
                "Character" => MindMapNodeKind::Character,
                "Event" => MindMapNodeKind::Event,
                "Location" => MindMapNodeKind::Location,
                _ => MindMapNodeKind::Custom,
            };

            MindMapGraphNode {
                id: n.id,
                title: n.title,
                kind,
                payload: None,
                tags: vec![],
                created_at: now,
                updated_at: now,
            }
        })
        .collect();

    let v2_edges = v1_graph
        .edges
        .into_iter()
        .enumerate()
        .map(|(i, e)| {
            let kind = match e.kind.as_str() {
                "hierarchy" => MindMapEdgeKind::Contains,
                "References" => MindMapEdgeKind::References,
                _ => MindMapEdgeKind::Custom,
            };
            MindMapGraphEdge {
                id: format!("migrated_edge_{}_{}_{}", i, e.from, e.to),
                from: e.from,
                to: e.to,
                kind,
                label: None,
                payload: None,
                created_at: now,
                updated_at: now,
            }
        })
        .collect();

    Ok(MindMapGraph {
        schema_version: 2,
        id: v1_graph.id,
        project_id: project_id.to_string(),
        title: "Migrated Graph".to_string(),
        nodes: v2_nodes,
        edges: v2_edges,
        anchors: v1_graph.anchors,
        links: v1_graph.links,
        created_at: now,
        updated_at: now,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migration_from_v1() {
        let v1_json = r#"{
            "id": "graph_1",
            "nodes": [
                {
                    "id": "node_1",
                    "title": "Character A",
                    "kind": "Character",
                    "parentId": null,
                    "depth": 0,
                    "x": 10.0,
                    "y": 20.0,
                    "radius": 10.0,
                    "width": 100.0,
                    "height": 50.0,
                    "collapsed": false
                }
            ],
            "edges": [
                {
                    "from": "node_1",
                    "to": "node_2",
                    "kind": "References"
                }
            ]
        }"#;

        let migrated = migrate_graph_schema(v1_json, "p1").unwrap();
        assert_eq!(migrated.schema_version, 2);
        assert_eq!(migrated.project_id, "p1");
        assert_eq!(migrated.nodes.len(), 1);
        assert_eq!(
            migrated.nodes[0].kind,
            crate::mind_map::graph::MindMapNodeKind::Character
        );
        assert_eq!(migrated.edges.len(), 1);
        assert_eq!(
            migrated.edges[0].kind,
            crate::mind_map::graph::MindMapEdgeKind::References
        );
    }

    #[test]
    fn test_unknown_schema_version_returns_error() {
        let future_json = r#"{
            "schemaVersion": 999,
            "id": "g1",
            "projectId": "p1",
            "title": "Future",
            "nodes": [],
            "edges": [],
            "anchors": [],
            "links": [],
            "createdAt": 0,
            "updatedAt": 0
        }"#;

        let result = migrate_graph_schema(future_json, "p1");
        assert!(result.is_err());
    }
}
