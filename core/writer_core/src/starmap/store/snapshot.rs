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
        // layout_revision 记录在 graph.json 中，layout 变更需要更新 graph_meta。
        self.dirty_graph_meta = true;
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

        // 增量模式：complete 快照且 since_revision > 0 时，只返回
        // `object_revisions[id] > since_revision` 的对象。since_revision == 0
        // 走全量（complete 模式），保持首次拉取返回全部已加载对象的行为。
        let incremental = complete && since_rev > 0;

        // 提取各对象 revision map 的引用，避免在过滤闭包中重复借用 self.graph_meta。
        let node_revs = self.graph_meta.as_ref().map(|m| &m.node_revisions);
        let edge_revs = self.graph_meta.as_ref().map(|m| &m.edge_revisions);
        let embed_revs = self.graph_meta.as_ref().map(|m| &m.embed_revisions);
        let link_revs = self.graph_meta.as_ref().map(|m| &m.link_revisions);
        let hyperlink_revs = self.graph_meta.as_ref().map(|m| &m.hyperlink_revisions);
        let layout_rev = self
            .graph_meta
            .as_ref()
            .map(|m| m.layout_revision)
            .unwrap_or(0);

        let nodes: Vec<StarMapNode> = if incremental {
            self.nodes
                .values()
                .filter(|n| node_revs.and_then(|r| r.get(&n.id)).copied().unwrap_or(0) > since_rev)
                .cloned()
                .collect()
        } else {
            self.nodes.values().cloned().collect()
        };
        let edges: Vec<StarMapEdge> = if incremental {
            self.edges
                .values()
                .filter(|e| edge_revs.and_then(|r| r.get(&e.id)).copied().unwrap_or(0) > since_rev)
                .cloned()
                .collect()
        } else {
            self.edges.values().cloned().collect()
        };
        let embeds: Vec<StarMapEmbed> = if incremental {
            self.embeds
                .values()
                .filter(|em| {
                    embed_revs
                        .and_then(|r| r.get(&em.instance_id))
                        .copied()
                        .unwrap_or(0)
                        > since_rev
                })
                .cloned()
                .collect()
        } else {
            self.embeds.values().cloned().collect()
        };
        let links: Vec<StarMapLink> = if incremental {
            self.links
                .values()
                .filter(|l| {
                    link_revs
                        .and_then(|r| r.get(&l.link_id))
                        .copied()
                        .unwrap_or(0)
                        > since_rev
                })
                .cloned()
                .collect()
        } else {
            self.links.values().cloned().collect()
        };
        let hyperlinks: Vec<StarMapHyperlink> = if incremental {
            self.hyperlinks
                .values()
                .filter(|h| {
                    hyperlink_revs
                        .and_then(|r| r.get(&h.hyperlink_id))
                        .copied()
                        .unwrap_or(0)
                        > since_rev
                })
                .cloned()
                .collect()
        } else {
            self.hyperlinks.values().cloned().collect()
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
            layout: if incremental && layout_rev <= since_rev {
                None
            } else {
                self.layout.clone()
            },
            viewport: self.viewport.clone(),
            diagnostics: self.recovery_log.clone(),
        })
    }

    pub(super) fn update_graph_meta_file(
        &mut self,
        dirty: &FlushDirtySet,
    ) -> Result<(u64, std::path::PathBuf)> {
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
                ..Default::default()
            });
        }

        self.merge_memory_ids_into_graph_meta();

        let next_revision = self.package_revision.saturating_add(1);

        // 记录本次事务真正写过的对象 revision，并移除已删除对象的 revision。
        // deletion log 的 tombstone（deleted_at_revision = next_revision）已在
        // merge_memory_ids_into_graph_meta 中追加，这里只维护对象 revision map。
        // 使用传入的 dirty 快照而非 self.dirty_*，因为 flush_save_queue 在到达
        // GraphMeta 分支前已清空对应 dirty 集合。
        if let Some(ref mut meta) = self.graph_meta {
            for node_id in &dirty.nodes {
                meta.node_revisions.insert(node_id.clone(), next_revision);
            }
            for edge_id in &dirty.edges {
                meta.edge_revisions.insert(edge_id.clone(), next_revision);
            }
            for instance_id in &dirty.embeds {
                meta.embed_revisions
                    .insert(instance_id.clone(), next_revision);
            }
            for link_id in &dirty.links {
                meta.link_revisions.insert(link_id.clone(), next_revision);
            }
            for hl_id in &dirty.hyperlinks {
                meta.hyperlink_revisions
                    .insert(hl_id.clone(), next_revision);
            }
            if dirty.layout {
                meta.layout_revision = next_revision;
            }
            for node_id in &dirty.deleted_nodes {
                meta.node_revisions.remove(node_id);
            }
            for edge_id in &dirty.deleted_edges {
                meta.edge_revisions.remove(edge_id);
            }
            for instance_id in &dirty.deleted_embeds {
                meta.embed_revisions.remove(instance_id);
            }
            for link_id in &dirty.deleted_links {
                meta.link_revisions.remove(link_id);
            }
            for hl_id in &dirty.deleted_hyperlinks {
                meta.hyperlink_revisions.remove(hl_id);
            }
        }

        let meta = self.graph_meta.as_ref().ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "graph_meta not initialized",
            ))
        })?;

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
            node_revisions: meta.node_revisions.clone(),
            edge_revisions: meta.edge_revisions.clone(),
            embed_revisions: meta.embed_revisions.clone(),
            link_revisions: meta.link_revisions.clone(),
            hyperlink_revisions: meta.hyperlink_revisions.clone(),
            layout_revision: meta.layout_revision,
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
            let source_node_id = target_path_node_id(&link.source, &self.starmap_id)
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
            let source_node_id = target_path_node_id(&hl.source, &self.starmap_id)
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
