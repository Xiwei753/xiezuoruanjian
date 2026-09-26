use serde::{Deserialize, Serialize};

use crate::starmap::semantic::{StarMapDisplayPolicy, StarMapOpenBehavior, StarMapProvenance};
use crate::starmap::types::reference::StarMapTargetPath;

/// 嵌入放置参数：位置、尺寸、缩放、层级。
///
/// 所有坐标为星图文档坐标（逻辑像素），平台渲染时乘以 dpr 转为物理像素。
/// `width`/`height` 允许为 0（折叠状态），不允许为负（验证拦截）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedPlacement {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub scale: f32,
    pub z_index: i32,
    pub collapsed: bool,
}

impl Default for StarMapEmbedPlacement {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 300.0,
            height: 200.0,
            scale: 1.0,
            z_index: 0,
            collapsed: false,
        }
    }
}

/// 嵌入目标视口：子星图在嵌入框内的初始视口参数。
///
/// `scale` 为子星图内容的缩放比，`offset_x`/`offset_y` 为子星图坐标偏移。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedViewport {
    pub scale: f32,
    pub offset_x: f32,
    pub offset_y: f32,
}

impl Default for StarMapEmbedViewport {
    fn default() -> Self {
        Self {
            scale: 1.0,
            offset_x: 0.0,
            offset_y: 0.0,
        }
    }
}

/// 星图嵌入（子星图放置实例）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbed {
    pub instance_id: String,
    pub target_starmap_id: String,
    pub label: Option<String>,
    pub display_policy: StarMapDisplayPolicy,
    pub open_behavior: StarMapOpenBehavior,
    pub placement: StarMapEmbedPlacement,
    pub target_viewport: StarMapEmbedViewport,
    pub host_path: StarMapTargetPath,
    pub provenance: StarMapProvenance,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedPatch {
    pub label: Option<Option<String>>,
    pub display_policy: Option<StarMapDisplayPolicy>,
    pub open_behavior: Option<StarMapOpenBehavior>,
    pub placement: Option<Option<StarMapEmbedPlacement>>,
    pub target_viewport: Option<Option<StarMapEmbedViewport>>,
    pub host_path: Option<StarMapTargetPath>,
}
