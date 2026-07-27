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
