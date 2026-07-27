use std::collections::VecDeque;

use crate::error::Result;
use crate::starmap::package_storage;

use super::types::*;
use super::StarMapStore;

impl StarMapStore {
    pub fn save_queue_len(&self) -> usize {
        self.save_queue.len()
    }

    pub fn enqueue_save(&mut self, entry: SaveQueueEntry) {
        if !self.save_queue.iter().any(|e| std::mem::discriminant(e) == std::mem::discriminant(&entry)) {
            self.save_queue.push_back(entry);
        }
    }

    pub fn drain_save_queue(&mut self) -> Vec<SaveQueueEntry> {
        self.save_queue.drain(..).collect()
    }

    pub fn flush_save_queue(&mut self) -> Result<()> {
        let mut remaining: VecDeque<SaveQueueEntry> = VecDeque::new();
        let mut any_processed = false;
        let mut failed_types: Vec<String> = Vec::new();
        while let Some(entry) = self.save_queue.pop_front() {
            let mut succeeded = true;
            any_processed = true;
            match entry {
                SaveQueueEntry::Node => {
                    let ids: Vec<String> = self.dirty_nodes.iter().cloned().collect();
                    for node_id in &ids {
                        if let Some(node) = self.nodes.get(node_id) {
                            if package_storage::save_node(&self.workspace, &self.starmap_id, node).is_err() {
                                succeeded = false;
                                break;
                            }
                        }
                        self.dirty_nodes.remove(node_id);
                    }
                }
                SaveQueueEntry::Edge => {
                    let ids: Vec<String> = self.dirty_edges.iter().cloned().collect();
                    for edge_id in &ids {
                        if let Some(edge) = self.edges.get(edge_id) {
                            if package_storage::save_edge(&self.workspace, &self.starmap_id, edge).is_err() {
                                succeeded = false;
                                break;
                            }
                        }
                        self.dirty_edges.remove(edge_id);
                    }
                }
                SaveQueueEntry::Embed => {
                    let ids: Vec<String> = self.dirty_embeds.iter().cloned().collect();
                    for instance_id in &ids {
                        if let Some(embed) = self.embeds.get(instance_id) {
                            if package_storage::save_embed(&self.workspace, &self.starmap_id, embed).is_err() {
                                succeeded = false;
                                break;
                            }
                        }
                        self.dirty_embeds.remove(instance_id);
                    }
                }
                SaveQueueEntry::Link => {
                    let ids: Vec<String> = self.dirty_links.iter().cloned().collect();
                    for link_id in &ids {
                        if let Some(link) = self.links.get(link_id) {
                            if package_storage::save_link(&self.workspace, &self.starmap_id, link).is_err() {
                                succeeded = false;
                                break;
                            }
                        }
                        self.dirty_links.remove(link_id);
                    }
                }
                SaveQueueEntry::Hyperlink => {
                    let ids: Vec<String> = self.dirty_hyperlinks.iter().cloned().collect();
                    for hl_id in &ids {
                        if let Some(hl) = self.hyperlinks.get(hl_id) {
                            if package_storage::save_hyperlink(&self.workspace, &self.starmap_id, hl).is_err() {
                                succeeded = false;
                                break;
                            }
                        }
                        self.dirty_hyperlinks.remove(hl_id);
                    }
                }
                SaveQueueEntry::Layout => {
                    if self.dirty_layout {
                        if let Some(ref layout) = self.layout {
                            if package_storage::save_layout(&self.workspace, &self.starmap_id, layout).is_err() {
                                succeeded = false;
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
                            Ok(written_revision) => {
                                self.dirty_graph_meta = false;
                                self.package_revision = written_revision;
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
                        match package_storage::delete_node_file(&self.workspace, &self.starmap_id, node_id) {
                            Ok(()) => { self.deleted_node_ids.remove(node_id); }
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
                        match package_storage::delete_edge_file(&self.workspace, &self.starmap_id, edge_id) {
                            Ok(()) => { self.deleted_edge_ids.remove(edge_id); }
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
                        match package_storage::delete_embed_file(&self.workspace, &self.starmap_id, instance_id) {
                            Ok(()) => { self.deleted_embed_ids.remove(instance_id); }
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
                        match package_storage::delete_link_file(&self.workspace, &self.starmap_id, link_id) {
                            Ok(()) => { self.deleted_link_ids.remove(link_id); }
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
                        match package_storage::delete_hyperlink_file(&self.workspace, &self.starmap_id, hl_id) {
                            Ok(()) => { self.deleted_hyperlink_ids.remove(hl_id); }
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
            self.flush_recovery_to_disk()?;
        }

        if any_processed && all_flushed {
            let node_count = self.graph_meta.as_ref()
                .map(|m| m.node_ids.len() as u32)
                .unwrap_or(self.nodes.len() as u32);
            let edge_count = self.graph_meta.as_ref()
                .map(|m| m.edge_ids.len() as u32)
                .unwrap_or(self.edges.len() as u32);
            let linked_chapters = self.graph_meta.as_ref()
                .map(|m| *m.node_kind_counts.get("Chapter").unwrap_or(&0))
                .unwrap_or(0u32);
            crate::starmap::update_starmap_stats(
                &self.workspace,
                &self.starmap_id,
                node_count,
                edge_count,
                linked_chapters,
            )?;
        }

        if !failed_types.is_empty() {
            return Err(crate::error::Error::SaveQueueFlushIncomplete {
                failed_types,
                remaining_queue_len: self.save_queue.len(),
            });
        }

        Ok(())
    }

    pub(super) fn record_delete_failure(&mut self, object_type: &str, object_id: &str, error: &crate::error::Error) {
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

    pub(super) fn has_pending_writes(&self) -> bool {
        self.is_dirty() || self.dirty_graph_meta
    }

    pub fn flush(&mut self) -> Result<()> {
        for node_id in &self.dirty_nodes {
            if let Some(node) = self.nodes.get(node_id) {
                package_storage::save_node(&self.workspace, &self.starmap_id, node)?;
            }
        }

        for edge_id in &self.dirty_edges {
            if let Some(edge) = self.edges.get(edge_id) {
                package_storage::save_edge(&self.workspace, &self.starmap_id, edge)?;
            }
        }

        for instance_id in &self.dirty_embeds {
            if let Some(embed) = self.embeds.get(instance_id) {
                package_storage::save_embed(&self.workspace, &self.starmap_id, embed)?;
            }
        }

        for link_id in &self.dirty_links {
            if let Some(link) = self.links.get(link_id) {
                package_storage::save_link(&self.workspace, &self.starmap_id, link)?;
            }
        }

        for hl_id in &self.dirty_hyperlinks {
            if let Some(hl) = self.hyperlinks.get(hl_id) {
                package_storage::save_hyperlink(&self.workspace, &self.starmap_id, hl)?;
            }
        }

        if self.dirty_layout {
            if let Some(ref layout) = self.layout {
                package_storage::save_layout(&self.workspace, &self.starmap_id, layout)?;
            }
        }

        let node_ids_to_delete: Vec<String> = self.deleted_node_ids.iter().cloned().collect();
        for node_id in &node_ids_to_delete {
            match package_storage::delete_node_file(&self.workspace, &self.starmap_id, node_id) {
                Ok(()) => { self.deleted_node_ids.remove(node_id); }
                Err(e) => {
                    self.record_delete_failure("node", node_id, &e);
                    self.flush_recovery_to_disk()?;
                    return Err(e);
                }
            }
        }

        let edge_ids_to_delete: Vec<String> = self.deleted_edge_ids.iter().cloned().collect();
        for edge_id in &edge_ids_to_delete {
            match package_storage::delete_edge_file(&self.workspace, &self.starmap_id, edge_id) {
                Ok(()) => { self.deleted_edge_ids.remove(edge_id); }
                Err(e) => {
                    self.record_delete_failure("edge", edge_id, &e);
                    self.flush_recovery_to_disk()?;
                    return Err(e);
                }
            }
        }

        let embed_ids_to_delete: Vec<String> = self.deleted_embed_ids.iter().cloned().collect();
        for instance_id in &embed_ids_to_delete {
            match package_storage::delete_embed_file(&self.workspace, &self.starmap_id, instance_id) {
                Ok(()) => { self.deleted_embed_ids.remove(instance_id); }
                Err(e) => {
                    self.record_delete_failure("embed", instance_id, &e);
                    self.flush_recovery_to_disk()?;
                    return Err(e);
                }
            }
        }

        let link_ids_to_delete: Vec<String> = self.deleted_link_ids.iter().cloned().collect();
        for link_id in &link_ids_to_delete {
            match package_storage::delete_link_file(&self.workspace, &self.starmap_id, link_id) {
                Ok(()) => { self.deleted_link_ids.remove(link_id); }
                Err(e) => {
                    self.record_delete_failure("link", link_id, &e);
                    self.flush_recovery_to_disk()?;
                    return Err(e);
                }
            }
        }

        let hl_ids_to_delete: Vec<String> = self.deleted_hyperlink_ids.iter().cloned().collect();
        for hl_id in &hl_ids_to_delete {
            match package_storage::delete_hyperlink_file(&self.workspace, &self.starmap_id, hl_id) {
                Ok(()) => { self.deleted_hyperlink_ids.remove(hl_id); }
                Err(e) => {
                    self.record_delete_failure("hyperlink", hl_id, &e);
                    self.flush_recovery_to_disk()?;
                    return Err(e);
                }
            }
        }

        let written_revision = self.update_graph_meta_file()?;
        self.package_revision = written_revision;

        let node_count = self.graph_meta.as_ref()
            .map(|m| m.node_ids.len() as u32)
            .unwrap_or(self.nodes.len() as u32);
        let edge_count = self.graph_meta.as_ref()
            .map(|m| m.edge_ids.len() as u32)
            .unwrap_or(self.edges.len() as u32);
        let linked_chapters = self.graph_meta.as_ref()
            .map(|m| *m.node_kind_counts.get("Chapter").unwrap_or(&0))
            .unwrap_or(0u32);
        crate::starmap::update_starmap_stats(
            &self.workspace,
            &self.starmap_id,
            node_count,
            edge_count,
            linked_chapters,
        )?;

        self.dirty_nodes.clear();
        self.dirty_edges.clear();
        self.dirty_embeds.clear();
        self.dirty_links.clear();
        self.dirty_hyperlinks.clear();
        self.dirty_layout = false;
        self.dirty_graph_meta = false;

        self.flush_recovery_to_disk()?;

        Ok(())
    }

    pub fn flush_viewport(&self) -> Result<()> {
        if let Some(ref viewport) = self.viewport {
            package_storage::save_viewport(&self.workspace, &self.starmap_id, viewport)?;
        }
        Ok(())
    }
}
