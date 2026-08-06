//! # 星图包存储入口 — 唯一可写真相
//!
//! `StarMapStore` 是星图 CRUD、解析、保存、迁移、导入和同步的唯一入口。
//! 所有对星图对象（节点、边、子星图放置、超链接、布局）的修改必须通过此类型执行。
//!
//! ## 核心设计
//!
//! - 运行时维护已加载对象缓存和 dirty record
//! - 增量写入：只保存实际修改的对象文件
//! - 同一对象的连续修改合并，拖动结束后写布局分片
//! - 加载结果返回结构化诊断（missing、corrupt、unsupportedVersion 等）
//! - 对象删除通过明确删除事务执行，保存过程根据 dirty record 写入
//!
//! ## 存储结构
//!
//! ```text
//! app-meta/starmaps/<starmap_id>/
//! ├── graph.json                          -- 星图元信息、成员 ID 列表、规范顺序、package revision
//! ├── nodes/<bucket>/<node_id>.json       -- 单个节点（bucket = hex 高 4 bit）
//! ├── edges/<bucket>/<edge_id>.json       -- 单条边
//! ├── child_starmaps/<bucket>/<instance_id>.json -- 子星图放置
//! ├── hyperlinks/<bucket>/<hyperlink_id>.json    -- 超链接
//! ├── links/<bucket>/<link_id>.json      -- 链接
//! ├── layouts/default/
//! │   ├── kind.json                       -- 布局类型
//! │   └── nodes/<bucket>.json            -- 布局节点分片
//! └── metadata/
//!     ├── migration.json                  -- 迁移记录
//!     └── recovery.json                  -- 解析失败对象的恢复记录
//!
//! session/starmaps/<starmap_id>/
//! └── viewport.json                       -- 设备本地视口（不进入同步数据）
//! ```

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use crate::starmap::types::*;

pub mod crud;
pub mod load;
pub mod meta;
pub mod migration;
pub mod recovery;
pub mod relation_index;
pub mod save;
pub mod snapshot;
pub mod types;

pub use meta::DeletedSinceLastSync;
pub use meta::GraphMeta;
pub use relation_index::{
    EdgeRelationIndex, EmbedHostIndex, HyperlinkRelationIndex, LinkRelationIndex,
};
pub use snapshot::{PhasedSnapshotRequest, StarMapPhasedSnapshot};
pub use types::*;

pub struct StarMapStore {
    pub(super) workspace: PathBuf,
    pub(super) starmap_id: String,
    pub(super) nodes: HashMap<String, StarMapNode>,
    pub(super) edges: HashMap<String, StarMapEdge>,
    pub(super) embeds: HashMap<String, StarMapEmbed>,
    pub(super) links: HashMap<String, StarMapLink>,
    pub(super) hyperlinks: HashMap<String, StarMapHyperlink>,
    pub(super) layout: Option<StarMapLayout>,
    pub(super) viewport: Option<StarMapViewport>,
    pub(super) graph_meta: Option<GraphMeta>,
    pub(super) dirty_nodes: HashSet<String>,
    pub(super) dirty_edges: HashSet<String>,
    pub(super) dirty_embeds: HashSet<String>,
    pub(super) dirty_links: HashSet<String>,
    pub(super) dirty_hyperlinks: HashSet<String>,
    pub(super) dirty_layout: bool,
    pub(super) dirty_graph_meta: bool,
    pub(super) deleted_node_ids: HashSet<String>,
    pub(super) deleted_edge_ids: HashSet<String>,
    pub(super) deleted_embed_ids: HashSet<String>,
    pub(super) deleted_link_ids: HashSet<String>,
    pub(super) deleted_hyperlink_ids: HashSet<String>,
    pub(super) package_revision: u64,
    pub(super) recovery_log: Vec<LoadDiagnostic>,
    pub(super) save_queue: VecDeque<SaveQueueEntry>,
    pub(super) current_load_phase: Option<LoadPhase>,
}

impl StarMapStore {
    pub fn new(workspace: &Path, starmap_id: &str) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
            starmap_id: starmap_id.to_string(),
            nodes: HashMap::new(),
            edges: HashMap::new(),
            embeds: HashMap::new(),
            links: HashMap::new(),
            hyperlinks: HashMap::new(),
            layout: None,
            viewport: None,
            graph_meta: None,
            dirty_nodes: HashSet::new(),
            dirty_edges: HashSet::new(),
            dirty_embeds: HashSet::new(),
            dirty_links: HashSet::new(),
            dirty_hyperlinks: HashSet::new(),
            dirty_layout: false,
            dirty_graph_meta: false,
            deleted_node_ids: HashSet::new(),
            deleted_edge_ids: HashSet::new(),
            deleted_embed_ids: HashSet::new(),
            deleted_link_ids: HashSet::new(),
            deleted_hyperlink_ids: HashSet::new(),
            package_revision: 0,
            recovery_log: Vec::new(),
            save_queue: VecDeque::new(),
            current_load_phase: None,
        }
    }

    pub fn starmap_id(&self) -> &str {
        &self.starmap_id
    }

    pub fn package_revision(&self) -> u64 {
        self.package_revision
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    pub fn embed_count(&self) -> usize {
        self.embeds.len()
    }

    pub fn hyperlink_count(&self) -> usize {
        self.hyperlinks.len()
    }

    pub fn link_count(&self) -> usize {
        self.links.len()
    }

    pub fn get_node(&self, node_id: &str) -> Option<&StarMapNode> {
        self.nodes.get(node_id)
    }

    pub fn get_edge(&self, edge_id: &str) -> Option<&StarMapEdge> {
        self.edges.get(edge_id)
    }

    pub fn get_embed(&self, instance_id: &str) -> Option<&StarMapEmbed> {
        self.embeds.get(instance_id)
    }

    pub fn get_hyperlink(&self, hyperlink_id: &str) -> Option<&StarMapHyperlink> {
        self.hyperlinks.get(hyperlink_id)
    }

    pub fn get_link(&self, link_id: &str) -> Option<&StarMapLink> {
        self.links.get(link_id)
    }

    pub fn get_layout(&self) -> Option<&StarMapLayout> {
        self.layout.as_ref()
    }

    pub fn get_viewport(&self) -> Option<&StarMapViewport> {
        self.viewport.as_ref()
    }

    pub fn all_nodes(&self) -> impl Iterator<Item = &StarMapNode> {
        self.nodes.values()
    }

    pub fn all_edges(&self) -> impl Iterator<Item = &StarMapEdge> {
        self.edges.values()
    }

    pub fn all_embeds(&self) -> impl Iterator<Item = &StarMapEmbed> {
        self.embeds.values()
    }

    pub fn all_hyperlinks(&self) -> impl Iterator<Item = &StarMapHyperlink> {
        self.hyperlinks.values()
    }

    pub fn all_links(&self) -> impl Iterator<Item = &StarMapLink> {
        self.links.values()
    }

    pub fn is_dirty(&self) -> bool {
        !self.dirty_nodes.is_empty()
            || !self.dirty_edges.is_empty()
            || !self.dirty_embeds.is_empty()
            || !self.dirty_links.is_empty()
            || !self.dirty_hyperlinks.is_empty()
            || self.dirty_layout
            || self.dirty_graph_meta
    }

    pub fn clear_persistent_deletion_log(&mut self) {
        if let Some(ref mut meta) = self.graph_meta {
            meta.deleted_since_last_sync.entries.clear();
            self.dirty_graph_meta = true;
        }
    }

    pub fn compact_deletion_log(&mut self, keep_since_revision: u64) {
        if let Some(ref mut meta) = self.graph_meta {
            meta.deleted_since_last_sync.compact(keep_since_revision);
            self.dirty_graph_meta = true;
        }
    }
}

#[cfg(test)]
mod tests;
