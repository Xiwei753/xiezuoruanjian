//! # 跨星图引用路径
//!
//! `StarMapTargetPath` 是星图中所有跨星图引用的统一类型。
//! 边的 from/to、链接的 source/target、嵌入的 host_path 都使用此类型。
//! Portal 不再使用此类型——Portal 直接持有 `destination_starmap_id` 和可选落点。
//!
//! ## 路径结构
//!
//! - `starmap_id`：**起始宿主图** ID。语义固定为"引用出发的宿主图"，
//!   即这条路径从哪张图开始解析。resolver 以此为起点逐段穿越。
//! - `segments`：中间层级穿越段（`EnterEmbed` 或 `EnterPortal`）
//! - `target`：路径终点的具体引用（节点/锚点/章节范围等）
//!
//! 路径段不再存"下一个 starmap_id"。进入子星图必须通过具体 `instance_id`（嵌入）
//! 或具体 portal 节点，resolver 再从当前图查出真正的目标星图。

use serde::{Deserialize, Serialize};

pub use crate::starmap::semantic::StarMapTargetDetail;

/// 跨星图引用路径：统一的引用模型。
///
/// `starmap_id` 语义固定为"起始宿主图"——引用出发的宿主图 ID，
/// resolver 以此为起点逐段穿越 segments 到达 target。
///
/// 替代旧的 `StarMapDeepTarget`、`StarMapEdgeEndpoint`、`StarMapEndpoint`、`StarMapEndpointPath`。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StarMapTargetPath {
    pub starmap_id: String,
    #[serde(default)]
    pub segments: Vec<StarMapPathSegment>,
    pub target: StarMapTargetDetail,
}

impl Default for StarMapTargetPath {
    fn default() -> Self {
        Self {
            starmap_id: String::new(),
            segments: Vec::new(),
            target: StarMapTargetDetail::Starmap,
        }
    }
}

/// 路径段：描述一次层级穿越。
///
/// - `EnterEmbed`：通过嵌入实例进入子星图空间
/// - `EnterPortal`：通过 portal 节点进入子星图空间
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StarMapPathSegment {
    EnterEmbed { instance_id: String },
    EnterPortal { node_id: String },
}
