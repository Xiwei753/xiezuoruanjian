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
//! ├── graph.json                     -- 星图元信息、成员 ID 列表、规范顺序、package revision
//! ├── nodes/<node_id>.json           -- 单个节点
//! ├── edges/<edge_id>.json           -- 单条边
//! ├── child_starmaps/<instance_id>.json -- 子星图放置
//! ├── hyperlinks/<hyperlink_id>.json -- 超链接
//! ├── layouts/default.json           -- 默认布局
//! └── metadata/
//!     ├── migration.json             -- 迁移记录
//!     └── recovery.json              -- 解析失败对象的恢复记录
//! ```

use std::collections::{HashMap, HashSet};
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
    Layout,
    GraphMeta,
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
    pub loaded_hyperlink_count: usize,
}

pub struct StarMapStore {
    workspace: PathBuf,
    starmap_id: String,
    nodes: HashMap<String, StarMapNode>,
    edges: HashMap<String, StarMapEdge>,
    embeds: HashMap<String, StarMapEmbed>,
    hyperlinks: HashMap<String, StarMapHyperlink>,
    layout: Option<StarMapLayout>,
    viewport: Option<StarMapViewport>,
    graph_meta: Option<GraphMeta>,
    dirty_nodes: HashSet<String>,
    dirty_edges: HashSet<String>,
    dirty_embeds: HashSet<String>,
    dirty_hyperlinks: HashSet<String>,
    dirty_layout: bool,
    dirty_graph_meta: bool,
    deleted_node_ids: HashSet<String>,
    deleted_edge_ids: HashSet<String>,
    deleted_embed_ids: HashSet<String>,
    deleted_hyperlink_ids: HashSet<String>,
    package_revision: u64,
    recovery_log: Vec<LoadDiagnostic>,
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
    pub hyperlink_ids: Vec<String>,
    pub package_revision: u64,
    pub updated_at: u64,
}

impl StarMapStore {
    pub fn new(workspace: &Path, starmap_id: &str) -> Self {
        Self {
            workspace: workspace.to_path_buf(),
            starmap_id: starmap_id.to_string(),
            nodes: HashMap::new(),
            edges: HashMap::new(),
            embeds: HashMap::new(),
            hyperlinks: HashMap::new(),
            layout: None,
            viewport: None,
            graph_meta: None,
            dirty_nodes: HashSet::new(),
            dirty_edges: HashSet::new(),
            dirty_embeds: HashSet::new(),
            dirty_hyperlinks: HashSet::new(),
            dirty_layout: false,
            dirty_graph_meta: false,
            deleted_node_ids: HashSet::new(),
            deleted_edge_ids: HashSet::new(),
            deleted_embed_ids: HashSet::new(),
            deleted_hyperlink_ids: HashSet::new(),
            package_revision: 0,
            recovery_log: Vec::new(),
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

    pub fn diagnostics(&self) -> &[LoadDiagnostic] {
        &self.recovery_log
    }

    pub fn is_dirty(&self) -> bool {
        !self.dirty_nodes.is_empty()
            || !self.dirty_edges.is_empty()
            || !self.dirty_embeds.is_empty()
            || !self.dirty_hyperlinks.is_empty()
            || self.dirty_layout
            || self.dirty_graph_meta
    }

    pub fn load_full(&mut self) -> Result<StarMapStoreResult> {
        self.recovery_log.clear();
        let mut diagnostics = Vec::new();

        let graph_dir = self.starmap_dir();
        let graph_json_path = graph_dir.join("graph.json");

        if graph_json_path.exists() {
            match self.load_graph_meta_from_file(&graph_json_path) {
                Ok(meta) => {
                    self.graph_meta = Some(meta);
                }
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
        } else {
            self.scan_objects_from_disk(&mut diagnostics);
        }

        let node_ids = self.graph_meta.as_ref()
            .map(|m| m.node_ids.clone())
            .unwrap_or_default();

        for node_id in &node_ids {
            if let Some(node) = self.try_load_node(node_id) {
                self.nodes.insert(node_id.clone(), node);
            }
        }

        let edge_ids = self.graph_meta.as_ref()
            .map(|m| m.edge_ids.clone())
            .unwrap_or_default();

        for edge_id in &edge_ids {
            if let Some(edge) = self.try_load_edge(edge_id) {
                self.edges.insert(edge_id.clone(), edge);
            }
        }

        let embed_ids = self.graph_meta.as_ref()
            .map(|m| m.embed_instance_ids.clone())
            .unwrap_or_default();

        for instance_id in &embed_ids {
            if let Some(embed) = self.try_load_embed(instance_id) {
                self.embeds.insert(instance_id.clone(), embed);
            }
        }

        let hl_ids = self.graph_meta.as_ref()
            .map(|m| m.hyperlink_ids.clone())
            .unwrap_or_default();

        for hl_id in &hl_ids {
            if let Some(hl) = self.try_load_hyperlink(hl_id) {
                self.hyperlinks.insert(hl_id.clone(), hl);
            }
        }

        self.layout = self.try_load_layout();
        self.viewport = self.try_load_viewport();

        self.package_revision = self.graph_meta.as_ref()
            .map(|m| m.package_revision)
            .unwrap_or(0);

        self.dirty_nodes.clear();
        self.dirty_edges.clear();
        self.dirty_embeds.clear();
        self.dirty_hyperlinks.clear();
        self.dirty_layout = false;
        self.dirty_graph_meta = false;
        self.deleted_node_ids.clear();
        self.deleted_edge_ids.clear();
        self.deleted_embed_ids.clear();
        self.deleted_hyperlink_ids.clear();

        diagnostics.extend(self.recovery_log.drain(..));
        self.recovery_log = diagnostics.clone();

        Ok(StarMapStoreResult {
            diagnostics,
            loaded_node_count: self.nodes.len(),
            loaded_edge_count: self.edges.len(),
            loaded_embed_count: self.embeds.len(),
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

        for node_id in &self.deleted_node_ids {
            let _ = package_storage::delete_node_file(&self.workspace, &self.starmap_id, node_id);
        }

        for edge_id in &self.deleted_edge_ids {
            let _ = package_storage::delete_edge_file(&self.workspace, &self.starmap_id, edge_id);
        }

        for instance_id in &self.deleted_embed_ids {
            let _ = package_storage::delete_embed_file(&self.workspace, &self.starmap_id, instance_id);
        }

        for hl_id in &self.deleted_hyperlink_ids {
            let _ = package_storage::delete_hyperlink_file(&self.workspace, &self.starmap_id, hl_id);
        }

        self.update_graph_meta_file()?;

        self.dirty_nodes.clear();
        self.dirty_edges.clear();
        self.dirty_embeds.clear();
        self.dirty_hyperlinks.clear();
        self.dirty_layout = false;
        self.dirty_graph_meta = false;
        self.deleted_node_ids.clear();
        self.deleted_edge_ids.clear();
        self.deleted_embed_ids.clear();
        self.deleted_hyperlink_ids.clear();

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
        let meta: GraphMeta = serde_json::from_str(&content)?;
        Ok(meta)
    }

    fn try_load_node(&mut self, node_id: &str) -> Option<StarMapNode> {
        let dir = self.starmap_dir().join("nodes");
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
        let dir = self.starmap_dir().join("edges");
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
        let dir = self.starmap_dir().join("child_starmaps");
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
        let dir = self.starmap_dir().join("hyperlinks");
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
        let path = self.starmap_dir().join("layouts").join("default.json");
        if !path.exists() {
            return None;
        }
        let content = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&content).ok()
    }

    fn try_load_viewport(&self) -> Option<StarMapViewport> {
        let path = self.starmap_dir().join("viewport.json");
        if !path.exists() {
            return None;
        }
        let content = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&content).ok()
    }

    fn scan_objects_from_disk(&mut self, diagnostics: &mut Vec<LoadDiagnostic>) {
        let nodes_dir = self.starmap_dir().join("nodes");
        if let Ok(entries) = std::fs::read_dir(&nodes_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    let id = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
                    if !id.is_empty() {
                        if let Some(node) = self.try_load_node(&id) {
                            self.nodes.insert(id, node);
                        }
                    }
                }
            }
        }

        let edges_dir = self.starmap_dir().join("edges");
        if let Ok(entries) = std::fs::read_dir(&edges_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    let id = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
                    if !id.is_empty() {
                        if let Some(edge) = self.try_load_edge(&id) {
                            self.edges.insert(id, edge);
                        }
                    }
                }
            }
        }

        let embeds_dir = self.starmap_dir().join("child_starmaps");
        if let Ok(entries) = std::fs::read_dir(&embeds_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    let id = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
                    if !id.is_empty() {
                        if let Some(embed) = self.try_load_embed(&id) {
                            self.embeds.insert(id, embed);
                        }
                    }
                }
            }
        }

        let hls_dir = self.starmap_dir().join("hyperlinks");
        if let Ok(entries) = std::fs::read_dir(&hls_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    let id = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
                    if !id.is_empty() {
                        if let Some(hl) = self.try_load_hyperlink(&id) {
                            self.hyperlinks.insert(id, hl);
                        }
                    }
                }
            }
        }

        let _ = diagnostics;
    }

    fn update_graph_meta_file(&self) -> Result<()> {
        let node_ids: Vec<String> = self.nodes.keys().cloned().collect();
        let edge_ids: Vec<String> = self.edges.keys().cloned().collect();
        let embed_instance_ids: Vec<String> = self.embeds.keys().cloned().collect();
        let hyperlink_ids: Vec<String> = self.hyperlinks.keys().cloned().collect();

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
            links: Vec::new(),
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
        let starmap_dir = dir.path().join("app-meta").join("starmaps").join("test-id");
        std::fs::create_dir_all(starmap_dir.join("nodes")).unwrap();
        std::fs::create_dir_all(starmap_dir.join("edges")).unwrap();
        std::fs::create_dir_all(starmap_dir.join("child_starmaps")).unwrap();
        std::fs::create_dir_all(starmap_dir.join("hyperlinks")).unwrap();
        std::fs::create_dir_all(starmap_dir.join("layouts")).unwrap();
        std::fs::create_dir_all(starmap_dir.join("metadata")).unwrap();

        let mut store = StarMapStore::new(dir.path(), "test-id");
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

        let meta = GraphMeta {
            schema_version: "2".to_string(),
            starmap_id: "test-id".to_string(),
            title: "Test".to_string(),
            node_ids: vec!["missing-node".to_string()],
            edge_ids: vec![],
            embed_instance_ids: vec![],
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
}
