use std::collections::{HashMap, HashSet};

use crate::error::Result;
use crate::starmap::package_storage;
use crate::starmap::types::*;

use super::meta::GraphMeta;
use super::relation_index::*;
use super::types::*;
use super::StarMapStore;

impl StarMapStore {
    pub(super) fn reload_graph_meta_if_stale(&mut self) {
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
        let mem_rev = self.graph_meta.as_ref().map(|m| m.package_revision).unwrap_or(0);
        if disk_meta.package_revision > mem_rev {
            self.graph_meta = Some(disk_meta);
            self.package_revision = self.graph_meta.as_ref().map(|m| m.package_revision).unwrap_or(0);
        }
    }

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
                        let last = self.recovery_log.last().cloned().unwrap();
                        if last.object_id == *hl_id {
                            diagnostics.push(last);
                            continue;
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
                        let last = self.recovery_log.last().cloned().unwrap();
                        if last.object_id == *link_id {
                            diagnostics.push(last);
                            continue;
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

    pub(super) fn ensure_graph_meta_initialized(&mut self) {
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

        self.package_revision = self.graph_meta.as_ref()
            .map(|m| m.package_revision)
            .unwrap_or(0);

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

    pub(super) fn load_graph_meta_phase(&mut self, diagnostics: &mut Vec<LoadDiagnostic>) {
        let graph_dir = self.starmap_dir();
        let graph_json_path = graph_dir.join("graph.json");

        if graph_json_path.exists() {
            let content = std::fs::read_to_string(&graph_json_path).unwrap_or_default();
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) {
                let schema_version_str = value.get("schemaVersion")
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
                        Ok(meta) => { self.graph_meta = Some(meta); }
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

    pub(super) fn load_viewport_objects(&mut self, diagnostics: &mut Vec<LoadDiagnostic>) {
        let viewport_node_ids: HashSet<String> = match (&self.layout, &self.viewport) {
            (Some(l), Some(vp)) => {
                let vp_left = vp.offset_x;
                let vp_top = vp.offset_y;
                let vp_right = vp.offset_x + vp.width / vp.scale;
                let vp_bottom = vp.offset_y + vp.height / vp.scale;
                l.nodes.iter()
                    .filter(|n| {
                        let node_left = n.x;
                        let node_top = n.y;
                        let node_right = n.x + n.width;
                        let node_bottom = n.y + n.height;
                        node_right > vp_left && node_left < vp_right
                            && node_bottom > vp_top && node_top < vp_bottom
                    })
                    .map(|n| n.node_id.clone())
                    .collect()
            }
            (Some(l), None) => {
                l.nodes.iter().map(|n| n.node_id.clone()).collect()
            }
            _ => HashSet::new(),
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

        let has_index = self.graph_meta.as_ref()
            .map(|m| !m.edge_relation_index.is_empty() || m.edge_ids.is_empty())
            .unwrap_or(false);

        if has_index {
            let edge_relation_index = self.graph_meta.as_ref().unwrap().edge_relation_index.clone();
            let embed_host_index = self.graph_meta.as_ref().unwrap().embed_host_index.clone();

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
        } else {
            self.rebuild_relation_indexes();
            self.load_viewport_objects(diagnostics);
            return;
        }

        let _ = diagnostics;
    }

    pub fn ensure_loaded(&mut self) -> Result<()> {
        if self.current_load_phase >= Some(LoadPhase::PrefetchNearbyObjects) {
            return Ok(());
        }
        self.load_phased(LoadPhase::PrefetchNearbyObjects)?;
        Ok(())
    }

    pub fn ensure_fully_loaded(&mut self) -> Result<()> {
        if self.current_load_phase >= Some(LoadPhase::BackgroundFullLoad) {
            return Ok(());
        }
        self.load_full()?;
        Ok(())
    }

    pub fn ensure_object_loaded(&mut self, node_id: &str) -> Result<()> {
        if self.nodes.contains_key(node_id) {
            return Ok(());
        }
        if let Some(node) = self.try_load_node(node_id) {
            self.nodes.insert(node_id.to_string(), node);
        }
        Ok(())
    }

    pub fn ensure_edge_loaded(&mut self, edge_id: &str) -> Result<()> {
        if self.edges.contains_key(edge_id) {
            return Ok(());
        }
        if let Some(edge) = self.try_load_edge(edge_id) {
            self.edges.insert(edge_id.to_string(), edge);
        }
        Ok(())
    }

    pub fn ensure_embed_loaded(&mut self, instance_id: &str) -> Result<()> {
        if self.embeds.contains_key(instance_id) {
            return Ok(());
        }
        if let Some(embed) = self.try_load_embed(instance_id) {
            self.embeds.insert(instance_id.to_string(), embed);
        }
        Ok(())
    }

    pub fn ensure_link_loaded(&mut self, link_id: &str) -> Result<()> {
        if self.links.contains_key(link_id) {
            return Ok(());
        }
        if let Some(link) = self.try_load_link(link_id) {
            self.links.insert(link_id.to_string(), link);
        }
        Ok(())
    }

    pub fn ensure_hyperlink_loaded(&mut self, hyperlink_id: &str) -> Result<()> {
        if self.hyperlinks.contains_key(hyperlink_id) {
            return Ok(());
        }
        if let Some(hl) = self.try_load_hyperlink(hyperlink_id) {
            self.hyperlinks.insert(hyperlink_id.to_string(), hl);
        }
        Ok(())
    }

    pub(super) fn rebuild_relation_indexes(&mut self) {
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

    pub(super) fn prefetch_nearby_objects(&mut self, _diagnostics: &mut Vec<LoadDiagnostic>) {
        let loaded_node_ids: HashSet<String> = self.nodes.keys().cloned().collect();
        let mut adjacent_node_ids: HashSet<String> = HashSet::new();

        let has_index = self.graph_meta.as_ref()
            .map(|m| !m.edge_relation_index.is_empty() || m.edge_ids.is_empty())
            .unwrap_or(false);

        if has_index {
            let edge_relation_index = self.graph_meta.as_ref().unwrap().edge_relation_index.clone();
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
        } else {
            self.rebuild_relation_indexes();
            self.prefetch_nearby_objects(_diagnostics);
            return;
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

        if has_edge_index {
            let edge_relation_index = self.graph_meta.as_ref().unwrap().edge_relation_index.clone();
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
        } else {
            self.rebuild_relation_indexes();
            let edge_relation_index = self.graph_meta.as_ref().unwrap().edge_relation_index.clone();
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

        if has_embed_index {
            let embed_host_index = self.graph_meta.as_ref().unwrap().embed_host_index.clone();
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
        } else {
            self.rebuild_relation_indexes();
            let embed_host_index = self.graph_meta.as_ref().unwrap().embed_host_index.clone();
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

    pub(super) fn load_remaining_objects(&mut self, _diagnostics: &mut Vec<LoadDiagnostic>) {
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

    pub(super) fn try_load_node(&mut self, node_id: &str) -> Option<StarMapNode> {
        let bucket_dir = self.starmap_dir().join("nodes").join(package_storage::bucket_for_id(node_id));
        let bucket_path = bucket_dir.join(format!("{}.json", node_id));
        let flat_path = self.starmap_dir().join("nodes").join(format!("{}.json", node_id));
        let (path, from_flat) = if bucket_path.exists() {
            (bucket_path.clone(), false)
        } else if flat_path.exists() {
            (flat_path.clone(), true)
        } else {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "node".to_string(),
                object_id: node_id.to_string(),
                detail: format!("node file not found: tried {} and {}", bucket_path.display(), flat_path.display()),
            });
            return None;
        };
        let content = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<StarMapNode>(&content) {
            Ok(node) => {
                if from_flat {
                    self.migrate_flat_to_bucket(&flat_path, &bucket_path);
                }
                Some(node)
            }
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

    pub(super) fn try_load_edge(&mut self, edge_id: &str) -> Option<StarMapEdge> {
        let bucket_dir = self.starmap_dir().join("edges").join(package_storage::bucket_for_id(edge_id));
        let bucket_path = bucket_dir.join(format!("{}.json", edge_id));
        let flat_path = self.starmap_dir().join("edges").join(format!("{}.json", edge_id));
        let (path, from_flat) = if bucket_path.exists() {
            (bucket_path.clone(), false)
        } else if flat_path.exists() {
            (flat_path.clone(), true)
        } else {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "edge".to_string(),
                object_id: edge_id.to_string(),
                detail: format!("edge file not found: tried {} and {}", bucket_path.display(), flat_path.display()),
            });
            return None;
        };
        let content = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<StarMapEdge>(&content) {
            Ok(edge) => {
                if from_flat {
                    self.migrate_flat_to_bucket(&flat_path, &bucket_path);
                }
                Some(edge)
            }
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

    pub(super) fn try_load_embed(&mut self, instance_id: &str) -> Option<StarMapEmbed> {
        let bucket_dir = self.starmap_dir().join("child_starmaps").join(package_storage::bucket_for_id(instance_id));
        let bucket_path = bucket_dir.join(format!("{}.json", instance_id));
        let flat_path = self.starmap_dir().join("child_starmaps").join(format!("{}.json", instance_id));
        let (path, from_flat) = if bucket_path.exists() {
            (bucket_path.clone(), false)
        } else if flat_path.exists() {
            (flat_path.clone(), true)
        } else {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "embed".to_string(),
                object_id: instance_id.to_string(),
                detail: format!("embed file not found: tried {} and {}", bucket_path.display(), flat_path.display()),
            });
            return None;
        };
        let content = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<StarMapEmbed>(&content) {
            Ok(embed) => {
                if from_flat {
                    self.migrate_flat_to_bucket(&flat_path, &bucket_path);
                }
                Some(embed)
            }
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

    pub(super) fn try_load_hyperlink(&mut self, hyperlink_id: &str) -> Option<StarMapHyperlink> {
        let bucket_dir = self.starmap_dir().join("hyperlinks").join(package_storage::bucket_for_id(hyperlink_id));
        let bucket_path = bucket_dir.join(format!("{}.json", hyperlink_id));
        let flat_path = self.starmap_dir().join("hyperlinks").join(format!("{}.json", hyperlink_id));
        let (path, from_flat) = if bucket_path.exists() {
            (bucket_path.clone(), false)
        } else if flat_path.exists() {
            (flat_path.clone(), true)
        } else {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "hyperlink".to_string(),
                object_id: hyperlink_id.to_string(),
                detail: format!("hyperlink file not found: tried {} and {}", bucket_path.display(), flat_path.display()),
            });
            return None;
        };
        let content = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<StarMapHyperlink>(&content) {
            Ok(hl) => {
                if from_flat {
                    self.migrate_flat_to_bucket(&flat_path, &bucket_path);
                }
                Some(hl)
            }
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

    pub(super) fn try_load_layout(&self) -> Option<StarMapLayout> {
        let dir = self.starmap_dir();
        if let Some(layout) = package_storage::load_layout_sharded(&dir) {
            return Some(layout);
        }
        if let Some(layout) = package_storage::load_legacy_layout(&dir) {
            if package_storage::save_layout_sharded(&dir, &layout).is_ok() {
                let legacy_path = dir.join("layouts").join("default.json");
                let _ = std::fs::remove_file(&legacy_path);
                self.record_migration("layout_sharded", "migrated legacy default.json to sharded format");
            }
            return Some(layout);
        }
        None
    }

    pub(super) fn try_load_link(&mut self, link_id: &str) -> Option<StarMapLink> {
        let bucket_dir = self.starmap_dir().join("links").join(package_storage::bucket_for_id(link_id));
        let bucket_path = bucket_dir.join(format!("{}.json", link_id));
        let flat_path = self.starmap_dir().join("links").join(format!("{}.json", link_id));
        let (path, from_flat) = if bucket_path.exists() {
            (bucket_path.clone(), false)
        } else if flat_path.exists() {
            (flat_path.clone(), true)
        } else {
            self.recovery_log.push(LoadDiagnostic {
                kind: LoadDiagnosticKind::Missing,
                object_type: "link".to_string(),
                object_id: link_id.to_string(),
                detail: format!("link file not found: tried {} and {}", bucket_path.display(), flat_path.display()),
            });
            return None;
        };
        let content = std::fs::read_to_string(&path).ok()?;
        match serde_json::from_str::<StarMapLink>(&content) {
            Ok(link) => {
                if from_flat {
                    self.migrate_flat_to_bucket(&flat_path, &bucket_path);
                }
                Some(link)
            }
            Err(e) => {
                self.recovery_log.push(LoadDiagnostic {
                    kind: LoadDiagnosticKind::Corrupt,
                    object_type: "link".to_string(),
                    object_id: link_id.to_string(),
                    detail: format!("parse error: {}", e),
                });
                None
            }
        }
    }

    pub(super) fn try_load_viewport(&self) -> Option<StarMapViewport> {
        package_storage::load_viewport(&self.workspace, &self.starmap_id)
    }

    pub(super) fn scan_objects_from_disk(&mut self, _diagnostics: &mut Vec<LoadDiagnostic>) {
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

    pub(super) fn scan_bucketed_dir_insert<F>(&mut self, subdir: &str, insert_fn: F)
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

    pub(super) fn detect_dangling_references(&self, diagnostics: &mut Vec<LoadDiagnostic>) {
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

    pub(super) fn detect_orphan_objects(&self, diagnostics: &mut Vec<LoadDiagnostic>) {
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

    pub(super) fn check_orphan_dir(&self, subdir: &str, declared_ids: &HashSet<&str>, object_type: &str, diagnostics: &mut Vec<LoadDiagnostic>) {
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
