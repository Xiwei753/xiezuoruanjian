//! # 星图图数据 CRUD 操作
//!
//! 节点、边、嵌入、链接的增删改查，以及深目标解析和图验证。
//! 所有写操作通过 `atomic_write_string` 持久化到 `graph.json`。

mod edge_ops;
mod embed_ops;
mod link_ops;
mod node_ops;
mod ops;
pub mod resolve;
pub mod validation;

pub(crate) use edge_ops::*;
pub(crate) use embed_ops::*;
pub(crate) use link_ops::*;
pub(crate) use node_ops::*;
pub use ops::*;
pub use resolve::resolve_deep_target;
