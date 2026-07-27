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

use crate::error::Result;
use crate::starmap::types::*;
use crate::starmap::package_storage;
use crate::storage::atomic_write_string;

use serde::{Deserialize, Serialize};

fn endpoint_node_id(endpoint: &crate::starmap::types::StarMapEndpoint) -> Option<&str> {
    match endpoint {
        crate::starmap::types::StarMapEndpoint::Node { node_id } => Some(node_id),
        crate::starmap::types::StarMapEndpoint::Anchor { node_id, .. } => Some(node_id),
        crate::starmap::types::StarMapEndpoint::Starmap => None,
    }
}

fn endpoint_path_node_id(path: &crate::starmap::types::StarMapEndpointPath) -> Option<&str> {
    match &path.endpoint {
        crate::starmap::types::StarMapEdgeEndpoint::Node { node_id } => Some(node_id),
        crate::starmap::types::StarMapEdgeEndpoint::Anchor { node_id, .. } => Some(node_id),
        crate::starmap::types::StarMapEdgeEndpoint::Starmap => None,
        crate::starmap::types::StarMapEdgeEndpoint::DeepTarget { .. } => None,
    }
}

fn edge_endpoint_node_id(ep: &crate::starmap::types::StarMapEdgeEndpoint) -> Option<&str> {
    match ep {
        crate::starmap::types::StarMapEdgeEndpoint::Node { node_id } => Some(node_id),
        crate::starmap::types::StarMapEdgeEndpoint::Anchor { node_id, .. } => Some(node_id),
        crate::starmap::types::StarMapEdgeEndpoint::Starmap => None,
        crate::starmap::types::StarMapEdgeEndpoint::DeepTarget { .. } => None,
    }
}

fn extract_eri_node_refs(eri: &EdgeRelationIndex) -> Vec<&str> {
    let mut refs = Vec::new();
    if !eri.from.is_empty() { refs.push(eri.from.as_str()); }
    if !eri.to.is_empty() { refs.push(eri.to.as_str()); }
    if let Some(ref ep) = eri.from_endpoint {
        if let Some(id) = edge_endpoint_node_id(ep) { refs.push(id); }
    }
    if let Some(ref ep) = eri.to_endpoint {
        if let Some(id) = edge_endpoint_node_id(ep) { refs.push(id); }
    }
    if let Some(ref path) = eri.from_endpoint_path {
        if let Some(id) = endpoint_path_node_id(path) { refs.push(id); }
    }
    if let Some(ref path) = eri.to_endpoint_path {
        if let Some(id) = endpoint_path_node_id(path) { refs.push(id); }
    }
    refs
}

fn extract_ehi_node_refs(ehi: &EmbedHostIndex) -> Vec<&str> {
    let mut refs = Vec::new();
    if !ehi.host_node_id.is_empty() { refs.push(ehi.host_node_id.as_str()); }
    if let Some(ref ep) = ehi.host_endpoint {
        if let Some(id) = endpoint_node_id(ep) { refs.push(id); }
    }
    refs
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DirtyKind {
    Node,
    Edge,
    Embed,
    Hyperlink,
    Link,
    Layout,
    GraphMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationEntry {
    pub kind: String,
    pub detail: String,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LoadDiagnosticKind {
    Missing,
    Corrupt,
    UnsupportedVersion,
    DanglingReference,
    OrphanObject,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadDiagnostic {
    pub kind: LoadDiagnosticKind,
    pub object_type: String,
    pub object_id: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapStoreResult {
    pub diagnostics: Vec<LoadDiagnostic>,
    pub loaded_node_count: usize,
    pub loaded_edge_count: usize,
    pub loaded_embed_count: usize,
    pub loaded_link_count: usize,
    pub loaded_hyperlink_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListWithDiagnostics<T> {
    pub items: Vec<T>,
    pub diagnostics: Vec<LoadDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LoadPhase {
    GraphMeta,
    ViewportAndLayoutIndex,
    CurrentViewportObjects,
    PrefetchNearbyObjects,
    BackgroundFullLoad,
}

impl LoadPhase {
    pub fn next(self) -> Option<LoadPhase> {
        match self {
            LoadPhase::GraphMeta => Some(LoadPhase::ViewportAndLayoutIndex),
            LoadPhase::ViewportAndLayoutIndex => Some(LoadPhase::CurrentViewportObjects),
            LoadPhase::CurrentViewportObjects => Some(LoadPhase::PrefetchNearbyObjects),
            LoadPhase::PrefetchNearbyObjects => Some(LoadPhase::BackgroundFullLoad),
            LoadPhase::BackgroundFullLoad => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SaveQueueEntry {
    Node,
    Edge,
    Embed,
    Link,
    Hyperlink,
    Layout,
    GraphMeta,
    DeleteNode,
    DeleteEdge,
    DeleteEmbed,
    DeleteLink,
    DeleteHyperlink,
}

pub struct StarMapStore {
    workspace: PathBuf,
    starmap_id: String,
    nodes: HashMap<String, StarMapNode>,
    edges: HashMap<String, StarMapEdge>,
    embeds: HashMap<String, StarMapEmbed>,
    links: HashMap<String, StarMapLink>,
    hyperlinks: HashMap<String, StarMapHyperlink>,
    layout: Option<StarMapLayout>,
    viewport: Option<StarMapViewport>,
    graph_meta: Option<GraphMeta>,
    dirty_nodes: HashSet<String>,
    dirty_edges: HashSet<String>,
    dirty_embeds: HashSet<String>,
    dirty_links: HashSet<String>,
    dirty_hyperlinks: HashSet<String>,
    dirty_layout: bool,
    dirty_graph_meta: bool,
    deleted_node_ids: HashSet<String>,
    deleted_edge_ids: HashSet<String>,
    deleted_embed_ids: HashSet<String>,
    deleted_link_ids: HashSet<String>,
    deleted_hyperlink_ids: HashSet<String>,
    package_revision: u64,
    recovery_log: Vec<LoadDiagnostic>,
    save_queue: VecDeque<SaveQueueEntry>,
    current_load_phase: Option<LoadPhase>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeRelationIndex {
    pub edge_id: String,
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub from_endpoint: Option<crate::starmap::types::StarMapEdgeEndpoint>,
    #[serde(default)]
    pub to_endpoint: Option<crate::starmap::types::StarMapEdgeEndpoint>,
    #[serde(default)]
    pub from_endpoint_path: Option<crate::starmap::types::StarMapEndpointPath>,
    #[serde(default)]
    pub to_endpoint_path: Option<crate::starmap::types::StarMapEndpointPath>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbedHostIndex {
    pub instance_id: String,
    pub host_node_id: String,
    #[serde(default)]
    pub host_endpoint: Option<crate::starmap::types::StarMapEndpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkRelationIndex {
    pub link_id: String,
    pub source_node_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HyperlinkRelationIndex {
    pub hyperlink_id: String,
    pub source_node_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphMeta {
    pub schema_version: String,
    pub starmap_id: String,
    pub title: String,
    pub node_ids: Vec<String>,
    pub edge_ids: Vec<String>,
    pub embed_instance_ids: Vec<String>,
    pub link_ids: Vec<String>,
    pub hyperlink_ids: Vec<String>,
    #[serde(default)]
    pub edge_relation_index: Vec<EdgeRelationIndex>,
    #[serde(default)]
    pub embed_host_index: Vec<EmbedHostIndex>,
    #[serde(default)]
    pub link_relation_index: Vec<LinkRelationIndex>,
    #[serde(default)]
    pub hyperlink_relation_index: Vec<HyperlinkRelationIndex>,
    #[serde(default)]
    pub node_kind_counts: HashMap<String, u32>,
    pub package_revision: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyGraphMeta {
    schema_version: u32,
    id: String,
    starmap_id: String,
    title: String,
    created_at: u64,
    updated_at: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapPhasedSnapshot {
    pub starmap_id: String,
    pub title: String,
    pub nodes: Vec<StarMapNode>,
    pub edges: Vec<StarMapEdge>,
    pub embeds: Vec<StarMapEmbed>,
    pub links: Vec<StarMapLink>,
    pub hyperlinks: Vec<StarMapHyperlink>,
    pub layout: Option<StarMapLayout>,
    pub viewport: Option<StarMapViewport>,
    pub diagnostics: Vec<LoadDiagnostic>,
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

    fn reload_graph_meta_if_stale(&mut self) {
        let graph_json_path = self.starmap_dir().join("graph.json");
        if !graph_json_path.exists() {
            return;
        }
        let Ok(content) = std::fs::read_to_string(&graph_json_path) else {
            return;
        };
        let Ok(disk_meta) = serde_json::from_str::<GraphMeta>(&content) else {
            return;
        };
        let mem_rev = self.graph_meta.as_ref().map(|m| m.package_revision).unwrap_or(0);
        if disk_meta.package_revision > mem_rev {
            self.graph_meta = Some(disk_meta);
            self.package_revision = self.graph_meta.as_ref().map(|m| m.package_revision).unwrap_or(0);
        }
    }

    pub fn list_hyperlinks_with_diagnostics(&mut self) -> ListWithDiagnostics<StarMapHyperlink> {
        self.reload_graph_meta_if_stale();
        let hl_ids = self.graph_meta_hyperlink_ids();
        let mut items = Vec::new();
        let mut diagnostics = Vec::new();
        for hl_id in &hl_ids {
            if !self.hyperlinks.contains_key(hl_id) {
                if let Some(hl) = self.try_load_hyperlink(hl_id) {
                    self.hyperlinks.insert(hl_id.clone(), hl);
                } else {
                    let recovery_len = self.recovery_log.len();
                    if recovery_len > 0 {
                        let last = self.recovery_log.last().cloned().unwrap();
                        if last.object_id == *hl_id {
                            diagnostics.push(last);
                            continue;
                        }
                    }
                    diagnostics.push(LoadDiagnostic {
                        kind: LoadDiagnosticKind::Missing,
                        object_type: "hyperlink".to_string(),
                        object_id: hl_id.clone(),
                        detail: "hyperlink could not be loaded".to_string(),
                    });
                    continue;
                }
            }
            if let Some(hl) = self.hyperlinks.get(hl_id).cloned() {
                items.push(hl);
            }
        }
        ListWithDiagnostics { items, diagnostics }
    }

    pub fn list_links_with_diagnostics(&mut self) -> ListWithDiagnostics<StarMapLink> {
        self.reload_graph_meta_if_stale();
        let link_ids = self.graph_meta_link_ids();
        let mut items = Vec::new();
        let mut diagnostics = Vec::new();
        for link_id in &link_ids {
            if !self.links.contains_key(link_id) {
                if let Some(link) = self.try_load_link(link_id) {
                    self.links.insert(link_id.clone(), link);
                } else {
                    let recovery_len = self.recovery_log.len();
                    if recovery_len > 0 {
                        let last = self.recovery_log.last().cloned().unwrap();
                        if last.object_id == *link_id {
                            diagnostics.push(last);
                            continue;
                        }
                    }
                    diagnostics.push(LoadDiagnostic {
                        kind: LoadDiagnosticKind::Missing,
                        object_type: "link".to_string(),
                        object_id: link_id.clone(),
                        detail: "link could not be loaded".to_string(),
                    });
                    continue;
                }
            }
            if let Some(link) = self.links.get(link_id).cloned() {
                items.push(link);
            }
        }
        ListWithDiagnostics { items, diagnostics }
    }

    pub fn graph_meta_hyperlink_ids(&self) -> Vec<String> {
        self.graph_meta.as_ref()
            .map(|m| m.hyperlink_ids.clone())
            .unwrap_or_default()
    }

    pub fn graph_meta_link_ids(&self) -> Vec<String> {
        self.graph_meta.as_ref()
            .map(|m| m.link_ids.clone())
            .unwrap_or_default()
    }

    pub fn diagnostics(&self) -> &[LoadDiagnostic] {
        &self.recovery_log
    }

    pub fn current_load_phase(&self) -> Option<LoadPhase> {
        self.current_load_phase
    }

    pub fn save_queue_len(&self) -> usize {
        self.save_queue.len()
    }

    fn ensure_graph_meta_initialized(&mut self) {
        if self.graph_meta.is_some() {
            return;
        }
        self.reload_graph_meta_if_stale();
        if self.graph_meta.is_none() {
            self.graph_meta = Some(GraphMeta {
                schema_version: "2".to_string(),
                starmap_id: self.starmap_id.clone(),
                title: String::new(),
                node_ids: Vec::new(),
                edge_ids: Vec::new(),
                embed_instance_ids: Vec::new(),
                link_ids: Vec::new(),
                hyperlink_ids: Vec::new(),
                edge_relation_index: Vec::new(),
                embed_host_index: Vec::new(),
                link_relation_index: Vec::new(),
                hyperlink_relation_index: Vec::new(),
                node_kind_counts: HashMap::new(),
                package_revision: self.package_revision,
                updated_at: crate::starmap::now_epoch(),
            });
        }
    }

    pub fn enqueue_save(&mut self, entry: SaveQueueEntry) {
        if !self.save_queue.iter().any(|e| std::mem::discriminant(e) == std::mem::discriminant(&entry)) {
            self.save_queue.push_back(entry);
        }
    }

    pub fn drain_save_queue(&mut self) -> Vec<SaveQueueEntry> {
        self.save_queue.drain(..).collect()
    }

    pub fn flush_save_queue(&mut self) -> Result<()> {
        let mut remaining: VecDeque<SaveQueueEntry> = VecDeque::new();
        let mut any_processed = false;
        let mut failed_types: Vec<String> = Vec::new();
        while let Some(entry) = self.save_queue.pop_front() {
            let mut succeeded = true;
            any_processed = true;
            match entry {
                SaveQueueEntry::Node => {
                    let ids: Vec<String> = self.dirty_nodes.iter().cloned().collect();
                    for node_id in &ids {
                        if let Some(node) = self.nodes.get(node_id) {
                            if package_storage::save_node(&self.workspace, &self.starmap_id, node).is_err() {
                                succeeded = false;
                                break;
                            }
                        }
                        self.dirty_nodes.remove(node_id);
                    }
                }
                SaveQueueEntry::Edge => {
                    let ids: Vec<String> = self.dirty_edges.iter().cloned().collect();
                    for edge_id in &ids {
                        if let Some(edge) = self.edges.get(edge_id) {
                            if package_storage::save_edge(&self.workspace, &self.starmap_id, edge).is_err() {
                                succeeded = false;
                                break;
                            }
                        }
                        self.dirty_edges.remove(edge_id);
                    }
                }
                SaveQueueEntry::Embed => {
                    let ids: Vec<String> = self.dirty_embeds.iter().cloned().collect();
                    for instance_id in &ids {
                        if let Some(embed) = self.embeds.get(instance_id) {
                            if package_storage::save_embed(&self.workspace, &self.starmap_id, embed).is_err() {
                                succeeded = false;
                                break;
                            }
                        }
                        self.dirty_embeds.remove(instance_id);
                    }
                }
                SaveQueueEntry::Link => {
                    let ids: Vec<String> = self.dirty_links.iter().cloned().collect();
                    for link_id in &ids {
                        if let Some(link) = self.links.get(link_id) {
                            if package_storage::save_link(&self.workspace, &self.starmap_id, link).is_err() {
                                succeeded = false;
                                break;
                            }
                        }
                        self.dirty_links.remove(link_id);
                    }
                }
                SaveQueueEntry::Hyperlink => {
                    let ids: Vec<String> = self.dirty_hyperlinks.iter().cloned().collect();
                    for hl_id in &ids {
                        if let Some(hl) = self.hyperlinks.get(hl_id) {
                            if package_storage::save_hyperlink(&self.workspace, &self.starmap_id, hl).is_err() {
                                succeeded = false;
                                break;
                            }
                        }
                        self.dirty_hyperlinks.remove(hl_id);
                    }
                }
                SaveQueueEntry::Layout => {
                    if self.dirty_layout {
                        if let Some(ref layout) = self.layout {
                            if package_storage::save_layout(&self.workspace, &self.starmap_id, layout).is_err() {
                                succeeded = false;
                            }
                        }
                        if succeeded {
                            self.dirty_layout = false;
                        }
                    }
                }
                SaveQueueEntry::GraphMeta => {
                    if self.dirty_graph_meta {
                        self.reload_graph_meta_if_stale();
                        match self.update_graph_meta_file() {
                            Ok(written_revision) => {
                                self.dirty_graph_meta = false;
                                self.package_revision = written_revision;
                            }
                            Err(_) => {
                                succeeded = false;
                            }
                        }
                    }
                }
                SaveQueueEntry::DeleteNode => {
                    let ids: Vec<String> = self.deleted_node_ids.iter().cloned().collect();
                    for node_id in &ids {
                        match package_storage::delete_node_file(&self.workspace, &self.starmap_id, node_id) {
                            Ok(()) => { self.deleted_node_ids.remove(node_id); }
                            Err(e) => {
                                self.record_delete_failure("node", node_id, &e);
                                succeeded = false;
                                break;
                            }
                        }
                    }
                }
                SaveQueueEntry::DeleteEdge => {
                    let ids: Vec<String> = self.deleted_edge_ids.iter().cloned().collect();
                    for edge_id in &ids {
                        match package_storage::delete_edge_file(&self.workspace, &self.starmap_id, edge_id) {
                            Ok(()) => { self.deleted_edge_ids.remove(edge_id); }
                            Err(e) => {
                                self.record_delete_failure("edge", edge_id, &e);
                                succeeded = false;
                                break;
                            }
                        }
                    }
                }
                SaveQueueEntry::DeleteEmbed => {
                    let ids: Vec<String> = self.deleted_embed_ids.iter().cloned().collect();
                    for instance_id in &ids {
                        match package_storage::delete_embed_file(&self.workspace, &self.starmap_id, instance_id) {
                            Ok(()) => { self.deleted_embed_ids.remove(instance_id); }
                            Err(e) => {
                                self.record_delete_failure("embed", instance_id, &e);
                                succeeded = false;
                                break;
                            }
                        }
                    }
                }
                SaveQueueEntry::DeleteLink => {
                    let ids: Vec<String> = self.deleted_link_ids.iter().cloned().collect();
                    for link_id in &ids {
                        match package_storage::delete_link_file(&self.workspace, &self.starmap_id, link_id) {
                            Ok(()) => { self.deleted_link_ids.remove(link_id); }
                            Err(e) => {
                                self.record_delete_failure("link", link_id, &e);
                                succeeded = false;
                                break;
                            }
                        }
                    }
                }
                SaveQueueEntry::DeleteHyperlink => {
                    let ids: Vec<String> = self.deleted_hyperlink_ids.iter().cloned().collect();
                    for hl_id in &ids {
                        match package_storage::delete_hyperlink_file(&self.workspace, &self.starmap_id, hl_id) {
                            Ok(()) => { self.deleted_hyperlink_ids.remove(hl_id); }
                            Err(e) => {
                                self.record_delete_failure("hyperlink", hl_id, &e);
                                succeeded = false;
                                break;
                            }
                        }
                    }
                }
            }
            if !succeeded {
                failed_types.push(format!("{:?}", entry));
                remaining.push_back(entry);
            }
        }
        self.save_queue = remaining;

        let all_flushed = !self.is_dirty() && !self.dirty_graph_meta && !self.has_pending_deletes();

        if self.has_pending_deletes() || self.has_pending_writes() {
            self.flush_recovery_to_disk()?;
        }

        if any_processed && all_flushed {
            let node_count = self.graph_meta.as_ref()
                .map(|m| m.node_ids.len() as u32)
                .unwrap_or(self.nodes.len() as u32);
            let edge_count = self.graph_meta.as_ref()
                .map(|m| m.edge_ids.len() as u32)
                .unwrap_or(self.edges.len() as u32);
            let linked_chapters = self.graph_meta.as_ref()
                .map(|m| *m.node_kind_counts.get("Chapter").unwrap_or(&0))
                .unwrap_or(0u32);
            crate::starmap::update_starmap_stats(
                &self.workspace,
                &self.starmap_id,
                node_count,
                edge_count,
                linked_chapters,
            )?;
        }

        if !failed_types.is_empty() {
            return Err(crate::error::Error::SaveQueueFlushIncomplete {
                failed_types,
                remaining_queue_len: self.save_queue.len(),
            });
        }

        Ok(())
    }

    fn record_delete_failure(&mut self, object_type: &str, object_id: &str, error: &crate::error::Error) {
        self.recovery_log.push(LoadDiagnostic {
            kind: LoadDiagnosticKind::Corrupt,
            object_type: object_type.to_string(),
            object_id: object_id.to_string(),
            detail: format!("delete failed: {:?}", error),
        });
    }

    pub fn has_pending_deletes(&self) -> bool {
        !self.deleted_node_ids.is_empty()
            || !self.deleted_edge_ids.is_empty()
            || !self.deleted_embed_ids.is_empty()
            || !self.deleted_link_ids.is_empty()
            || !self.deleted_hyperlink_ids.is_empty()
    }

    fn has_pending_writes(&self) -> bool {
        self.is_dirty() || self.dirty_graph_meta
    }

    pub fn load_phased(&mut self, up_to: LoadPhase) -> Result<StarMapStoreResult> {
        self.recovery_log.clear();
        let mut diagnostics = Vec::new();

        self.load_recovery_from_disk();

        let mut current = self.current_load_phase.unwrap_or(LoadPhase::GraphMeta);

        loop {
            match current {
                LoadPhase::GraphMeta => {
                    self.load_graph_meta_phase(&mut diagnostics);
                    self.current_load_phase = Some(LoadPhase::GraphMeta);
                }
                LoadPhase::ViewportAndLayoutIndex => {
                    self.layout = self.try_load_layout();
                    self.viewport = self.try_load_viewport();
                    self.current_load_phase = Some(LoadPhase::ViewportAndLayoutIndex);
                }
                LoadPhase::CurrentViewportObjects => {
                    self.load_viewport_objects(&mut diagnostics);
                    self.current_load_phase = Some(LoadPhase::CurrentViewportObjects);
                }
                LoadPhase::PrefetchNearbyObjects => {
                    self.prefetch_nearby_objects(&mut diagnostics);
                    self.current_load_phase = Some(LoadPhase::PrefetchNearbyObjects);
                }
                LoadPhase::BackgroundFullLoad => {
                    self.load_remaining_objects(&mut diagnostics);
                    self.detect_dangling_references(&mut diagnostics);
                    self.detect_orphan_objects(&mut diagnostics);
                    self.current_load_phase = Some(LoadPhase::BackgroundFullLoad);
                }
            }

            if current == up_to {
                break;
            }

            match current.next() {
                Some(next) => current = next,
                None => break,
            }
        }

        self.package_revision = self.graph_meta.as_ref()
            .map(|m| m.package_revision)
            .unwrap_or(0);

        diagnostics.extend(self.recovery_log.drain(..));
        self.recovery_log = diagnostics.clone();

        Ok(StarMapStoreResult {
            diagnostics,
            loaded_node_count: self.nodes.len(),
            loaded_edge_count: self.edges.len(),
            loaded_embed_count: self.embeds.len(),
            loaded_link_count: self.links.len(),
            loaded_hyperlink_count: self.hyperlinks.len(),
        })
    }

    fn load_graph_meta_phase(&mut self, diagnostics: &mut Vec<LoadDiagnostic>) {
        let graph_dir = self.starmap_dir();
        let graph_json_path = graph_dir.join("graph.json");

        if graph_json_path.exists() {
            let content = std::fs::read_to_string(&graph_json_path).unwrap_or_default();
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
                let schema_version_str = value.get("schemaVersion")
                    .or_else(|| value.get("schema_version"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                if let Some(ref sv) = schema_version_str {
                    if sv != "2" && sv != "1" {
                        diagnostics.push(LoadDiagnostic {
                            kind: LoadDiagnosticKind::UnsupportedVersion,
                            object_type: "graph".to_string(),
                            object_id: self.starmap_id.clone(),
                            detail: format!("unsupported schemaVersion: {}", sv),
                        });
                    }
                }

                let is_new_format = schema_version_str.as_deref() == Some("2");

                if is_new_format {
                    match serde_json::from_str::<GraphMeta>(&content) {
                        Ok(meta) => { self.graph_meta = Some(meta); }
                        Err(e) => {
                            diagnostics.push(LoadDiagnostic {
                                kind: LoadDiagnosticKind::Corrupt,
                                object_type: "graph".to_string(),
                                object_id: self.starmap_id.clone(),
                                detail: format!("graph.json v2 parse failed: {}", e),
                            });
                        }
                    }
                } else if let Ok(graph) = serde_json::from_str::<StarMapGraph>(&content) {
                    self.graph_meta = Some(GraphMeta {
                        schema_version: "2".to_string(),
                        starmap_id: graph.starmap_id.clone(),
                        title: graph.title.clone(),
                        node_ids: graph.nodes.iter().map(|n| n.id.clone()).collect(),
                        edge_ids: graph.edges.iter().map(|e| e.id.clone()).collect(),
                        embed_instance_ids: graph.embeds.iter().map(|e| e.instance_id.clone()).collect(),
                        link_ids: graph.links.iter().map(|l| l.link_id.clone()).collect(),
                        hyperlink_ids: vec![],
                        edge_relation_index: graph.edges.iter().map(|e| EdgeRelationIndex {
                            edge_id: e.id.clone(),
                            from: e.from.clone().unwrap_or_default(),
                            to: e.to.clone().unwrap_or_default(),
                            from_endpoint: e.from_endpoint.clone(),
                            to_endpoint: e.to_endpoint.clone(),
                            from_endpoint_path: e.from_endpoint_path.clone(),
                            to_endpoint_path: e.to_endpoint_path.clone(),
                        }).collect(),
                        embed_host_index: graph.embeds.iter().map(|e| EmbedHostIndex {
                            instance_id: e.instance_id.clone(),
                            host_node_id: e.source_node_id.clone().unwrap_or_default(),
                            host_endpoint: e.host_endpoint.clone(),
                        }).collect(),
                        link_relation_index: graph.links.iter().map(|l| LinkRelationIndex {
                            link_id: l.link_id.clone(),
                            source_node_id: endpoint_node_id(&l.source).unwrap_or_default().to_string(),
                        }).collect(),
                        hyperlink_relation_index: vec![],
                        node_kind_counts: {
                            let mut counts = HashMap::new();
                            for node in &graph.nodes {
                                *counts.entry(format!("{:?}", node.kind)).or_insert(0u32) += 1;
                            }
                            counts
                        },
                        package_revision: 0,
                        updated_at: graph.updated_at,
                    });
                    for node in &graph.nodes {
                        self.nodes.insert(node.id.clone(), node.clone());
                        self.dirty_nodes.insert(node.id.clone());
                    }
                    for edge in &graph.edges {
                        self.edges.insert(edge.id.clone(), edge.clone());
                        self.dirty_edges.insert(edge.id.clone());
                    }
                    for embed in &graph.embeds {
                        self.embeds.insert(embed.instance_id.clone(), embed.clone());
                        self.dirty_embeds.insert(embed.instance_id.clone());
                    }
                    for link in &graph.links {
                        self.links.insert(link.link_id.clone(), link.clone());
                        self.dirty_links.insert(link.link_id.clone());
                    }
                    self.dirty_graph_meta = true;
                    self.enqueue_save(SaveQueueEntry::Node);
                    self.enqueue_save(SaveQueueEntry::Edge);
                    self.enqueue_save(SaveQueueEntry::Embed);
                    self.enqueue_save(SaveQueueEntry::Link);
                    self.enqueue_save(SaveQueueEntry::GraphMeta);
                    self.record_migration("graph_v1_to_v2", "migrated inline v1 graph.json to v2 package format");
                } else {
                    match self.load_graph_meta_from_file(&graph_json_path) {
                        Ok(meta) => { self.graph_meta = Some(meta); }
                        Err(e) => {
                            diagnostics.push(LoadDiagnostic {
                                kind: LoadDiagnosticKind::Corrupt,
                                object_type: "graph".to_string(),
                                object_id: self.starmap_id.clone(),
                                detail: format!("graph.json parse failed: {}", e),
                            });
                        }
                    }
                }
            } else {
                diagnostics.push(LoadDiagnostic {
                    kind: LoadDiagnosticKind::Corrupt,
                    object_type: "graph".to_string(),
                    object_id: self.starmap_id.clone(),
                    detail: "graph.json is not valid JSON".to_string(),
                });
            }
        }
    }

    fn load_viewport_objects(&mut self, diagnostics: &mut Vec<LoadDiagnostic>) {
        let viewport_node_ids: HashSet<String> = match (&self.layout, &self.viewport) {
            (Some(l), Some(vp)) => {
                let vp_left = vp.offset_x;
                let vp_top = vp.offset_y;
                let vp_right = vp.offset_x + vp.width / vp.scale;
                let vp_bottom = vp.offset_y + vp.height / vp.scale;
                l.nodes.iter()
                    .filter(|n| {
                        let node_left = n.x;
                        let node_top = n.y;
                        let node_right = n.x + n.width;
                        let node_bottom = n.y + n.height;
                        node_right > vp_left && node_left < vp_right
                            && node_bottom > vp_top && node_top < vp_bottom
                    })
                    .map(|n| n.node_id.clone())
                    .collect()
            }
            (Some(l), None) => {
                l.nodes.iter().map(|n| n.node_id.clone()).collect()
            }
            _ => HashSet::new(),
        };

        if viewport_node_ids.is_empty() {
            let _ = diagnostics;
            return;
        }

        for node_id in &viewport_node_ids {
            if !self.nodes.contains_key(node_id) {
                if let Some(node) = self.try_load_node(node_id) {
                    self.nodes.insert(node_id.clone(), node);
                }
            }
        }

        let has_index = self.graph_meta.as_ref()
            .map(|m| !m.edge_relation_index.is_empty() || m.edge_ids.is_empty())
            .unwrap_or(false);

        if has_index {
            let edge_relation_index = self.graph_meta.as_ref().unwrap().edge_relation_index.clone();
            let embed_host_index = self.graph_meta.as_ref().unwrap().embed_host_index.clone();

            for eri in &edge_relation_index {
                if self.edges.contains_key(&eri.edge_id) {
                    continue;
                }
                let refs = extract_eri_node_refs(eri);
                let any_in_viewport = refs.iter().any(|id| viewport_node_ids.contains(*id));
                if any_in_viewport {
                    if let Some(edge) = self.try_load_edge(&eri.edge_id) {
                        self.edges.insert(eri.edge_id.clone(), edge);
                    }
                }
            }
            for ehi in &embed_host_index {
                if self.embeds.contains_key(&ehi.instance_id) {
                    continue;
                }
                let refs = extract_ehi_node_refs(ehi);
                let any_in_viewport = refs.iter().any(|id| viewport_node_ids.contains(*id));
                if any_in_viewport {
                    if let Some(embed) = self.try_load_embed(&ehi.instance_id) {
                        self.embeds.insert(ehi.instance_id.clone(), embed);
                    }
                }
            }
        } else {
            self.rebuild_relation_indexes();
            self.load_viewport_objects(diagnostics);
            return;
        }

        let _ = diagnostics;
    }

    pub fn ensure_loaded(&mut self) -> Result<()> {
        if self.current_load_phase >= Some(LoadPhase::PrefetchNearbyObjects) {
            return Ok(());
        }
        self.load_phased(LoadPhase::PrefetchNearbyObjects)?;
        Ok(())
    }

    pub fn ensure_fully_loaded(&mut self) -> Result<()> {
        if self.current_load_phase >= Some(LoadPhase::BackgroundFullLoad) {
            return Ok(());
        }
        self.load_full()?;
        Ok(())
    }

    pub fn ensure_object_loaded(&mut self, node_id: &str) -> Result<()> {
        if self.nodes.contains_key(node_id) {
            return Ok(());
        }
        if let Some(node) = self.try_load_node(node_id) {
            self.nodes.insert(node_id.to_string(), node);
        }
        Ok(())
    }

    pub fn ensure_edge_loaded(&mut self, edge_id: &str) -> Result<()> {
        if self.edges.contains_key(edge_id) {
            return Ok(());
        }
        if let Some(edge) = self.try_load_edge(edge_id) {
            self.edges.insert(edge_id.to_string(), edge);
        }
        Ok(())
    }

    pub fn ensure_embed_loaded(&mut self, instance_id: &str) -> Result<()> {
        if self.embeds.contains_key(instance_id) {
            return Ok(());
        }
        if let Some(embed) = self.try_load_embed(instance_id) {
            self.embeds.insert(instance_id.to_string(), embed);
        }
        Ok(())
    }

    pub fn ensure_link_loaded(&mut self, link_id: &str) -> Result<()> {
        if self.links.contains_key(link_id) {
            return Ok(());
        }
        if let Some(link) = self.try_load_link(link_id) {
            self.links.insert(link_id.to_string(), link);
        }
        Ok(())
    }

    pub fn ensure_hyperlink_loaded(&mut self, hyperlink_id: &str) -> Result<()> {
        if self.hyperlinks.contains_key(hyperlink_id) {
            return Ok(());
        }
        if let Some(hl) = self.try_load_hyperlink(hyperlink_id) {
            self.hyperlinks.insert(hyperlink_id.to_string(), hl);
        }
        Ok(())
    }

    fn rebuild_relation_indexes(&mut self) {
        let edge_ids = self.graph_meta.as_ref()
            .map(|m| m.edge_ids.clone())
            .unwrap_or_default();
        let embed_ids = self.graph_meta.as_ref()
            .map(|m| m.embed_instance_ids.clone())
            .unwrap_or_default();
        let link_ids = self.graph_meta.as_ref()
            .map(|m| m.link_ids.clone())
            .unwrap_or_default();
        let hl_ids = self.graph_meta.as_ref()
            .map(|m| m.hyperlink_ids.clone())
            .unwrap_or_default();

        let mut edge_relation_index = Vec::new();
        for edge_id in &edge_ids {
            if !self.edges.contains_key(edge_id) {
                if let Some(edge) = self.try_load_edge(edge_id) {
                    self.edges.insert(edge_id.clone(), edge);
                }
            }
            if let Some(edge) = self.edges.get(edge_id) {
                edge_relation_index.push(EdgeRelationIndex {
                    edge_id: edge.id.clone(),
                    from: edge.from.clone().unwrap_or_default(),
                    to: edge.to.clone().unwrap_or_default(),
                    from_endpoint: edge.from_endpoint.clone(),
                    to_endpoint: edge.to_endpoint.clone(),
                    from_endpoint_path: edge.from_endpoint_path.clone(),
                    to_endpoint_path: edge.to_endpoint_path.clone(),
                });
            }
        }

        let mut embed_host_index = Vec::new();
        for instance_id in &embed_ids {
            if !self.embeds.contains_key(instance_id) {
                if let Some(embed) = self.try_load_embed(instance_id) {
                    self.embeds.insert(instance_id.clone(), embed);
                }
            }
            if let Some(embed) = self.embeds.get(instance_id) {
                embed_host_index.push(EmbedHostIndex {
                    instance_id: embed.instance_id.clone(),
                    host_node_id: embed.source_node_id.clone().unwrap_or_default(),
                    host_endpoint: embed.host_endpoint.clone(),
                });
            }
        }

        let mut link_relation_index = Vec::new();
        for link_id in &link_ids {
            if !self.links.contains_key(link_id) {
                if let Some(link) = self.try_load_link(link_id) {
                    self.links.insert(link_id.clone(), link);
                }
            }
            if let Some(link) = self.links.get(link_id) {
                link_relation_index.push(LinkRelationIndex {
                    link_id: link.link_id.clone(),
                    source_node_id: endpoint_node_id(&link.source).unwrap_or_default().to_string(),
                });
            }
        }

        let mut hyperlink_relation_index = Vec::new();
        for hl_id in &hl_ids {
            if !self.hyperlinks.contains_key(hl_id) {
                if let Some(hl) = self.try_load_hyperlink(hl_id) {
                    self.hyperlinks.insert(hl_id.clone(), hl);
                }
            }
            if let Some(hl) = self.hyperlinks.get(hl_id) {
                hyperlink_relation_index.push(HyperlinkRelationIndex {
                    hyperlink_id: hl.hyperlink_id.clone(),
                    source_node_id: endpoint_path_node_id(&hl.source).unwrap_or_default().to_string(),
                });
            }
        }

        let mut node_kind_counts = HashMap::new();
        for node in self.nodes.values() {
            *node_kind_counts.entry(format!("{:?}", node.kind)).or_insert(0u32) += 1;
        }

        if let Some(ref mut meta) = self.graph_meta {
            meta.edge_relation_index = edge_relation_index;
            meta.embed_host_index = embed_host_index;
            meta.link_relation_index = link_relation_index;
            meta.hyperlink_relation_index = hyperlink_relation_index;
            meta.node_kind_counts = node_kind_counts;
        }
        self.dirty_graph_meta = true;
        self.enqueue_save(SaveQueueEntry::GraphMeta);
        self.record_migration("rebuild_relation_indexes", "rebuilt relation indexes from object files for no-index legacy package");
    }

    fn prefetch_nearby_objects(&mut self, _diagnostics: &mut Vec<LoadDiagnostic>) {
        let loaded_node_ids: HashSet<String> = self.nodes.keys().cloned().collect();
        let mut adjacent_node_ids: HashSet<String> = HashSet::new();

        let has_index = self.graph_meta.as_ref()
            .map(|m| !m.edge_relation_index.is_empty() || m.edge_ids.is_empty())
            .unwrap_or(false);

        if has_index {
            let edge_relation_index = self.graph_meta.as_ref().unwrap().edge_relation_index.clone();
            for eri in &edge_relation_index {
                let refs = extract_eri_node_refs(eri);
                for node_id in &refs {
                    if loaded_node_ids.contains(*node_id) {
                        for other_id in &refs {
                            if other_id != node_id && !self.nodes.contains_key(*other_id) && !other_id.is_empty() {
                                adjacent_node_ids.insert(other_id.to_string());
                            }
                        }
                    }
                }
            }
        } else {
            self.rebuild_relation_indexes();
            self.prefetch_nearby_objects(_diagnostics);
            return;
        }

        for node_id in &adjacent_node_ids {
            if !self.nodes.contains_key(node_id) {
                if let Some(node) = self.try_load_node(node_id) {
                    self.nodes.insert(node_id.clone(), node);
                }
            }
        }

        let has_edge_index = self.graph_meta.as_ref()
            .map(|m| !m.edge_relation_index.is_empty() || m.edge_ids.is_empty())
            .unwrap_or(false);
        let has_embed_index = self.graph_meta.as_ref()
            .map(|m| !m.embed_host_index.is_empty() || m.embed_instance_ids.is_empty())
            .unwrap_or(false);

        if has_edge_index {
            let edge_relation_index = self.graph_meta.as_ref().unwrap().edge_relation_index.clone();
            for eri in &edge_relation_index {
                if !self.edges.contains_key(&eri.edge_id) {
                    let refs = extract_eri_node_refs(eri);
                    let any_loaded = refs.iter().any(|id| self.nodes.contains_key(*id));
                    if any_loaded {
                        if let Some(edge) = self.try_load_edge(&eri.edge_id) {
                            self.edges.insert(eri.edge_id.clone(), edge);
                        }
                    }
                }
            }
        } else {
            self.rebuild_relation_indexes();
            let edge_relation_index = self.graph_meta.as_ref().unwrap().edge_relation_index.clone();
            for eri in &edge_relation_index {
                if !self.edges.contains_key(&eri.edge_id) {
                    let refs = extract_eri_node_refs(eri);
                    let any_loaded = refs.iter().any(|id| self.nodes.contains_key(*id));
                    if any_loaded {
                        if let Some(edge) = self.try_load_edge(&eri.edge_id) {
                            self.edges.insert(eri.edge_id.clone(), edge);
                        }
                    }
                }
            }
        }

        if has_embed_index {
            let embed_host_index = self.graph_meta.as_ref().unwrap().embed_host_index.clone();
            for ehi in &embed_host_index {
                if !self.embeds.contains_key(&ehi.instance_id) {
                    let refs = extract_ehi_node_refs(ehi);
                    let any_loaded = refs.iter().any(|id| self.nodes.contains_key(*id));
                    if any_loaded {
                        if let Some(embed) = self.try_load_embed(&ehi.instance_id) {
                            self.embeds.insert(ehi.instance_id.clone(), embed);
                        }
                    }
                }
            }
        } else {
            self.rebuild_relation_indexes();
            let embed_host_index = self.graph_meta.as_ref().unwrap().embed_host_index.clone();
            for ehi in &embed_host_index {
                if !self.embeds.contains_key(&ehi.instance_id) {
                    let refs = extract_ehi_node_refs(ehi);
                    let any_loaded = refs.iter().any(|id| self.nodes.contains_key(*id));
                    if any_loaded {
                        if let Some(embed) = self.try_load_embed(&ehi.instance_id) {
                            self.embeds.insert(ehi.instance_id.clone(), embed);
                        }
                    }
                }
            }
        }
    }

    fn load_remaining_objects(&mut self, _diagnostics: &mut Vec<LoadDiagnostic>) {
        let all_node_ids = self.graph_meta.as_ref()
            .map(|m| m.node_ids.clone())
            .unwrap_or_default();
        let all_edge_ids = self.graph_meta.as_ref()
            .map(|m| m.edge_ids.clone())
            .unwrap_or_default();
        let all_embed_ids = self.graph_meta.as_ref()
            .map(|m| m.embed_instance_ids.clone())
            .unwrap_or_default();
        let all_hl_ids = self.graph_meta.as_ref()
            .map(|m| m.hyperlink_ids.clone())
            .unwrap_or_default();
        let all_link_ids = self.graph_meta.as_ref()
            .map(|m| m.link_ids.clone())
            .unwrap_or_default();

        for node_id in &all_node_ids {
            if !self.nodes.contains_key(node_id) {
                if let Some(node) = self.try_load_node(node_id) {
                    self.nodes.insert(node_id.clone(), node);
                }
            }
        }
        for edge_id in &all_edge_ids {
            if !self.edges.contains_key(edge_id) {
                if let Some(edge) = self.try_load_edge(edge_id) {
                    self.edges.insert(edge_id.clone(), edge);
                }
            }
        }
        for instance_id in &all_embed_ids {
            if !self.embeds.contains_key(instance_id) {
                if let Some(embed) = self.try_load_embed(instance_id) {
                    self.embeds.insert(instance_id.clone(), embed);
                }
            }
        }
        for hl_id in &all_hl_ids {
            if !self.hyperlinks.contains_key(hl_id) {
                if let Some(hl) = self.try_load_hyperlink(hl_id) {
                    self.hyperlinks.insert(hl_id.clone(), hl);
                }
            }
        }
        for link_id in &all_link_ids {
            if !self.links.contains_key(link_id) {
                if let Some(link) = self.try_load_link(link_id) {
                    self.links.insert(link_id.clone(), link);
                }
            }
        }
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

    pub fn load_full(&mut self) -> Result<StarMapStoreResult> {
        self.recovery_log.clear();
        let mut diagnostics = Vec::new();

        self.load_recovery_from_disk();

        let graph_dir = self.starmap_dir();
        let graph_json_path = graph_dir.join("graph.json");

        if graph_json_path.exists() {
            let content = std::fs::read_to_string(&graph_json_path).unwrap_or_default();
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
                let schema_version_str = value.get("schemaVersion")
                    .or_else(|| value.get("schema_version"))
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let is_new_format = schema_version_str.as_deref() == Some("2");

                if let Some(ref sv) = schema_version_str {
                    if sv != "2" && sv != "1" {
                        diagnostics.push(LoadDiagnostic {
                            kind: LoadDiagnosticKind::UnsupportedVersion,
                            object_type: "graph".to_string(),
                            object_id: self.starmap_id.clone(),
                            detail: format!("unsupported schemaVersion: {}", sv),
                        });
                    }
                }

                if is_new_format {
                    match serde_json::from_str::<GraphMeta>(&content) {
                        Ok(meta) => { self.graph_meta = Some(meta); }
                        Err(e) => {
                            diagnostics.push(LoadDiagnostic {
                                kind: LoadDiagnosticKind::Corrupt,
                                object_type: "graph".to_string(),
                                object_id: self.starmap_id.clone(),
                                detail: format!("graph.json v2 parse failed: {}", e),
                            });
                            self.scan_objects_from_disk(&mut diagnostics);
                        }
                    }
                } else if let Ok(graph) = serde_json::from_str::<StarMapGraph>(&content) {
                    self.graph_meta = Some(GraphMeta {
                        schema_version: "2".to_string(),
                        starmap_id: graph.starmap_id.clone(),
                        title: graph.title.clone(),
                        node_ids: graph.nodes.iter().map(|n| n.id.clone()).collect(),
                        edge_ids: graph.edges.iter().map(|e| e.id.clone()).collect(),
                        embed_instance_ids: graph.embeds.iter().map(|e| e.instance_id.clone()).collect(),
                        link_ids: graph.links.iter().map(|l| l.link_id.clone()).collect(),
                        hyperlink_ids: vec![],
                        edge_relation_index: graph.edges.iter().map(|e| EdgeRelationIndex {
                            edge_id: e.id.clone(),
                            from: e.from.clone().unwrap_or_default(),
                            to: e.to.clone().unwrap_or_default(),
                            from_endpoint: e.from_endpoint.clone(),
                            to_endpoint: e.to_endpoint.clone(),
                            from_endpoint_path: e.from_endpoint_path.clone(),
                            to_endpoint_path: e.to_endpoint_path.clone(),
                        }).collect(),
                        embed_host_index: graph.embeds.iter().map(|e| EmbedHostIndex {
                            instance_id: e.instance_id.clone(),
                            host_node_id: e.source_node_id.clone().unwrap_or_default(),
                            host_endpoint: e.host_endpoint.clone(),
                        }).collect(),
                        link_relation_index: graph.links.iter().map(|l| LinkRelationIndex {
                            link_id: l.link_id.clone(),
                            source_node_id: endpoint_node_id(&l.source).unwrap_or_default().to_string(),
                        }).collect(),
                        hyperlink_relation_index: vec![],
                        node_kind_counts: {
                            let mut counts = HashMap::new();
                            for node in &graph.nodes {
                                *counts.entry(format!("{:?}", node.kind)).or_insert(0u32) += 1;
                            }
                            counts
                        },
                        package_revision: 0,
                        updated_at: graph.updated_at,
                    });
                    for node in &graph.nodes {
                        self.nodes.insert(node.id.clone(), node.clone());
                        self.dirty_nodes.insert(node.id.clone());
                    }
                    for edge in &graph.edges {
                        self.edges.insert(edge.id.clone(), edge.clone());
                        self.dirty_edges.insert(edge.id.clone());
                    }
                    for embed in &graph.embeds {
                        self.embeds.insert(embed.instance_id.clone(), embed.clone());
                        self.dirty_embeds.insert(embed.instance_id.clone());
                    }
                    for link in &graph.links {
                        self.links.insert(link.link_id.clone(), link.clone());
                        self.dirty_links.insert(link.link_id.clone());
                    }
                    self.dirty_graph_meta = true;
                    self.enqueue_save(SaveQueueEntry::Node);
                    self.enqueue_save(SaveQueueEntry::Edge);
                    self.enqueue_save(SaveQueueEntry::Embed);
                    self.enqueue_save(SaveQueueEntry::Link);
                    self.enqueue_save(SaveQueueEntry::GraphMeta);
                    self.record_migration("graph_v1_to_v2", "migrated inline v1 graph.json to v2 package format");
                } else {
                    match self.load_graph_meta_from_file(&graph_json_path) {
                        Ok(meta) => { self.graph_meta = Some(meta); }
                        Err(e) => {
                            diagnostics.push(LoadDiagnostic {
                                kind: LoadDiagnosticKind::Corrupt,
                                object_type: "graph".to_string(),
                                object_id: self.starmap_id.clone(),
                                detail: format!("graph.json parse failed: {}", e),
                            });
                            self.scan_objects_from_disk(&mut diagnostics);
                        }
                    }
                }
            } else {
                diagnostics.push(LoadDiagnostic {
                    kind: LoadDiagnosticKind::Corrupt,
                    object_type: "graph".to_string(),
                    object_id: self.starmap_id.clone(),
                    detail: "graph.json is not valid JSON".to_string(),
                });
                self.scan_objects_from_disk(&mut diagnostics);
            }
        } else {
            self.scan_objects_from_disk(&mut diagnostics);
        }

        let node_ids = self.graph_meta.as_ref()
            .map(|m| m.node_ids.clone())
            .unwrap_or_default();

        for node_id in &node_ids {
            if !self.nodes.contains_key(node_id) {
                if let Some(node) = self.try_load_node(node_id) {
                    self.nodes.insert(node_id.clone(), node);
                }
            }
        }

        let edge_ids = self.graph_meta.as_ref()
            .map(|m| m.edge_ids.clone())
            .unwrap_or_default();

        for edge_id in &edge_ids {
            if !self.edges.contains_key(edge_id) {
                if let Some(edge) = self.try_load_edge(edge_id) {
                    self.edges.insert(edge_id.clone(), edge);
                }
            }
        }

        let embed_ids = self.graph_meta.as_ref()
            .map(|m| m.embed_instance_ids.clone())
            .unwrap_or_default();

        for instance_id in &embed_ids {
            if !self.embeds.contains_key(instance_id) {
                if let Some(embed) = self.try_load_embed(instance_id) {
                    self.embeds.insert(instance_id.clone(), embed);
                }
            }
        }

        let hl_ids = self.graph_meta.as_ref()
            .map(|m| m.hyperlink_ids.clone())
            .unwrap_or_default();

        for hl_id in &hl_ids {
            if !self.hyperlinks.contains_key(hl_id) {
                if let Some(hl) = self.try_load_hyperlink(hl_id) {
                    self.hyperlinks.insert(hl_id.clone(), hl);
                }
            }
        }

        let link_ids = self.graph_meta.as_ref()
            .map(|m| m.link_ids.clone())
            .unwrap_or_default();

        for link_id in &link_ids {
            if !self.links.contains_key(link_id) {
                if let Some(link_path) = self.try_load_link(link_id) {
                    self.links.insert(link_id.clone(), link_path);
                }
            }
        }

        self.layout = self.try_load_layout();
        self.viewport = self.try_load_viewport();

        self.detect_dangling_references(&mut diagnostics);
        self.detect_orphan_objects(&mut diagnostics);

        self.package_revision = self.graph_meta.as_ref()
            .map(|m| m.package_revision)
            .unwrap_or(0);

        self.current_load_phase = Some(LoadPhase::BackgroundFullLoad);

        diagnostics.extend(self.recovery_log.drain(..));
        self.recovery_log = diagnostics.clone();

        Ok(StarMapStoreResult {
            diagnostics,
            loaded_node_count: self.nodes.len(),
            loaded_edge_count: self.edges.len(),
            loaded_embed_count: self.embeds.len(),
            loaded_link_count: self.links.len(),
            loaded_hyperlink_count: self.hyperlinks.len(),
        })
    }

    pub fn upsert_node(&mut self, node: StarMapNode) {
        let node_id = node.id.clone();
        let kind_key = format!("{:?}", node.kind);
        let is_new = !self.nodes.contains_key(&node_id);
        self.nodes.insert(node_id.clone(), node);
        self.dirty_nodes.insert(node_id.clone());
        if is_new {
            if self.graph_meta.is_none() {
                self.ensure_graph_meta_initialized();
            }
            if let Some(ref mut meta) = self.graph_meta {
                if !meta.node_ids.contains(&node_id) {
                    meta.node_ids.push(node_id);
                }
                *meta.node_kind_counts.entry(kind_key).or_insert(0u32) += 1;
            }
            self.dirty_graph_meta = true;
        }
    }

    pub fn remove_node(&mut self, node_id: &str) {
        if let Some(node) = self.nodes.get(node_id) {
            let kind_key = format!("{:?}", node.kind);
            if let Some(ref mut meta) = self.graph_meta {
                if let Some(count) = meta.node_kind_counts.get_mut(&kind_key) {
                    *count = count.saturating_sub(1);
                }
            }
        }
        self.nodes.remove(node_id);
        self.dirty_nodes.remove(node_id);
        self.deleted_node_ids.insert(node_id.to_string());
        if self.graph_meta.is_none() {
            self.ensure_graph_meta_initialized();
        }
        if let Some(ref mut meta) = self.graph_meta {
            meta.node_ids.retain(|id| id != node_id);
        }
        self.dirty_graph_meta = true;
    }

    pub fn upsert_edge(&mut self, edge: StarMapEdge) {
        let edge_id = edge.id.clone();
        let is_new = !self.edges.contains_key(&edge_id);
        let from = edge.from.clone().unwrap_or_default();
        let to = edge.to.clone().unwrap_or_default();
        let from_endpoint = edge.from_endpoint.clone();
        let to_endpoint = edge.to_endpoint.clone();
        let from_endpoint_path = edge.from_endpoint_path.clone();
        let to_endpoint_path = edge.to_endpoint_path.clone();
        self.edges.insert(edge_id.clone(), edge);
        self.dirty_edges.insert(edge_id.clone());
        if self.graph_meta.is_none() {
            self.ensure_graph_meta_initialized();
        }
        if let Some(ref mut meta) = self.graph_meta {
            if is_new {
                if !meta.edge_ids.contains(&edge_id) {
                    meta.edge_ids.push(edge_id.clone());
                }
                meta.edge_relation_index.push(EdgeRelationIndex {
                    edge_id: edge_id.clone(),
                    from: from.clone(),
                    to: to.clone(),
                    from_endpoint,
                    to_endpoint,
                    from_endpoint_path,
                    to_endpoint_path,
                });
            } else {
                if let Some(eri) = meta.edge_relation_index.iter_mut().find(|e| e.edge_id == edge_id) {
                    eri.from = from;
                    eri.to = to;
                    eri.from_endpoint = from_endpoint;
                    eri.to_endpoint = to_endpoint;
                    eri.from_endpoint_path = from_endpoint_path;
                    eri.to_endpoint_path = to_endpoint_path;
                }
            }
        }
        self.dirty_graph_meta = true;
    }

    pub fn remove_edge(&mut self, edge_id: &str) {
        self.edges.remove(edge_id);
        self.dirty_edges.remove(edge_id);
        self.deleted_edge_ids.insert(edge_id.to_string());
        if self.graph_meta.is_none() {
            self.ensure_graph_meta_initialized();
        }
        if let Some(ref mut meta) = self.graph_meta {
            meta.edge_ids.retain(|id| id != edge_id);
            meta.edge_relation_index.retain(|eri| eri.edge_id != edge_id);
        }
        self.dirty_graph_meta = true;
    }

    pub fn upsert_embed(&mut self, embed: StarMapEmbed) {
        let instance_id = embed.instance_id.clone();
        let is_new = !self.embeds.contains_key(&instance_id);
        let host_node_id = embed.source_node_id.clone().unwrap_or_default();
        let host_endpoint = embed.host_endpoint.clone();
        self.embeds.insert(instance_id.clone(), embed);
        self.dirty_embeds.insert(instance_id.clone());
        if self.graph_meta.is_none() {
            self.ensure_graph_meta_initialized();
        }
        if let Some(ref mut meta) = self.graph_meta {
            if is_new {
                if !meta.embed_instance_ids.contains(&instance_id) {
                    meta.embed_instance_ids.push(instance_id.clone());
                }
                meta.embed_host_index.push(EmbedHostIndex {
                    instance_id: instance_id.clone(),
                    host_node_id: host_node_id.clone(),
                    host_endpoint,
                });
            } else {
                if let Some(ehi) = meta.embed_host_index.iter_mut().find(|e| e.instance_id == instance_id) {
                    ehi.host_node_id = host_node_id;
                    ehi.host_endpoint = host_endpoint;
                }
            }
        }
        self.dirty_graph_meta = true;
    }

    pub fn remove_embed(&mut self, instance_id: &str) {
        self.embeds.remove(instance_id);
        self.dirty_embeds.remove(instance_id);
        self.deleted_embed_ids.insert(instance_id.to_string());
        if self.graph_meta.is_none() {
            self.ensure_graph_meta_initialized();
        }
        if let Some(ref mut meta) = self.graph_meta {
            meta.embed_instance_ids.retain(|id| id != instance_id);
            meta.embed_host_index.retain(|ehi| ehi.instance_id != instance_id);
        }
        self.dirty_graph_meta = true;
    }

    pub fn upsert_hyperlink(&mut self, hl: StarMapHyperlink) {
        let hl_id = hl.hyperlink_id.clone();
        let is_new = !self.hyperlinks.contains_key(&hl_id);
        let source_node_id = endpoint_path_node_id(&hl.source).unwrap_or_default().to_string();
        self.hyperlinks.insert(hl_id.clone(), hl);
        self.dirty_hyperlinks.insert(hl_id.clone());
        if is_new {
            if self.graph_meta.is_none() {
                self.ensure_graph_meta_initialized();
            }
            if let Some(ref mut meta) = self.graph_meta {
                if !meta.hyperlink_ids.contains(&hl_id) {
                    meta.hyperlink_ids.push(hl_id.clone());
                }
                meta.hyperlink_relation_index.push(HyperlinkRelationIndex {
                    hyperlink_id: hl_id.clone(),
                    source_node_id,
                });
            }
            self.dirty_graph_meta = true;
        } else {
            if let Some(ref mut meta) = self.graph_meta {
                if let Some(hri) = meta.hyperlink_relation_index.iter_mut().find(|hri| hri.hyperlink_id == hl_id) {
                    hri.source_node_id = source_node_id;
                }
            }
            self.dirty_graph_meta = true;
        }
    }

    pub fn upsert_link(&mut self, link: StarMapLink) {
        let link_id = link.link_id.clone();
        let is_new = !self.links.contains_key(&link_id);
        let source_node_id = endpoint_node_id(&link.source).unwrap_or_default().to_string();
        self.links.insert(link_id.clone(), link);
        self.dirty_links.insert(link_id.clone());
        if is_new {
            if self.graph_meta.is_none() {
                self.ensure_graph_meta_initialized();
            }
            if let Some(ref mut meta) = self.graph_meta {
                if !meta.link_ids.contains(&link_id) {
                    meta.link_ids.push(link_id.clone());
                }
                meta.link_relation_index.push(LinkRelationIndex {
                    link_id: link_id.clone(),
                    source_node_id,
                });
            }
            self.dirty_graph_meta = true;
        } else {
            if let Some(ref mut meta) = self.graph_meta {
                if let Some(lri) = meta.link_relation_index.iter_mut().find(|lri| lri.link_id == link_id) {
                    lri.source_node_id = source_node_id;
                }
            }
            self.dirty_graph_meta = true;
        }
    }

    pub fn remove_link(&mut self, link_id: &str) {
        self.links.remove(link_id);
        self.dirty_links.remove(link_id);
        self.deleted_link_ids.insert(link_id.to_string());
        if self.graph_meta.is_none() {
            self.ensure_graph_meta_initialized();
        }
        if let Some(ref mut meta) = self.graph_meta {
            meta.link_ids.retain(|id| id != link_id);
            meta.link_relation_index.retain(|lri| lri.link_id != link_id);
        }
        self.dirty_graph_meta = true;
    }

    pub fn remove_hyperlink(&mut self, hyperlink_id: &str) {
        self.hyperlinks.remove(hyperlink_id);
        self.dirty_hyperlinks.remove(hyperlink_id);
        self.deleted_hyperlink_ids.insert(hyperlink_id.to_string());
        if self.graph_meta.is_none() {
            self.ensure_graph_meta_initialized();
        }
        if let Some(ref mut meta) = self.graph_meta {
            meta.hyperlink_ids.retain(|id| id != hyperlink_id);
            meta.hyperlink_relation_index.retain(|hri| hri.hyperlink_id != hyperlink_id);
        }
        self.dirty_graph_meta = true;
    }

    pub fn set_layout(&mut self, layout: StarMapLayout) {
        self.layout = Some(layout);
        self.dirty_layout = true;
    }

    pub fn set_viewport(&mut self, viewport: StarMapViewport) {
        self.viewport = Some(viewport);
    }

    pub fn add_node(
        &mut self,
        node: StarMapNode,
        default_x: f32,
        default_y: f32,
    ) -> StarMapNode {
        let result = node.clone();
        self.upsert_node(node);
        if let Some(ref mut layout) = self.layout {
            layout.nodes.push(StarMapLayoutNode {
                node_id: result.id.clone(),
                x: default_x,
                y: default_y,
                width: 150.0,
                height: 60.0,
                radius: 30.0,
                collapsed: false,
                z_index: 0,
                scale: 1.0,
                depth: 0.0,
                focus_weight: 0.0,
                orbit_group: None,
            });
            self.dirty_layout = true;
        }
        result
    }

    pub fn update_node(&mut self, node_id: &str, patch: &StarMapNodePatch) -> Result<StarMapNode> {
        if !self.nodes.contains_key(node_id) {
            self.ensure_object_loaded(node_id)?;
        }
        let node = self.nodes.get_mut(node_id).ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Node not found",
            ))
        })?;
        if let Some(ref t) = patch.title { node.title = t.clone(); }
        if let Some(ref k) = patch.kind {
            let old_kind_key = format!("{:?}", node.kind);
            let new_kind_key = format!("{:?}", k);
            if old_kind_key != new_kind_key {
                if let Some(ref mut meta) = self.graph_meta {
                    if let Some(count) = meta.node_kind_counts.get_mut(&old_kind_key) {
                        *count = count.saturating_sub(1);
                    }
                    *meta.node_kind_counts.entry(new_kind_key).or_insert(0u32) += 1;
                }
                self.dirty_graph_meta = true;
            }
            node.kind = k.clone();
        }
        if let Some(ref p) = patch.payload { node.payload = p.clone(); }
        if let Some(ref t) = patch.tags { node.tags = t.clone(); }
        if let Some(ref c) = patch.content { node.content = c.clone(); }
        if let Some(ref a) = patch.anchors { node.anchors = a.clone(); }
        if let Some(ref p) = patch.portal { node.portal = p.clone(); }
        if let Some(ref dp) = patch.display_policy { node.display_policy = dp.clone(); }
        if let Some(ref ob) = patch.open_behavior { node.open_behavior = ob.clone(); }
        if let Some(ref p) = patch.provenance { node.provenance = p.clone(); }
        node.updated_at = crate::starmap::now_epoch();
        let updated = node.clone();
        self.dirty_nodes.insert(node_id.to_string());
        Ok(updated)
    }

    pub fn delete_node(&mut self, node_id: &str) -> Result<()> {
        if !self.nodes.contains_key(node_id) {
            self.ensure_object_loaded(node_id)?;
        }
        if !self.nodes.contains_key(node_id) {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Node not found",
            )));
        }

        let edge_ids_to_remove: Vec<String> = self.graph_meta.as_ref()
            .map(|m| m.edge_relation_index.iter()
                .filter(|eri| extract_eri_node_refs(eri).contains(&node_id))
                .map(|eri| eri.edge_id.clone())
                .collect())
            .unwrap_or_default();

        let embed_ids_to_remove: Vec<String> = self.graph_meta.as_ref()
            .map(|m| m.embed_host_index.iter()
                .filter(|ehi| extract_ehi_node_refs(ehi).contains(&node_id))
                .map(|ehi| ehi.instance_id.clone())
                .collect())
            .unwrap_or_default();

        let link_ids_to_remove: Vec<String> = self.graph_meta.as_ref()
            .map(|m| m.link_relation_index.iter()
                .filter(|lri| lri.source_node_id == node_id)
                .map(|lri| lri.link_id.clone())
                .collect())
            .unwrap_or_default();

        let hyperlink_ids_to_remove: Vec<String> = self.graph_meta.as_ref()
            .map(|m| m.hyperlink_relation_index.iter()
                .filter(|hri| hri.source_node_id == node_id)
                .map(|hri| hri.hyperlink_id.clone())
                .collect())
            .unwrap_or_default();

        self.remove_node(node_id);

        for eid in &edge_ids_to_remove {
            self.remove_edge(eid);
        }

        for iid in &embed_ids_to_remove {
            self.remove_embed(iid);
        }

        for lid in &link_ids_to_remove {
            self.remove_link(lid);
        }

        for hlid in &hyperlink_ids_to_remove {
            self.remove_hyperlink(hlid);
        }

        if let Some(ref mut layout) = self.layout {
            layout.nodes.retain(|n| n.node_id != node_id);
            self.dirty_layout = true;
        }

        Ok(())
    }

    pub fn add_edge(&mut self, edge: StarMapEdge) -> Result<StarMapEdge> {
        if let Some(ref from_id) = edge.from {
            if !self.nodes.contains_key(from_id) {
                let _ = self.ensure_object_loaded(from_id);
            }
        }
        if let Some(ref to_id) = edge.to {
            if !self.nodes.contains_key(to_id) {
                let _ = self.ensure_object_loaded(to_id);
            }
        }
        let node_id_exists = |id: &str| -> bool {
            self.nodes.contains_key(id)
                || self.graph_meta.as_ref().map_or(false, |m| m.node_ids.contains(&id.to_string()))
        };
        let from_valid = edge.from_target.is_some()
            || edge.from_endpoint.is_some()
            || edge.from_endpoint_path.is_some()
            || edge.from.as_ref()
                .map(|id| node_id_exists(id))
                .unwrap_or(false);
        let to_valid = edge.to_target.is_some()
            || edge.to_endpoint.is_some()
            || edge.to_endpoint_path.is_some()
            || edge.to.as_ref()
                .map(|id| node_id_exists(id))
                .unwrap_or(false);

        if !from_valid || !to_valid {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Edge nodes do not exist and no deep target is provided",
            )));
        }

        let result = edge.clone();
        self.upsert_edge(edge);
        Ok(result)
    }

    pub fn update_edge(&mut self, edge_id: &str, patch: &StarMapEdgePatch) -> Result<StarMapEdge> {
        if !self.edges.contains_key(edge_id) {
            self.ensure_edge_loaded(edge_id)?;
        }
        let edge = self.edges.get_mut(edge_id).ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Edge not found",
            ))
        })?;
        if let Some(ref k) = patch.kind { edge.kind = k.clone(); }
        if let Some(ref l) = patch.label { edge.label = l.clone(); }
        if let Some(ref p) = patch.payload { edge.payload = p.clone(); }
        let endpoints_changed = patch.from_target.is_some()
            || patch.to_target.is_some()
            || patch.from_endpoint.is_some()
            || patch.to_endpoint.is_some()
            || patch.from_endpoint_path.is_some()
            || patch.to_endpoint_path.is_some();
        if let Some(ref ft) = patch.from_target {
            edge.from_target = ft.clone();
            edge.from = ft.as_ref().and_then(|t| match &t.target {
                crate::starmap::semantic::StarMapTargetDetail::Node { node_id } => Some(node_id.clone()),
                crate::starmap::semantic::StarMapTargetDetail::Anchor { node_id, .. } => Some(node_id.clone()),
                _ => None,
            });
        }
        if let Some(ref tt) = patch.to_target {
            edge.to_target = tt.clone();
            edge.to = tt.as_ref().and_then(|t| match &t.target {
                crate::starmap::semantic::StarMapTargetDetail::Node { node_id } => Some(node_id.clone()),
                crate::starmap::semantic::StarMapTargetDetail::Anchor { node_id, .. } => Some(node_id.clone()),
                _ => None,
            });
        }
        if let Some(ref fe) = patch.from_endpoint { edge.from_endpoint = fe.clone(); }
        if let Some(ref te) = patch.to_endpoint { edge.to_endpoint = te.clone(); }
        if let Some(ref fep) = patch.from_endpoint_path { edge.from_endpoint_path = fep.clone(); }
        if let Some(ref tep) = patch.to_endpoint_path { edge.to_endpoint_path = tep.clone(); }
        edge.updated_at = crate::starmap::now_epoch();
        let updated = edge.clone();
        self.dirty_edges.insert(edge_id.to_string());
        if endpoints_changed {
            if let Some(ref mut meta) = self.graph_meta {
                if let Some(eri) = meta.edge_relation_index.iter_mut().find(|e| e.edge_id == edge_id) {
                    eri.from = updated.from.clone().unwrap_or_default();
                    eri.to = updated.to.clone().unwrap_or_default();
                    eri.from_endpoint = updated.from_endpoint.clone();
                    eri.to_endpoint = updated.to_endpoint.clone();
                    eri.from_endpoint_path = updated.from_endpoint_path.clone();
                    eri.to_endpoint_path = updated.to_endpoint_path.clone();
                }
            }
            self.dirty_graph_meta = true;
        }
        Ok(updated)
    }

    pub fn delete_edge(&mut self, edge_id: &str) -> Result<()> {
        if !self.edges.contains_key(edge_id) {
            self.ensure_edge_loaded(edge_id)?;
        }
        if !self.edges.contains_key(edge_id) {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Edge not found",
            )));
        }
        self.remove_edge(edge_id);
        Ok(())
    }

    pub fn add_embed(&mut self, embed: StarMapEmbed) -> Result<StarMapEmbed> {
        if self.embeds.contains_key(&embed.instance_id) {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Duplicate embed instance_id",
            )));
        }
        let result = embed.clone();
        self.upsert_embed(embed);
        Ok(result)
    }

    pub fn update_embed(&mut self, instance_id: &str, patch: &StarMapEmbedPatch) -> Result<StarMapEmbed> {
        if !self.embeds.contains_key(instance_id) {
            self.ensure_embed_loaded(instance_id)?;
        }
        let embed = self.embeds.get_mut(instance_id).ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Embed not found",
            ))
        })?;
        if let Some(ref l) = patch.label { embed.label = l.clone(); }
        if let Some(ref dp) = patch.display_policy { embed.display_policy = dp.clone(); }
        if let Some(ref ob) = patch.open_behavior { embed.open_behavior = ob.clone(); }
        if let Some(Some(ref pl)) = patch.placement { embed.placement = pl.clone(); }
        if let Some(Some(ref vp)) = patch.target_viewport { embed.target_viewport = vp.clone(); }
        if let Some(Some(ref vp)) = patch.viewport {
            embed.placement.width = vp.width;
            embed.placement.height = vp.height;
            embed.target_viewport.scale = vp.scale;
            embed.target_viewport.offset_x = vp.offset_x;
            embed.target_viewport.offset_y = vp.offset_y;
        }
        let host_changed = patch.source_node_id.is_some()
            || patch.host_endpoint.is_some()
            || patch.host_anchor.is_some();
        if let Some(ref sni) = patch.source_node_id { embed.source_node_id = sni.clone(); }
        if let Some(ref ep) = patch.host_endpoint { embed.host_endpoint = ep.clone(); }
        if let Some(Some(ref anchor_id)) = patch.host_anchor {
            if let Some(ref node_id) = embed.source_node_id {
                embed.host_endpoint = Some(StarMapEndpoint::Anchor {
                    node_id: node_id.clone(),
                    anchor_id: anchor_id.clone(),
                });
            }
        }
        embed.updated_at = crate::starmap::now_epoch();
        let updated = embed.clone();
        self.dirty_embeds.insert(instance_id.to_string());
        if host_changed {
            if let Some(ref mut meta) = self.graph_meta {
                if let Some(ehi) = meta.embed_host_index.iter_mut().find(|e| e.instance_id == instance_id) {
                    ehi.host_node_id = updated.source_node_id.clone().unwrap_or_default();
                    ehi.host_endpoint = updated.host_endpoint.clone();
                }
            }
            self.dirty_graph_meta = true;
        }
        Ok(updated)
    }

    pub fn delete_embed(&mut self, instance_id: &str) -> Result<()> {
        if !self.embeds.contains_key(instance_id) {
            self.ensure_embed_loaded(instance_id)?;
        }
        if !self.embeds.contains_key(instance_id) {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Embed not found",
            )));
        }
        self.remove_embed(instance_id);
        Ok(())
    }

    pub fn add_link(&mut self, link: StarMapLink) -> Result<StarMapLink> {
        if self.links.contains_key(&link.link_id) {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Duplicate link_id",
            )));
        }
        let result = link.clone();
        self.upsert_link(link);
        Ok(result)
    }

    pub fn update_link(&mut self, link_id: &str, patch: &StarMapLinkPatch) -> Result<StarMapLink> {
        if !self.links.contains_key(link_id) {
            self.ensure_link_loaded(link_id)?;
        }
        let link = self.links.get_mut(link_id).ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Link not found",
            ))
        })?;
        if let Some(ref s) = patch.source { link.source = s.clone(); }
        if let Some(ref t) = patch.target { link.target = t.clone(); }
        if let Some(ref l) = patch.label { link.label = l.clone(); }
        link.updated_at = crate::starmap::now_epoch();
        let updated = link.clone();
        self.dirty_links.insert(link_id.to_string());
        Ok(updated)
    }

    pub fn delete_link(&mut self, link_id: &str) -> Result<()> {
        if !self.links.contains_key(link_id) {
            self.ensure_link_loaded(link_id)?;
        }
        if !self.links.contains_key(link_id) {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Link not found",
            )));
        }
        self.remove_link(link_id);
        Ok(())
    }

    pub fn add_hyperlink(&mut self, hl: StarMapHyperlink) -> Result<StarMapHyperlink> {
        if self.hyperlinks.contains_key(&hl.hyperlink_id) {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "Duplicate hyperlink_id",
            )));
        }
        let result = hl.clone();
        self.upsert_hyperlink(hl);
        Ok(result)
    }

    pub fn update_hyperlink(&mut self, hyperlink_id: &str, label: Option<&str>, target_uri: Option<&str>) -> Result<StarMapHyperlink> {
        if !self.hyperlinks.contains_key(hyperlink_id) {
            self.ensure_hyperlink_loaded(hyperlink_id)?;
        }
        let hl = self.hyperlinks.get_mut(hyperlink_id).ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Hyperlink not found",
            ))
        })?;
        if let Some(l) = label { hl.label = Some(l.to_string()); }
        if let Some(u) = target_uri { hl.target_uri = u.to_string(); }
        hl.updated_at = crate::starmap::now_epoch();
        let updated = hl.clone();
        self.dirty_hyperlinks.insert(hyperlink_id.to_string());
        Ok(updated)
    }

    pub fn delete_hyperlink(&mut self, hyperlink_id: &str) -> Result<()> {
        if !self.hyperlinks.contains_key(hyperlink_id) {
            self.ensure_hyperlink_loaded(hyperlink_id)?;
        }
        if !self.hyperlinks.contains_key(hyperlink_id) {
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Hyperlink not found",
            )));
        }
        self.remove_hyperlink(hyperlink_id);
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        for node_id in &self.dirty_nodes {
            if let Some(node) = self.nodes.get(node_id) {
                package_storage::save_node(&self.workspace, &self.starmap_id, node)?;
            }
        }

        for edge_id in &self.dirty_edges {
            if let Some(edge) = self.edges.get(edge_id) {
                package_storage::save_edge(&self.workspace, &self.starmap_id, edge)?;
            }
        }

        for instance_id in &self.dirty_embeds {
            if let Some(embed) = self.embeds.get(instance_id) {
                package_storage::save_embed(&self.workspace, &self.starmap_id, embed)?;
            }
        }

        for link_id in &self.dirty_links {
            if let Some(link) = self.links.get(link_id) {
                package_storage::save_link(&self.workspace, &self.starmap_id, link)?;
            }
        }

        for hl_id in &self.dirty_hyperlinks {
            if let Some(hl) = self.hyperlinks.get(hl_id) {
                package_storage::save_hyperlink(&self.workspace, &self.starmap_id, hl)?;
            }
        }

        if self.dirty_layout {
            if let Some(ref layout) = self.layout {
                package_storage::save_layout(&self.workspace, &self.starmap_id, layout)?;
            }
        }

        let node_ids_to_delete: Vec<String> = self.deleted_node_ids.iter().cloned().collect();
        for node_id in &node_ids_to_delete {
            match package_storage::delete_node_file(&self.workspace, &self.starmap_id, node_id) {
                Ok(()) => { self.deleted_node_ids.remove(node_id); }
                Err(e) => {
                    self.record_delete_failure("node", node_id, &e);
                    self.flush_recovery_to_disk()?;
                    return Err(e);
                }
            }
        }

        let edge_ids_to_delete: Vec<String> = self.deleted_edge_ids.iter().cloned().collect();
        for edge_id in &edge_ids_to_delete {
            match package_storage::delete_edge_file(&self.workspace, &self.starmap_id, edge_id) {
                Ok(()) => { self.deleted_edge_ids.remove(edge_id); }
                Err(e) => {
                    self.record_delete_failure("edge", edge_id, &e);
                    self.flush_recovery_to_disk()?;
                    return Err(e);
                }
            }
        }

        let embed_ids_to_delete: Vec<String> = self.deleted_embed_ids.iter().cloned().collect();
        for instance_id in &embed_ids_to_delete {
            match package_storage::delete_embed_file(&self.workspace, &self.starmap_id, instance_id) {
                Ok(()) => { self.deleted_embed_ids.remove(instance_id); }
                Err(e) => {
                    self.record_delete_failure("embed", instance_id, &e);
                    self.flush_recovery_to_disk()?;
                    return Err(e);
                }
            }
        }

        let link_ids_to_delete: Vec<String> = self.deleted_link_ids.iter().cloned().collect();
        for link_id in &link_ids_to_delete {
            match package_storage::delete_link_file(&self.workspace, &self.starmap_id, link_id) {
                Ok(()) => { self.deleted_link_ids.remove(link_id); }
                Err(e) => {
                    self.record_delete_failure("link", link_id, &e);
                    self.flush_recovery_to_disk()?;
                    return Err(e);
                }
            }
        }

        let hl_ids_to_delete: Vec<String> = self.deleted_hyperlink_ids.iter().cloned().collect();
        for hl_id in &hl_ids_to_delete {
            match package_storage::delete_hyperlink_file(&self.workspace, &self.starmap_id, hl_id) {
                Ok(()) => { self.deleted_hyperlink_ids.remove(hl_id); }
                Err(e) => {
                    self.record_delete_failure("hyperlink", hl_id, &e);
                    self.flush_recovery_to_disk()?;
                    return Err(e);
                }
            }
        }

        let written_revision = self.update_graph_meta_file()?;
        self.package_revision = written_revision;

        let node_count = self.graph_meta.as_ref()
            .map(|m| m.node_ids.len() as u32)
            .unwrap_or(self.nodes.len() as u32);
        let edge_count = self.graph_meta.as_ref()
            .map(|m| m.edge_ids.len() as u32)
            .unwrap_or(self.edges.len() as u32);
        let linked_chapters = self.graph_meta.as_ref()
            .map(|m| *m.node_kind_counts.get("Chapter").unwrap_or(&0))
            .unwrap_or(0u32);
        crate::starmap::update_starmap_stats(
            &self.workspace,
            &self.starmap_id,
            node_count,
            edge_count,
            linked_chapters,
        )?;

        self.dirty_nodes.clear();
        self.dirty_edges.clear();
        self.dirty_embeds.clear();
        self.dirty_links.clear();
        self.dirty_hyperlinks.clear();
        self.dirty_layout = false;
        self.dirty_graph_meta = false;

        self.flush_recovery_to_disk()?;

        Ok(())
    }

    pub fn flush_viewport(&self) -> Result<()> {
        if let Some(ref viewport) = self.viewport {
            package_storage::save_viewport(&self.workspace, &self.starmap_id, viewport)?;
        }
        Ok(())
    }

    fn starmap_dir(&self) -> PathBuf {
        self.workspace.join("app-meta").join("starmaps").join(&self.starmap_id)
    }

    fn load_graph_meta_from_file(&self, path: &Path) -> Result<GraphMeta> {
        let content = std::fs::read_to_string(path)?;
        let value: serde_json::Value = serde_json::from_str(&content)?;

        if let Some(schema_version) = value.get("schemaVersion").or_else(|| value.get("schema_version")) {
            if let Some(sv_str) = schema_version.as_str() {
                if sv_str == "2" {
                    let meta: GraphMeta = serde_json::from_str(&content)?;
                    return Ok(meta);
                }
            }
            if let Some(sv_num) = schema_version.as_u64() {
                if sv_num == 1 {
                    if value.get("nodes").is_some() && value.get("nodes").and_then(|v| v.as_array()).is_some() {
                        let graph: StarMapGraph = serde_json::from_str(&content)?;
                        return Ok(GraphMeta {
                            schema_version: "2".to_string(),
                            starmap_id: graph.starmap_id.clone(),
                            title: graph.title.clone(),
                            node_ids: graph.nodes.iter().map(|n| n.id.clone()).collect(),
                            edge_ids: graph.edges.iter().map(|e| e.id.clone()).collect(),
                            embed_instance_ids: graph.embeds.iter().map(|e| e.instance_id.clone()).collect(),
                            link_ids: graph.links.iter().map(|l| l.link_id.clone()).collect(),
                        hyperlink_ids: vec![],
                        edge_relation_index: graph.edges.iter().map(|e| EdgeRelationIndex {
                            edge_id: e.id.clone(),
                            from: e.from.clone().unwrap_or_default(),
                            to: e.to.clone().unwrap_or_default(),
                            from_endpoint: e.from_endpoint.clone(),
                            to_endpoint: e.to_endpoint.clone(),
                            from_endpoint_path: e.from_endpoint_path.clone(),
                            to_endpoint_path: e.to_endpoint_path.clone(),
                        }).collect(),
                        embed_host_index: graph.embeds.iter().map(|e| EmbedHostIndex {
                            instance_id: e.instance_id.clone(),
                            host_node_id: e.source_node_id.clone().unwrap_or_default(),
                            host_endpoint: e.host_endpoint.clone(),
                        }).collect(),
                        link_relation_index: graph.links.iter().map(|l| LinkRelationIndex {
                            link_id: l.link_id.clone(),
                            source_node_id: endpoint_node_id(&l.source).unwrap_or_default().to_string(),
                        }).collect(),
                        hyperlink_relation_index: vec![],
                        node_kind_counts: {
                            let mut counts = HashMap::new();
                            for node in &graph.nodes {
                                *counts.entry(format!("{:?}", node.kind)).or_insert(0u32) += 1;
                            }
                            counts
                        },
                        package_revision: 0,
                        updated_at: graph.updated_at,
                        });
                    }
                    let meta: LegacyGraphMeta = serde_json::from_str(&content)?;
                    return Ok(GraphMeta {
                        schema_version: "2".to_string(),
                        starmap_id: meta.starmap_id.clone(),
                        title: meta.title.clone(),
                        node_ids: vec![],
                        edge_ids: vec![],
                        embed_instance_ids: vec![],
                        link_ids: vec![],
                        hyperlink_ids: vec![],
                        edge_relation_index: vec![],
                        embed_host_index: vec![],
                        link_relation_index: vec![],
                        hyperlink_relation_index: vec![],
                        node_kind_counts: HashMap::new(),
                        package_revision: 0,
                        updated_at: meta.updated_at,
                    });
                }
            }
        }

        let meta: GraphMeta = serde_json::from_str(&content)?;
        Ok(meta)
    }

    fn migrate_flat_to_bucket(&mut self, flat_path: &Path, bucket_path: &Path) {
        if !flat_path.exists() || bucket_path.exists() {
            return;
        }
        if let Some(parent) = bucket_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                self.recovery_log.push(LoadDiagnostic {
                    kind: LoadDiagnosticKind::Corrupt,
                    object_type: "migration".to_string(),
                    object_id: flat_path.to_string_lossy().to_string(),
                    detail: format!("flat_to_bucket: create_dir_all failed: {}", e),
                });
                return;
            }
        }
        let content = match std::fs::read(&flat_path) {
            Ok(c) => c,
            Err(e) => {
                self.recovery_log.push(LoadDiagnostic {
                    kind: LoadDiagnosticKind::Corrupt,
                    object_type: "migration".to_string(),
                    object_id: flat_path.to_string_lossy().to_string(),
                    detail: format!("flat_to_bucket: read failed: {}", e),
                });
                return;
            }
        };
        let tmp_path = bucket_path.with_extension("json.tmp");
        if let Err(e) = std::fs::write(&tmp_path, &content) {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Corrupt,
                object_type: "migration".to_string(),
                object_id: flat_path.to_string_lossy().to_string(),
                detail: format!("flat_to_bucket: write tmp failed: {}", e),
            });
            return;
        }
        if let Err(e) = std::fs::rename(&tmp_path, bucket_path) {
            let _ = std::fs::remove_file(&tmp_path);
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Corrupt,
                object_type: "migration".to_string(),
                object_id: flat_path.to_string_lossy().to_string(),
                detail: format!("flat_to_bucket: rename failed: {}", e),
            });
            return;
        }
        if let Err(e) = std::fs::remove_file(flat_path) {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Corrupt,
                object_type: "migration".to_string(),
                object_id: flat_path.to_string_lossy().to_string(),
                detail: format!("flat_to_bucket: remove old file failed: {}", e),
            });
        }
        let file_name = flat_path.file_name().unwrap_or_default().to_string_lossy();
        self.record_migration("flat_to_bucket", &format!("migrated {} from flat to bucket", file_name));
    }

    fn try_load_node(&mut self, node_id: &str) -> Option<StarMapNode> {
        let bucket_dir = self.starmap_dir().join("nodes").join(package_storage::bucket_for_id(node_id));
        let bucket_path = bucket_dir.join(format!("{}.json", node_id));
        let flat_path = self.starmap_dir().join("nodes").join(format!("{}.json", node_id));
        let (path, from_flat) = if bucket_path.exists() {
            (bucket_path.clone(), false)
        } else if flat_path.exists() {
            (flat_path.clone(), true)
        } else {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "node".to_string(),
                object_id: node_id.to_string(),
                detail: format!("node file not found: tried {} and {}", bucket_path.display(), flat_path.display()),
            });
            return None;
        };
        let content = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<StarMapNode>(&content) {
            Ok(node) => {
                if from_flat {
                    self.migrate_flat_to_bucket(&flat_path, &bucket_path);
                }
                Some(node)
            }
            Err(e) => {
                self.recovery_log.push(LoadDiagnostic {
                    kind: LoadDiagnosticKind::Corrupt,
                    object_type: "node".to_string(),
                    object_id: node_id.to_string(),
                    detail: format!("parse error: {}", e),
                });
                None
            }
        }
    }

    fn try_load_edge(&mut self, edge_id: &str) -> Option<StarMapEdge> {
        let bucket_dir = self.starmap_dir().join("edges").join(package_storage::bucket_for_id(edge_id));
        let bucket_path = bucket_dir.join(format!("{}.json", edge_id));
        let flat_path = self.starmap_dir().join("edges").join(format!("{}.json", edge_id));
        let (path, from_flat) = if bucket_path.exists() {
            (bucket_path.clone(), false)
        } else if flat_path.exists() {
            (flat_path.clone(), true)
        } else {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "edge".to_string(),
                object_id: edge_id.to_string(),
                detail: format!("edge file not found: tried {} and {}", bucket_path.display(), flat_path.display()),
            });
            return None;
        };
        let content = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<StarMapEdge>(&content) {
            Ok(edge) => {
                if from_flat {
                    self.migrate_flat_to_bucket(&flat_path, &bucket_path);
                }
                Some(edge)
            }
            Err(e) => {
                self.recovery_log.push(LoadDiagnostic {
                    kind: LoadDiagnosticKind::Corrupt,
                    object_type: "edge".to_string(),
                    object_id: edge_id.to_string(),
                    detail: format!("parse error: {}", e),
                });
                None
            }
        }
    }

    fn try_load_embed(&mut self, instance_id: &str) -> Option<StarMapEmbed> {
        let bucket_dir = self.starmap_dir().join("child_starmaps").join(package_storage::bucket_for_id(instance_id));
        let bucket_path = bucket_dir.join(format!("{}.json", instance_id));
        let flat_path = self.starmap_dir().join("child_starmaps").join(format!("{}.json", instance_id));
        let (path, from_flat) = if bucket_path.exists() {
            (bucket_path.clone(), false)
        } else if flat_path.exists() {
            (flat_path.clone(), true)
        } else {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "embed".to_string(),
                object_id: instance_id.to_string(),
                detail: format!("embed file not found: tried {} and {}", bucket_path.display(), flat_path.display()),
            });
            return None;
        };
        let content = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<StarMapEmbed>(&content) {
            Ok(embed) => {
                if from_flat {
                    self.migrate_flat_to_bucket(&flat_path, &bucket_path);
                }
                Some(embed)
            }
            Err(e) => {
                self.recovery_log.push(LoadDiagnostic {
                    kind: LoadDiagnosticKind::Corrupt,
                    object_type: "embed".to_string(),
                    object_id: instance_id.to_string(),
                    detail: format!("parse error: {}", e),
                });
                None
            }
        }
    }

    fn try_load_hyperlink(&mut self, hyperlink_id: &str) -> Option<StarMapHyperlink> {
        let bucket_dir = self.starmap_dir().join("hyperlinks").join(package_storage::bucket_for_id(hyperlink_id));
        let bucket_path = bucket_dir.join(format!("{}.json", hyperlink_id));
        let flat_path = self.starmap_dir().join("hyperlinks").join(format!("{}.json", hyperlink_id));
        let (path, from_flat) = if bucket_path.exists() {
            (bucket_path.clone(), false)
        } else if flat_path.exists() {
            (flat_path.clone(), true)
        } else {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "hyperlink".to_string(),
                object_id: hyperlink_id.to_string(),
                detail: format!("hyperlink file not found: tried {} and {}", bucket_path.display(), flat_path.display()),
            });
            return None;
        };
        let content = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<StarMapHyperlink>(&content) {
            Ok(hl) => {
                if from_flat {
                    self.migrate_flat_to_bucket(&flat_path, &bucket_path);
                }
                Some(hl)
            }
            Err(e) => {
                self.recovery_log.push(LoadDiagnostic {
                    kind: LoadDiagnosticKind::Corrupt,
                    object_type: "hyperlink".to_string(),
                    object_id: hyperlink_id.to_string(),
                    detail: format!("parse error: {}", e),
                });
                None
            }
        }
    }

    fn try_load_layout(&self) -> Option<StarMapLayout> {
        let dir = self.starmap_dir();
        if let Some(layout) = package_storage::load_layout_sharded(&dir) {
            return Some(layout);
        }
        if let Some(layout) = package_storage::load_legacy_layout(&dir) {
            if package_storage::save_layout_sharded(&dir, &layout).is_ok() {
                let legacy_path = dir.join("layouts").join("default.json");
                let _ = std::fs::remove_file(&legacy_path);
                self.record_migration("layout_sharded", "migrated legacy default.json to sharded format");
            }
            return Some(layout);
        }
        None
    }

    fn try_load_link(&mut self, link_id: &str) -> Option<StarMapLink> {
        let bucket_dir = self.starmap_dir().join("links").join(package_storage::bucket_for_id(link_id));
        let bucket_path = bucket_dir.join(format!("{}.json", link_id));
        let flat_path = self.starmap_dir().join("links").join(format!("{}.json", link_id));
        let (path, from_flat) = if bucket_path.exists() {
            (bucket_path.clone(), false)
        } else if flat_path.exists() {
            (flat_path.clone(), true)
        } else {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "link".to_string(),
                object_id: link_id.to_string(),
                detail: format!("link file not found: tried {} and {}", bucket_path.display(), flat_path.display()),
            });
            return None;
        };
        let content = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<StarMapLink>(&content) {
            Ok(link) => {
                if from_flat {
                    self.migrate_flat_to_bucket(&flat_path, &bucket_path);
                }
                Some(link)
            }
            Err(e) => {
                self.recovery_log.push(LoadDiagnostic {
                    kind: LoadDiagnosticKind::Corrupt,
                    object_type: "link".to_string(),
                    object_id: link_id.to_string(),
                    detail: format!("parse error: {}", e),
                });
                None
            }
        }
    }

    fn try_load_viewport(&self) -> Option<StarMapViewport> {
        package_storage::load_viewport(&self.workspace, &self.starmap_id)
    }

    fn scan_objects_from_disk(&mut self, _diagnostics: &mut Vec<LoadDiagnostic>) {
        self.scan_bucketed_dir_insert("nodes", |s, id, diag| {
            if let Some(node) = s.try_load_node(id) {
                s.nodes.insert(id.to_string(), node);
            }
            let _ = diag;
        });
        self.scan_bucketed_dir_insert("edges", |s, id, diag| {
            if let Some(edge) = s.try_load_edge(id) {
                s.edges.insert(id.to_string(), edge);
            }
            let _ = diag;
        });
        self.scan_bucketed_dir_insert("child_starmaps", |s, id, diag| {
            if let Some(embed) = s.try_load_embed(id) {
                s.embeds.insert(id.to_string(), embed);
            }
            let _ = diag;
        });
        self.scan_bucketed_dir_insert("hyperlinks", |s, id, diag| {
            if let Some(hl) = s.try_load_hyperlink(id) {
                s.hyperlinks.insert(id.to_string(), hl);
            }
            let _ = diag;
        });
        self.scan_bucketed_dir_insert("links", |s, id, diag| {
            if let Some(link) = s.try_load_link(id) {
                s.links.insert(id.to_string(), link);
            }
            let _ = diag;
        });
    }

    fn scan_bucketed_dir_insert<F>(&mut self, subdir: &str, insert_fn: F)
    where
        F: Fn(&mut Self, &str, &mut Vec<LoadDiagnostic>),
    {
        let base_dir = self.starmap_dir().join(subdir);
        let mut diag = Vec::new();
        if let Ok(bucket_entries) = std::fs::read_dir(&base_dir) {
            for bucket_entry in bucket_entries.flatten() {
                let bucket_path = bucket_entry.path();
                if bucket_path.is_dir() {
                    if let Ok(file_entries) = std::fs::read_dir(&bucket_path) {
                        for file_entry in file_entries.flatten() {
                            let path = file_entry.path();
                            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                                let id = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                                if !id.is_empty() {
                                    insert_fn(self, id, &mut diag);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn detect_dangling_references(&self, diagnostics: &mut Vec<LoadDiagnostic>) {
        let node_ids: HashSet<&str> = self.nodes.keys().map(|s| s.as_str()).collect();
        for edge in self.edges.values() {
            if let Some(ref from_id) = edge.from {
                if !node_ids.contains(from_id.as_str()) {
                    diagnostics.push(LoadDiagnostic {
                        kind: LoadDiagnosticKind::DanglingReference,
                        object_type: "edge".to_string(),
                        object_id: edge.id.clone(),
                        detail: format!("edge references non-existent from node: {}", from_id),
                    });
                }
            }
            if let Some(ref to_id) = edge.to {
                if !node_ids.contains(to_id.as_str()) {
                    diagnostics.push(LoadDiagnostic {
                        kind: LoadDiagnosticKind::DanglingReference,
                        object_type: "edge".to_string(),
                        object_id: edge.id.clone(),
                        detail: format!("edge references non-existent to node: {}", to_id),
                    });
                }
            }
            if let Some(ref ep) = edge.from_endpoint {
                let nid = match ep {
                    StarMapEdgeEndpoint::Node { node_id } => node_id.as_str(),
                    StarMapEdgeEndpoint::Anchor { node_id, .. } => node_id.as_str(),
                    _ => "",
                };
                if !nid.is_empty() && !node_ids.contains(nid) {
                    diagnostics.push(LoadDiagnostic {
                        kind: LoadDiagnosticKind::DanglingReference,
                        object_type: "edge".to_string(),
                        object_id: edge.id.clone(),
                        detail: format!("edge from_endpoint references non-existent node: {}", nid),
                    });
                }
            }
            if let Some(ref ep) = edge.to_endpoint {
                let nid = match ep {
                    StarMapEdgeEndpoint::Node { node_id } => node_id.as_str(),
                    StarMapEdgeEndpoint::Anchor { node_id, .. } => node_id.as_str(),
                    _ => "",
                };
                if !nid.is_empty() && !node_ids.contains(nid) {
                    diagnostics.push(LoadDiagnostic {
                        kind: LoadDiagnosticKind::DanglingReference,
                        object_type: "edge".to_string(),
                        object_id: edge.id.clone(),
                        detail: format!("edge to_endpoint references non-existent node: {}", nid),
                    });
                }
            }
        }
        for embed in self.embeds.values() {
            if let Some(ref source_id) = embed.source_node_id {
                if !node_ids.contains(source_id.as_str()) {
                    diagnostics.push(LoadDiagnostic {
                        kind: LoadDiagnosticKind::DanglingReference,
                        object_type: "embed".to_string(),
                        object_id: embed.instance_id.clone(),
                        detail: format!("embed references non-existent source_node_id: {}", source_id),
                    });
                }
            }
        }
    }

    fn detect_orphan_objects(&self, diagnostics: &mut Vec<LoadDiagnostic>) {
        let declared_node_ids: HashSet<&str> = self.graph_meta.as_ref()
            .map(|m| m.node_ids.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();
        let declared_edge_ids: HashSet<&str> = self.graph_meta.as_ref()
            .map(|m| m.edge_ids.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();
        let declared_embed_ids: HashSet<&str> = self.graph_meta.as_ref()
            .map(|m| m.embed_instance_ids.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();
        let declared_hl_ids: HashSet<&str> = self.graph_meta.as_ref()
            .map(|m| m.hyperlink_ids.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();
        let declared_link_ids: HashSet<&str> = self.graph_meta.as_ref()
            .map(|m| m.link_ids.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();

        self.check_orphan_dir("nodes", &declared_node_ids, "node", diagnostics);
        self.check_orphan_dir("edges", &declared_edge_ids, "edge", diagnostics);
        self.check_orphan_dir("child_starmaps", &declared_embed_ids, "embed", diagnostics);
        self.check_orphan_dir("hyperlinks", &declared_hl_ids, "hyperlink", diagnostics);
        self.check_orphan_dir("links", &declared_link_ids, "link", diagnostics);
    }

    fn check_orphan_dir(&self, subdir: &str, declared_ids: &HashSet<&str>, object_type: &str, diagnostics: &mut Vec<LoadDiagnostic>) {
        let base_dir = self.starmap_dir().join(subdir);
        if let Ok(bucket_entries) = std::fs::read_dir(&base_dir) {
            for bucket_entry in bucket_entries.flatten() {
                let bucket_path = bucket_entry.path();
                if bucket_path.is_dir() {
                    if let Ok(file_entries) = std::fs::read_dir(&bucket_path) {
                        for file_entry in file_entries.flatten() {
                            let path = file_entry.path();
                            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                                if let Some(id) = path.file_stem().and_then(|s| s.to_str()) {
                                    if !id.is_empty() && !declared_ids.contains(id) {
                                        diagnostics.push(LoadDiagnostic {
                                            kind: LoadDiagnosticKind::OrphanObject,
                                            object_type: object_type.to_string(),
                                            object_id: id.to_string(),
                                            detail: format!("file exists on disk but not listed in graph.json: {}", path.display()),
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn metadata_dir(&self) -> PathBuf {
        self.starmap_dir().join("metadata")
    }

    fn load_recovery_from_disk(&mut self) {
        let path = self.metadata_dir().join("recovery.json");
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(log) = serde_json::from_str::<Vec<LoadDiagnostic>>(&content) {
                    self.recovery_log = log;
                }
            }
        }
    }

    fn flush_recovery_to_disk(&self) -> Result<()> {
        let dir = self.metadata_dir();
        std::fs::create_dir_all(&dir)?;
        let json = serde_json::to_string_pretty(&self.recovery_log)?;
        let path = dir.join("recovery.json");
        atomic_write_string(&path, &json)?;
        Ok(())
    }

    fn record_migration(&self, kind: &str, detail: &str) {
        let dir = self.metadata_dir();
        if std::fs::create_dir_all(&dir).is_err() {
            return;
        }
        let path = dir.join("migration.json");
        let mut entries: Vec<MigrationEntry> = if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|c| serde_json::from_str(&c).ok())
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        entries.push(MigrationEntry {
            kind: kind.to_string(),
            detail: detail.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        });
        if let Ok(json) = serde_json::to_string_pretty(&entries) {
            let _ = atomic_write_string(&path, &json);
        }
    }

    fn update_graph_meta_file(&mut self) -> Result<u64> {
        if self.graph_meta.is_none() {
            self.reload_graph_meta_if_stale();
        }
        if self.graph_meta.is_none() {
            self.graph_meta = Some(GraphMeta {
                schema_version: "2".to_string(),
                starmap_id: self.starmap_id.clone(),
                title: String::new(),
                node_ids: Vec::new(),
                edge_ids: Vec::new(),
                embed_instance_ids: Vec::new(),
                link_ids: Vec::new(),
                hyperlink_ids: Vec::new(),
                edge_relation_index: Vec::new(),
                embed_host_index: Vec::new(),
                link_relation_index: Vec::new(),
                hyperlink_relation_index: Vec::new(),
                node_kind_counts: HashMap::new(),
                package_revision: self.package_revision,
                updated_at: crate::starmap::now_epoch(),
            });
        }

        self.merge_memory_ids_into_graph_meta();

        let meta = self.graph_meta.as_ref()
            .ok_or_else(|| crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "graph_meta not initialized",
            )))?;

        let next_revision = self.package_revision.saturating_add(1);

        let meta_to_write = GraphMeta {
            schema_version: meta.schema_version.clone(),
            starmap_id: meta.starmap_id.clone(),
            title: meta.title.clone(),
            node_ids: meta.node_ids.clone(),
            edge_ids: meta.edge_ids.clone(),
            embed_instance_ids: meta.embed_instance_ids.clone(),
            link_ids: meta.link_ids.clone(),
            hyperlink_ids: meta.hyperlink_ids.clone(),
            edge_relation_index: meta.edge_relation_index.clone(),
            embed_host_index: meta.embed_host_index.clone(),
            link_relation_index: meta.link_relation_index.clone(),
            hyperlink_relation_index: meta.hyperlink_relation_index.clone(),
            node_kind_counts: meta.node_kind_counts.clone(),
            package_revision: next_revision,
            updated_at: crate::starmap::now_epoch(),
        };

        let json = serde_json::to_string_pretty(&meta_to_write)?;
        let path = self.starmap_dir().join("graph.json");
        atomic_write_string(&path, &json)?;

        Ok(next_revision)
    }

    fn merge_memory_ids_into_graph_meta(&mut self) {
        let Some(ref mut meta) = self.graph_meta else { return };

        for node_id in self.nodes.keys() {
            if !meta.node_ids.contains(node_id) && !self.deleted_node_ids.contains(node_id) {
                meta.node_ids.push(node_id.clone());
            }
        }
        for edge in self.edges.values() {
            if self.deleted_edge_ids.contains(&edge.id) {
                continue;
            }
            if let Some(eri) = meta.edge_relation_index.iter_mut().find(|eri| eri.edge_id == edge.id) {
                eri.from = edge.from.clone().unwrap_or_default();
                eri.to = edge.to.clone().unwrap_or_default();
                eri.from_endpoint = edge.from_endpoint.clone();
                eri.to_endpoint = edge.to_endpoint.clone();
                eri.from_endpoint_path = edge.from_endpoint_path.clone();
                eri.to_endpoint_path = edge.to_endpoint_path.clone();
            } else {
                if !meta.edge_ids.contains(&edge.id) {
                    meta.edge_ids.push(edge.id.clone());
                }
                meta.edge_relation_index.push(EdgeRelationIndex {
                    edge_id: edge.id.clone(),
                    from: edge.from.clone().unwrap_or_default(),
                    to: edge.to.clone().unwrap_or_default(),
                    from_endpoint: edge.from_endpoint.clone(),
                    to_endpoint: edge.to_endpoint.clone(),
                    from_endpoint_path: edge.from_endpoint_path.clone(),
                    to_endpoint_path: edge.to_endpoint_path.clone(),
                });
            }
        }
        for embed in self.embeds.values() {
            if self.deleted_embed_ids.contains(&embed.instance_id) {
                continue;
            }
            if let Some(ehi) = meta.embed_host_index.iter_mut().find(|ehi| ehi.instance_id == embed.instance_id) {
                ehi.host_node_id = embed.source_node_id.clone().unwrap_or_default();
                ehi.host_endpoint = embed.host_endpoint.clone();
            } else {
                if !meta.embed_instance_ids.contains(&embed.instance_id) {
                    meta.embed_instance_ids.push(embed.instance_id.clone());
                }
                meta.embed_host_index.push(EmbedHostIndex {
                    instance_id: embed.instance_id.clone(),
                    host_node_id: embed.source_node_id.clone().unwrap_or_default(),
                    host_endpoint: embed.host_endpoint.clone(),
                });
            }
        }
        for link in self.links.values() {
            if self.deleted_link_ids.contains(&link.link_id) {
                continue;
            }
            if !meta.link_ids.contains(&link.link_id) {
                meta.link_ids.push(link.link_id.clone());
            }
            let source_node_id = endpoint_node_id(&link.source).unwrap_or_default().to_string();
            if let Some(lri) = meta.link_relation_index.iter_mut().find(|lri| lri.link_id == link.link_id) {
                lri.source_node_id = source_node_id;
            } else {
                meta.link_relation_index.push(LinkRelationIndex {
                    link_id: link.link_id.clone(),
                    source_node_id,
                });
            }
        }
        for hl in self.hyperlinks.values() {
            if self.deleted_hyperlink_ids.contains(&hl.hyperlink_id) {
                continue;
            }
            if !meta.hyperlink_ids.contains(&hl.hyperlink_id) {
                meta.hyperlink_ids.push(hl.hyperlink_id.clone());
            }
            let source_node_id = endpoint_path_node_id(&hl.source).unwrap_or_default().to_string();
            if let Some(hri) = meta.hyperlink_relation_index.iter_mut().find(|hri| hri.hyperlink_id == hl.hyperlink_id) {
                hri.source_node_id = source_node_id;
            } else {
                meta.hyperlink_relation_index.push(HyperlinkRelationIndex {
                    hyperlink_id: hl.hyperlink_id.clone(),
                    source_node_id,
                });
            }
        }

        meta.node_ids.retain(|id| !self.deleted_node_ids.contains(id));
        meta.edge_ids.retain(|id| !self.deleted_edge_ids.contains(id));
        meta.edge_relation_index.retain(|eri| !self.deleted_edge_ids.contains(&eri.edge_id));
        meta.embed_instance_ids.retain(|id| !self.deleted_embed_ids.contains(id));
        meta.embed_host_index.retain(|ehi| !self.deleted_embed_ids.contains(&ehi.instance_id));
        meta.link_ids.retain(|id| !self.deleted_link_ids.contains(id));
        meta.link_relation_index.retain(|lri| !self.deleted_link_ids.contains(&lri.link_id));
        meta.hyperlink_ids.retain(|id| !self.deleted_hyperlink_ids.contains(id));
        meta.hyperlink_relation_index.retain(|hri| !self.deleted_hyperlink_ids.contains(&hri.hyperlink_id));
    }

    pub fn to_starmap_graph(&self) -> StarMapGraph {
        StarMapGraph {
            schema_version: 1,
            id: self.starmap_id.clone(),
            starmap_id: self.starmap_id.clone(),
            title: self.graph_meta.as_ref().map(|m| m.title.clone()).unwrap_or_default(),
            nodes: self.nodes.values().cloned().collect(),
            edges: self.edges.values().cloned().collect(),
            embeds: self.embeds.values().cloned().collect(),
            links: self.links.values().cloned().collect(),
            created_at: 0,
            updated_at: crate::starmap::now_epoch(),
        }
    }

    pub fn get_phased_snapshot(&mut self) -> Result<StarMapPhasedSnapshot> {
        self.load_phased(LoadPhase::PrefetchNearbyObjects)?;
        Ok(StarMapPhasedSnapshot {
            starmap_id: self.starmap_id.clone(),
            title: self.graph_meta.as_ref().map(|meta| meta.title.clone()).unwrap_or_default(),
            nodes: self.nodes.values().cloned().collect(),
            edges: self.edges.values().cloned().collect(),
            embeds: self.embeds.values().cloned().collect(),
            links: self.links.values().cloned().collect(),
            hyperlinks: self.hyperlinks.values().cloned().collect(),
            layout: self.layout.clone(),
            viewport: self.viewport.clone(),
            diagnostics: self.recovery_log.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn store_new_has_zero_counts() {
        let dir = TempDir::new().unwrap();
        let store = StarMapStore::new(dir.path(), "test-id");
        assert_eq!(store.node_count(), 0);
        assert_eq!(store.edge_count(), 0);
        assert_eq!(store.embed_count(), 0);
        assert_eq!(store.hyperlink_count(), 0);
        assert_eq!(store.package_revision(), 0);
        assert!(!store.is_dirty());
    }

    fn write_to_bucket(dir: &std::path::Path, subdir: &str, id: &str, json: &str) {
        let bucket = package_storage::bucket_for_id(id);
        let path = dir.join(subdir).join(bucket).join(format!("{}.json", id));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, json).unwrap();
    }

    fn make_test_node(id: &str, title: &str) -> StarMapNode {
        use crate::starmap::semantic::{StarMapNodeContent, StarMapDisplayPolicy, StarMapOpenBehavior, StarMapProvenance};
        StarMapNode {
            id: id.to_string(),
            title: title.to_string(),
            kind: StarMapNodeKind::Concept,
            payload: None,
            tags: vec![],
            content: StarMapNodeContent::Empty,
            anchors: vec![],
            portal: None,
            display_policy: StarMapDisplayPolicy::default(),
            open_behavior: StarMapOpenBehavior::default(),
            provenance: StarMapProvenance::default(),
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn upsert_node_marks_dirty() {
        let dir = TempDir::new().unwrap();
        let mut store = StarMapStore::new(dir.path(), "test-id");
        store.upsert_node(make_test_node("n1", "Test Node"));
        assert!(store.is_dirty());
        assert_eq!(store.node_count(), 1);
        assert!(store.get_node("n1").is_some());
    }

    #[test]
    fn remove_node_marks_deleted() {
        let dir = TempDir::new().unwrap();
        let mut store = StarMapStore::new(dir.path(), "test-id");
        store.upsert_node(make_test_node("n1", "Test Node"));
        store.remove_node("n1");
        assert_eq!(store.node_count(), 0);
        assert!(store.get_node("n1").is_none());
    }

    #[test]
    fn flush_increments_package_revision() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Test Node"));
        store.flush().unwrap();
        assert_eq!(store.package_revision(), 1);
        assert!(!store.is_dirty());
    }

    #[test]
    fn load_full_returns_diagnostics_for_missing_files() {
        let dir = TempDir::new().unwrap();
        let starmap_dir = dir.path().join("app-meta").join("starmaps").join("test-id");
        std::fs::create_dir_all(starmap_dir.join("nodes")).unwrap();
        std::fs::create_dir_all(starmap_dir.join("edges")).unwrap();
        std::fs::create_dir_all(starmap_dir.join("child_starmaps")).unwrap();
        std::fs::create_dir_all(starmap_dir.join("hyperlinks")).unwrap();
        std::fs::create_dir_all(starmap_dir.join("links")).unwrap();

        let meta = GraphMeta {
            schema_version: "2".to_string(),
            starmap_id: "test-id".to_string(),
            title: "Test".to_string(),
            node_ids: vec!["missing-node".to_string()],
            edge_ids: vec![],
            embed_instance_ids: vec![],
            link_ids: vec![],
            hyperlink_ids: vec![],
            edge_relation_index: vec![],
            embed_host_index: vec![],
            link_relation_index: vec![],
            hyperlink_relation_index: vec![],
            node_kind_counts: HashMap::new(),
            package_revision: 1,
            updated_at: 0,
        };
        let json = serde_json::to_string_pretty(&meta).unwrap();
        std::fs::write(starmap_dir.join("graph.json"), json).unwrap();

        let mut store = StarMapStore::new(dir.path(), "test-id");
        let result = store.load_full().unwrap();
        assert_eq!(result.loaded_node_count, 0);
        assert!(!result.diagnostics.is_empty());
        assert_eq!(result.diagnostics[0].kind, LoadDiagnosticKind::Missing);
    }

    fn make_test_link(link_id: &str, label: &str) -> StarMapLink {
        use crate::starmap::semantic::{StarMapDeepTarget, StarMapTargetDetail};
        StarMapLink {
            link_id: link_id.to_string(),
            source: StarMapEndpoint::Starmap,
            target: StarMapDeepTarget {
                starmap_id: "other".to_string(),
                path: vec![],
                target: StarMapTargetDetail::Starmap,
            },
            label: Some(label.to_string()),
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn link_save_reload_update_delete_round_trip() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        let link = make_test_link("l1", "Test Link");
        store.upsert_link(link.clone());
        store.flush().unwrap();
        assert_eq!(store.link_count(), 1);
        assert!(store.get_link("l1").is_some());

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        let result = store2.load_full().unwrap();
        assert_eq!(result.loaded_link_count, 1);
        assert!(store2.get_link("l1").is_some());
        assert_eq!(store2.get_link("l1").unwrap().label.as_deref(), Some("Test Link"));

        let patch = StarMapLinkPatch {
            source: None,
            target: None,
            label: Some(Some("Updated Link".to_string())),
        };
        store2.update_link("l1", &patch).unwrap();
        store2.flush().unwrap();

        let mut store3 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store3.load_full().unwrap();
        assert_eq!(store3.link_count(), 1);
        assert_eq!(store3.get_link("l1").unwrap().label.as_deref(), Some("Updated Link"));

        store3.delete_link("l1").unwrap();
        store3.flush().unwrap();

        let mut store4 = StarMapStore::new(dir.path(), &meta.starmap_id);
        let result4 = store4.load_full().unwrap();
        assert_eq!(result4.loaded_link_count, 0);
        assert!(store4.get_link("l1").is_none());
    }

    #[test]
    fn load_full_returns_diagnostics_for_missing_link() {
        let dir = TempDir::new().unwrap();
        let starmap_dir = dir.path().join("app-meta").join("starmaps").join("test-id");
        std::fs::create_dir_all(starmap_dir.join("nodes")).unwrap();
        std::fs::create_dir_all(starmap_dir.join("edges")).unwrap();
        std::fs::create_dir_all(starmap_dir.join("child_starmaps")).unwrap();
        std::fs::create_dir_all(starmap_dir.join("hyperlinks")).unwrap();
        std::fs::create_dir_all(starmap_dir.join("links")).unwrap();

        let meta = GraphMeta {
            schema_version: "2".to_string(),
            starmap_id: "test-id".to_string(),
            title: "Test".to_string(),
            node_ids: vec![],
            edge_ids: vec![],
            embed_instance_ids: vec![],
            link_ids: vec!["missing-link".to_string()],
            hyperlink_ids: vec![],
            edge_relation_index: vec![],
            embed_host_index: vec![],
            link_relation_index: vec![],
            hyperlink_relation_index: vec![],
            node_kind_counts: HashMap::new(),
            package_revision: 1,
            updated_at: 0,
        };
        let json = serde_json::to_string_pretty(&meta).unwrap();
        std::fs::write(starmap_dir.join("graph.json"), json).unwrap();

        let mut store = StarMapStore::new(dir.path(), "test-id");
        let result = store.load_full().unwrap();
        assert_eq!(result.loaded_link_count, 0);
        let link_diag: Vec<_> = result.diagnostics.iter()
            .filter(|d| d.object_type == "link")
            .collect();
        assert!(!link_diag.is_empty());
        assert_eq!(link_diag[0].kind, LoadDiagnosticKind::Missing);
    }

    #[test]
    fn load_full_detects_dangling_edge_reference() {
        let dir = TempDir::new().unwrap();
        let starmap_dir = dir.path().join("app-meta").join("starmaps").join("test-id");
        std::fs::create_dir_all(starmap_dir.join("nodes")).unwrap();
        std::fs::create_dir_all(starmap_dir.join("edges")).unwrap();
        std::fs::create_dir_all(starmap_dir.join("child_starmaps")).unwrap();
        std::fs::create_dir_all(starmap_dir.join("hyperlinks")).unwrap();
        std::fs::create_dir_all(starmap_dir.join("links")).unwrap();

        let node = make_test_node("n1", "Node1");
        let node_json = serde_json::to_string_pretty(&node).unwrap();
        write_to_bucket(&starmap_dir, "nodes", "n1", &node_json);

        let edge = StarMapEdge {
            id: "e1".to_string(),
            kind: StarMapEdgeKind::References,
            label: None,
            payload: None,
            from: Some("n1".to_string()),
            to: Some("nonexistent".to_string()),
            from_target: None,
            to_target: None,
            from_endpoint: None,
            to_endpoint: None,
            from_endpoint_path: None,
            to_endpoint_path: None,
            created_at: 0,
            updated_at: 0,
        };
        let edge_json = serde_json::to_string_pretty(&edge).unwrap();
        write_to_bucket(&starmap_dir, "edges", "e1", &edge_json);

        let meta = GraphMeta {
            schema_version: "2".to_string(),
            starmap_id: "test-id".to_string(),
            title: "Test".to_string(),
            node_ids: vec!["n1".to_string()],
            edge_ids: vec!["e1".to_string()],
            embed_instance_ids: vec![],
            link_ids: vec![],
            hyperlink_ids: vec![],
            edge_relation_index: vec![],
            embed_host_index: vec![],
            link_relation_index: vec![],
            hyperlink_relation_index: vec![],
            node_kind_counts: HashMap::new(),
            package_revision: 1,
            updated_at: 0,
        };
        let json = serde_json::to_string_pretty(&meta).unwrap();
        std::fs::write(starmap_dir.join("graph.json"), json).unwrap();

        let mut store = StarMapStore::new(dir.path(), "test-id");
        let result = store.load_full().unwrap();
        let dangling: Vec<_> = result.diagnostics.iter()
            .filter(|d| d.kind == LoadDiagnosticKind::DanglingReference)
            .collect();
        assert!(!dangling.is_empty());
        assert!(dangling[0].detail.contains("nonexistent"));
    }

    #[test]
    fn load_full_detects_orphan_object_on_disk() {
        let dir = TempDir::new().unwrap();
        let starmap_dir = dir.path().join("app-meta").join("starmaps").join("test-id");
        std::fs::create_dir_all(starmap_dir.join("nodes")).unwrap();
        std::fs::create_dir_all(starmap_dir.join("edges")).unwrap();
        std::fs::create_dir_all(starmap_dir.join("child_starmaps")).unwrap();
        std::fs::create_dir_all(starmap_dir.join("hyperlinks")).unwrap();
        std::fs::create_dir_all(starmap_dir.join("links")).unwrap();

        let orphan_node = make_test_node("orphan-node", "Orphan");
        let orphan_json = serde_json::to_string_pretty(&orphan_node).unwrap();
        write_to_bucket(&starmap_dir, "nodes", "orphan-node", &orphan_json);

        let meta = GraphMeta {
            schema_version: "2".to_string(),
            starmap_id: "test-id".to_string(),
            title: "Test".to_string(),
            node_ids: vec![],
            edge_ids: vec![],
            embed_instance_ids: vec![],
            link_ids: vec![],
            hyperlink_ids: vec![],
            edge_relation_index: vec![],
            embed_host_index: vec![],
            link_relation_index: vec![],
            hyperlink_relation_index: vec![],
            node_kind_counts: HashMap::new(),
            package_revision: 1,
            updated_at: 0,
        };
        let json = serde_json::to_string_pretty(&meta).unwrap();
        std::fs::write(starmap_dir.join("graph.json"), json).unwrap();

        let mut store = StarMapStore::new(dir.path(), "test-id");
        let result = store.load_full().unwrap();
        let orphan: Vec<_> = result.diagnostics.iter()
            .filter(|d| d.kind == LoadDiagnosticKind::OrphanObject)
            .collect();
        assert!(!orphan.is_empty());
        assert_eq!(orphan[0].object_id, "orphan-node");
    }

    #[test]
    fn load_full_detects_unsupported_version() {
        let dir = TempDir::new().unwrap();
        let starmap_dir = dir.path().join("app-meta").join("starmaps").join("test-id");
        std::fs::create_dir_all(starmap_dir.join("nodes")).unwrap();
        std::fs::create_dir_all(starmap_dir.join("edges")).unwrap();
        std::fs::create_dir_all(starmap_dir.join("child_starmaps")).unwrap();
        std::fs::create_dir_all(starmap_dir.join("hyperlinks")).unwrap();
        std::fs::create_dir_all(starmap_dir.join("links")).unwrap();

        let meta = GraphMeta {
            schema_version: "99".to_string(),
            starmap_id: "test-id".to_string(),
            title: "Test".to_string(),
            node_ids: vec![],
            edge_ids: vec![],
            embed_instance_ids: vec![],
            link_ids: vec![],
            hyperlink_ids: vec![],
            edge_relation_index: vec![],
            embed_host_index: vec![],
            link_relation_index: vec![],
            hyperlink_relation_index: vec![],
            node_kind_counts: HashMap::new(),
            package_revision: 1,
            updated_at: 0,
        };
        let json = serde_json::to_string_pretty(&meta).unwrap();
        std::fs::write(starmap_dir.join("graph.json"), json).unwrap();

        let mut store = StarMapStore::new(dir.path(), "test-id");
        let result = store.load_full().unwrap();
        let unsupported: Vec<_> = result.diagnostics.iter()
            .filter(|d| d.kind == LoadDiagnosticKind::UnsupportedVersion)
            .collect();
        assert!(!unsupported.is_empty());
        assert!(unsupported[0].detail.contains("99"));
    }

    #[test]
    fn flush_persists_recovery_log_to_disk() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Test"));
        store.recovery_log.push(LoadDiagnostic {
            kind: LoadDiagnosticKind::Corrupt,
            object_type: "node".to_string(),
            object_id: "bad-node".to_string(),
            detail: "test corrupt".to_string(),
        });
        store.flush().unwrap();

        let recovery_path = dir.path()
            .join("app-meta").join("starmaps").join(&meta.starmap_id)
            .join("metadata").join("recovery.json");
        assert!(recovery_path.exists());

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.load_full().unwrap();
        assert!(!store2.diagnostics().is_empty());
        let corrupt_diag: Vec<_> = store2.diagnostics().iter()
            .filter(|d| d.kind == LoadDiagnosticKind::Corrupt && d.object_id == "bad-node")
            .collect();
        assert!(!corrupt_diag.is_empty());
    }

    #[test]
    fn save_queue_deduplicates_entries() {
        let dir = TempDir::new().unwrap();
        let mut store = StarMapStore::new(dir.path(), "test-id");
        store.enqueue_save(SaveQueueEntry::Node);
        store.enqueue_save(SaveQueueEntry::Node);
        store.enqueue_save(SaveQueueEntry::Edge);
        assert_eq!(store.save_queue_len(), 2);
    }

    #[test]
    fn drain_save_queue_clears() {
        let dir = TempDir::new().unwrap();
        let mut store = StarMapStore::new(dir.path(), "test-id");
        store.enqueue_save(SaveQueueEntry::Node);
        store.enqueue_save(SaveQueueEntry::Edge);
        let entries = store.drain_save_queue();
        assert_eq!(entries.len(), 2);
        assert_eq!(store.save_queue_len(), 0);
    }

    #[test]
    fn load_phased_graph_meta_only() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        store.flush().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        let result = store2.load_phased(LoadPhase::GraphMeta).unwrap();
        assert_eq!(store2.current_load_phase(), Some(LoadPhase::GraphMeta));
        assert_eq!(result.loaded_node_count, 0);
        assert!(store2.get_node("n1").is_none());
    }

    #[test]
    fn load_phased_to_current_viewport_objects() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        let mut layout = StarMapLayout::default();
        layout.nodes.push(StarMapLayoutNode {
            node_id: "n1".to_string(),
            x: 0.0, y: 0.0, width: 100.0, height: 50.0,
            radius: 25.0, collapsed: false, z_index: 0,
            scale: 1.0, depth: 0.0, focus_weight: 0.0, orbit_group: None,
        });
        store.set_layout(layout);
        store.set_viewport(StarMapViewport {
            scale: 1.0, offset_x: 0.0, offset_y: 0.0, width: 200.0, height: 200.0,
        });
        store.flush().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        let result = store2.load_phased(LoadPhase::CurrentViewportObjects).unwrap();
        assert_eq!(store2.current_load_phase(), Some(LoadPhase::CurrentViewportObjects));
        assert_eq!(result.loaded_node_count, 1);
        assert!(store2.get_node("n1").is_some());
    }

    #[test]
    fn load_phased_viewport_objects_with_layout() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        store.upsert_node(make_test_node("n2", "Node2"));
        let mut layout = StarMapLayout::default();
        layout.nodes.push(StarMapLayoutNode {
            node_id: "n1".to_string(),
            x: 0.0, y: 0.0, width: 100.0, height: 50.0,
            radius: 25.0, collapsed: false, z_index: 0,
            scale: 1.0, depth: 0.0, focus_weight: 0.0, orbit_group: None,
        });
        store.set_layout(layout);
        store.flush().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        let result = store2.load_phased(LoadPhase::CurrentViewportObjects).unwrap();
        assert_eq!(store2.current_load_phase(), Some(LoadPhase::CurrentViewportObjects));
        assert_eq!(result.loaded_node_count, 1);
        assert!(store2.get_node("n1").is_some());
        assert!(store2.get_node("n2").is_none());
    }

    #[test]
    fn load_phased_full_equivalent_to_load_full() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        store.upsert_node(make_test_node("n2", "Node2"));
        store.flush().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        let result = store2.load_phased(LoadPhase::BackgroundFullLoad).unwrap();
        assert_eq!(store2.current_load_phase(), Some(LoadPhase::BackgroundFullLoad));
        assert_eq!(result.loaded_node_count, 2);
    }

    #[test]
    fn load_phase_sequence() {
        assert_eq!(LoadPhase::GraphMeta.next(), Some(LoadPhase::ViewportAndLayoutIndex));
        assert_eq!(LoadPhase::ViewportAndLayoutIndex.next(), Some(LoadPhase::CurrentViewportObjects));
        assert_eq!(LoadPhase::CurrentViewportObjects.next(), Some(LoadPhase::PrefetchNearbyObjects));
        assert_eq!(LoadPhase::PrefetchNearbyObjects.next(), Some(LoadPhase::BackgroundFullLoad));
        assert_eq!(LoadPhase::BackgroundFullLoad.next(), None);
    }

    #[test]
    fn flush_save_queue_handles_delete_entries() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        store.flush().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.load_full().unwrap();
        store2.remove_node("n1");
        assert!(store2.has_pending_deletes());

        store2.enqueue_save(SaveQueueEntry::DeleteNode);
        store2.enqueue_save(SaveQueueEntry::GraphMeta);
        store2.flush_save_queue().unwrap();
        assert!(!store2.has_pending_deletes());

        let mut store3 = StarMapStore::new(dir.path(), &meta.starmap_id);
        let result = store3.load_full().unwrap();
        assert_eq!(result.loaded_node_count, 0);
    }

    #[test]
    fn flush_delete_failure_retains_deleted_ids() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        store.flush().unwrap();

        let node_path = dir.path()
            .join("app-meta").join("starmaps").join(&meta.starmap_id)
            .join("nodes").join(package_storage::bucket_for_id("n1")).join("n1.json");
        assert!(node_path.exists());

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.load_full().unwrap();
        store2.remove_node("n1");

        let result = store2.flush();
        assert!(result.is_ok());
        assert!(!store2.has_pending_deletes());
    }

    #[test]
    fn ensure_loaded_skips_repeated_load() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        let mut layout = StarMapLayout::default();
        layout.nodes.push(StarMapLayoutNode {
            node_id: "n1".to_string(),
            x: 0.0, y: 0.0, width: 100.0, height: 50.0,
            radius: 25.0, collapsed: false, z_index: 0,
            scale: 1.0, depth: 0.0, focus_weight: 0.0, orbit_group: None,
        });
        store.set_layout(layout);
        store.set_viewport(StarMapViewport {
            scale: 1.0, offset_x: 0.0, offset_y: 0.0, width: 200.0, height: 200.0,
        });
        store.flush().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.ensure_loaded().unwrap();
        assert_eq!(store2.node_count(), 1);

        store2.ensure_loaded().unwrap();
        assert_eq!(store2.node_count(), 1);
    }

    #[test]
    fn load_phased_viewport_only_loads_layout_nodes() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "InViewport"));
        store.upsert_node(make_test_node("n2", "OutOfViewport"));
        let mut layout = StarMapLayout::default();
        layout.nodes.push(StarMapLayoutNode {
            node_id: "n1".to_string(),
            x: 0.0, y: 0.0, width: 100.0, height: 50.0,
            radius: 25.0, collapsed: false, z_index: 0,
            scale: 1.0, depth: 0.0, focus_weight: 0.0, orbit_group: None,
        });
        store.set_layout(layout);
        store.flush().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        let result = store2.load_phased(LoadPhase::CurrentViewportObjects).unwrap();
        assert_eq!(store2.current_load_phase(), Some(LoadPhase::CurrentViewportObjects));
        assert!(store2.get_node("n1").is_some());
        assert!(store2.get_node("n2").is_none());
        assert_eq!(result.loaded_node_count, 1);
    }

    #[test]
    fn prefetch_nearby_loads_adjacent_nodes() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Visible"));
        store.upsert_node(make_test_node("n2", "Adjacent"));
        let edge = StarMapEdge {
            id: "e1".to_string(),
            kind: StarMapEdgeKind::References,
            label: None,
            payload: None,
            from: Some("n1".to_string()),
            to: Some("n2".to_string()),
            from_target: None,
            to_target: None,
            from_endpoint: None,
            to_endpoint: None,
            from_endpoint_path: None,
            to_endpoint_path: None,
            created_at: 0,
            updated_at: 0,
        };
        store.upsert_edge(edge);
        let mut layout = StarMapLayout::default();
        layout.nodes.push(StarMapLayoutNode {
            node_id: "n1".to_string(),
            x: 0.0, y: 0.0, width: 100.0, height: 50.0,
            radius: 25.0, collapsed: false, z_index: 0,
            scale: 1.0, depth: 0.0, focus_weight: 0.0, orbit_group: None,
        });
        store.set_layout(layout);
        store.flush().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.load_phased(LoadPhase::CurrentViewportObjects).unwrap();
        assert!(store2.get_node("n1").is_some());

        store2.load_phased(LoadPhase::PrefetchNearbyObjects).unwrap();
        assert!(store2.get_node("n2").is_some());
        assert!(store2.get_edge("e1").is_some());
    }

    #[test]
    fn save_queue_delete_variants_exist() {
        let dir = TempDir::new().unwrap();
        let mut store = StarMapStore::new(dir.path(), "test-id");
        store.enqueue_save(SaveQueueEntry::DeleteNode);
        store.enqueue_save(SaveQueueEntry::DeleteEdge);
        store.enqueue_save(SaveQueueEntry::DeleteEmbed);
        store.enqueue_save(SaveQueueEntry::DeleteLink);
        store.enqueue_save(SaveQueueEntry::DeleteHyperlink);
        assert_eq!(store.save_queue_len(), 5);
    }

    #[test]
    fn flush_save_queue_increments_package_revision() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Test Node"));
        store.enqueue_save(SaveQueueEntry::Node);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        assert_eq!(store.package_revision(), 0);
        store.flush_save_queue().unwrap();
        assert_eq!(store.package_revision(), 1);
    }

    #[test]
    fn flush_delete_failure_returns_error_and_retains_id() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        store.flush().unwrap();

        let node_path = dir.path()
            .join("app-meta").join("starmaps").join(&meta.starmap_id)
            .join("nodes").join(package_storage::bucket_for_id("n1")).join("n1.json");
        assert!(node_path.exists());

        std::fs::remove_file(&node_path).unwrap();
        std::fs::create_dir_all(&node_path).unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.load_full().unwrap();
        store2.remove_node("n1");
        assert!(store2.has_pending_deletes());

        let result = store2.flush();
        assert!(result.is_err());
        assert!(store2.has_pending_deletes());
    }

    #[test]
    fn flush_delete_succeeds_clears_deleted_ids() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        store.flush().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.load_full().unwrap();
        store2.remove_node("n1");
        assert!(store2.has_pending_deletes());

        store2.flush().unwrap();
        assert!(!store2.has_pending_deletes());
    }

    #[test]
    fn load_full_preserves_pending_deletes() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        store.upsert_node(make_test_node("n2", "Node2"));
        store.flush().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.load_full().unwrap();
        store2.remove_node("n1");
        assert!(store2.has_pending_deletes());

        store2.load_full().unwrap();
        assert!(store2.has_pending_deletes());
    }

    #[test]
    fn load_phased_preserves_pending_deletes() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        store.flush().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.load_phased(LoadPhase::CurrentViewportObjects).unwrap();
        store2.remove_node("n1");
        assert!(store2.has_pending_deletes());

        store2.load_phased(LoadPhase::BackgroundFullLoad).unwrap();
        assert!(store2.has_pending_deletes());
    }

    #[test]
    fn ensure_loaded_uses_phased_loading() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        let mut layout = StarMapLayout::default();
        layout.nodes.push(StarMapLayoutNode {
            node_id: "n1".to_string(),
            x: 0.0, y: 0.0, width: 100.0, height: 50.0,
            radius: 25.0, collapsed: false, z_index: 0,
            scale: 1.0, depth: 0.0, focus_weight: 0.0, orbit_group: None,
        });
        store.set_layout(layout);
        store.set_viewport(StarMapViewport {
            scale: 1.0, offset_x: 0.0, offset_y: 0.0, width: 200.0, height: 200.0,
        });
        store.flush().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.ensure_loaded().unwrap();
        assert_eq!(store2.current_load_phase(), Some(LoadPhase::PrefetchNearbyObjects));
        assert!(store2.get_node("n1").is_some());
    }

    #[test]
    fn ensure_fully_loaded_reaches_background_phase() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        store.flush().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.ensure_fully_loaded().unwrap();
        assert_eq!(store2.current_load_phase(), Some(LoadPhase::BackgroundFullLoad));
        assert!(store2.get_node("n1").is_some());
    }

    #[test]
    fn save_starmap_graph_corrupt_existing_returns_error() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        store.flush().unwrap();

        let graph_json = dir.path()
            .join("app-meta").join("starmaps").join(&meta.starmap_id)
            .join("graph.json");
        std::fs::write(&graph_json, "not valid json at all {{{").unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        let result = store2.load_full();
        assert!(result.is_ok());
        assert!(!store2.diagnostics().is_empty());
    }

    #[test]
    fn save_starmap_graph_new_store_no_graph_json_succeeds() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let graph_json = dir.path()
            .join("app-meta").join("starmaps").join(&meta.starmap_id)
            .join("graph.json");
        assert!(!graph_json.exists());

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        assert!(store.load_full().is_ok());
    }

    #[test]
    fn viewport_culling_excludes_offscreen_nodes() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Visible"));
        store.upsert_node(make_test_node("n2", "Offscreen"));
        let mut layout = StarMapLayout::default();
        layout.nodes.push(StarMapLayoutNode {
            node_id: "n1".to_string(),
            x: 10.0, y: 10.0, width: 80.0, height: 40.0,
            radius: 20.0, collapsed: false, z_index: 0,
            scale: 1.0, depth: 0.0, focus_weight: 0.0, orbit_group: None,
        });
        layout.nodes.push(StarMapLayoutNode {
            node_id: "n2".to_string(),
            x: 5000.0, y: 5000.0, width: 80.0, height: 40.0,
            radius: 20.0, collapsed: false, z_index: 0,
            scale: 1.0, depth: 0.0, focus_weight: 0.0, orbit_group: None,
        });
        store.set_layout(layout);
        store.set_viewport(StarMapViewport {
            scale: 1.0, offset_x: 0.0, offset_y: 0.0, width: 200.0, height: 200.0,
        });
        store.flush().unwrap();
        store.flush_viewport().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        let result = store2.load_phased(LoadPhase::CurrentViewportObjects).unwrap();
        assert!(store2.get_node("n1").is_some());
        assert!(store2.get_node("n2").is_none());
        assert_eq!(result.loaded_node_count, 1);
    }

    #[test]
    fn bucket_directory_structure_on_save() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        store.flush().unwrap();

        let bucket = package_storage::bucket_for_id("n1");
        let node_path = dir.path()
            .join("app-meta").join("starmaps").join(&meta.starmap_id)
            .join("nodes").join(bucket).join("n1.json");
        assert!(node_path.exists());
    }

    #[test]
    fn viewport_saved_to_session_path() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.set_viewport(StarMapViewport {
            scale: 2.0, offset_x: 100.0, offset_y: 50.0, width: 800.0, height: 600.0,
        });
        store.flush_viewport().unwrap();

        let session_path = dir.path()
            .join("session").join("starmaps").join(&meta.starmap_id)
            .join("viewport.json");
        assert!(session_path.exists());

        let pkg_viewport = dir.path()
            .join("app-meta").join("starmaps").join(&meta.starmap_id)
            .join("viewport.json");
        assert!(!pkg_viewport.exists());
    }

    #[test]
    fn flush_save_queue_returns_error_on_write_failure() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        store.enqueue_save(SaveQueueEntry::Node);
        store.enqueue_save(SaveQueueEntry::GraphMeta);

        let nodes_bucket_dir = dir.path()
            .join("app-meta").join("starmaps").join(&meta.starmap_id)
            .join("nodes").join(package_storage::bucket_for_id("n1"));
        std::fs::create_dir_all(&nodes_bucket_dir).unwrap();
        let node_file = nodes_bucket_dir.join("n1.json");
        std::fs::write(&node_file, "existing").unwrap();

        let mut perms = std::fs::metadata(&nodes_bucket_dir).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&nodes_bucket_dir, perms).unwrap();

        let result = store.flush_save_queue();

        let mut perms2 = std::fs::metadata(&nodes_bucket_dir).unwrap().permissions();
        perms2.set_readonly(false);
        std::fs::set_permissions(&nodes_bucket_dir, perms2).unwrap();

        if result.is_err() {
            if let Err(e) = result {
                assert_eq!(e.code(), "SAVE_QUEUE_FLUSH_INCOMPLETE");
            }
        }
    }

    #[test]
    fn prefetch_nearby_does_not_load_all_objects() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Visible"));
        store.upsert_node(make_test_node("n2", "Disconnected"));
        let mut layout = StarMapLayout::default();
        layout.nodes.push(StarMapLayoutNode {
            node_id: "n1".to_string(),
            x: 0.0, y: 0.0, width: 100.0, height: 50.0,
            radius: 25.0, collapsed: false, z_index: 0,
            scale: 1.0, depth: 0.0, focus_weight: 0.0, orbit_group: None,
        });
        store.set_layout(layout);
        store.set_viewport(StarMapViewport {
            scale: 1.0, offset_x: 0.0, offset_y: 0.0, width: 200.0, height: 200.0,
        });
        store.flush().unwrap();
        store.flush_viewport().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.load_phased(LoadPhase::PrefetchNearbyObjects).unwrap();
        assert!(store2.get_node("n1").is_some());
        assert!(store2.get_node("n2").is_none());
    }

    #[test]
    fn deferred_save_merges_consecutive_operations() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "First"));
        store.enqueue_save(SaveQueueEntry::Node);
        store.enqueue_save(SaveQueueEntry::GraphMeta);

        store.upsert_node(make_test_node("n2", "Second"));
        store.enqueue_save(SaveQueueEntry::Node);
        store.enqueue_save(SaveQueueEntry::GraphMeta);

        store.upsert_node(make_test_node("n3", "Third"));
        store.enqueue_save(SaveQueueEntry::Node);
        store.enqueue_save(SaveQueueEntry::GraphMeta);

        assert_eq!(store.save_queue_len(), 2);
        assert!(store.is_dirty());

        let node_file_1 = dir.path()
            .join("app-meta").join("starmaps").join(&meta.starmap_id)
            .join("nodes").join(package_storage::bucket_for_id("n1"))
            .join("n1.json");
        let node_file_3 = dir.path()
            .join("app-meta").join("starmaps").join(&meta.starmap_id)
            .join("nodes").join(package_storage::bucket_for_id("n3"))
            .join("n3.json");
        assert!(!node_file_1.exists());
        assert!(!node_file_3.exists());

        store.flush_save_queue().unwrap();

        assert!(node_file_1.exists());
        assert!(node_file_3.exists());
        assert!(!store.is_dirty());
        assert_eq!(store.save_queue_len(), 0);
    }

    #[test]
    fn deferred_save_with_delete_merges_into_single_flush() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        store.upsert_node(make_test_node("n2", "Node2"));
        store.flush().unwrap();

        store.remove_node("n1");
        store.enqueue_save(SaveQueueEntry::DeleteNode);
        store.enqueue_save(SaveQueueEntry::GraphMeta);

        store.upsert_node(make_test_node("n3", "Node3"));
        store.enqueue_save(SaveQueueEntry::Node);
        store.enqueue_save(SaveQueueEntry::GraphMeta);

        assert_eq!(store.save_queue_len(), 3);
        assert!(store.has_pending_deletes());

        store.flush_save_queue().unwrap();

        let node_file_1 = dir.path()
            .join("app-meta").join("starmaps").join(&meta.starmap_id)
            .join("nodes").join(package_storage::bucket_for_id("n1"))
            .join("n1.json");
        let node_file_3 = dir.path()
            .join("app-meta").join("starmaps").join(&meta.starmap_id)
            .join("nodes").join(package_storage::bucket_for_id("n3"))
            .join("n3.json");
        assert!(!node_file_1.exists());
        assert!(node_file_3.exists());
        assert!(!store.has_pending_deletes());
        assert!(!store.is_dirty());
    }

    #[test]
    fn layout_flush_only_on_explicit_save() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        store.flush().unwrap();

        let mut layout = StarMapLayout::default();
        layout.nodes.push(StarMapLayoutNode {
            node_id: "n1".to_string(),
            x: 10.0, y: 20.0, width: 100.0, height: 50.0,
            radius: 25.0, collapsed: false, z_index: 0,
            scale: 1.0, depth: 0.0, focus_weight: 0.0, orbit_group: None,
        });
        store.set_layout(layout);
        store.enqueue_save(SaveQueueEntry::Layout);
        store.enqueue_save(SaveQueueEntry::GraphMeta);

        assert!(store.is_dirty());
        assert_eq!(store.save_queue_len(), 2);

        store.upsert_node(make_test_node("n2", "Node2"));
        store.enqueue_save(SaveQueueEntry::Node);
        store.enqueue_save(SaveQueueEntry::GraphMeta);

        assert_eq!(store.save_queue_len(), 3);

        store.flush_save_queue().unwrap();
        assert!(!store.is_dirty());
    }

    #[test]
    fn ensure_loaded_preserves_dirty_after_crud() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        store.enqueue_save(SaveQueueEntry::Node);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        store.flush().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.ensure_loaded().unwrap();
        store2.upsert_node(make_test_node("n2", "Node2"));
        store2.enqueue_save(SaveQueueEntry::Node);
        store2.enqueue_save(SaveQueueEntry::GraphMeta);
        assert!(store2.is_dirty());

        store2.ensure_loaded().unwrap();
        assert!(store2.is_dirty());
        assert!(store2.dirty_nodes.contains("n2"));
    }

    #[test]
    fn ensure_object_loaded_for_edge_embed_link_hyperlink() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        let node = make_test_node("n1", "Node1");
        store.upsert_node(node.clone());
        let edge = StarMapEdge {
            id: "e1".to_string(),
            from: Some("n1".to_string()),
            to: Some("n1".to_string()),
            kind: StarMapEdgeKind::RelatedTo,
            label: None,
            payload: None,
            from_target: None,
            to_target: None,
            from_endpoint: None,
            to_endpoint: None,
            from_endpoint_path: None,
            to_endpoint_path: None,
            created_at: 0,
            updated_at: 0,
        };
        store.upsert_edge(edge.clone());
        store.enqueue_save(SaveQueueEntry::Node);
        store.enqueue_save(SaveQueueEntry::Edge);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        store.flush().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.load_phased(LoadPhase::GraphMeta).unwrap();
        assert!(!store2.edges.contains_key("e1"));

        store2.ensure_edge_loaded("e1").unwrap();
        assert!(store2.edges.contains_key("e1"));
    }

    #[test]
    fn migration_json_recorded_on_v1_load() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        store.enqueue_save(SaveQueueEntry::Node);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        store.flush().unwrap();

        let migration_path = store.starmap_dir().join("metadata").join("migration.json");
        assert!(!migration_path.exists());

        store.record_migration("test_migration", "test detail");
        assert!(migration_path.exists());
    }

    #[test]
    fn flush_package_revision_memory_matches_disk() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Test Node"));
        store.flush().unwrap();
        let mem_rev = store.package_revision();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        let result = store2.load_full();
        assert!(result.is_ok());
        let disk_rev = store2.package_revision();
        assert_eq!(mem_rev, disk_rev, "memory and disk package_revision must match after flush");
    }

    #[test]
    fn merge_memory_ids_updates_edge_endpoint_in_index() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        store.upsert_node(make_test_node("n2", "Node2"));
        store.upsert_node(make_test_node("n3", "Node3"));

        let edge = crate::starmap::types::StarMapEdge {
            id: "e1".to_string(),
            from: Some("n1".to_string()),
            to: Some("n2".to_string()),
            kind: crate::starmap::types::StarMapEdgeKind::RelatedTo,
            label: None,
            payload: None,
            from_target: None,
            to_target: None,
            from_endpoint: None,
            to_endpoint: None,
            from_endpoint_path: None,
            to_endpoint_path: None,
            created_at: 0,
            updated_at: 0,
        };
        store.upsert_edge(edge.clone());
        store.flush().unwrap();

        let updated_edge = crate::starmap::types::StarMapEdge {
            from: Some("n3".to_string()),
            ..edge
        };
        store.upsert_edge(updated_edge);
        store.flush().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        let _ = store2.load_full();
        let meta2 = store2.graph_meta.as_ref().unwrap();
        let eri = meta2.edge_relation_index.iter().find(|e| e.edge_id == "e1").unwrap();
        assert_eq!(eri.from, "n3", "edge_relation_index should reflect updated endpoint");
        assert_eq!(eri.to, "n2");
    }

    #[test]
    fn merge_memory_ids_updates_embed_host_in_index() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        store.upsert_node(make_test_node("n2", "Node2"));

        let embed = crate::starmap::types::StarMapEmbed {
            instance_id: "em1".to_string(),
            target_starmap_id: String::new(),
            label: None,
            display_policy: crate::starmap::semantic::StarMapDisplayPolicy::default(),
            open_behavior: crate::starmap::semantic::StarMapOpenBehavior::default(),
            placement: crate::starmap::types::StarMapEmbedPlacement::default(),
            target_viewport: crate::starmap::types::StarMapEmbedViewport::default(),
            source_node_id: Some("n1".to_string()),
            host_endpoint: None,
            provenance: crate::starmap::semantic::StarMapProvenance::default(),
            created_at: 0,
            updated_at: 0,
        };
        store.upsert_embed(embed.clone());
        store.flush().unwrap();

        let updated_embed = crate::starmap::types::StarMapEmbed {
            source_node_id: Some("n2".to_string()),
            ..embed
        };
        store.upsert_embed(updated_embed);
        store.flush().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        let _ = store2.load_full();
        let meta2 = store2.graph_meta.as_ref().unwrap();
        let ehi = meta2.embed_host_index.iter().find(|e| e.instance_id == "em1").unwrap();
        assert_eq!(ehi.host_node_id, "n2", "embed_host_index should reflect updated host");
    }

    #[test]
    fn list_links_with_diagnostics_returns_missing_diagnostic() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        let graph_meta = GraphMeta {
            schema_version: "2".to_string(),
            starmap_id: meta.starmap_id.clone(),
            title: "Test".to_string(),
            node_ids: vec![],
            edge_ids: vec![],
            embed_instance_ids: vec![],
            link_ids: vec!["missing-link".to_string()],
            hyperlink_ids: vec![],
            edge_relation_index: vec![],
            embed_host_index: vec![],
            link_relation_index: vec![],
            hyperlink_relation_index: vec![],
            node_kind_counts: HashMap::new(),
            package_revision: 0,
            updated_at: 0,
        };
        store.graph_meta = Some(graph_meta);
        store.flush().unwrap();

        let result = store.list_links_with_diagnostics();
        assert!(!result.diagnostics.is_empty(), "should report missing link as diagnostic");
        assert_eq!(result.diagnostics[0].object_id, "missing-link");
    }

    #[test]
    fn upsert_edge_existing_marks_dirty_graph_meta() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        let node1 = make_test_node("n1", "A");
        let node2 = make_test_node("n2", "B");
        let node3 = make_test_node("n3", "C");
        store.upsert_node(node1);
        store.upsert_node(node2);
        store.upsert_node(node3);

        use crate::starmap::types::{StarMapEdge, StarMapEdgeKind};
        let edge = StarMapEdge {
            id: "e1".to_string(),
            from: Some("n1".to_string()),
            to: Some("n2".to_string()),
            kind: StarMapEdgeKind::References,
            label: None,
            payload: None,
            from_target: None,
            to_target: None,
            from_endpoint: None,
            to_endpoint: None,
            from_endpoint_path: None,
            to_endpoint_path: None,
            created_at: 0,
            updated_at: 0,
        };
        store.upsert_edge(edge);
        store.flush().unwrap();

        let edge_updated = StarMapEdge {
            id: "e1".to_string(),
            from: Some("n1".to_string()),
            to: Some("n3".to_string()),
            kind: StarMapEdgeKind::References,
            label: None,
            payload: None,
            from_target: None,
            to_target: None,
            from_endpoint: None,
            to_endpoint: None,
            from_endpoint_path: None,
            to_endpoint_path: None,
            created_at: 0,
            updated_at: 0,
        };
        store.upsert_edge(edge_updated);
        assert!(store.dirty_graph_meta, "upsert_edge on existing edge should mark dirty_graph_meta because relation index changed");

        store.flush().unwrap();
        let meta_on_disk: GraphMeta = serde_json::from_str(
            &std::fs::read_to_string(store.starmap_dir().join("graph.json")).unwrap()
        ).unwrap();
        let eri = meta_on_disk.edge_relation_index.iter().find(|e| e.edge_id == "e1").unwrap();
        assert_eq!(eri.to, "n3", "disk relation index should reflect updated endpoint");
    }

    #[test]
    fn update_edge_endpoint_marks_dirty_graph_meta() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "A"));
        store.upsert_node(make_test_node("n2", "B"));
        store.upsert_node(make_test_node("n3", "C"));

        use crate::starmap::types::{StarMapEdge, StarMapEdgeKind, StarMapEdgePatch};
        let edge = StarMapEdge {
            id: "e1".to_string(),
            from: Some("n1".to_string()),
            to: Some("n2".to_string()),
            kind: StarMapEdgeKind::References,
            label: None,
            payload: None,
            from_target: None,
            to_target: None,
            from_endpoint: None,
            to_endpoint: None,
            from_endpoint_path: None,
            to_endpoint_path: None,
            created_at: 0,
            updated_at: 0,
        };
        store.upsert_edge(edge);
        store.flush().unwrap();

        let patch = StarMapEdgePatch {
            kind: None,
            label: None,
            payload: None,
            from_target: Some(Some(crate::starmap::semantic::StarMapDeepTarget {
                starmap_id: meta.starmap_id.clone(),
                path: vec![],
                target: crate::starmap::semantic::StarMapTargetDetail::Node { node_id: "n3".to_string() },
            })),
            to_target: None,
            from_endpoint: None,
            to_endpoint: None,
            from_endpoint_path: None,
            to_endpoint_path: None,
        };
        store.update_edge("e1", &patch).unwrap();
        assert!(store.dirty_graph_meta, "update_edge changing from_target should mark dirty_graph_meta");

        store.flush().unwrap();
        let meta_on_disk: GraphMeta = serde_json::from_str(
            &std::fs::read_to_string(store.starmap_dir().join("graph.json")).unwrap()
        ).unwrap();
        let eri = meta_on_disk.edge_relation_index.iter().find(|e| e.edge_id == "e1").unwrap();
        assert_eq!(eri.from, "n3", "disk relation index should reflect updated from endpoint");
    }

    #[test]
    fn update_embed_host_marks_dirty_graph_meta() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "A"));
        store.upsert_node(make_test_node("n2", "B"));

        use crate::starmap::types::{StarMapEmbed, StarMapEmbedPlacement, StarMapEmbedViewport, StarMapEmbedPatch};
        let embed = StarMapEmbed {
            instance_id: "emb1".to_string(),
            target_starmap_id: "other".to_string(),
            label: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            placement: StarMapEmbedPlacement::default(),
            target_viewport: StarMapEmbedViewport::default(),
            source_node_id: Some("n1".to_string()),
            host_endpoint: None,
            provenance: Default::default(),
            created_at: 0,
            updated_at: 0,
        };
        store.upsert_embed(embed);
        store.flush().unwrap();

        let patch = StarMapEmbedPatch {
            label: None,
            display_policy: None,
            open_behavior: None,
            viewport: None,
            placement: None,
            target_viewport: None,
            source_node_id: Some(Some("n2".to_string())),
            host_anchor: None,
            host_endpoint: None,
        };
        store.update_embed("emb1", &patch).unwrap();
        assert!(store.dirty_graph_meta, "update_embed changing source_node_id should mark dirty_graph_meta");

        store.flush().unwrap();
        let meta_on_disk: GraphMeta = serde_json::from_str(
            &std::fs::read_to_string(store.starmap_dir().join("graph.json")).unwrap()
        ).unwrap();
        let ehi = meta_on_disk.embed_host_index.iter().find(|e| e.instance_id == "emb1").unwrap();
        assert_eq!(ehi.host_node_id, "n2", "disk host index should reflect updated host");
    }

    #[test]
    fn delete_also_removes_flat_path() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let starmap_dir = dir.path().join("app-meta").join("starmaps").join(&meta.starmap_id);
        let nodes_dir = starmap_dir.join("nodes");
        std::fs::create_dir_all(&nodes_dir).unwrap();

        let flat_path = nodes_dir.join("n1.json");
        std::fs::write(&flat_path, "{}").unwrap();
        assert!(flat_path.exists(), "flat file should exist before delete");

        package_storage::delete_node_file(dir.path(), &meta.starmap_id, "n1").unwrap();
        assert!(!flat_path.exists(), "flat file should be removed by delete_node_file");
    }

    #[test]
    fn migrate_flat_to_bucket_records_migration() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "A"));
        store.flush().unwrap();

        let starmap_dir = store.starmap_dir();
        let nodes_dir = starmap_dir.join("nodes");
        let flat_path = nodes_dir.join("n1.json");
        let bucket_dir = nodes_dir.join(package_storage::bucket_for_id("n1"));
        let bucket_path = bucket_dir.join("n1.json");

        std::fs::create_dir_all(&bucket_dir).unwrap();
        std::fs::write(&flat_path, "{}").unwrap();
        let _ = std::fs::remove_file(&bucket_path);

        store.migrate_flat_to_bucket(&flat_path, &bucket_path);
        assert!(bucket_path.exists(), "bucket file should exist after migration");
        assert!(!flat_path.exists(), "flat file should be removed after migration");

        let migration_path = starmap_dir.join("metadata").join("migration.json");
        assert!(migration_path.exists(), "migration.json should be recorded");
        let content = std::fs::read_to_string(&migration_path).unwrap();
        assert!(content.contains("flat_to_bucket"), "migration record should mention flat_to_bucket");
    }

    #[test]
    fn merge_memory_ids_removes_deleted_ids() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "A"));
        store.upsert_node(make_test_node("n2", "B"));
        store.upsert_node(make_test_node("n3", "C"));
        store.flush().unwrap();

        store.remove_node("n2");
        store.remove_node("n3");
        assert!(store.deleted_node_ids.contains("n2"));
        assert!(store.deleted_node_ids.contains("n3"));

        store.merge_memory_ids_into_graph_meta();
        let meta_ids = store.graph_meta.as_ref().unwrap().node_ids.clone();
        assert!(meta_ids.contains(&"n1".to_string()), "n1 should remain");
        assert!(!meta_ids.contains(&"n2".to_string()), "n2 should be removed from graph_meta");
        assert!(!meta_ids.contains(&"n3".to_string()), "n3 should be removed from graph_meta");
    }

    #[test]
    fn merge_memory_ids_skips_deleted_edge_in_index() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "A"));
        store.upsert_node(make_test_node("n2", "B"));
        store.upsert_node(make_test_node("n3", "C"));

        use crate::starmap::types::{StarMapEdge, StarMapEdgeKind};
        store.upsert_edge(StarMapEdge {
            id: "e1".to_string(), from: Some("n1".to_string()), to: Some("n2".to_string()),
            kind: StarMapEdgeKind::References, label: None, payload: None,
            from_target: None, to_target: None, from_endpoint: None, to_endpoint: None,
            from_endpoint_path: None, to_endpoint_path: None, created_at: 0, updated_at: 0,
        });
        store.flush().unwrap();

        store.remove_edge("e1");
        assert!(store.deleted_edge_ids.contains("e1"));

        store.merge_memory_ids_into_graph_meta();
        let meta = store.graph_meta.as_ref().unwrap();
        assert!(!meta.edge_ids.contains(&"e1".to_string()), "deleted edge should be removed from edge_ids");
        assert!(!meta.edge_relation_index.iter().any(|e| e.edge_id == "e1"), "deleted edge should be removed from relation index");
    }

    #[test]
    fn flush_stats_uses_graph_meta_counts() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "A"));
        store.upsert_node(make_test_node("n2", "B"));
        store.upsert_node(make_test_node("n3", "C"));
        store.upsert_node(make_test_node("n4", "D"));

        use crate::starmap::types::{StarMapEdge, StarMapEdgeKind};
        store.upsert_edge(StarMapEdge {
            id: "e1".to_string(), from: Some("n1".to_string()), to: Some("n2".to_string()),
            kind: StarMapEdgeKind::References, label: None, payload: None,
            from_target: None, to_target: None, from_endpoint: None, to_endpoint: None,
            from_endpoint_path: None, to_endpoint_path: None, created_at: 0, updated_at: 0,
        });
        store.flush().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        let _ = store2.load_full();
        assert_eq!(store2.nodes.len(), 4, "store2 should have loaded 4 nodes");
        assert_eq!(store2.graph_meta.as_ref().unwrap().node_ids.len(), 4, "graph_meta should have 4 node ids");

        store2.remove_node("n4");
        assert_eq!(store2.nodes.len(), 3, "cache has 3 after removal");
        assert_eq!(store2.graph_meta.as_ref().unwrap().node_ids.len(), 3, "graph_meta node_ids should have 3 after removal");

        let result = store2.flush();
        assert!(result.is_ok(), "flush should succeed");

        let graph_meta = store2.graph_meta.as_ref().unwrap();
        assert_eq!(graph_meta.node_ids.len(), 3, "final graph_meta should have 3 node ids");
        assert_eq!(graph_meta.edge_ids.len(), 1, "final graph_meta should have 1 edge id");
    }

    #[test]
    fn prefetch_only_loads_adjacent_edges() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        for i in 1..=10 {
            store.upsert_node(make_test_node(&format!("n{}", i), &format!("Node{}", i)));
        }

        use crate::starmap::types::{StarMapEdge, StarMapEdgeKind};
        for i in 1..=9 {
            store.upsert_edge(StarMapEdge {
                id: format!("e{}", i), from: Some(format!("n{}", i)), to: Some(format!("n{}", i+1)),
                kind: StarMapEdgeKind::References, label: None, payload: None,
                from_target: None, to_target: None, from_endpoint: None, to_endpoint: None,
                from_endpoint_path: None, to_endpoint_path: None, created_at: 0, updated_at: 0,
            });
        }
        store.flush().unwrap();

        let disk_meta: GraphMeta = serde_json::from_str(
            &std::fs::read_to_string(store.starmap_dir().join("graph.json")).unwrap()
        ).unwrap();
        assert!(!disk_meta.edge_relation_index.is_empty(), "graph_meta should have edge_relation_index");

        let mut fresh = StarMapStore::new(dir.path(), &meta.starmap_id);
        fresh.graph_meta = Some(disk_meta);
        fresh.upsert_node(make_test_node("n2", "Node2"));
        fresh.upsert_node(make_test_node("n3", "Node3"));
        assert_eq!(fresh.nodes.len(), 2, "only loaded 2 selected nodes");

        fresh.prefetch_nearby_objects(&mut vec![]);

        assert!(fresh.nodes.contains_key("n1"), "n1 should be loaded via prefetch (adjacent to n2 via e1)");
        assert!(fresh.nodes.contains_key("n4"), "n4 should be loaded via prefetch (adjacent to n3 via e3)");
        assert!(fresh.edges.contains_key("e1"), "edge e1 (n1-n2) should be loaded");
        assert!(fresh.edges.contains_key("e2"), "edge e2 (n2-n3) should be loaded");
        assert!(fresh.edges.contains_key("e3"), "edge e3 (n3-n4) should be loaded");
        assert!(!fresh.edges.contains_key("e5"), "edge e5 (n5-n6) should NOT be loaded (no loaded node adjacent)");
        assert!(!fresh.edges.contains_key("e9"), "edge e9 (n9-n10) should NOT be loaded (no loaded node adjacent)");
    }

    #[test]
    fn edge_relation_index_preserves_endpoint_fields() {
        use crate::starmap::types::{StarMapEdge, StarMapEdgeKind, StarMapEdgeEndpoint, StarMapEndpointPath};
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        store.upsert_node(make_test_node("n2", "Node2"));
        store.upsert_edge(StarMapEdge {
            id: "e1".to_string(),
            from: Some("n1".to_string()),
            to: Some("n2".to_string()),
            kind: StarMapEdgeKind::References,
            label: None,
            payload: None,
            from_target: None,
            to_target: None,
            from_endpoint: Some(StarMapEdgeEndpoint::Node { node_id: "n1".to_string() }),
            to_endpoint: Some(StarMapEdgeEndpoint::Anchor { node_id: "n2".to_string(), anchor_id: "a1".to_string() }),
            from_endpoint_path: None,
            to_endpoint_path: Some(StarMapEndpointPath {
                segments: vec![],
                endpoint: StarMapEdgeEndpoint::Node { node_id: "n2".to_string() },
            }),
            created_at: 0,
            updated_at: 0,
        });
        store.flush().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.load_full().unwrap();
        let eri = store2.graph_meta.as_ref().unwrap().edge_relation_index.iter().find(|e| e.edge_id == "e1").unwrap();
        assert!(eri.from_endpoint.is_some(), "from_endpoint should be preserved in index");
        assert!(eri.to_endpoint.is_some(), "to_endpoint should be preserved in index");
        assert!(eri.to_endpoint_path.is_some(), "to_endpoint_path should be preserved in index");
    }

    #[test]
    fn embed_host_index_preserves_host_endpoint() {
        use crate::starmap::types::{StarMapEmbed, StarMapEndpoint};
        use crate::starmap::semantic::{StarMapDisplayPolicy, StarMapOpenBehavior, StarMapProvenance};
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        store.upsert_embed(StarMapEmbed {
            instance_id: "emb1".to_string(),
            target_starmap_id: "sm-child".to_string(),
            label: None,
            display_policy: StarMapDisplayPolicy::default(),
            open_behavior: StarMapOpenBehavior::default(),
            placement: Default::default(),
            target_viewport: Default::default(),
            source_node_id: Some("n1".to_string()),
            host_endpoint: Some(StarMapEndpoint::Anchor { node_id: "n1".to_string(), anchor_id: "a1".to_string() }),
            provenance: StarMapProvenance::default(),
            created_at: 0,
            updated_at: 0,
        });
        store.flush().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.load_full().unwrap();
        let ehi = store2.graph_meta.as_ref().unwrap().embed_host_index.iter().find(|e| e.instance_id == "emb1").unwrap();
        assert!(ehi.host_endpoint.is_some(), "host_endpoint should be preserved in index");
    }

    #[test]
    fn update_edge_endpoint_marks_dirty_and_updates_index() {
        use crate::starmap::types::{StarMapEdge, StarMapEdgeKind, StarMapEdgeEndpoint, StarMapEdgePatch};
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        store.upsert_node(make_test_node("n2", "Node2"));
        store.upsert_edge(StarMapEdge {
            id: "e1".to_string(),
            from: Some("n1".to_string()),
            to: Some("n2".to_string()),
            kind: StarMapEdgeKind::References,
            label: None,
            payload: None,
            from_target: None,
            to_target: None,
            from_endpoint: None,
            to_endpoint: None,
            from_endpoint_path: None,
            to_endpoint_path: None,
            created_at: 0,
            updated_at: 0,
        });
        store.flush().unwrap();
        assert!(!store.is_dirty());

        let patch = StarMapEdgePatch {
            kind: None,
            label: None,
            payload: None,
            from_target: None,
            to_target: None,
            from_endpoint: Some(Some(StarMapEdgeEndpoint::Node { node_id: "n1".to_string() })),
            to_endpoint: None,
            from_endpoint_path: None,
            to_endpoint_path: None,
        };
        store.update_edge("e1", &patch).unwrap();
        assert!(store.is_dirty(), "update_edge with endpoint change should mark dirty");
        let eri = store.graph_meta.as_ref().unwrap().edge_relation_index.iter().find(|e| e.edge_id == "e1").unwrap();
        assert!(eri.from_endpoint.is_some(), "index should reflect updated from_endpoint");
    }

    #[test]
    fn update_embed_host_endpoint_marks_dirty_and_updates_index() {
        use crate::starmap::types::{StarMapEmbed, StarMapEmbedPatch, StarMapEndpoint};
        use crate::starmap::semantic::{StarMapDisplayPolicy, StarMapOpenBehavior, StarMapProvenance};
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_node(make_test_node("n1", "Node1"));
        store.upsert_embed(StarMapEmbed {
            instance_id: "emb1".to_string(),
            target_starmap_id: "sm-child".to_string(),
            label: None,
            display_policy: StarMapDisplayPolicy::default(),
            open_behavior: StarMapOpenBehavior::default(),
            placement: Default::default(),
            target_viewport: Default::default(),
            source_node_id: Some("n1".to_string()),
            host_endpoint: None,
            provenance: StarMapProvenance::default(),
            created_at: 0,
            updated_at: 0,
        });
        store.flush().unwrap();
        assert!(!store.is_dirty());

        let patch = StarMapEmbedPatch {
            label: None,
            display_policy: None,
            open_behavior: None,
            viewport: None,
            placement: None,
            target_viewport: None,
            source_node_id: None,
            host_anchor: None,
            host_endpoint: Some(Some(StarMapEndpoint::Anchor { node_id: "n1".to_string(), anchor_id: "a1".to_string() })),
        };
        store.update_embed("emb1", &patch).unwrap();
        assert!(store.is_dirty(), "update_embed with host_endpoint change should mark dirty");
        let ehi = store.graph_meta.as_ref().unwrap().embed_host_index.iter().find(|e| e.instance_id == "emb1").unwrap();
        assert!(ehi.host_endpoint.is_some(), "index should reflect updated host_endpoint");
    }

    #[test]
    fn migrate_flat_to_bucket_atomic_on_failure() {
        let dir = TempDir::new().unwrap();
        let flat_dir = dir.path().join("flat_src");
        std::fs::create_dir_all(&flat_dir).unwrap();
        let flat_path = flat_dir.join("test.json");
        std::fs::write(&flat_path, r#"{"test": true}"#).unwrap();

        let bucket_dir = dir.path().join("bucket_dst");
        let bucket_path = bucket_dir.join("test.json");

        let mut store = StarMapStore::new(dir.path(), "dummy-id");
        store.migrate_flat_to_bucket(&flat_path, &bucket_path);
        assert!(bucket_path.exists(), "bucket file should exist after migration");
        assert!(!flat_path.exists(), "flat file should be removed after successful migration");
    }

    #[test]
    fn list_links_with_diagnostics_returns_corrupt_for_bad_file() {
        use crate::starmap::types::{StarMapLink, StarMapEndpoint};
        use crate::starmap::semantic::{StarMapDeepTarget, StarMapTargetDetail};
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let link = StarMapLink {
            link_id: "l1".to_string(),
            source: StarMapEndpoint::Node { node_id: "n1".to_string() },
            target: StarMapDeepTarget { starmap_id: meta.starmap_id.clone(), path: vec![], target: StarMapTargetDetail::Node { node_id: "other".to_string() } },
            label: None,
            created_at: 0,
            updated_at: 0,
        };
        package_storage::save_link(dir.path(), &meta.starmap_id, &link).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.upsert_link(link.clone());
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        store.flush().unwrap();

        let starmap_dir = dir.path().join("app-meta").join("starmaps").join(&meta.starmap_id);
        let bucket = package_storage::bucket_for_id("l1");
        let link_bucket = starmap_dir.join("links").join(bucket).join("l1.json");
        let link_flat = starmap_dir.join("links").join("l1.json");
        let link_path = if link_bucket.exists() { link_bucket } else { link_flat };
        assert!(link_path.exists(), "link file should exist at {:?}", link_path);
        std::fs::write(&link_path, "THIS_IS_NOT_JSON_AT_ALL").unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        let graph_json_path = starmap_dir.join("graph.json");
        let content = std::fs::read_to_string(&graph_json_path).unwrap();
        let gm: GraphMeta = serde_json::from_str(&content).unwrap();
        store2.graph_meta = Some(gm);
        store2.current_load_phase = Some(LoadPhase::GraphMeta);

        let result = store2.list_links_with_diagnostics();
        assert!(!result.diagnostics.is_empty(), "should have diagnostics for corrupt link, got {} diagnostics", result.diagnostics.len());
        assert_eq!(result.diagnostics[0].kind, LoadDiagnosticKind::Corrupt, "should report Corrupt not Missing");
    }

    #[test]
    fn add_link_updates_graph_meta_link_ids() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        let link = make_test_link("l1", "Test");
        store.add_link(link).unwrap();
        assert!(store.dirty_links.contains("l1"));
        assert!(store.dirty_graph_meta, "add_link must mark dirty_graph_meta");
        assert!(store.graph_meta.is_some(), "add_link must initialize graph_meta");
        let meta_ids = store.graph_meta.as_ref().unwrap().link_ids.clone();
        assert!(meta_ids.contains(&"l1".to_string()), "add_link must add link_id to graph_meta.link_ids");

        store.flush().unwrap();
        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.load_full().unwrap();
        assert_eq!(store2.link_count(), 1);
        assert!(store2.graph_meta.as_ref().unwrap().link_ids.contains(&"l1".to_string()),
            "link_id must persist in graph.json after flush");
    }

    #[test]
    fn delete_link_updates_graph_meta_link_ids() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        let link = make_test_link("l1", "Test");
        store.add_link(link).unwrap();
        store.flush().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.load_full().unwrap();
        store2.delete_link("l1").unwrap();
        assert!(store2.deleted_link_ids.contains(&"l1".to_string()),
            "delete_link must mark deleted_link_ids");
        assert!(store2.dirty_graph_meta, "delete_link must mark dirty_graph_meta");
        assert!(!store2.graph_meta.as_ref().unwrap().link_ids.contains(&"l1".to_string()),
            "delete_link must remove link_id from graph_meta.link_ids");

        store2.flush().unwrap();
        let mut store3 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store3.load_full().unwrap();
        assert_eq!(store3.link_count(), 0);
        assert!(!store3.graph_meta.as_ref().unwrap().link_ids.contains(&"l1".to_string()),
            "link_id must be removed from graph.json after flush");
    }

    #[test]
    fn link_flush_save_queue_persists_via_save_queue() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        let link = make_test_link("l1", "Test");
        store.add_link(link).unwrap();
        store.enqueue_save(SaveQueueEntry::Link);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        store.flush_save_queue().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.load_full().unwrap();
        assert_eq!(store2.link_count(), 1);
        assert_eq!(store2.get_link("l1").unwrap().label.as_deref(), Some("Test"));
    }

    #[test]
    fn delete_link_flush_save_queue_persists_via_save_queue() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        let link = make_test_link("l1", "Test");
        store.add_link(link).unwrap();
        assert!(store.dirty_links.contains("l1"));
        assert!(store.dirty_graph_meta, "add_link must mark dirty_graph_meta");
        let meta_ids = store.graph_meta.as_ref().unwrap().link_ids.clone();
        assert!(meta_ids.contains(&"l1".to_string()), "add_link must add link_id to graph_meta.link_ids");

        store.flush().unwrap();
        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.load_full().unwrap();
        assert_eq!(store2.link_count(), 1);
        assert!(store2.graph_meta.as_ref().unwrap().link_ids.contains(&"l1".to_string()),
            "link_id must persist in graph.json after flush");
    }

    #[test]
    fn hyperlink_add_update_delete_round_trip() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.load_full().unwrap();

        let hl = StarMapHyperlink {
            hyperlink_id: "hl1".to_string(),
            source: StarMapEndpointPath {
                segments: vec![],
                endpoint: StarMapEdgeEndpoint::Starmap,
            },
            target_uri: "https://example.com".to_string(),
            label: Some("TestHL".to_string()),
            target_starmap_id: None,
            created_at: 0,
            updated_at: 0,
        };
        let result = store.add_hyperlink(hl).unwrap();
        assert_eq!(result.hyperlink_id, "hl1");
        assert!(store.dirty_hyperlinks.contains("hl1"));
        assert!(store.dirty_graph_meta, "add_hyperlink must mark dirty_graph_meta");
        assert!(store.graph_meta.as_ref().unwrap().hyperlink_ids.contains(&"hl1".to_string()));

        store.enqueue_save(SaveQueueEntry::Hyperlink);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        store.flush_save_queue().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.load_full().unwrap();
        assert_eq!(store2.hyperlink_count(), 1);
        assert_eq!(store2.get_hyperlink("hl1").unwrap().label.as_deref(), Some("TestHL"));

        let updated = store2.update_hyperlink("hl1", Some("UpdatedHL"), None).unwrap();
        assert_eq!(updated.label.as_deref(), Some("UpdatedHL"));
        store2.enqueue_save(SaveQueueEntry::Hyperlink);
        store2.flush_save_queue().unwrap();

        let mut store3 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store3.load_full().unwrap();
        assert_eq!(store3.get_hyperlink("hl1").unwrap().label.as_deref(), Some("UpdatedHL"));

        store3.delete_hyperlink("hl1").unwrap();
        assert!(!store3.graph_meta.as_ref().unwrap().hyperlink_ids.contains(&"hl1".to_string()));
        assert!(store3.dirty_graph_meta, "delete_hyperlink must mark dirty_graph_meta");
        store3.enqueue_save(SaveQueueEntry::DeleteHyperlink);
        store3.enqueue_save(SaveQueueEntry::GraphMeta);
        store3.flush_save_queue().unwrap();

        let mut store4 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store4.load_full().unwrap();
        assert_eq!(store4.hyperlink_count(), 0);
    }

    #[test]
    fn delete_node_cascades_to_links_and_hyperlinks() {
        let dir = TempDir::new().unwrap();
        crate::workspace::create_workspace(dir.path()).unwrap();
        let meta = crate::starmap::create_starmap(dir.path(), "Test", "", None).unwrap();

        let mut store = StarMapStore::new(dir.path(), &meta.starmap_id);
        store.load_full().unwrap();

        let node = StarMapNode {
            id: "n1".to_string(),
            title: "Node1".to_string(),
            kind: StarMapNodeKind::Chapter,
            payload: None,
            tags: vec![],
            content: Default::default(),
            anchors: vec![],
            portal: None,
            display_policy: Default::default(),
            open_behavior: Default::default(),
            provenance: Default::default(),
            created_at: 0,
            updated_at: 0,
        };
        store.upsert_node(node);
        store.enqueue_save(SaveQueueEntry::Node);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        store.flush_save_queue().unwrap();

        let link = StarMapLink {
            link_id: "l1".to_string(),
            source: StarMapEndpoint::Node { node_id: "n1".to_string() },
            target: crate::starmap::semantic::StarMapDeepTarget {
                starmap_id: "other".to_string(),
                path: vec![],
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            },
            label: Some("LinkToN1".to_string()),
            created_at: 0,
            updated_at: 0,
        };
        store.upsert_link(link);
        store.enqueue_save(SaveQueueEntry::Link);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        store.flush_save_queue().unwrap();

        let hl = StarMapHyperlink {
            hyperlink_id: "hl1".to_string(),
            source: StarMapEndpointPath {
                segments: vec![],
                endpoint: StarMapEdgeEndpoint::Node { node_id: "n1".to_string() },
            },
            target_uri: "https://example.com".to_string(),
            label: Some("HLonN1".to_string()),
            target_starmap_id: None,
            created_at: 0,
            updated_at: 0,
        };
        store.upsert_hyperlink(hl);
        store.enqueue_save(SaveQueueEntry::Hyperlink);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        store.flush_save_queue().unwrap();

        let mut store2 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store2.load_full().unwrap();
        assert_eq!(store2.link_count(), 1);
        assert_eq!(store2.hyperlink_count(), 1);

        store2.delete_node("n1").unwrap();
        assert!(store2.get_link("l1").is_none(), "delete_node must cascade remove link");
        assert!(store2.get_hyperlink("hl1").is_none(), "delete_node must cascade remove hyperlink");
        assert!(store2.deleted_link_ids.contains(&"l1".to_string()));
        assert!(store2.deleted_hyperlink_ids.contains(&"hl1".to_string()));
        assert!(store2.dirty_graph_meta);

        store2.enqueue_save(SaveQueueEntry::DeleteNode);
        store2.enqueue_save(SaveQueueEntry::DeleteLink);
        store2.enqueue_save(SaveQueueEntry::DeleteHyperlink);
        store2.enqueue_save(SaveQueueEntry::GraphMeta);
        store2.flush_save_queue().unwrap();

        let mut store3 = StarMapStore::new(dir.path(), &meta.starmap_id);
        store3.load_full().unwrap();
        assert_eq!(store3.node_count(), 0);
        assert_eq!(store3.link_count(), 0);
        assert_eq!(store3.hyperlink_count(), 0);
    }
}
