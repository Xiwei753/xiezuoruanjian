use std::collections::{HashMap, HashSet};

use crate::starmap::types::*;

use super::super::relation_index::*;
use super::super::types::*;
use super::super::StarMapStore;

impl StarMapStore {
    pub fn list_hyperlinks_with_diagnostics(&mut self) -> ListWithDiagnostics<StarMapHyperlink> {
        self.reload_graph_meta_if_stale();
        let hl_ids = self.graph_meta_hyperlink_ids();
        let mut items = Vec::new();
        let mut diagnostics = Vec::new();
        for hl_id in &hl_ids {
            if !self.hyperlinks.contains_key(hl_id) {
                if let Some(hl) = self.try_load_hyperlink(hl_id) {
                    self.hyperlinks.insert(hl_id.clone(), hl);
                } else {
                    let recovery_len = self.recovery_log.len();
                    if recovery_len > 0 {
                        if let Some(last) = self.recovery_log.last().cloned() {
                            if last.object_id == *hl_id {
                                diagnostics.push(last);
                                continue;
                            }
                        }
                    }
                    diagnostics.push(LoadDiagnostic {
                        kind: LoadDiagnosticKind::Missing,
                        object_type: "hyperlink".to_string(),
                        object_id: hl_id.clone(),
                        detail: "hyperlink could not be loaded".to_string(),
                    });
                    continue;
                }
            }
            if let Some(hl) = self.hyperlinks.get(hl_id).cloned() {
                items.push(hl);
            }
        }
        ListWithDiagnostics { items, diagnostics }
    }

    pub fn list_links_with_diagnostics(&mut self) -> ListWithDiagnostics<StarMapLink> {
        self.reload_graph_meta_if_stale();
        let link_ids = self.graph_meta_link_ids();
        let mut items = Vec::new();
        let mut diagnostics = Vec::new();
        for link_id in &link_ids {
            if !self.links.contains_key(link_id) {
                if let Some(link) = self.try_load_link(link_id) {
                    self.links.insert(link_id.clone(), link);
                } else {
                    let recovery_len = self.recovery_log.len();
                    if recovery_len > 0 {
                        if let Some(last) = self.recovery_log.last().cloned() {
                            if last.object_id == *link_id {
                                diagnostics.push(last);
                                continue;
                            }
                        }
                    }
                    diagnostics.push(LoadDiagnostic {
                        kind: LoadDiagnosticKind::Missing,
                        object_type: "link".to_string(),
                        object_id: link_id.clone(),
                        detail: "link could not be loaded".to_string(),
                    });
                    continue;
                }
            }
            if let Some(link) = self.links.get(link_id).cloned() {
                items.push(link);
            }
        }
        ListWithDiagnostics { items, diagnostics }
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

    pub(in crate::starmap::store) fn rebuild_relation_indexes(&mut self) {
        let edge_ids = self.graph_meta.as_ref()
            .map(|m| m.edge_ids.clone())
            .unwrap_or_default();
        let embed_ids = self.graph_meta.as_ref()
            .map(|m| m.embed_instance_ids.clone())
            .unwrap_or_default();
        let link_ids = self.graph_meta.as_ref()
            .map(|m| m.link_ids.clone())
            .unwrap_or_default();
        let hl_ids = self.graph_meta.as_ref()
            .map(|m| m.hyperlink_ids.clone())
            .unwrap_or_default();

        let mut edge_relation_index = Vec::new();
        for edge_id in &edge_ids {
            if !self.edges.contains_key(edge_id) {
                if let Some(edge) = self.try_load_edge(edge_id) {
                    self.edges.insert(edge_id.clone(), edge);
                }
            }
            if let Some(edge) = self.edges.get(edge_id) {
                edge_relation_index.push(EdgeRelationIndex {
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

        let mut embed_host_index = Vec::new();
        for instance_id in &embed_ids {
            if !self.embeds.contains_key(instance_id) {
                if let Some(embed) = self.try_load_embed(instance_id) {
                    self.embeds.insert(instance_id.clone(), embed);
                }
            }
            if let Some(embed) = self.embeds.get(instance_id) {
                embed_host_index.push(EmbedHostIndex {
                    instance_id: embed.instance_id.clone(),
                    host_node_id: embed.source_node_id.clone().unwrap_or_default(),
                    host_endpoint: embed.host_endpoint.clone(),
                });
            }
        }

        let mut link_relation_index = Vec::new();
        for link_id in &link_ids {
            if !self.links.contains_key(link_id) {
                if let Some(link) = self.try_load_link(link_id) {
                    self.links.insert(link_id.clone(), link);
                }
            }
            if let Some(link) = self.links.get(link_id) {
                link_relation_index.push(LinkRelationIndex {
                    link_id: link.link_id.clone(),
                    source_node_id: endpoint_node_id(&link.source).unwrap_or_default().to_string(),
                });
            }
        }

        let mut hyperlink_relation_index = Vec::new();
        for hl_id in &hl_ids {
            if !self.hyperlinks.contains_key(hl_id) {
                if let Some(hl) = self.try_load_hyperlink(hl_id) {
                    self.hyperlinks.insert(hl_id.clone(), hl);
                }
            }
            if let Some(hl) = self.hyperlinks.get(hl_id) {
                hyperlink_relation_index.push(HyperlinkRelationIndex {
                    hyperlink_id: hl.hyperlink_id.clone(),
                    source_node_id: endpoint_path_node_id(&hl.source).unwrap_or_default().to_string(),
                });
            }
        }

        let mut node_kind_counts = HashMap::new();
        for node in self.nodes.values() {
            *node_kind_counts.entry(format!("{:?}", node.kind)).or_insert(0u32) += 1;
        }

        if let Some(ref mut meta) = self.graph_meta {
            meta.edge_relation_index = edge_relation_index;
            meta.embed_host_index = embed_host_index;
            meta.link_relation_index = link_relation_index;
            meta.hyperlink_relation_index = hyperlink_relation_index;
            meta.node_kind_counts = node_kind_counts;
        }
        self.dirty_graph_meta = true;
        self.enqueue_save(SaveQueueEntry::GraphMeta);
        self.record_migration("rebuild_relation_indexes", "rebuilt relation indexes from object files for no-index legacy package");
    }

    pub(in crate::starmap::store) fn prefetch_nearby_objects(&mut self, _diagnostics: &mut Vec<LoadDiagnostic>) {
        let loaded_node_ids: HashSet<String> = self.nodes.keys().cloned().collect();
        let mut adjacent_node_ids: HashSet<String> = HashSet::new();

        let has_index = self.graph_meta.as_ref()
            .map(|m| !m.edge_relation_index.is_empty() || m.edge_ids.is_empty())
            .unwrap_or(false);

        let mut has_index_after_rebuild = has_index;
        if !has_index {
            self.rebuild_relation_indexes();
            has_index_after_rebuild = self.graph_meta.as_ref()
                .map(|m| !m.edge_relation_index.is_empty() || m.edge_ids.is_empty())
                .unwrap_or(false);
        }

        if has_index_after_rebuild {
            if let Some(meta) = self.graph_meta.as_ref() {
                let edge_relation_index = meta.edge_relation_index.clone();
                for eri in &edge_relation_index {
                    let refs = extract_eri_node_refs(eri);
                    for node_id in &refs {
                        if loaded_node_ids.contains(*node_id) {
                            for other_id in &refs {
                                if other_id != node_id && !self.nodes.contains_key(*other_id) && !other_id.is_empty() {
                                    adjacent_node_ids.insert(other_id.to_string());
                                }
                            }
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

        let has_edge_index = self.graph_meta.as_ref()
            .map(|m| !m.edge_relation_index.is_empty() || m.edge_ids.is_empty())
            .unwrap_or(false);
        let has_embed_index = self.graph_meta.as_ref()
            .map(|m| !m.embed_host_index.is_empty() || m.embed_instance_ids.is_empty())
            .unwrap_or(false);

        if has_edge_index || self.graph_meta.is_some() {
            if !has_edge_index {
                self.rebuild_relation_indexes();
            }
            if let Some(ref meta) = self.graph_meta {
                let edge_relation_index = meta.edge_relation_index.clone();
                for eri in &edge_relation_index {
                    if !self.edges.contains_key(&eri.edge_id) {
                        let refs = extract_eri_node_refs(eri);
                        let any_loaded = refs.iter().any(|id| self.nodes.contains_key(*id));
                        if any_loaded {
                            if let Some(edge) = self.try_load_edge(&eri.edge_id) {
                                self.edges.insert(eri.edge_id.clone(), edge);
                            }
                        }
                    }
                }
            }
        }

        if has_embed_index || self.graph_meta.is_some() {
            if !has_embed_index {
                self.rebuild_relation_indexes();
            }
            if let Some(ref meta) = self.graph_meta {
                let embed_host_index = meta.embed_host_index.clone();
                for ehi in &embed_host_index {
                    if !self.embeds.contains_key(&ehi.instance_id) {
                        let refs = extract_ehi_node_refs(ehi);
                        let any_loaded = refs.iter().any(|id| self.nodes.contains_key(*id));
                        if any_loaded {
                            if let Some(embed) = self.try_load_embed(&ehi.instance_id) {
                                self.embeds.insert(ehi.instance_id.clone(), embed);
                            }
                        }
                    }
                }
            }
        }
    }

    pub(in crate::starmap::store) fn load_remaining_objects(&mut self, _diagnostics: &mut Vec<LoadDiagnostic>) {
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

    pub(in crate::starmap::store) fn scan_objects_from_disk(&mut self, _diagnostics: &mut Vec<LoadDiagnostic>) {
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

    pub(in crate::starmap::store) fn scan_bucketed_dir_insert<F>(&mut self, subdir: &str, insert_fn: F)
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

    pub(in crate::starmap::store) fn detect_dangling_references(&self, diagnostics: &mut Vec<LoadDiagnostic>) {
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

    pub(in crate::starmap::store) fn detect_orphan_objects(&self, diagnostics: &mut Vec<LoadDiagnostic>) {
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

    pub(in crate::starmap::store) fn check_orphan_dir(&self, subdir: &str, declared_ids: &HashSet<&str>, object_type: &str, diagnostics: &mut Vec<LoadDiagnostic>) {
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
}
