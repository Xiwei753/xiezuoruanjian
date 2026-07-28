use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::starmap::types::*;

use super::meta::GraphMeta;
use super::relation_index::*;
use super::types::*;
use super::StarMapStore;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PhasedSnapshotRequest {
    pub target_phase: LoadPhase,
    pub since_revision: u64,
}

impl Default for PhasedSnapshotRequest {
    fn default() -> Self {
        Self {
            target_phase: LoadPhase::PrefetchNearbyObjects,
            since_revision: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StarMapPhasedSnapshot {
    pub starmap_id: String,
    pub title: String,
    pub load_phase: LoadPhase,
    pub package_revision: u64,
    pub complete: bool,
    pub since_revision: u64,
    pub nodes: Vec<StarMapNode>,
    pub edges: Vec<StarMapEdge>,
    pub embeds: Vec<StarMapEmbed>,
    pub links: Vec<StarMapLink>,
    pub hyperlinks: Vec<StarMapHyperlink>,
    pub deleted_node_ids: Vec<String>,
    pub deleted_edge_ids: Vec<String>,
    pub deleted_embed_ids: Vec<String>,
    pub deleted_link_ids: Vec<String>,
    pub deleted_hyperlink_ids: Vec<String>,
    pub layout: Option<StarMapLayout>,
    pub viewport: Option<StarMapViewport>,
    pub diagnostics: Vec<LoadDiagnostic>,
}

impl StarMapStore {
    pub fn set_layout(&mut self, layout: StarMapLayout) {
        self.layout = Some(layout);
        self.dirty_layout = true;
    }

    pub fn set_viewport(&mut self, viewport: StarMapViewport) {
        self.viewport = Some(viewport);
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

    pub fn get_phased_snapshot(&mut self, request: &PhasedSnapshotRequest) -> Result<StarMapPhasedSnapshot> {
        self.load_phased(request.target_phase)?;
        let complete = request.target_phase == LoadPhase::BackgroundFullLoad;
        let since_rev = request.since_revision;

        let skip_unchanged = complete
            && since_rev > 0
            && since_rev == self.package_revision;

        let (nodes, edges, embeds, links, hyperlinks) = if skip_unchanged {
            (vec![], vec![], vec![], vec![], vec![])
        } else {
            (
                self.nodes.values().cloned().collect(),
                self.edges.values().cloned().collect(),
                self.embeds.values().cloned().collect(),
                self.links.values().cloned().collect(),
                self.hyperlinks.values().cloned().collect(),
            )
        };

        let deleted_node_ids: Vec<String> = self.deleted_node_ids.iter().cloned().collect();
        let deleted_edge_ids: Vec<String> = self.deleted_edge_ids.iter().cloned().collect();
        let deleted_embed_ids: Vec<String> = self.deleted_embed_ids.iter().cloned().collect();
        let deleted_link_ids: Vec<String> = self.deleted_link_ids.iter().cloned().collect();
        let deleted_hyperlink_ids: Vec<String> = self.deleted_hyperlink_ids.iter().cloned().collect();

        Ok(StarMapPhasedSnapshot {
            starmap_id: self.starmap_id.clone(),
            title: self.graph_meta.as_ref().map(|meta| meta.title.clone()).unwrap_or_default(),
            load_phase: request.target_phase,
            package_revision: self.package_revision,
            complete,
            since_revision: since_rev,
            nodes,
            edges,
            embeds,
            links,
            hyperlinks,
            deleted_node_ids,
            deleted_edge_ids,
            deleted_embed_ids,
            deleted_link_ids,
            deleted_hyperlink_ids,
            layout: self.layout.clone(),
            viewport: self.viewport.clone(),
            diagnostics: self.recovery_log.clone(),
        })
    }

    pub(super) fn update_graph_meta_file(&mut self) -> Result<u64> {
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
        crate::storage::atomic_write_string(&path, &json)?;

        Ok(next_revision)
    }

    pub(super) fn merge_memory_ids_into_graph_meta(&mut self) {
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
}
