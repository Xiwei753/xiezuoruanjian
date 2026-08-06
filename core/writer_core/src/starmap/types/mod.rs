//! # 星图数据类型（Core 层跨平台契约）
//!
//! 定义 `StarMapDocument`、节点、边、嵌入、链接、布局等核心数据结构。
//! 这些类型通过 JSON 序列化持久化，且被 FFI 层和平台端共享。
//! 新增字段必须使用 `#[serde(default)]` 以保持向前兼容。

mod embed;
mod graph;
mod layout;
mod link;

use serde::{Deserialize, Serialize};

pub use embed::*;
pub use graph::*;
pub use layout::*;
pub use link::*;

fn default_accent_color() -> String {
    "#7B8CDE".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapDocument {
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
    #[serde(default)]
    pub graph: StarMapGraph,
    #[serde(default)]
    pub layout: StarMapLayout,
    #[serde(default)]
    pub viewport: StarMapViewport,
    #[serde(default)]
    pub child_map_placements: Vec<StarMapChildMapPlacement>,
    #[serde(default)]
    pub hyperlinks: Vec<StarMapHyperlink>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Default for StarMapDocument {
    fn default() -> Self {
        Self {
            starmap_id: String::new(),
            title: String::new(),
            description: String::new(),
            project_id: None,
            parent_starmap_id: None,
            is_main_for_project: false,
            accent_color: default_accent_color(),
            graph: StarMapGraph::default(),
            layout: StarMapLayout::default(),
            viewport: StarMapViewport::default(),
            child_map_placements: vec![],
            hyperlinks: vec![],
            created_at: 0,
            updated_at: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEndpointPath {
    #[serde(default)]
    pub segments: Vec<StarMapEndpointPathSegment>,
    pub endpoint: StarMapEdgeEndpoint,
}

impl StarMapEndpointPath {
    pub fn has_cycle(&self) -> bool {
        let mut visited = std::collections::HashSet::new();
        for seg in &self.segments {
            let StarMapEndpointPathSegment::EnterChildMap { starmap_id } = seg;
            if !visited.insert(starmap_id.clone()) {
                return true;
            }
        }
        false
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StarMapEndpointPathSegment {
    EnterChildMap { starmap_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapChildMapPlacement {
    pub instance_id: String,
    pub target_starmap_id: String,
    pub placement: StarMapEmbedPlacement,
    #[serde(default)]
    pub target_viewport: StarMapEmbedViewport,
    #[serde(default)]
    pub display_policy: crate::starmap::semantic::StarMapDisplayPolicy,
    #[serde(default)]
    pub open_behavior: crate::starmap::semantic::StarMapOpenBehavior,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapHyperlink {
    pub hyperlink_id: String,
    pub source: StarMapEndpointPath,
    pub target_uri: String,
    pub label: Option<String>,
    #[serde(default)]
    pub target_starmap_id: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}
