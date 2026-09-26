//! # 星图数据类型（Core 层跨平台契约）
//!
//! 定义节点、边、嵌入、链接、布局等核心数据结构。
//! 这些类型通过 JSON 序列化持久化，且被 FFI 层和平台端共享。
//! 新增字段必须使用 `#[serde(default)]` 以保持向前兼容。

mod embed;
mod graph;
mod layout;
mod link;
pub mod reference;

use serde::{Deserialize, Serialize};

pub use embed::*;
pub use graph::*;
pub use layout::*;
pub use link::*;
pub use reference::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapHyperlink {
    pub hyperlink_id: String,
    pub source: StarMapTargetPath,
    pub target_uri: String,
    pub label: Option<String>,
    #[serde(default)]
    pub target_starmap_id: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}
