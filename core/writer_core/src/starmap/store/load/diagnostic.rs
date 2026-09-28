use std::collections::{HashMap, HashSet};

use crate::error::Result;
use crate::starmap::graph::resolve::{resolve_target, GraphResolverContext};
use crate::starmap::semantic::StarMapTargetResolveStatus;
use crate::starmap::types::*;

use super::super::relation_index::*;
use super::super::types::*;
use super::super::StarMapStore;

impl StarMapStore {
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn list_hyperlinks_with_diagnostics(
        &mut self,
    ) -> Result<ListWithDiagnostics<StarMapHyperlink>> {
        self.reload_graph_meta_if_stale()?;
        let hl_ids = self.graph_meta_hyperlink_ids();
        let mut items = Vec::new();
        let mut diagnostics = Vec::new();
        for hl_id in &hl_ids {
            if !self.hyperlinks.contains_key(hl_id) {
                match self.try_load_hyperlink(hl_id) {
                    Ok(hl) => {
                        self.hyperlinks.insert(hl_id.clone(), hl);
                    }
                    Err(e) => {
                        // 保持 ListWithDiagnostics 语义：单个坏对象不让整个列表失败。
                        // try_load 已 push diagnostic 到 recovery_log，这里同步到 diagnostics。
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
                            kind: LoadDiagnosticKind::Corrupt,
                            object_type: "hyperlink".to_string(),
                            object_id: hl_id.clone(),
                            detail: format!("{}", e),
                        });
                        continue;
                    }
                }
            }
            if let Some(hl) = self.hyperlinks.get(hl_id).cloned() {
                items.push(hl);
            }
        }
        Ok(ListWithDiagnostics { items, diagnostics })
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn list_links_with_diagnostics(&mut self) -> Result<ListWithDiagnostics<StarMapLink>> {
        self.reload_graph_meta_if_stale()?;
        let link_ids = self.graph_meta_link_ids();
        let mut items = Vec::new();
        let mut diagnostics = Vec::new();
        for link_id in &link_ids {
            if !self.links.contains_key(link_id) {
                match self.try_load_link(link_id) {
                    Ok(link) => {
                        self.links.insert(link_id.clone(), link);
                    }
                    Err(e) => {
                        // 保持 ListWithDiagnostics 语义：单个坏对象不让整个列表失败。
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
                            kind: LoadDiagnosticKind::Corrupt,
                            object_type: "link".to_string(),
                            object_id: link_id.clone(),
                            detail: format!("{}", e),
                        });
                        continue;
                    }
                }
            }
            if let Some(link) = self.links.get(link_id).cloned() {
                items.push(link);
            }
        }
        Ok(ListWithDiagnostics { items, diagnostics })
    }

    pub fn graph_meta_hyperlink_ids(&self) -> Vec<String> {
        self.graph_meta
            .as_ref()
            .map(|m| m.hyperlink_ids.clone())
            .unwrap_or_default()
    }

    pub fn graph_meta_link_ids(&self) -> Vec<String> {
        self.graph_meta
            .as_ref()
            .map(|m| m.link_ids.clone())
            .unwrap_or_default()
    }

    pub fn diagnostics(&self) -> &[LoadDiagnostic] {
        &self.recovery_log
    }

    pub fn current_load_phase(&self) -> Option<LoadPhase> {
        self.current_load_phase
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub(in crate::starmap::store) fn rebuild_relation_indexes(&mut self) -> Result<()> {
        let edge_ids = self
            .graph_meta
            .as_ref()
            .map(|m| m.edge_ids.clone())
            .unwrap_or_default();
        let embed_ids = self
            .graph_meta
            .as_ref()
            .map(|m| m.embed_instance_ids.clone())
            .unwrap_or_default();
        let link_ids = self
            .graph_meta
            .as_ref()
            .map(|m| m.link_ids.clone())
            .unwrap_or_default();
        let hl_ids = self
            .graph_meta
            .as_ref()
            .map(|m| m.hyperlink_ids.clone())
            .unwrap_or_default();

        let mut edge_relation_index = Vec::new();
        for edge_id in &edge_ids {
            if !self.edges.contains_key(edge_id) {
                let edge = self.try_load_edge(edge_id)?;
                self.edges.insert(edge_id.clone(), edge);
            }
            if let Some(edge) = self.edges.get(edge_id) {
                edge_relation_index.push(EdgeRelationIndex {
                    edge_id: edge.id.clone(),
                    from: edge.from.clone(),
                    to: edge.to.clone(),
                });
            }
        }

        let mut embed_host_index = Vec::new();
        for instance_id in &embed_ids {
            if !self.embeds.contains_key(instance_id) {
                let embed = self.try_load_embed(instance_id)?;
                self.embeds.insert(instance_id.clone(), embed);
            }
            if let Some(embed) = self.embeds.get(instance_id) {
                embed_host_index.push(EmbedHostIndex {
                    instance_id: embed.instance_id.clone(),
                    host_path: embed.host_path.clone(),
                });
            }
        }

        let mut link_relation_index = Vec::new();
        for link_id in &link_ids {
            if !self.links.contains_key(link_id) {
                let link = self.try_load_link(link_id)?;
                self.links.insert(link_id.clone(), link);
            }
            if let Some(link) = self.links.get(link_id) {
                link_relation_index.push(LinkRelationIndex {
                    link_id: link.link_id.clone(),
                    source: link.source.clone(),
                    target: link.target.clone(),
                });
            }
        }

        let mut hyperlink_relation_index = Vec::new();
        for hl_id in &hl_ids {
            if !self.hyperlinks.contains_key(hl_id) {
                let hl = self.try_load_hyperlink(hl_id)?;
                self.hyperlinks.insert(hl_id.clone(), hl);
            }
            if let Some(hl) = self.hyperlinks.get(hl_id) {
                hyperlink_relation_index.push(HyperlinkRelationIndex {
                    hyperlink_id: hl.hyperlink_id.clone(),
                    source_node_id: target_path_node_id(&hl.source, &self.starmap_id)
                        .unwrap_or_default()
                        .to_string(),
                });
            }
        }

        let mut node_kind_counts = HashMap::new();
        for node in self.nodes.values() {
            *node_kind_counts
                .entry(format!("{:?}", node.kind))
                .or_insert(0u32) += 1;
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
        Ok(())
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub(in crate::starmap::store) fn prefetch_nearby_objects(
        &mut self,
        _diagnostics: &mut Vec<LoadDiagnostic>,
    ) -> Result<()> {
        let loaded_node_ids: HashSet<String> = self.nodes.keys().cloned().collect();
        let mut adjacent_node_ids: HashSet<String> = HashSet::new();

        let has_index = self
            .graph_meta
            .as_ref()
            .map(|m| !m.edge_relation_index.is_empty() || m.edge_ids.is_empty())
            .unwrap_or(false);

        let mut has_index_after_rebuild = has_index;
        if !has_index {
            self.rebuild_relation_indexes()?;
            has_index_after_rebuild = self
                .graph_meta
                .as_ref()
                .map(|m| !m.edge_relation_index.is_empty() || m.edge_ids.is_empty())
                .unwrap_or(false);
        }

        if has_index_after_rebuild {
            if let Some(meta) = self.graph_meta.as_ref() {
                let edge_relation_index = meta.edge_relation_index.clone();
                // clone 成员列表，让 meta 借用提前释放。
                let meta_node_ids: std::collections::HashSet<String> =
                    meta.node_ids.iter().cloned().collect();
                for eri in &edge_relation_index {
                    let refs = extract_eri_node_refs(eri, &self.starmap_id);
                    for node_id in &refs {
                        if loaded_node_ids.contains(*node_id) {
                            for other_id in &refs {
                                if other_id != node_id
                                    && !self.nodes.contains_key(*other_id)
                                    && !other_id.is_empty()
                                    && meta_node_ids.contains(*other_id)
                                {
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
                let node = self.try_load_node(node_id)?;
                self.nodes.insert(node_id.clone(), node);
            }
        }

        let has_edge_index = self
            .graph_meta
            .as_ref()
            .map(|m| !m.edge_relation_index.is_empty() || m.edge_ids.is_empty())
            .unwrap_or(false);
        let has_embed_index = self
            .graph_meta
            .as_ref()
            .map(|m| !m.embed_host_index.is_empty() || m.embed_instance_ids.is_empty())
            .unwrap_or(false);

        if has_edge_index || self.graph_meta.is_some() {
            if !has_edge_index {
                self.rebuild_relation_indexes()?;
            }
            if let Some(ref meta) = self.graph_meta {
                let edge_relation_index = meta.edge_relation_index.clone();
                // clone 成员列表，让 meta 借用提前释放。
                let meta_edge_ids: std::collections::HashSet<String> =
                    meta.edge_ids.iter().cloned().collect();
                for eri in &edge_relation_index {
                    // 严格服从 GraphMeta 成员列表：只加载 meta.edge_ids 声明的 edge。
                    if !meta_edge_ids.contains(&eri.edge_id) {
                        continue;
                    }
                    if !self.edges.contains_key(&eri.edge_id) {
                        let refs = extract_eri_node_refs(eri, &self.starmap_id);
                        let any_loaded = refs.iter().any(|id| self.nodes.contains_key(*id));
                        if any_loaded {
                            let edge = self.try_load_edge(&eri.edge_id)?;
                            self.edges.insert(eri.edge_id.clone(), edge);
                        }
                    }
                }
            }
        }

        if has_embed_index || self.graph_meta.is_some() {
            if !has_embed_index {
                self.rebuild_relation_indexes()?;
            }
            if let Some(ref meta) = self.graph_meta {
                let embed_host_index = meta.embed_host_index.clone();
                // clone 成员列表，让 meta 借用提前释放。
                let meta_embed_ids: std::collections::HashSet<String> =
                    meta.embed_instance_ids.iter().cloned().collect();
                for ehi in &embed_host_index {
                    // 严格服从 GraphMeta 成员列表：只加载 meta.embed_instance_ids 声明的 embed。
                    if !meta_embed_ids.contains(&ehi.instance_id) {
                        continue;
                    }
                    if !self.embeds.contains_key(&ehi.instance_id) {
                        let refs = extract_ehi_node_refs(ehi, &self.starmap_id);
                        let any_loaded = refs.iter().any(|id| self.nodes.contains_key(*id));
                        if any_loaded {
                            let embed = self.try_load_embed(&ehi.instance_id)?;
                            self.embeds.insert(ehi.instance_id.clone(), embed);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub(in crate::starmap::store) fn load_remaining_objects(
        &mut self,
        _diagnostics: &mut Vec<LoadDiagnostic>,
    ) -> Result<()> {
        let all_node_ids = self
            .graph_meta
            .as_ref()
            .map(|m| m.node_ids.clone())
            .unwrap_or_default();
        let all_edge_ids = self
            .graph_meta
            .as_ref()
            .map(|m| m.edge_ids.clone())
            .unwrap_or_default();
        let all_embed_ids = self
            .graph_meta
            .as_ref()
            .map(|m| m.embed_instance_ids.clone())
            .unwrap_or_default();
        let all_hl_ids = self
            .graph_meta
            .as_ref()
            .map(|m| m.hyperlink_ids.clone())
            .unwrap_or_default();
        let all_link_ids = self
            .graph_meta
            .as_ref()
            .map(|m| m.link_ids.clone())
            .unwrap_or_default();

        for node_id in &all_node_ids {
            if !self.nodes.contains_key(node_id) {
                let node = self.try_load_node(node_id)?;
                self.nodes.insert(node_id.clone(), node);
            }
        }
        for edge_id in &all_edge_ids {
            if !self.edges.contains_key(edge_id) {
                let edge = self.try_load_edge(edge_id)?;
                self.edges.insert(edge_id.clone(), edge);
            }
        }
        for instance_id in &all_embed_ids {
            if !self.embeds.contains_key(instance_id) {
                let embed = self.try_load_embed(instance_id)?;
                self.embeds.insert(instance_id.clone(), embed);
            }
        }
        for hl_id in &all_hl_ids {
            if !self.hyperlinks.contains_key(hl_id) {
                let hl = self.try_load_hyperlink(hl_id)?;
                self.hyperlinks.insert(hl_id.clone(), hl);
            }
        }
        for link_id in &all_link_ids {
            if !self.links.contains_key(link_id) {
                let link = self.try_load_link(link_id)?;
                self.links.insert(link_id.clone(), link);
            }
        }
        Ok(())
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub(in crate::starmap::store) fn detect_dangling_references(
        &self,
        diagnostics: &mut Vec<LoadDiagnostic>,
    ) {
        // 构造 resolver context：当前完整图作为 overlay，其他图从磁盘读取。
        let mut overlays = std::collections::HashMap::new();
        overlays.insert(self.starmap_id.clone(), self.to_starmap_graph());
        let context = GraphResolverContext {
            app_data_root: self.app_data_root.clone(),
            overlays,
        };

        // 嵌套辅助函数：对一条引用路径做 resolve，失败时推入 DanglingReference diagnostic。
        fn check_path(
            context: &GraphResolverContext,
            diagnostics: &mut Vec<LoadDiagnostic>,
            path: &StarMapTargetPath,
            object_type: &str,
            object_id: &str,
            endpoint: &str,
        ) {
            if let Err(status) = resolve_target(context, path) {
                let detail = format!(
                    "{} {} {}",
                    endpoint,
                    object_id,
                    match status {
                        StarMapTargetResolveStatus::MissingStarmap => {
                            "references non-existent starmap"
                        }
                        StarMapTargetResolveStatus::MissingNode => {
                            "references non-existent node"
                        }
                        StarMapTargetResolveStatus::MissingAnchor => {
                            "references non-existent anchor"
                        }
                        StarMapTargetResolveStatus::MissingEmbed => {
                            "references non-existent embed"
                        }
                        StarMapTargetResolveStatus::MissingPortal => {
                            "references non-existent portal"
                        }
                        StarMapTargetResolveStatus::CycleDetected => "contains a cycle",
                        StarMapTargetResolveStatus::InvalidRange => "has invalid range",
                        StarMapTargetResolveStatus::TooDeep => "path too deep",
                        StarMapTargetResolveStatus::UnsupportedVersion => {
                            "target starmap has unsupported schema version"
                        }
                        StarMapTargetResolveStatus::CorruptStarmap => "target starmap is corrupt",
                        StarMapTargetResolveStatus::ReadFailed => "target starmap read failed",
                        StarMapTargetResolveStatus::Unresolved => "unresolved",
                        StarMapTargetResolveStatus::Resolved => "resolved",
                    }
                );
                diagnostics.push(LoadDiagnostic {
                    kind: LoadDiagnosticKind::DanglingReference,
                    object_type: object_type.to_string(),
                    object_id: object_id.to_string(),
                    detail,
                });
            }
        }

        // owned path 检查：起点 starmap_id 必须等于宿主图，否则直接记 DanglingReference。
        //
        // 写入 validation 已要求 `path.starmap_id == graph.starmap_id`，但 load diagnostics
        // 直接 resolve_target。若磁盘坏数据写成 `path.starmap_id = B`，只要 B 上目标存在，
        // resolver 返回 Resolved，diagnostics 反而认为正常。这里在 resolve 前先校验起点。
        fn check_owned_path(
            context: &GraphResolverContext,
            diagnostics: &mut Vec<LoadDiagnostic>,
            host_starmap_id: &str,
            path: &StarMapTargetPath,
            object_type: &str,
            object_id: &str,
            endpoint: &str,
        ) {
            if path.starmap_id != host_starmap_id {
                diagnostics.push(LoadDiagnostic {
                    kind: LoadDiagnosticKind::DanglingReference,
                    object_type: object_type.to_string(),
                    object_id: object_id.to_string(),
                    detail: format!(
                        "{} {} invalid host path: starmap_id '{}' does not match host graph '{}'",
                        endpoint, object_id, path.starmap_id, host_starmap_id
                    ),
                });
                return;
            }
            check_path(context, diagnostics, path, object_type, object_id, endpoint);
        }

        let host_starmap_id = self.starmap_id.clone();
        for edge in self.edges.values() {
            check_owned_path(
                &context,
                diagnostics,
                &host_starmap_id,
                &edge.from,
                "edge",
                &edge.id,
                "edge from",
            );
            check_owned_path(
                &context,
                diagnostics,
                &host_starmap_id,
                &edge.to,
                "edge",
                &edge.id,
                "edge to",
            );
        }
        for embed in self.embeds.values() {
            // host_path 属于当前图，走 owned path 检查（起点必须等于宿主图）。
            check_owned_path(
                &context,
                diagnostics,
                &host_starmap_id,
                &embed.host_path,
                "embed",
                &embed.instance_id,
                "embed host_path",
            );
            // target_starmap_id 是 embed 真正嵌进去的目标图，属于直接目标（非 owned path），
            // 和 Portal destination 一样构造 synthetic path 检查目标星图是否存在。
            let target_path = StarMapTargetPath {
                starmap_id: embed.target_starmap_id.clone(),
                segments: vec![],
                target: crate::starmap::semantic::StarMapTargetDetail::Starmap,
            };
            check_path(
                &context,
                diagnostics,
                &target_path,
                "embed",
                &embed.instance_id,
                "embed target_starmap_id",
            );
        }
        for link in self.links.values() {
            check_owned_path(
                &context,
                diagnostics,
                &host_starmap_id,
                &link.source,
                "link",
                &link.link_id,
                "link source",
            );
            check_owned_path(
                &context,
                diagnostics,
                &host_starmap_id,
                &link.target,
                "link",
                &link.link_id,
                "link target",
            );
        }
        for hl in self.hyperlinks.values() {
            check_owned_path(
                &context,
                diagnostics,
                &host_starmap_id,
                &hl.source,
                "hyperlink",
                &hl.hyperlink_id,
                "hyperlink source",
            );
        }
        // Portal destination：构造 StarMapTargetPath 检查目标星图和落点。
        // Portal destination 是直接目标，不属于 owned path 起点规则，用 check_path。
        for node in self.nodes.values() {
            if let Some(portal) = &node.portal {
                let target = portal
                    .destination_target
                    .clone()
                    .unwrap_or(crate::starmap::semantic::StarMapTargetDetail::Starmap);
                let path = StarMapTargetPath {
                    starmap_id: portal.destination_starmap_id.clone(),
                    segments: vec![],
                    target,
                };
                check_path(
                    &context,
                    diagnostics,
                    &path,
                    "node",
                    &node.id,
                    "node portal destination",
                );
            }
        }
    }

    pub(in crate::starmap::store) fn detect_orphan_objects(
        &self,
        diagnostics: &mut Vec<LoadDiagnostic>,
    ) {
        let declared_node_ids: HashSet<&str> = self
            .graph_meta
            .as_ref()
            .map(|m| m.node_ids.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();
        let declared_edge_ids: HashSet<&str> = self
            .graph_meta
            .as_ref()
            .map(|m| m.edge_ids.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();
        let declared_embed_ids: HashSet<&str> = self
            .graph_meta
            .as_ref()
            .map(|m| m.embed_instance_ids.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();
        let declared_hl_ids: HashSet<&str> = self
            .graph_meta
            .as_ref()
            .map(|m| m.hyperlink_ids.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();
        let declared_link_ids: HashSet<&str> = self
            .graph_meta
            .as_ref()
            .map(|m| m.link_ids.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();

        self.check_orphan_dir("nodes", &declared_node_ids, "node", diagnostics);
        self.check_orphan_dir("edges", &declared_edge_ids, "edge", diagnostics);
        self.check_orphan_dir("embeds", &declared_embed_ids, "embed", diagnostics);
        self.check_orphan_dir("hyperlinks", &declared_hl_ids, "hyperlink", diagnostics);
        self.check_orphan_dir("links", &declared_link_ids, "link", diagnostics);
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub(in crate::starmap::store) fn check_orphan_dir(
        &self,
        subdir: &str,
        declared_ids: &HashSet<&str>,
        object_type: &str,
        diagnostics: &mut Vec<LoadDiagnostic>,
    ) {
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
