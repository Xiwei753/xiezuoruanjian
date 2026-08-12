use std::collections::HashMap;

use crate::error::Result;
use crate::starmap::types::*;

use super::super::meta::{DeletedSinceLastSync, GraphMeta};
use super::super::relation_index::*;
use super::super::types::*;
use super::super::StarMapStore;

impl StarMapStore {
    pub(in crate::starmap::store) fn reload_graph_meta_if_stale(&mut self) {
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
        let mem_rev = self
            .graph_meta
            .as_ref()
            .map(|m| m.package_revision)
            .unwrap_or(0);
        if disk_meta.package_revision > mem_rev {
            self.graph_meta = Some(disk_meta);
            self.package_revision = self
                .graph_meta
                .as_ref()
                .map(|m| m.package_revision)
                .unwrap_or(0);
        }
    }

    pub(in crate::starmap::store) fn ensure_graph_meta_initialized(&mut self) {
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
                deleted_since_last_sync: DeletedSinceLastSync::default(),
            });
        }
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

        self.package_revision = self
            .graph_meta
            .as_ref()
            .map(|m| m.package_revision)
            .unwrap_or(0);

        diagnostics.append(&mut self.recovery_log);
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

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub(in crate::starmap::store) fn load_graph_meta_phase(
        &mut self,
        diagnostics: &mut Vec<LoadDiagnostic>,
    ) {
        let graph_dir = self.starmap_dir();
        let graph_json_path = graph_dir.join("graph.json");

        if graph_json_path.exists() {
            let content = std::fs::read_to_string(&graph_json_path).unwrap_or_default();
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
                let schema_version_str = value
                    .get("schemaVersion")
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
                        Ok(meta) => {
                            self.graph_meta = Some(meta);
                        }
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
                        embed_instance_ids: graph
                            .embeds
                            .iter()
                            .map(|e| e.instance_id.clone())
                            .collect(),
                        link_ids: graph.links.iter().map(|l| l.link_id.clone()).collect(),
                        hyperlink_ids: vec![],
                        edge_relation_index: graph
                            .edges
                            .iter()
                            .map(|e| EdgeRelationIndex {
                                edge_id: e.id.clone(),
                                from: e.from.clone().unwrap_or_default(),
                                to: e.to.clone().unwrap_or_default(),
                                from_endpoint: e.from_endpoint.clone(),
                                to_endpoint: e.to_endpoint.clone(),
                                from_endpoint_path: e.from_endpoint_path.clone(),
                                to_endpoint_path: e.to_endpoint_path.clone(),
                            })
                            .collect(),
                        embed_host_index: graph
                            .embeds
                            .iter()
                            .map(|e| EmbedHostIndex {
                                instance_id: e.instance_id.clone(),
                                host_node_id: e.source_node_id.clone().unwrap_or_default(),
                                host_endpoint: e.host_endpoint.clone(),
                            })
                            .collect(),
                        link_relation_index: graph
                            .links
                            .iter()
                            .map(|l| LinkRelationIndex {
                                link_id: l.link_id.clone(),
                                source_node_id: endpoint_node_id(&l.source)
                                    .unwrap_or_default()
                                    .to_string(),
                            })
                            .collect(),
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
                        deleted_since_last_sync: DeletedSinceLastSync::default(),
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
                    self.record_migration(
                        "graph_v1_to_v2",
                        "migrated inline v1 graph.json to v2 package format",
                    );
                } else {
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

    pub(in crate::starmap::store) fn load_viewport_objects(
        &mut self,
        diagnostics: &mut Vec<LoadDiagnostic>,
    ) {
        self.load_viewport_objects_impl(diagnostics, false);
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    fn load_viewport_objects_impl(
        &mut self,
        diagnostics: &mut Vec<LoadDiagnostic>,
        index_already_rebuilt: bool,
    ) {
        let viewport_node_ids: std::collections::HashSet<String> =
            match (&self.layout, &self.viewport) {
                (Some(l), Some(vp)) => {
                    let vp_left = vp.offset_x;
                    let vp_top = vp.offset_y;
                    let vp_right = vp.offset_x + vp.width / vp.scale;
                    let vp_bottom = vp.offset_y + vp.height / vp.scale;
                    l.nodes
                        .iter()
                        .filter(|n| {
                            let node_left = n.x;
                            let node_top = n.y;
                            let node_right = n.x + n.width;
                            let node_bottom = n.y + n.height;
                            node_right > vp_left
                                && node_left < vp_right
                                && node_bottom > vp_top
                                && node_top < vp_bottom
                        })
                        .map(|n| n.node_id.clone())
                        .collect()
                }
                (Some(l), None) => l.nodes.iter().map(|n| n.node_id.clone()).collect(),
                _ => std::collections::HashSet::new(),
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

        let has_index = self
            .graph_meta
            .as_ref()
            .map(|m| !m.edge_relation_index.is_empty() || m.edge_ids.is_empty())
            .unwrap_or(false);

        if has_index {
            if let Some(meta) = self.graph_meta.as_ref() {
                let edge_relation_index = meta.edge_relation_index.clone();
                let embed_host_index = meta.embed_host_index.clone();

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
            }
        } else if !index_already_rebuilt {
            self.rebuild_relation_indexes();
            self.load_viewport_objects_impl(diagnostics, true);
        }

        let _ = diagnostics;
    }
}
