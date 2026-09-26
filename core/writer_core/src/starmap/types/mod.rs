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
    pub created_at: u64,
    pub updated_at: u64,
}

/// Hyperlink 增量补丁。
///
/// `label: Option<Option<String>>` 语义：
/// - `None`：不修改。
/// - `Some(None)`：清空 label。
/// - `Some(Some(s))`：设为 `s`。
///
/// `target_uri: Option<String>` 语义：
/// - `None`：不修改。
/// - `Some(s)`：替换为 `s`（必填，不支持清空——hyperlink 必须有 target_uri）。
///
/// `source: Option<StarMapTargetPath>` 语义：
/// - `None`：不修改。
/// - `Some(p)`：替换为 `p`。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct StarMapHyperlinkPatch {
    pub label: Option<Option<String>>,
    pub target_uri: Option<String>,
    pub source: Option<StarMapTargetPath>,
}
