use std::collections::VecDeque;
use std::path::PathBuf;

use crate::error::Result;
use crate::starmap::package_storage;

use super::super::types::*;
use super::super::StarMapStore;

impl StarMapStore {
    pub fn save_queue_len(&self) -> usize {
        self.save_queue.len()
    }

    pub fn enqueue_save(&mut self, entry: SaveQueueEntry) {
        if !self
            .save_queue
            .iter()
            .any(|e| std::mem::discriminant(e) == std::mem::discriminant(&entry))
        {
            self.save_queue.push_back(entry);
        }
    }

    pub fn drain_save_queue(&mut self) -> Vec<SaveQueueEntry> {
        self.save_queue.drain(..).collect()
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn flush_save_queue(&mut self) -> Result<Vec<PathBuf>> {
        let mut remaining: VecDeque<SaveQueueEntry> = VecDeque::new();
        let mut any_processed = false;
        let mut failed_types: Vec<String> = Vec::new();
        let mut changed_paths: Vec<PathBuf> = Vec::new();
        while let Some(entry) = self.save_queue.pop_front() {
            let mut succeeded = true;
            any_processed = true;
            match entry {
                SaveQueueEntry::Node => {
                    let ids: Vec<String> = self.dirty_nodes.iter().cloned().collect();
                    for node_id in &ids {
                        if let Some(node) = self.nodes.get(node_id) {
                            match package_storage::save_node(
                                &self.app_data_root,
                                &self.starmap_id,
                                node,
                            ) {
                                Ok(rel_path) => changed_paths.push(rel_path),
                                Err(_) => {
                                    succeeded = false;
                                    break;
                                }
                            }
                        }
                        self.dirty_nodes.remove(node_id);
                    }
                }
                SaveQueueEntry::Edge => {
                    let ids: Vec<String> = self.dirty_edges.iter().cloned().collect();
                    for edge_id in &ids {
                        if let Some(edge) = self.edges.get(edge_id) {
                            match package_storage::save_edge(
                                &self.app_data_root,
                                &self.starmap_id,
                                edge,
                            ) {
                                Ok(rel_path) => changed_paths.push(rel_path),
                                Err(_) => {
                                    succeeded = false;
                                    break;
                                }
                            }
                        }
                        self.dirty_edges.remove(edge_id);
                    }
                }
                SaveQueueEntry::Embed => {
                    let ids: Vec<String> = self.dirty_embeds.iter().cloned().collect();
                    for instance_id in &ids {
                        if let Some(embed) = self.embeds.get(instance_id) {
                            match package_storage::save_embed(
                                &self.app_data_root,
                                &self.starmap_id,
                                embed,
                            ) {
                                Ok(rel_path) => changed_paths.push(rel_path),
                                Err(_) => {
                                    succeeded = false;
                                    break;
                                }
                            }
                        }
                        self.dirty_embeds.remove(instance_id);
                    }
                }
                SaveQueueEntry::Link => {
                    let ids: Vec<String> = self.dirty_links.iter().cloned().collect();
                    for link_id in &ids {
                        if let Some(link) = self.links.get(link_id) {
                            match package_storage::save_link(
                                &self.app_data_root,
                                &self.starmap_id,
                                link,
                            ) {
                                Ok(rel_path) => changed_paths.push(rel_path),
                                Err(_) => {
                                    succeeded = false;
                                    break;
                                }
                            }
                        }
                        self.dirty_links.remove(link_id);
                    }
                }
                SaveQueueEntry::Hyperlink => {
                    let ids: Vec<String> = self.dirty_hyperlinks.iter().cloned().collect();
                    for hl_id in &ids {
                        if let Some(hl) = self.hyperlinks.get(hl_id) {
                            match package_storage::save_hyperlink(
                                &self.app_data_root,
                                &self.starmap_id,
                                hl,
                            ) {
                                Ok(rel_path) => changed_paths.push(rel_path),
                                Err(_) => {
                                    succeeded = false;
                                    break;
                                }
                            }
                        }
                        self.dirty_hyperlinks.remove(hl_id);
                    }
                }
                SaveQueueEntry::Layout => {
                    if self.dirty_layout {
                        if let Some(ref layout) = self.layout {
                            match package_storage::save_layout(
                                &self.app_data_root,
                                &self.starmap_id,
                                layout,
                            ) {
                                Ok(paths) => changed_paths.extend(paths),
                                Err(_) => {
                                    succeeded = false;
                                }
                            }
                        }
                        if succeeded {
                            self.dirty_layout = false;
                        }
                    }
                }
                SaveQueueEntry::GraphMeta => {
                    if self.dirty_graph_meta {
                        self.reload_graph_meta_if_stale();
                        match self.update_graph_meta_file() {
                            Ok((written_revision, rel_path)) => {
                                self.dirty_graph_meta = false;
                                self.package_revision = written_revision;
                                changed_paths.push(rel_path);
                            }
                            Err(_) => {
                                succeeded = false;
                            }
                        }
                    }
                }
                SaveQueueEntry::DeleteNode => {
                    let ids: Vec<String> = self.deleted_node_ids.iter().cloned().collect();
                    for node_id in &ids {
                        match package_storage::delete_node_file(
                            &self.app_data_root,
                            &self.starmap_id,
                            node_id,
                        ) {
                            Ok(paths) => {
                                changed_paths.extend(paths);
                                self.deleted_node_ids.remove(node_id);
                            }
                            Err(e) => {
                                self.record_delete_failure("node", node_id, &e);
                                succeeded = false;
                                break;
                            }
                        }
                    }
                }
                SaveQueueEntry::DeleteEdge => {
                    let ids: Vec<String> = self.deleted_edge_ids.iter().cloned().collect();
                    for edge_id in &ids {
                        match package_storage::delete_edge_file(
                            &self.app_data_root,
                            &self.starmap_id,
                            edge_id,
                        ) {
                            Ok(paths) => {
                                changed_paths.extend(paths);
                                self.deleted_edge_ids.remove(edge_id);
                            }
                            Err(e) => {
                                self.record_delete_failure("edge", edge_id, &e);
                                succeeded = false;
                                break;
                            }
                        }
                    }
                }
                SaveQueueEntry::DeleteEmbed => {
                    let ids: Vec<String> = self.deleted_embed_ids.iter().cloned().collect();
                    for instance_id in &ids {
                        match package_storage::delete_embed_file(
                            &self.app_data_root,
                            &self.starmap_id,
                            instance_id,
                        ) {
                            Ok(paths) => {
                                changed_paths.extend(paths);
                                self.deleted_embed_ids.remove(instance_id);
                            }
                            Err(e) => {
                                self.record_delete_failure("embed", instance_id, &e);
                                succeeded = false;
                                break;
                            }
                        }
                    }
                }
                SaveQueueEntry::DeleteLink => {
                    let ids: Vec<String> = self.deleted_link_ids.iter().cloned().collect();
                    for link_id in &ids {
                        match package_storage::delete_link_file(
                            &self.app_data_root,
                            &self.starmap_id,
                            link_id,
                        ) {
                            Ok(paths) => {
                                changed_paths.extend(paths);
                                self.deleted_link_ids.remove(link_id);
                            }
                            Err(e) => {
                                self.record_delete_failure("link", link_id, &e);
                                succeeded = false;
                                break;
                            }
                        }
                    }
                }
                SaveQueueEntry::DeleteHyperlink => {
                    let ids: Vec<String> = self.deleted_hyperlink_ids.iter().cloned().collect();
                    for hl_id in &ids {
                        match package_storage::delete_hyperlink_file(
                            &self.app_data_root,
                            &self.starmap_id,
                            hl_id,
                        ) {
                            Ok(paths) => {
                                changed_paths.extend(paths);
                                self.deleted_hyperlink_ids.remove(hl_id);
                            }
                            Err(e) => {
                                self.record_delete_failure("hyperlink", hl_id, &e);
                                succeeded = false;
                                break;
                            }
                        }
                    }
                }
            }
            if !succeeded {
                failed_types.push(format!("{:?}", entry));
                remaining.push_back(entry);
            }
        }
        self.save_queue = remaining;

        let all_flushed = !self.is_dirty() && !self.dirty_graph_meta && !self.has_pending_deletes();

        if self.has_pending_deletes() || self.has_pending_writes() {
            let recovery_path = self.flush_recovery_to_disk()?;
            changed_paths.push(recovery_path);
        }

        if any_processed && all_flushed {
            let node_count: u32 = self
                .graph_meta
                .as_ref()
                .map(|m| m.node_ids.len().try_into().unwrap_or(u32::MAX))
                .unwrap_or_else(|| self.nodes.len().try_into().unwrap_or(u32::MAX));
            let edge_count: u32 = self
                .graph_meta
                .as_ref()
                .map(|m| m.edge_ids.len().try_into().unwrap_or(u32::MAX))
                .unwrap_or_else(|| self.edges.len().try_into().unwrap_or(u32::MAX));
            let linked_chapters = self
                .graph_meta
                .as_ref()
                .map(|m| *m.node_kind_counts.get("Chapter").unwrap_or(&0))
                .unwrap_or(0u32);
            let stats_paths = crate::starmap::update_starmap_stats(
                &self.app_data_root,
                &self.starmap_id,
                node_count,
                edge_count,
                linked_chapters,
            )?;
            changed_paths.extend(stats_paths);
        }

        if !failed_types.is_empty() {
            return Err(crate::error::Error::SaveQueueFlushIncomplete {
                failed_types,
                remaining_queue_len: self.save_queue.len(),
            });
        }

        Ok(changed_paths)
    }

    pub(in crate::starmap::store) fn record_delete_failure(
        &mut self,
        object_type: &str,
        object_id: &str,
        error: &crate::error::Error,
    ) {
        self.recovery_log.push(LoadDiagnostic {
            kind: LoadDiagnosticKind::Corrupt,
            object_type: object_type.to_string(),
            object_id: object_id.to_string(),
            detail: format!("delete failed: {:?}", error),
        });
    }

    pub fn has_pending_deletes(&self) -> bool {
        !self.deleted_node_ids.is_empty()
            || !self.deleted_edge_ids.is_empty()
            || !self.deleted_embed_ids.is_empty()
            || !self.deleted_link_ids.is_empty()
            || !self.deleted_hyperlink_ids.is_empty()
    }

    pub(in crate::starmap::store) fn has_pending_writes(&self) -> bool {
        self.is_dirty() || self.dirty_graph_meta
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn flush(&mut self) -> Result<Vec<PathBuf>> {
        let mut changed_paths: Vec<PathBuf> = Vec::new();

        for node_id in &self.dirty_nodes {
            if let Some(node) = self.nodes.get(node_id) {
                let rel_path =
                    package_storage::save_node(&self.app_data_root, &self.starmap_id, node)?;
                changed_paths.push(rel_path);
            }
        }

        for edge_id in &self.dirty_edges {
            if let Some(edge) = self.edges.get(edge_id) {
                let rel_path =
                    package_storage::save_edge(&self.app_data_root, &self.starmap_id, edge)?;
                changed_paths.push(rel_path);
            }
        }

        for instance_id in &self.dirty_embeds {
            if let Some(embed) = self.embeds.get(instance_id) {
                let rel_path =
                    package_storage::save_embed(&self.app_data_root, &self.starmap_id, embed)?;
                changed_paths.push(rel_path);
            }
        }

        for link_id in &self.dirty_links {
            if let Some(link) = self.links.get(link_id) {
                let rel_path =
                    package_storage::save_link(&self.app_data_root, &self.starmap_id, link)?;
                changed_paths.push(rel_path);
            }
        }

        for hl_id in &self.dirty_hyperlinks {
            if let Some(hl) = self.hyperlinks.get(hl_id) {
                let rel_path =
                    package_storage::save_hyperlink(&self.app_data_root, &self.starmap_id, hl)?;
                changed_paths.push(rel_path);
            }
        }

        if self.dirty_layout {
            if let Some(ref layout) = self.layout {
                let paths =
                    package_storage::save_layout(&self.app_data_root, &self.starmap_id, layout)?;
                changed_paths.extend(paths);
            }
        }

        let node_ids_to_delete: Vec<String> = self.deleted_node_ids.iter().cloned().collect();
        for node_id in &node_ids_to_delete {
            match package_storage::delete_node_file(&self.app_data_root, &self.starmap_id, node_id)
            {
                Ok(paths) => {
                    changed_paths.extend(paths);
                    self.deleted_node_ids.remove(node_id);
                }
                Err(e) => {
                    self.record_delete_failure("node", node_id, &e);
                    let recovery_path = self.flush_recovery_to_disk()?;
                    changed_paths.push(recovery_path);
                    return Err(e);
                }
            }
        }

        let edge_ids_to_delete: Vec<String> = self.deleted_edge_ids.iter().cloned().collect();
        for edge_id in &edge_ids_to_delete {
            match package_storage::delete_edge_file(&self.app_data_root, &self.starmap_id, edge_id)
            {
                Ok(paths) => {
                    changed_paths.extend(paths);
                    self.deleted_edge_ids.remove(edge_id);
                }
                Err(e) => {
                    self.record_delete_failure("edge", edge_id, &e);
                    let recovery_path = self.flush_recovery_to_disk()?;
                    changed_paths.push(recovery_path);
                    return Err(e);
                }
            }
        }

        let embed_ids_to_delete: Vec<String> = self.deleted_embed_ids.iter().cloned().collect();
        for instance_id in &embed_ids_to_delete {
            match package_storage::delete_embed_file(
                &self.app_data_root,
                &self.starmap_id,
                instance_id,
            ) {
                Ok(paths) => {
                    changed_paths.extend(paths);
                    self.deleted_embed_ids.remove(instance_id);
                }
                Err(e) => {
                    self.record_delete_failure("embed", instance_id, &e);
                    let recovery_path = self.flush_recovery_to_disk()?;
                    changed_paths.push(recovery_path);
                    return Err(e);
                }
            }
        }

        let link_ids_to_delete: Vec<String> = self.deleted_link_ids.iter().cloned().collect();
        for link_id in &link_ids_to_delete {
            match package_storage::delete_link_file(&self.app_data_root, &self.starmap_id, link_id)
            {
                Ok(paths) => {
                    changed_paths.extend(paths);
                    self.deleted_link_ids.remove(link_id);
                }
                Err(e) => {
                    self.record_delete_failure("link", link_id, &e);
                    let recovery_path = self.flush_recovery_to_disk()?;
                    changed_paths.push(recovery_path);
                    return Err(e);
                }
            }
        }

        let hl_ids_to_delete: Vec<String> = self.deleted_hyperlink_ids.iter().cloned().collect();
        for hl_id in &hl_ids_to_delete {
            match package_storage::delete_hyperlink_file(
                &self.app_data_root,
                &self.starmap_id,
                hl_id,
            ) {
                Ok(paths) => {
                    changed_paths.extend(paths);
                    self.deleted_hyperlink_ids.remove(hl_id);
                }
                Err(e) => {
                    self.record_delete_failure("hyperlink", hl_id, &e);
                    let recovery_path = self.flush_recovery_to_disk()?;
                    changed_paths.push(recovery_path);
                    return Err(e);
                }
            }
        }

        let (written_revision, graph_meta_path) = self.update_graph_meta_file()?;
        self.package_revision = written_revision;
        changed_paths.push(graph_meta_path);

        let node_count: u32 = self
            .graph_meta
            .as_ref()
            .map(|m| m.node_ids.len().try_into().unwrap_or(u32::MAX))
            .unwrap_or_else(|| self.nodes.len().try_into().unwrap_or(u32::MAX));
        let edge_count: u32 = self
            .graph_meta
            .as_ref()
            .map(|m| m.edge_ids.len().try_into().unwrap_or(u32::MAX))
            .unwrap_or_else(|| self.edges.len().try_into().unwrap_or(u32::MAX));
        let linked_chapters = self
            .graph_meta
            .as_ref()
            .map(|m| *m.node_kind_counts.get("Chapter").unwrap_or(&0))
            .unwrap_or(0u32);
        let stats_paths = crate::starmap::update_starmap_stats(
            &self.app_data_root,
            &self.starmap_id,
            node_count,
            edge_count,
            linked_chapters,
        )?;
        changed_paths.extend(stats_paths);

        self.dirty_nodes.clear();
        self.dirty_edges.clear();
        self.dirty_embeds.clear();
        self.dirty_links.clear();
        self.dirty_hyperlinks.clear();
        self.dirty_layout = false;
        self.dirty_graph_meta = false;

        let recovery_path = self.flush_recovery_to_disk()?;
        changed_paths.push(recovery_path);

        Ok(changed_paths)
    }

    pub fn flush_viewport(&self) -> Result<Vec<PathBuf>> {
        let mut changed_paths: Vec<PathBuf> = Vec::new();
        if let Some(ref viewport) = self.viewport {
            let rel_path =
                package_storage::save_viewport(&self.app_data_root, &self.starmap_id, viewport)?;
            changed_paths.push(rel_path);
        }
        Ok(changed_paths)
    }
}
