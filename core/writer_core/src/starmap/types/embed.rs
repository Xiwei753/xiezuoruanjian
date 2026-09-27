use serde::{Deserialize, Serialize};

use crate::starmap::semantic::StarMapProvenance;
use crate::starmap::types::graph::StarMapPoint;
use crate::starmap::types::reference::StarMapTargetPath;

/// 星图嵌入：当前星图里放置另一个星图实例的领域语义。
///
/// 只保存 `position`（在宿主星图文档坐标系下的位置）和 `host_path`
/// （宿主路径）。显示/交互/渲染参数（width/height/scale/z_index/collapsed、
/// viewport、display_policy、open_behavior）全部退出 Core，由平台端自行管理。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbed {
    pub instance_id: String,
    pub target_starmap_id: String,
    pub label: Option<String>,
    /// 嵌入在宿主星图文档坐标系下的位置。必填的数据字段，
    /// 旧 schema 一次性迁移时从 placement.x/y 合并而来。
    pub position: StarMapPoint,
    pub host_path: StarMapTargetPath,
    pub provenance: StarMapProvenance,
    pub created_at: u64,
    pub updated_at: u64,
}

/// Embed 局部更新补丁。
///
/// 只保留真正可修改的数据字段：`label`、`position`、`host_path`。
/// `None` 表示"不修改"，`Some(None)` 表示"清空可选字段"（如 label）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEmbedPatch {
    pub label: Option<Option<String>>,
    pub position: Option<StarMapPoint>,
    pub host_path: Option<StarMapTargetPath>,
}
