//! # 图服务模块 (Graph Service)
//!
//! 本模块实现了知识图谱和关系图的管理功能，用于可视化和管理写作项目中的复杂关系。
//!
//! ## 主要功能
//!
//! - **节点管理**: 支持多种节点类型（书籍、角色、组织、地点、事件、概念、章节等）
//! - **边关系管理**: 支持多种关系类型（关系、血缘、阵营、敌对、依赖、因果、伏笔等）
//! - **多视图模式**: 支持平面视图和星球视图两种展示模式
//! - **作用域控制**: 支持全局图谱和项目级图谱
//! - **位置管理**: 支持 2D 和 3D 坐标定位
//! - **持久化存储**: 将图谱数据保存为 JSON 文件
//!
//! ## 节点类型
//!
//! - `Book`: 书籍
//! - `Character`: 角色
//! - `Organization`: 组织
//! - `Location`: 地点
//! - `Event`: 事件
//! - `Concept`: 概念
//! - `Chapter`: 章节
//! - `Custom`: 自定义类型
//!
//! ## 边类型
//!
//! - `Relationship`: 一般关系
//! - `Bloodline`: 血缘关系
//! - `Faction`: 阵营关系
//! - `Hostile`: 敌对关系
//! - `Dependency`: 依赖关系
//! - `Causality`: 因果关系
//! - `Foreshadowing`: 伏笔关系
//! - `Timeline`: 时间线关系
//! - `Emotion`: 情感关系
//! - `Custom`: 自定义关系
//!
//! ## 依赖关系
//!
//! - `serde` / `serde_json`: 序列化/反序列化
//! - `tempfile`: 原子写入临时文件
//! - `std::fs`: 文件系统操作
//!
//! ## 使用场景
//!
//! - 小说人物关系图
//! - 故事情节脉络梳理
//! - 世界观设定管理
//! - 写作大纲可视化

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GraphScope {
    Global,
    Project,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GraphViewMode {
    Flat,
    Planet,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GraphNodeType {
    Book,
    Character,
    Organization,
    Location,
    Event,
    Concept,
    Chapter,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GraphEdgeType {
    Relationship,
    Bloodline,
    Faction,
    Hostile,
    Dependency,
    Causality,
    Foreshadowing,
    Timeline,
    Emotion,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum GraphArrowType {
    None,
    OneWay,
    TwoWay,
    Dashed,
    Dotted,
    Thick,
    Weak,
    Strong,
    Conflict,
    Hidden,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position2D {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position3D {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub label: String,
    pub node_type: GraphNodeType,
    pub position2d: Option<Position2D>,
    pub position3d: Option<Position3D>,
    pub group: Option<String>,
    pub tags: Vec<String>,
    pub note: Option<String>,
    pub linked_project_id: Option<String>,
    pub linked_character_id: Option<String>,
    pub linked_chapter_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub id: String,
    pub source: String,
    pub target: String,
    pub edge_type: GraphEdgeType,
    pub arrow_type: GraphArrowType,
    pub label: Option<String>,
    pub description: Option<String>,
    pub note: Option<String>,
    pub color: Option<String>,
    pub weight: Option<f64>,
    pub direction: Option<String>,
    pub style: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphDocument {
    pub version: u32,
    pub scope: GraphScope,
    pub view_mode: GraphViewMode,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

impl GraphDocument {
    pub fn new(scope: GraphScope) -> Self {
        Self {
            version: 1,
            scope,
            view_mode: GraphViewMode::Flat,
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }
}

pub struct GraphService {
    workspace_path: PathBuf,
}

impl GraphService {
    pub fn new<P: AsRef<Path>>(workspace_path: P) -> Self {
        Self {
            workspace_path: workspace_path.as_ref().to_path_buf(),
        }
    }

    fn get_graph_file_path(&self, project_id: Option<&str>) -> PathBuf {
        if let Some(pid) = project_id {
            self.workspace_path
                .join("projects")
                .join(pid)
                .join("graph.json")
        } else {
            self.workspace_path.join("global_graph.json")
        }
    }

    pub fn load_graph(&self, project_id: Option<&str>) -> crate::Result<GraphDocument> {
        let path = self.get_graph_file_path(project_id);
        if !path.exists() {
            let scope = if project_id.is_some() {
                GraphScope::Project
            } else {
                GraphScope::Global
            };
            return Ok(GraphDocument::new(scope));
        }

        let content = fs::read_to_string(&path).map_err(crate::Error::Io)?;

        let doc: GraphDocument = serde_json::from_str(&content).map_err(crate::Error::Json)?;

        Ok(doc)
    }

    pub fn save_graph(&self, project_id: Option<&str>, doc: &GraphDocument) -> crate::Result<()> {
        let path = self.get_graph_file_path(project_id);

        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(crate::Error::Io)?;
            }
        }

        let temp_dir = path.parent().unwrap_or(Path::new(""));
        let mut temp_file = tempfile::Builder::new()
            .prefix("graph_")
            .suffix(".tmp")
            .tempfile_in(temp_dir)
            .map_err(crate::Error::Io)?;

        let json = serde_json::to_string_pretty(doc).map_err(crate::Error::Json)?;

        use std::io::Write;
        temp_file
            .write_all(json.as_bytes())
            .map_err(crate::Error::Io)?;
        temp_file.flush().map_err(crate::Error::Io)?;

        temp_file
            .persist(&path)
            .map_err(|e| crate::Error::Io(e.into()))?;

        Ok(())
    }

    pub fn generate_graph(&self) -> crate::Result<()> {
        Err(crate::Error::NotImplemented)
    }
}
