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
            starmap_id: self.starmap_id.clone(),
            nodes: self.nodes.values().cloned().collect(),
            edges: self.edges.values().cloned().collect(),
            embeds: self.embeds.values().cloned().collect(),
            links: self.links.values().cloned().collect(),
            hyperlinks: self.hyperlinks.values().cloned().collect(),
        }
    }

    // TODO(#597): 既有代码可读性技术债，待后续重构拆分
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting
    )]
    pub fn get_phased_snapshot(
        &mut self,
        request: &PhasedSnapshotRequest,
    ) -> Result<StarMapPhasedSnapshot> {
        self.load_phased(request.target_phase)?;
        let complete = request.target_phase == LoadPhase::BackgroundFullLoad;
        let since_rev = request.since_revision;

        let skip_unchanged = complete && since_rev > 0 && since_rev == self.package_revision;

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

        let persistent_entries = self
            .graph_meta
            .as_ref()
            .map(|m| {
                m.deleted_since_last_sync
                    .entries_since(since_rev)
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let deleted_node_ids: Vec<String> = self
            .deleted_node_ids
            .iter()
            .chain(
                persistent_entries
                    .iter()
                    .filter(|e| e.object_type == "node")
                    .map(|e| &e.object_id),
            )
            .cloned()
            .collect();
        let deleted_edge_ids: Vec<String> = self
            .deleted_edge_ids
            .iter()
            .chain(
                persistent_entries
                    .iter()
                    .filter(|e| e.object_type == "edge")
                    .map(|e| &e.object_id),
            )
            .cloned()
            .collect();
        let deleted_embed_ids: Vec<String> = self
            .deleted_embed_ids
            .iter()
            .chain(
                persistent_entries
                    .iter()
                    .filter(|e| e.object_type == "embed")
                    .map(|e| &e.object_id),
            )
            .cloned()
            .collect();
        let deleted_link_ids: Vec<String> = self
            .deleted_link_ids
            .iter()
            .chain(
                persistent_entries
                    .iter()
                    .filter(|e| e.object_type == "link")
                    .map(|e| &e.object_id),
            )
            .cloned()
            .collect();
        let deleted_hyperlink_ids: Vec<String> = self
            .deleted_hyperlink_ids
            .iter()
            .chain(
                persistent_entries
                    .iter()
                    .filter(|e| e.object_type == "hyperlink")
                    .map(|e| &e.object_id),
            )
            .cloned()
            .collect();

        Ok(StarMapPhasedSnapshot {
            starmap_id: self.starmap_id.clone(),
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

    pub(super) fn update_graph_meta_file(&mut self) -> Result<(u64, std::path::PathBuf)> {
        if self.graph_meta.is_none() {
            self.reload_graph_meta_if_stale();
        }
        if self.graph_meta.is_none() {
            self.graph_meta = Some(GraphMeta {
                schema_version: "2".to_string(),
                starmap_id: self.starmap_id.clone(),
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
                deleted_since_last_sync: super::meta::DeletedSinceLastSync::default(),
            });
        }

        self.merge_memory_ids_into_graph_meta();

        let meta = self.graph_meta.as_ref().ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "graph_meta not initialized",
            ))
        })?;

        let next_revision = self.package_revision.saturating_add(1);

        let meta_to_write = GraphMeta {
            schema_version: meta.schema_version.clone(),
            starmap_id: meta.starmap_id.clone(),
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
            deleted_since_last_sync: meta.deleted_since_last_sync.clone(),
        };

        let json = serde_json::to_string_pretty(&meta_to_write)?;
        let path = self.starmap_dir().join("graph.json");
        crate::storage::atomic_write_string(&path, &json)?;

        let rel_path = std::path::PathBuf::from("starmaps")
            .join(&self.starmap_id)
            .join("graph.json");
        Ok((next_revision, rel_path))
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub(super) fn merge_memory_ids_into_graph_meta(&mut self) {
        let Some(ref mut meta) = self.graph_meta else {
            return;
        };

        // 增量合并：保留磁盘上已有的对象 IDs，添加内存中新增的，移除已删除的。
        // 不能无条件从 scratch 重建，因为 store 可能只部分加载，或者磁盘上有
        // 其他 store 实例直接写入的对象（例如测试中局部 store 写入的 hyperlink）。
        // 从 scratch 会丢失这些对象。增量合并保留磁盘已有，添加内存新增，移除
        // 明确删除的（deleted_*_ids）。删除处理在 flush 中先删文件后清空 deleted_*_ids，
        // 因此调用方需保证 merge 在清空 deleted_*_ids 之前执行。

        // --- node_ids + node_kind_counts ---
        for node_id in self.nodes.keys() {
            if !meta.node_ids.contains(node_id) && !self.deleted_node_ids.contains(node_id) {
                meta.node_ids.push(node_id.clone());
            }
        }
        meta.node_ids
            .retain(|id| !self.deleted_node_ids.contains(id));
        // node_kind_counts: 仅在 fully loaded 时从 scratch 重建
        if self.current_load_phase >= Some(LoadPhase::BackgroundFullLoad) {
            meta.node_kind_counts.clear();
            for node in self.nodes.values() {
                *meta
                    .node_kind_counts
                    .entry(format!("{:?}", node.kind))
                    .or_insert(0u32) += 1;
            }
        }

        // --- edge_ids + edge_relation_index ---
        for edge in self.edges.values() {
            if self.deleted_edge_ids.contains(&edge.id) {
                continue;
            }
            if let Some(eri) = meta
                .edge_relation_index
                .iter_mut()
                .find(|eri| eri.edge_id == edge.id)
            {
                eri.from = edge.from.clone();
                eri.to = edge.to.clone();
            } else if !meta.edge_ids.contains(&edge.id) {
                meta.edge_ids.push(edge.id.clone());
                meta.edge_relation_index.push(EdgeRelationIndex {
                    edge_id: edge.id.clone(),
                    from: edge.from.clone(),
                    to: edge.to.clone(),
                });
            }
        }
        meta.edge_ids
            .retain(|id| !self.deleted_edge_ids.contains(id));
        meta.edge_relation_index
            .retain(|eri| !self.deleted_edge_ids.contains(&eri.edge_id));

        // --- embed_instance_ids + embed_host_index ---
        for embed in self.embeds.values() {
            if self.deleted_embed_ids.contains(&embed.instance_id) {
                continue;
            }
            if let Some(ehi) = meta
                .embed_host_index
                .iter_mut()
                .find(|ehi| ehi.instance_id == embed.instance_id)
            {
                ehi.host_path = embed.host_path.clone();
            } else if !meta.embed_instance_ids.contains(&embed.instance_id) {
                meta.embed_instance_ids.push(embed.instance_id.clone());
                meta.embed_host_index.push(EmbedHostIndex {
                    instance_id: embed.instance_id.clone(),
                    host_path: embed.host_path.clone(),
                });
            }
        }
        meta.embed_instance_ids
            .retain(|id| !self.deleted_embed_ids.contains(id));
        meta.embed_host_index
            .retain(|ehi| !self.deleted_embed_ids.contains(&ehi.instance_id));

        // --- link_ids + link_relation_index ---
        for link in self.links.values() {
            if self.deleted_link_ids.contains(&link.link_id) {
                continue;
            }
            let source_node_id = target_path_node_id(&link.source)
                .unwrap_or_default()
                .to_string();
            if !meta.link_ids.contains(&link.link_id) {
                meta.link_ids.push(link.link_id.clone());
            }
            if let Some(lri) = meta
                .link_relation_index
                .iter_mut()
                .find(|lri| lri.link_id == link.link_id)
            {
                lri.source_node_id = source_node_id;
            } else {
                meta.link_relation_index.push(LinkRelationIndex {
                    link_id: link.link_id.clone(),
                    source_node_id,
                });
            }
        }
        meta.link_ids
            .retain(|id| !self.deleted_link_ids.contains(id));
        meta.link_relation_index
            .retain(|lri| !self.deleted_link_ids.contains(&lri.link_id));

        // --- hyperlink_ids + hyperlink_relation_index ---
        for hl in self.hyperlinks.values() {
            if self.deleted_hyperlink_ids.contains(&hl.hyperlink_id) {
                continue;
            }
            let source_node_id = target_path_node_id(&hl.source)
                .unwrap_or_default()
                .to_string();
            if !meta.hyperlink_ids.contains(&hl.hyperlink_id) {
                meta.hyperlink_ids.push(hl.hyperlink_id.clone());
            }
            if let Some(hri) = meta
                .hyperlink_relation_index
                .iter_mut()
                .find(|hri| hri.hyperlink_id == hl.hyperlink_id)
            {
                hri.source_node_id = source_node_id;
            } else {
                meta.hyperlink_relation_index.push(HyperlinkRelationIndex {
                    hyperlink_id: hl.hyperlink_id.clone(),
                    source_node_id,
                });
            }
        }
        meta.hyperlink_ids
            .retain(|id| !self.deleted_hyperlink_ids.contains(id));
        meta.hyperlink_relation_index
            .retain(|hri| !self.deleted_hyperlink_ids.contains(&hri.hyperlink_id));

        // Update deleted_since_last_sync: add entries for newly deleted objects
        let next_rev = self.package_revision.saturating_add(1);
        for node_id in &self.deleted_node_ids {
            meta.deleted_since_last_sync
                .add_entry("node", node_id, next_rev);
        }
        for edge_id in &self.deleted_edge_ids {
            meta.deleted_since_last_sync
                .add_entry("edge", edge_id, next_rev);
        }
        for instance_id in &self.deleted_embed_ids {
            meta.deleted_since_last_sync
                .add_entry("embed", instance_id, next_rev);
        }
        for link_id in &self.deleted_link_ids {
            meta.deleted_since_last_sync
                .add_entry("link", link_id, next_rev);
        }
        for hl_id in &self.deleted_hyperlink_ids {
            meta.deleted_since_last_sync
                .add_entry("hyperlink", hl_id, next_rev);
        }

        // Remove deleted_since_last_sync entries for objects that are back in memory
        for node_id in self.nodes.keys() {
            meta.deleted_since_last_sync.remove_entry("node", node_id);
        }
        for edge_id in self.edges.keys() {
            meta.deleted_since_last_sync.remove_entry("edge", edge_id);
        }
        for instance_id in self.embeds.keys() {
            meta.deleted_since_last_sync
                .remove_entry("embed", instance_id);
        }
        for link_id in self.links.keys() {
            meta.deleted_since_last_sync.remove_entry("link", link_id);
        }
        for hl_id in self.hyperlinks.keys() {
            meta.deleted_since_last_sync
                .remove_entry("hyperlink", hl_id);
        }
    }
}
