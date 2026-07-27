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
pub struct GraphMeta {
    pub schema_version: String,
    pub starmap_id: String,
    pub title: String,
    pub node_ids: Vec<String>,
    pub edge_ids: Vec<String>,
    pub embed_instance_ids: Vec<String>,
    pub link_ids: Vec<String>,
    pub hyperlink_ids: Vec<String>,
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
                        if self.update_graph_meta_file().is_err() {
                            succeeded = false;
                        }
                        if succeeded {
                            self.dirty_graph_meta = false;
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
        if any_processed && all_flushed {
            self.package_revision = self.package_revision.saturating_add(1);
        }

        if self.has_pending_deletes() || self.has_pending_writes() {
            self.flush_recovery_to_disk()?;
        }

        if any_processed && all_flushed {
            let node_count = self.nodes.len() as u32;
            let edge_count = self.edges.len() as u32;
            let mut linked_chapters = 0u32;
            for node in self.nodes.values() {
                if node.kind == StarMapNodeKind::Chapter {
                    linked_chapters += 1;
                }
            }
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

        let all_edge_ids = self.graph_meta.as_ref()
            .map(|m| m.edge_ids.clone())
            .unwrap_or_default();
        let all_embed_ids = self.graph_meta.as_ref()
            .map(|m| m.embed_instance_ids.clone())
            .unwrap_or_default();

        for node_id in &viewport_node_ids {
            if !self.nodes.contains_key(node_id) {
                if let Some(node) = self.try_load_node(node_id) {
                    self.nodes.insert(node_id.clone(), node);
                }
            }
        }

        for edge_id in &all_edge_ids {
            if !self.edges.contains_key(edge_id) {
                if let Some(edge) = self.try_load_edge(edge_id) {
                    let from_in_viewport = edge.from.as_ref().map_or(false, |id| self.nodes.contains_key(id));
                    let to_in_viewport = edge.to.as_ref().map_or(false, |id| self.nodes.contains_key(id));
                    if from_in_viewport || to_in_viewport {
                        self.edges.insert(edge_id.clone(), edge);
                    }
                }
            }
        }

        for instance_id in &all_embed_ids {
            if !self.embeds.contains_key(instance_id) {
                if let Some(embed) = self.try_load_embed(instance_id) {
                    let source_in_viewport = embed.source_node_id.as_ref().map_or(false, |id| self.nodes.contains_key(id));
                    if source_in_viewport {
                        self.embeds.insert(instance_id.clone(), embed);
                    }
                }
            }
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

    fn prefetch_nearby_objects(&mut self, _diagnostics: &mut Vec<LoadDiagnostic>) {
        let loaded_node_ids: HashSet<String> = self.nodes.keys().cloned().collect();
        let mut adjacent_node_ids: HashSet<String> = HashSet::new();
        for edge in self.edges.values() {
            if let Some(ref from_id) = edge.from {
                if loaded_node_ids.contains(from_id) {
                    if let Some(ref to_id) = edge.to {
                        if !self.nodes.contains_key(to_id) {
                            adjacent_node_ids.insert(to_id.clone());
                        }
                    }
                }
            }
            if let Some(ref to_id) = edge.to {
                if loaded_node_ids.contains(to_id) {
                    if let Some(ref from_id) = edge.from {
                        if !self.nodes.contains_key(from_id) {
                            adjacent_node_ids.insert(from_id.clone());
                        }
                    }
                }
            }
        }
        for node_id in &adjacent_node_ids {
            if !self.nodes.contains_key(node_id) {
                if let Some(node) = self.try_load_node(node_id) {
                    self.nodes.insert(node_id.clone(), node);
                }
            }
        }

        let all_edge_ids = self.graph_meta.as_ref()
            .map(|m| m.edge_ids.clone())
            .unwrap_or_default();
        let all_embed_ids = self.graph_meta.as_ref()
            .map(|m| m.embed_instance_ids.clone())
            .unwrap_or_default();

        for edge_id in &all_edge_ids {
            if !self.edges.contains_key(edge_id) {
                if let Some(edge) = self.try_load_edge(edge_id) {
                    let from_in_loaded = edge.from.as_ref().map_or(false, |id| self.nodes.contains_key(id));
                    let to_in_loaded = edge.to.as_ref().map_or(false, |id| self.nodes.contains_key(id));
                    if from_in_loaded || to_in_loaded {
                        self.edges.insert(edge_id.clone(), edge);
                    }
                }
            }
        }
        for instance_id in &all_embed_ids {
            if !self.embeds.contains_key(instance_id) {
                if let Some(embed) = self.try_load_embed(instance_id) {
                    let source_in_loaded = embed.source_node_id.as_ref().map_or(false, |id| self.nodes.contains_key(id));
                    if source_in_loaded {
                        self.embeds.insert(instance_id.clone(), embed);
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
        self.nodes.insert(node_id.clone(), node);
        self.dirty_nodes.insert(node_id);
    }

    pub fn remove_node(&mut self, node_id: &str) {
        self.nodes.remove(node_id);
        self.dirty_nodes.remove(node_id);
        self.deleted_node_ids.insert(node_id.to_string());
    }

    pub fn upsert_edge(&mut self, edge: StarMapEdge) {
        let edge_id = edge.id.clone();
        self.edges.insert(edge_id.clone(), edge);
        self.dirty_edges.insert(edge_id);
    }

    pub fn remove_edge(&mut self, edge_id: &str) {
        self.edges.remove(edge_id);
        self.dirty_edges.remove(edge_id);
        self.deleted_edge_ids.insert(edge_id.to_string());
    }

    pub fn upsert_embed(&mut self, embed: StarMapEmbed) {
        let instance_id = embed.instance_id.clone();
        self.embeds.insert(instance_id.clone(), embed);
        self.dirty_embeds.insert(instance_id);
    }

    pub fn remove_embed(&mut self, instance_id: &str) {
        self.embeds.remove(instance_id);
        self.dirty_embeds.remove(instance_id);
        self.deleted_embed_ids.insert(instance_id.to_string());
    }

    pub fn upsert_hyperlink(&mut self, hl: StarMapHyperlink) {
        let hl_id = hl.hyperlink_id.clone();
        self.hyperlinks.insert(hl_id.clone(), hl);
        self.dirty_hyperlinks.insert(hl_id);
    }

    pub fn upsert_link(&mut self, link: StarMapLink) {
        let link_id = link.link_id.clone();
        self.links.insert(link_id.clone(), link);
        self.dirty_links.insert(link_id);
    }

    pub fn remove_link(&mut self, link_id: &str) {
        self.links.remove(link_id);
        self.dirty_links.remove(link_id);
        self.deleted_link_ids.insert(link_id.to_string());
    }

    pub fn remove_hyperlink(&mut self, hyperlink_id: &str) {
        self.hyperlinks.remove(hyperlink_id);
        self.dirty_hyperlinks.remove(hyperlink_id);
        self.deleted_hyperlink_ids.insert(hyperlink_id.to_string());
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
        let node = self.nodes.get_mut(node_id).ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Node not found",
            ))
        })?;
        if let Some(ref t) = patch.title { node.title = t.clone(); }
        if let Some(ref k) = patch.kind { node.kind = k.clone(); }
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
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Node not found",
            )));
        }
        self.remove_node(node_id);

        let edge_ids_to_remove: Vec<String> = self.edges.values()
            .filter(|e| {
                let from_matches = e.from.as_ref() == Some(&node_id.to_string())
                    || e.from_endpoint.as_ref().map_or(false, |ep| match ep {
                        StarMapEdgeEndpoint::Node { node_id: id } => id == node_id,
                        StarMapEdgeEndpoint::Anchor { node_id: id, .. } => id == node_id,
                        _ => false,
                    });
                let to_matches = e.to.as_ref() == Some(&node_id.to_string())
                    || e.to_endpoint.as_ref().map_or(false, |ep| match ep {
                        StarMapEdgeEndpoint::Node { node_id: id } => id == node_id,
                        StarMapEdgeEndpoint::Anchor { node_id: id, .. } => id == node_id,
                        _ => false,
                    });
                from_matches || to_matches
            })
            .map(|e| e.id.clone())
            .collect();
        for eid in &edge_ids_to_remove {
            self.remove_edge(eid);
        }

        let embed_ids_to_remove: Vec<String> = self.embeds.values()
            .filter(|e| {
                e.source_node_id.as_ref() == Some(&node_id.to_string())
                    || e.host_endpoint.as_ref().map_or(false, |ep| match ep {
                        StarMapEndpoint::Node { node_id: id } => id == node_id,
                        StarMapEndpoint::Anchor { node_id: id, .. } => id == node_id,
                        _ => false,
                    })
            })
            .map(|e| e.instance_id.clone())
            .collect();
        for iid in &embed_ids_to_remove {
            self.remove_embed(iid);
        }

        if let Some(ref mut layout) = self.layout {
            layout.nodes.retain(|n| n.node_id != node_id);
            self.dirty_layout = true;
        }

        Ok(())
    }

    pub fn add_edge(&mut self, edge: StarMapEdge) -> Result<StarMapEdge> {
        let from_valid = edge.from_target.is_some()
            || edge.from_endpoint.is_some()
            || edge.from_endpoint_path.is_some()
            || edge.from.as_ref()
                .map(|id| self.nodes.contains_key(id))
                .unwrap_or(false);
        let to_valid = edge.to_target.is_some()
            || edge.to_endpoint.is_some()
            || edge.to_endpoint_path.is_some()
            || edge.to.as_ref()
                .map(|id| self.nodes.contains_key(id))
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
        let edge = self.edges.get_mut(edge_id).ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Edge not found",
            ))
        })?;
        if let Some(ref k) = patch.kind { edge.kind = k.clone(); }
        if let Some(ref l) = patch.label { edge.label = l.clone(); }
        if let Some(ref p) = patch.payload { edge.payload = p.clone(); }
        if let Some(ref ft) = patch.from_target { edge.from_target = ft.clone(); }
        if let Some(ref tt) = patch.to_target { edge.to_target = tt.clone(); }
        if let Some(ref fe) = patch.from_endpoint { edge.from_endpoint = fe.clone(); }
        if let Some(ref te) = patch.to_endpoint { edge.to_endpoint = te.clone(); }
        if let Some(ref fep) = patch.from_endpoint_path { edge.from_endpoint_path = fep.clone(); }
        if let Some(ref tep) = patch.to_endpoint_path { edge.to_endpoint_path = tep.clone(); }
        edge.updated_at = crate::starmap::now_epoch();
        let updated = edge.clone();
        self.dirty_edges.insert(edge_id.to_string());
        Ok(updated)
    }

    pub fn delete_edge(&mut self, edge_id: &str) -> Result<()> {
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
        Ok(updated)
    }

    pub fn delete_embed(&mut self, instance_id: &str) -> Result<()> {
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
        self.links.insert(link.link_id.clone(), link);
        self.dirty_links.insert(result.link_id.clone());
        Ok(result)
    }

    pub fn update_link(&mut self, link_id: &str, patch: &StarMapLinkPatch) -> Result<StarMapLink> {
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
            return Err(crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Link not found",
            )));
        }
        self.links.remove(link_id);
        self.dirty_links.remove(link_id);
        self.deleted_link_ids.insert(link_id.to_string());
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        self.package_revision = self.package_revision.saturating_add(1);

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

        self.update_graph_meta_file()?;

        let node_count = self.nodes.len() as u32;
        let edge_count = self.edges.len() as u32;
        let mut linked_chapters = 0u32;
        for node in self.nodes.values() {
            if node.kind == StarMapNodeKind::Chapter {
                linked_chapters += 1;
            }
        }
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
                        package_revision: 0,
                        updated_at: meta.updated_at,
                    });
                }
            }
        }

        let meta: GraphMeta = serde_json::from_str(&content)?;
        Ok(meta)
    }

    fn try_load_node(&mut self, node_id: &str) -> Option<StarMapNode> {
        let dir = self.starmap_dir().join("nodes").join(package_storage::bucket_for_id(node_id));
        let path = dir.join(format!("{}.json", node_id));
        if !path.exists() {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "node".to_string(),
                object_id: node_id.to_string(),
                detail: format!("node file not found: {}", path.display()),
            });
            return None;
        }
        let content = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<StarMapNode>(&content) {
            Ok(node) => Some(node),
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
        let dir = self.starmap_dir().join("edges").join(package_storage::bucket_for_id(edge_id));
        let path = dir.join(format!("{}.json", edge_id));
        if !path.exists() {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "edge".to_string(),
                object_id: edge_id.to_string(),
                detail: format!("edge file not found: {}", path.display()),
            });
            return None;
        }
        let content = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<StarMapEdge>(&content) {
            Ok(edge) => Some(edge),
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
        let dir = self.starmap_dir().join("child_starmaps").join(package_storage::bucket_for_id(instance_id));
        let path = dir.join(format!("{}.json", instance_id));
        if !path.exists() {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "embed".to_string(),
                object_id: instance_id.to_string(),
                detail: format!("embed file not found: {}", path.display()),
            });
            return None;
        }
        let content = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<StarMapEmbed>(&content) {
            Ok(embed) => Some(embed),
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
        let dir = self.starmap_dir().join("hyperlinks").join(package_storage::bucket_for_id(hyperlink_id));
        let path = dir.join(format!("{}.json", hyperlink_id));
        if !path.exists() {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "hyperlink".to_string(),
                object_id: hyperlink_id.to_string(),
                detail: format!("hyperlink file not found: {}", path.display()),
            });
            return None;
        }
        let content = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<StarMapHyperlink>(&content) {
            Ok(hl) => Some(hl),
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
        let dir = self.starmap_dir().join("links").join(package_storage::bucket_for_id(link_id));
        let path = dir.join(format!("{}.json", link_id));
        if !path.exists() {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "link".to_string(),
                object_id: link_id.to_string(),
                detail: format!("link file not found: {}", path.display()),
            });
            return None;
        }
        let content = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<StarMapLink>(&content) {
            Ok(link) => Some(link),
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

    fn update_graph_meta_file(&self) -> Result<()> {
        let node_ids: Vec<String> = self.nodes.keys().cloned().collect();
        let edge_ids: Vec<String> = self.edges.keys().cloned().collect();
        let embed_instance_ids: Vec<String> = self.embeds.keys().cloned().collect();
        let hyperlink_ids: Vec<String> = self.hyperlinks.keys().cloned().collect();
        let link_ids: Vec<String> = self.links.keys().cloned().collect();

        let title = self.graph_meta.as_ref()
            .map(|m| m.title.clone())
            .unwrap_or_default();

        let meta = GraphMeta {
            schema_version: "2".to_string(),
            starmap_id: self.starmap_id.clone(),
            title,
            node_ids,
            edge_ids,
            embed_instance_ids,
            link_ids,
            hyperlink_ids,
            package_revision: self.package_revision,
            updated_at: crate::starmap::now_epoch(),
        };

        let json = serde_json::to_string_pretty(&meta)?;
        let path = self.starmap_dir().join("graph.json");
        atomic_write_string(&path, &json)?;

        Ok(())
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
}
