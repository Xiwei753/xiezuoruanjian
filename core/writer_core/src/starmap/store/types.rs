use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// 一次 flush 事务中涉及的 dirty 对象集合。
///
/// `flush_save_queue` 在清空 dirty 集合前快照，传给 `update_graph_meta_file`
/// 记录对象 revision。之所以需要快照，是因为 `flush_save_queue` 按队列顺序
/// 处理 `Node`/`Edge`/… 分支时会逐个清空对应 dirty 集合，等到 `GraphMeta`
/// 分支执行时 dirty 已被清空，无法再从中读取本次事务改了哪些对象。
#[derive(Debug, Clone, Default)]
pub struct FlushDirtySet {
    pub nodes: HashSet<String>,
    pub edges: HashSet<String>,
    pub embeds: HashSet<String>,
    pub links: HashSet<String>,
    pub hyperlinks: HashSet<String>,
    pub layout: bool,
    pub deleted_nodes: HashSet<String>,
    pub deleted_edges: HashSet<String>,
    pub deleted_embeds: HashSet<String>,
    pub deleted_links: HashSet<String>,
    pub deleted_hyperlinks: HashSet<String>,
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
