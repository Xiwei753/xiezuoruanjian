use serde::{Deserialize, Serialize};

use super::layout::StarMapViewport;

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
///
/// 使用自定义 `Deserialize` 实现以兼容旧格式：
/// - 旧格式 `viewport` 字段（`StarMapViewport`）会被合并到新格式的
///   `placement`（width/height）和 `target_viewport`（scale/offset）中。
/// - 旧格式 `host_anchor` 字符串会被转换为 `host_endpoint::Anchor`，
///   但需要 `source_node_id` 同时存在才能构造完整端点。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbed {
    pub instance_id: String,
    pub target_starmap_id: String,
    pub label: Option<String>,
    pub display_policy: crate::starmap::semantic::StarMapDisplayPolicy,
    pub open_behavior: crate::starmap::semantic::StarMapOpenBehavior,
    pub placement: StarMapEmbedPlacement,
    pub target_viewport: StarMapEmbedViewport,
    pub source_node_id: Option<String>,
    pub host_endpoint: Option<StarMapEndpoint>,
    pub provenance: crate::starmap::semantic::StarMapProvenance,
    pub created_at: u64,
    pub updated_at: u64,
}

/// 自定义反序列化：兼容旧格式字段迁移。
///
/// 1. 旧 `viewport` → 新 `placement.width/height` + `target_viewport.scale/offset`
/// 2. 旧 `host_anchor` (String) → 新 `host_endpoint::Anchor { node_id, anchor_id }`
///    （需要 `source_node_id` 同时存在，否则 host_anchor 被丢弃）
impl<'de> Deserialize<'de> for StarMapEmbed {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            instance_id: String,
            target_starmap_id: String,
            label: Option<String>,
            #[serde(default)]
            display_policy: crate::starmap::semantic::StarMapDisplayPolicy,
            #[serde(default)]
            open_behavior: crate::starmap::semantic::StarMapOpenBehavior,
            placement: Option<StarMapEmbedPlacement>,
            target_viewport: Option<StarMapEmbedViewport>,
            viewport: Option<StarMapViewport>,
            source_node_id: Option<String>,
            host_endpoint: Option<StarMapEndpoint>,
            host_anchor: Option<String>,
            #[serde(default)]
            provenance: crate::starmap::semantic::StarMapProvenance,
            created_at: u64,
            updated_at: u64,
        }

        let raw = Raw::deserialize(deserializer)?;
        let mut placement = raw.placement.unwrap_or_default();
        let mut target_viewport = raw.target_viewport.unwrap_or_default();

        if let Some(vp) = raw.viewport {
            placement.width = vp.width;
            placement.height = vp.height;
            target_viewport.scale = vp.scale;
            target_viewport.offset_x = vp.offset_x;
            target_viewport.offset_y = vp.offset_y;
        }

        let host_endpoint = raw.host_endpoint.or_else(|| {
            raw.host_anchor.and_then(|anchor_id| {
                raw.source_node_id
                    .as_ref()
                    .map(|node_id| StarMapEndpoint::Anchor {
                        node_id: node_id.clone(),
                        anchor_id,
                    })
            })
        });

        Ok(StarMapEmbed {
            instance_id: raw.instance_id,
            target_starmap_id: raw.target_starmap_id,
            label: raw.label,
            display_policy: raw.display_policy,
            open_behavior: raw.open_behavior,
            placement,
            target_viewport,
            source_node_id: raw.source_node_id,
            host_endpoint,
            provenance: raw.provenance,
            created_at: raw.created_at,
            updated_at: raw.updated_at,
        })
    }
}

/// 端点引用（用于嵌入和链接的 source/host）。
///
/// 与 `StarMapEdgeEndpoint` 类似但更简单：无 `DeepTarget` 变体，
/// 因为嵌入/链接的 source 端始终在当前星图内。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StarMapEndpoint {
    Node { node_id: String },
    Anchor { node_id: String, anchor_id: String },
    Starmap,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedPatch {
    pub label: Option<Option<String>>,
    pub display_policy: Option<crate::starmap::semantic::StarMapDisplayPolicy>,
    pub open_behavior: Option<crate::starmap::semantic::StarMapOpenBehavior>,
    pub viewport: Option<Option<StarMapViewport>>,
    pub placement: Option<Option<StarMapEmbedPlacement>>,
    pub target_viewport: Option<Option<StarMapEmbedViewport>>,
    pub source_node_id: Option<Option<String>>,
    pub host_anchor: Option<Option<String>>,
    pub host_endpoint: Option<Option<StarMapEndpoint>>,
}
