//! # 星图数据类型（Core 层跨平台契约）
//!
//! 定义节点、边、嵌入、链接、布局等核心数据结构。
//! 这些类型通过 JSON 序列化持久化，且被 FFI 层和平台端共享。
//! 新增字段必须使用 `#[serde(default)]` 以保持向前兼容。

mod embed;
mod graph;
mod link;
pub mod reference;

pub use embed::*;
pub use graph::*;
pub use link::*;
pub use reference::*;
