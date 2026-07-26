//! # Core 全局搜索索引
//!
//! 统一索引：chapterBody、chapterTitle、chapterNote、projectTitle、volumeTitle、
//! starmapTitle、starmapNode、starmapEdgeLabel、starmapHyperlink、setting。
//!
//! 提供：
//! - `global_search(query, scope, limit, cursor)` — 分页搜索
//! - `rebuild_search_index(project_id)` — 重建索引
//! - `get_search_index_status()` — 索引状态
//! - `enqueue_search_index_update(update)` — 增量更新队列
//!
//! 索引属于可删除、可重建的本地缓存。

pub mod types;
pub mod backend;
pub mod service;
pub mod extractor;
pub mod update_queue;
pub mod rebuild;
pub mod api;

pub use types::*;
pub use service::SearchIndexService;
pub use api::{global_search, rebuild_search_index, get_search_index_status, enqueue_search_index_update};
