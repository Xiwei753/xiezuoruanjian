use std::collections::HashMap;

use crate::error::Result;
use crate::starmap::types::*;

use super::super::meta::GraphMeta;
use super::super::relation_index::*;
use super::super::types::*;
use super::super::StarMapStore;

impl StarMapStore {
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
}
