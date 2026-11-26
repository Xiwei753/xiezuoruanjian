//! # 星图包存储入口 — 唯一可写真相
//!
//! `StarMapStore` 是星图 CRUD、解析、保存和同步的唯一入口。
//! 所有对星图对象（节点、边、嵌入、链接）的修改必须通过此类型执行。
//!
//! ## 核心设计
//!
//! - 运行时维护已加载对象缓存和 dirty record
//! - 增量写入：只保存实际修改的对象文件
//! - 同一对象的连续修改合并
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
//! ├── embeds/<bucket>/<instance_id>.json -- 嵌入实例（含宿主图内 position）
//! └── metadata/
//!     └── recovery.json                  -- 解析失败对象的恢复记录
//! ```
//!
//! 布局/视口/运动策略已退出 Core，由平台端自行管理。
//! 节点位置在 `node.position`，嵌入位置在 `embed.position`。
//!
//! 导航链接不在包内：唯一持久化所有者是 annotations 仓库（作品域
//! `projects/<id>/annotations/links/*.json`，全局域 `annotations/links/*.json`），
//! `StarMapStore.links` 只是按画布过滤的读投影，见 [`link_repository`]。

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use crate::starmap::types::*;

pub mod crud;
pub(crate) mod link_repository;
pub mod load;
pub mod meta;
pub mod recovery;
pub mod relation_index;
pub mod save;
pub mod snapshot;
pub mod types;

pub use meta::DeletedSinceLastSync;
pub use meta::GraphMeta;
pub use relation_index::{
    CascadeIds, CascadeOrigin, EdgeRelationIndex, EmbedHostIndex, LinkRelationIndex,
    PathDependencyIndex, PathDependencySource,
};
pub use snapshot::{PhasedSnapshotRequest, StarMapPhasedSnapshot};
pub use types::*;

/// 一次删除实际移除的对象集合（含传递级联对象与链接墓碑）。
///
/// 删除的事实来源是 store 的删除计划，而不是调用方按请求参数猜的路径：
/// 级联删掉的 edge/embed/link 必须与节点删除进入同一份变更事实与写事务。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeletedStarmapObjects {
    pub nodes: Vec<String>,
    pub edges: Vec<String>,
    pub embeds: Vec<String>,
    /// 已写 annotations 仓库 tombstone 的导航链接 ID。
    pub links: Vec<String>,
}

impl DeletedStarmapObjects {
    /// 由被点中的 origin 与级联计划构造删除事实。
    pub fn from_cascade(origin: &CascadeOrigin, cascade: &CascadeIds) -> Self {
        let mut deleted = Self {
            edges: cascade.edge_ids.clone(),
            embeds: cascade.embed_ids.clone(),
            links: cascade.link_ids.clone(),
            ..Self::default()
        };
        match origin {
            CascadeOrigin::Node(node_id) => deleted.nodes.push(node_id.clone()),
            CascadeOrigin::Embed(instance_id) => deleted.embeds.push(instance_id.clone()),
        }
        deleted.embeds.sort();
        deleted.embeds.dedup();
        deleted
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
            && self.edges.is_empty()
            && self.embeds.is_empty()
            && self.links.is_empty()
    }
}

pub struct StarMapStore {
    pub(super) app_data_root: PathBuf,
    pub(super) starmap_id: String,
    pub(super) nodes: HashMap<String, StarMapNode>,
    /// instance_id -> shared node asset_id。库索引是持久事实源；
    /// 这里只是当前画布的加载缓存。
    pub(super) node_assets: HashMap<String, String>,
    pub(super) node_assets_loaded: bool,
    /// 节点迁移解码用的素材库视图（`starmaps/library/references.json` 反向
    /// 绑定）。懒加载一次，同一 store 生命周期内复用，见 `try_load_node`。
    pub(super) node_object_bindings: Option<crate::starmap::migration::nodes::NodeObjectBindings>,
    pub(super) edges: HashMap<String, StarMapEdge>,
    pub(super) embeds: HashMap<String, StarMapEmbed>,
    /// 链接读投影：唯一持久化所有者是 annotations 仓库，见
    /// [`link_repository`]。
    pub(super) links: HashMap<String, StarMapLink>,
    /// annotations 链接集合的版本指纹；变化时重建读投影。
    pub(super) link_projection_version: Option<link_repository::LinkProjectionVersion>,
    /// 最近一次加载/刷新是否重建了链接投影，供 facade 失效派生索引。
    pub(super) link_projection_rebuilt: bool,
    pub(super) graph_meta: Option<GraphMeta>,
    pub(super) dirty_nodes: HashSet<String>,
    pub(super) dirty_edges: HashSet<String>,
    pub(super) dirty_embeds: HashSet<String>,
    pub(super) dirty_graph_meta: bool,
    pub(super) deleted_node_ids: HashSet<String>,
    pub(super) deleted_edge_ids: HashSet<String>,
    pub(super) deleted_embed_ids: HashSet<String>,
    pub(super) deleted_link_ids: HashSet<String>,
    pub(super) package_revision: u64,
    pub(super) recovery_log: Vec<LoadDiagnostic>,
    pub(super) save_queue: VecDeque<SaveQueueEntry>,
    pub(super) current_load_phase: Option<LoadPhase>,
}

impl StarMapStore {
    pub fn new(app_data_root: &Path, starmap_id: &str) -> Self {
        Self {
            app_data_root: app_data_root.to_path_buf(),
            starmap_id: starmap_id.to_string(),
            nodes: HashMap::new(),
            node_assets: HashMap::new(),
            node_assets_loaded: false,
            node_object_bindings: None,
            edges: HashMap::new(),
            embeds: HashMap::new(),
            links: HashMap::new(),
            link_projection_version: None,
            link_projection_rebuilt: false,
            graph_meta: None,
            dirty_nodes: HashSet::new(),
            dirty_edges: HashSet::new(),
            dirty_embeds: HashSet::new(),
            dirty_graph_meta: false,
            deleted_node_ids: HashSet::new(),
            deleted_edge_ids: HashSet::new(),
            deleted_embed_ids: HashSet::new(),
            deleted_link_ids: HashSet::new(),
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

    pub fn link_count(&self) -> usize {
        self.links.len()
    }

    pub fn get_node(&self, node_id: &str) -> Option<&StarMapNode> {
        self.nodes.get(node_id)
    }

    /// 当前画布实例对应的共享素材 ID。旧数据尚未登记时，实例 ID 自身就是
    /// 共享本体 ID。
    pub fn asset_id_for_node(&self, instance_id: &str) -> String {
        self.node_assets
            .get(instance_id)
            .cloned()
            .unwrap_or_else(|| instance_id.to_string())
    }

    pub fn set_node_asset(&mut self, instance_id: &str, asset_id: &str) {
        self.node_assets
            .insert(instance_id.to_string(), asset_id.to_string());
        self.node_assets_loaded = true;
    }

    pub(super) fn ensure_node_assets_loaded(&mut self) {
        if self.node_assets_loaded {
            return;
        }
        self.node_assets_loaded = true;
        let Ok(index) = crate::starmap::library::load_reference_index(&self.app_data_root) else {
            return;
        };
        for record in index.nodes {
            if record.starmap_id == self.starmap_id {
                self.node_assets.insert(record.instance_id, record.asset_id);
            }
        }
    }

    /// 把共享定义同步到当前画布的内存实例（只改共享字段，保留位置）。
    /// 不把画布文件标脏：画布只持久化实例，共享内容由定义文件承载。
    pub fn apply_shared_node_definition(
        &mut self,
        definition: &crate::starmap::types::StarMapNodeDefinition,
    ) -> Vec<String> {
        self.ensure_node_assets_loaded();
        let ids: Vec<String> = self
            .nodes
            .iter()
            .filter(|(instance_id, _)| {
                self.node_assets
                    .get(instance_id.as_str())
                    .map(|asset_id| asset_id == &definition.asset_id)
                    .unwrap_or(false)
            })
            .map(|(instance_id, _)| instance_id.clone())
            .collect();
        for instance_id in &ids {
            if let Some(node) = self.nodes.get_mut(instance_id) {
                definition.apply_to_node(node);
            }
        }
        ids
    }

    pub fn get_edge(&self, edge_id: &str) -> Option<&StarMapEdge> {
        self.edges.get(edge_id)
    }

    pub fn get_embed(&self, instance_id: &str) -> Option<&StarMapEmbed> {
        self.embeds.get(instance_id)
    }

    pub fn get_link(&self, link_id: &str) -> Option<&StarMapLink> {
        self.links.get(link_id)
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

    pub fn all_links(&self) -> impl Iterator<Item = &StarMapLink> {
        self.links.values()
    }

    /// 本次 flush 会写到的相对路径；用于在真正写盘前登记文件级回滚范围。
    pub fn pending_flush_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        for id in &self.dirty_nodes {
            paths.push(crate::starmap::package_storage::node_rel_path(
                &self.starmap_id,
                id,
            ));
        }
        for id in &self.dirty_edges {
            paths.push(crate::starmap::package_storage::edge_rel_path(
                &self.starmap_id,
                id,
            ));
        }
        for id in &self.dirty_embeds {
            paths.push(crate::starmap::package_storage::embed_rel_path(
                &self.starmap_id,
                id,
            ));
        }
        for id in &self.deleted_node_ids {
            paths.push(crate::starmap::package_storage::node_rel_path(
                &self.starmap_id,
                id,
            ));
        }
        for id in &self.deleted_edge_ids {
            paths.push(crate::starmap::package_storage::edge_rel_path(
                &self.starmap_id,
                id,
            ));
        }
        for id in &self.deleted_embed_ids {
            paths.push(crate::starmap::package_storage::embed_rel_path(
                &self.starmap_id,
                id,
            ));
        }
        // 链接没有包内文件：删除 tombstone 落在 annotations 仓库，由
        // [`link_repository`] 的 CRUD 直接写入，不参与 flush 的文件级回滚。
        if self.dirty_graph_meta {
            paths.push(crate::starmap::package_storage::graph_meta_rel_path(
                &self.starmap_id,
            ));
        }
        paths
    }

    pub fn is_dirty(&self) -> bool {
        !self.dirty_nodes.is_empty()
            || !self.dirty_edges.is_empty()
            || !self.dirty_embeds.is_empty()
            || self.dirty_graph_meta
    }

    /// 快照当前 dirty 集合，供测试验证 `update_graph_meta_file` 记录对象 revision。
    /// 生产路径 `flush_save_queue` 已改用 `successful_writes`/`successful_deletes`，
    /// 不再调用此方法；保留供测试断言使用。
    #[cfg(test)]
    pub(in crate::starmap::store) fn collect_flush_dirty_set(&self) -> FlushDirtySet {
        FlushDirtySet {
            nodes: self.dirty_nodes.clone(),
            edges: self.dirty_edges.clone(),
            embeds: self.dirty_embeds.clone(),
            links: HashSet::new(),
            deleted_nodes: self.deleted_node_ids.clone(),
            deleted_edges: self.deleted_edge_ids.clone(),
            deleted_embeds: self.deleted_embed_ids.clone(),
            deleted_links: self.deleted_link_ids.clone(),
        }
    }

    /// 确认到 `acknowledged_revision`（含）为止的删除 tombstone 已被同步方
    /// 持久化，可以安全清理。保留 `deleted_at_revision > acknowledged_revision`
    /// 的 tombstone。清理后标记 graph_meta 为 dirty，下次 flush 会把清理后的
    /// deletion log 写回磁盘。
    pub fn acknowledge_deletions(
        &mut self,
        acknowledged_revision: u64,
    ) -> crate::error::Result<()> {
        // Fix 4: ack revision 不能大于当前 package_revision。
        self.reload_graph_meta_if_stale()?;
        if acknowledged_revision > self.package_revision {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "acknowledged_revision ({}) cannot exceed current package_revision ({})",
                    acknowledged_revision, self.package_revision
                ),
            )));
        }
        if let Some(ref mut meta) = self.graph_meta {
            meta.deleted_since_last_sync
                .acknowledge(acknowledged_revision);
            self.dirty_graph_meta = true;
            // Fix 4: 必须 enqueue GraphMeta，否则 flush_save_queue 不会写 graph meta。
            self.enqueue_save(crate::starmap::store::types::SaveQueueEntry::GraphMeta);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
