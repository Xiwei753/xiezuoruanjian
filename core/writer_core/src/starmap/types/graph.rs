use serde::{Deserialize, Serialize};

use crate::starmap::semantic::{
    StarMapAnchor, StarMapDisplayPolicy, StarMapNodeContent, StarMapOpenBehavior, StarMapPortal,
    StarMapProvenance,
};

use super::StarMapEndpointPath;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StarMapEdgeEndpoint {
    Node {
        node_id: String,
    },
    Anchor {
        node_id: String,
        anchor_id: String,
    },
    Starmap,
    DeepTarget {
        target: crate::starmap::semantic::StarMapDeepTarget,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StarMapNodeKind {
    Character,
    Event,
    Location,
    Item,
    Concept,
    Theme,
    Note,
    Organization,
    Timeline,
    Plot,
    Foreshadowing,
    Chapter,
    #[serde(other)]
    Custom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StarMapEdgeKind {
    Contains,
    References,
    AppearsIn,
    Causes,
    RelatedTo,
    LocatedAt,
    CharacterRelation,
    Timeline,
    Foreshadows,
    Resolves,
    DependsOn,
    ConflictsWith,
    #[serde(other)]
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapGraph {
    pub schema_version: u32,
    pub id: String,
    pub starmap_id: String,
    pub title: String,
    pub nodes: Vec<StarMapNode>,
    pub edges: Vec<StarMapEdge>,
    #[serde(default)]
    pub embeds: Vec<super::StarMapEmbed>,
    #[serde(default)]
    pub links: Vec<super::StarMapLink>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Default for StarMapGraph {
    fn default() -> Self {
        Self {
            schema_version: 1,
            id: String::new(),
            starmap_id: String::new(),
            title: String::new(),
            nodes: vec![],
            edges: vec![],
            embeds: vec![],
            links: vec![],
            created_at: 0,
            updated_at: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapNode {
    pub id: String,
    pub title: String,
    pub kind: StarMapNodeKind,
    pub payload: Option<serde_json::Value>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub content: StarMapNodeContent,
    #[serde(default)]
    pub anchors: Vec<StarMapAnchor>,
    #[serde(default)]
    pub portal: Option<StarMapPortal>,
    #[serde(default)]
    pub display_policy: StarMapDisplayPolicy,
    #[serde(default)]
    pub open_behavior: StarMapOpenBehavior,
    #[serde(default)]
    pub provenance: StarMapProvenance,

    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEdge {
    pub id: String,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    pub kind: StarMapEdgeKind,
    pub label: Option<String>,
    pub payload: Option<serde_json::Value>,
    #[serde(default)]
    pub from_target: Option<crate::starmap::semantic::StarMapDeepTarget>,
    #[serde(default)]
    pub to_target: Option<crate::starmap::semantic::StarMapDeepTarget>,
    #[serde(default)]
    pub from_endpoint: Option<StarMapEdgeEndpoint>,
    #[serde(default)]
    pub to_endpoint: Option<StarMapEdgeEndpoint>,
    #[serde(default)]
    pub from_endpoint_path: Option<StarMapEndpointPath>,
    #[serde(default)]
    pub to_endpoint_path: Option<StarMapEndpointPath>,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapNodePatch {
    pub title: Option<String>,
    pub kind: Option<StarMapNodeKind>,
    pub payload: Option<Option<serde_json::Value>>,
    pub tags: Option<Vec<String>>,
    pub content: Option<StarMapNodeContent>,
    pub anchors: Option<Vec<StarMapAnchor>>,
    pub portal: Option<Option<StarMapPortal>>,
    pub display_policy: Option<StarMapDisplayPolicy>,
    pub open_behavior: Option<StarMapOpenBehavior>,
    pub provenance: Option<StarMapProvenance>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapEdgePatch {
    pub kind: Option<StarMapEdgeKind>,
    pub label: Option<Option<String>>,
    pub payload: Option<Option<serde_json::Value>>,
    pub from_target: Option<Option<crate::starmap::semantic::StarMapDeepTarget>>,
    pub to_target: Option<Option<crate::starmap::semantic::StarMapDeepTarget>>,
    pub from_endpoint: Option<Option<StarMapEdgeEndpoint>>,
    pub to_endpoint: Option<Option<StarMapEdgeEndpoint>>,
    pub from_endpoint_path: Option<Option<StarMapEndpointPath>>,
    pub to_endpoint_path: Option<Option<StarMapEndpointPath>>,
}
