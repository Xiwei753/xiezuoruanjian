use std::path::PathBuf;

use crate::error::Result;
use crate::starmap::package_storage;
use crate::storage::journal::workspace_change::{
    ensure_sync_tombstones_from_facts, SyncDeleteFact,
};

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
        // 事务化 flush：GraphMeta 作为 commit record 必须最后写入。
        // Phase 1 写对象文件/删对象文件，Phase 2 写 GraphMeta，Phase 3 清 dirty。
        // 任何 Phase 失败都不影响后续重试，因为 dirty/deleted 集合保持原样。
        //
        // 本轮待处理集合从 dirty/deleted 集合重新生成，save_queue 只做"需要 flush"的提示，
        // 不决定具体哪些对象参与。这样即使上一轮部分类型成功后从 save_queue 消失，
        // 下一轮重试时仍能从 dirty 集合重新发现它们。

        // 清空 save_queue，本轮从 dirty 集合重新生成待处理集合。
        self.save_queue.clear();

        let mut failed_types: Vec<String> = Vec::new();
        let mut changed_paths: Vec<PathBuf> = Vec::new();

        // 本次 flush 中成功写入/删除的对象集合，GraphMeta 成功后才统一清 dirty。
        let mut successful_writes = FlushDirtySet::default();
        let mut successful_deletes = FlushDirtySet::default();

        // Phase 1: 从 dirty/deleted 集合判断需要处理哪些类型，依次写入/删除对象文件。
        // 写成功时只记录到 successful_writes，不从正式 dirty 集合移除。
        // 删除成功时只记录到 successful_deletes，不从正式 deleted_*_ids 移除。
        // 失败时不 requeue——dirty 集合保持原样，下一轮会重新发现。

        if !self.dirty_nodes.is_empty() {
            let ids: Vec<String> = self.dirty_nodes.iter().cloned().collect();
            let mut succeeded = true;
            for node_id in &ids {
                if let Some(node) = self.nodes.get(node_id) {
                    match package_storage::save_node(&self.app_data_root, &self.starmap_id, node) {
                        Ok(rel_path) => {
                            changed_paths.push(rel_path);
                            successful_writes.nodes.insert(node_id.clone());
                        }
                        Err(_) => {
                            succeeded = false;
                            break;
                        }
                    }
                } else {
                    // 对象不在内存中（可能已删除），跳过但视为成功。
                    successful_writes.nodes.insert(node_id.clone());
                }
            }
            if !succeeded {
                failed_types.push("Node".to_string());
            }
        }

        if !self.dirty_edges.is_empty() {
            let ids: Vec<String> = self.dirty_edges.iter().cloned().collect();
            let mut succeeded = true;
            for edge_id in &ids {
                if let Some(edge) = self.edges.get(edge_id) {
                    match package_storage::save_edge(&self.app_data_root, &self.starmap_id, edge) {
                        Ok(rel_path) => {
                            changed_paths.push(rel_path);
                            successful_writes.edges.insert(edge_id.clone());
                        }
                        Err(_) => {
                            succeeded = false;
                            break;
                        }
                    }
                } else {
                    successful_writes.edges.insert(edge_id.clone());
                }
            }
            if !succeeded {
                failed_types.push("Edge".to_string());
            }
        }

        if !self.dirty_embeds.is_empty() {
            let ids: Vec<String> = self.dirty_embeds.iter().cloned().collect();
            let mut succeeded = true;
            for instance_id in &ids {
                if let Some(embed) = self.embeds.get(instance_id) {
                    match package_storage::save_embed(&self.app_data_root, &self.starmap_id, embed)
                    {
                        Ok(rel_path) => {
                            changed_paths.push(rel_path);
                            successful_writes.embeds.insert(instance_id.clone());
                        }
                        Err(_) => {
                            succeeded = false;
                            break;
                        }
                    }
                } else {
                    successful_writes.embeds.insert(instance_id.clone());
                }
            }
            if !succeeded {
                failed_types.push("Embed".to_string());
            }
        }

        if !self.dirty_links.is_empty() {
            let ids: Vec<String> = self.dirty_links.iter().cloned().collect();
            let mut succeeded = true;
            for link_id in &ids {
                if let Some(link) = self.links.get(link_id) {
                    match package_storage::save_link(&self.app_data_root, &self.starmap_id, link) {
                        Ok(rel_path) => {
                            changed_paths.push(rel_path);
                            successful_writes.links.insert(link_id.clone());
                        }
                        Err(_) => {
                            succeeded = false;
                            break;
                        }
                    }
                } else {
                    successful_writes.links.insert(link_id.clone());
                }
            }
            if !succeeded {
                failed_types.push("Link".to_string());
            }
        }

        if !self.dirty_hyperlinks.is_empty() {
            let ids: Vec<String> = self.dirty_hyperlinks.iter().cloned().collect();
            let mut succeeded = true;
            for hl_id in &ids {
                if let Some(hl) = self.hyperlinks.get(hl_id) {
                    match package_storage::save_hyperlink(&self.app_data_root, &self.starmap_id, hl)
                    {
                        Ok(rel_path) => {
                            changed_paths.push(rel_path);
                            successful_writes.hyperlinks.insert(hl_id.clone());
                        }
                        Err(_) => {
                            succeeded = false;
                            break;
                        }
                    }
                } else {
                    successful_writes.hyperlinks.insert(hl_id.clone());
                }
            }
            if !succeeded {
                failed_types.push("Hyperlink".to_string());
            }
        }

        // ── 对象级删除接 LWW tombstone ──
        // 在执行任何 delete_*_file 前，生成固定 trash 路径和完整 SyncDeleteFact，
        // 先 durable rename 到 trash（不直接 unlink），再幂等写 SyncState.tombstones。
        // 这样删一个节点，只要它之前进过 known_files，flush 后不会重新触发
        // "known file missing without tombstone"。
        let has_object_deletes = !self.deleted_node_ids.is_empty()
            || !self.deleted_edge_ids.is_empty()
            || !self.deleted_embed_ids.is_empty()
            || !self.deleted_link_ids.is_empty()
            || !self.deleted_hyperlink_ids.is_empty();

        let mut object_delete_facts: Vec<SyncDeleteFact> = Vec::new();
        let object_delete_trash_prefix: Option<String> = if has_object_deletes {
            let token = format!(
                "{}_{}_objects",
                chrono::Utc::now().timestamp_millis(),
                uuid::Uuid::new_v4()
            );
            Some(format!("sync/trash/{token}"))
        } else {
            None
        };

        if !self.deleted_node_ids.is_empty() {
            let ids: Vec<String> = self.deleted_node_ids.iter().cloned().collect();
            let trash_prefix = object_delete_trash_prefix.as_ref().ok_or_else(|| {
                crate::error::Error::Other(
                    "flush_save_queue: trash prefix missing for node deletes".to_string(),
                )
            })?;
            let trash_root = self.app_data_root.join(trash_prefix);
            let mut succeeded = true;
            for node_id in &ids {
                match package_storage::delete_node_file_to_trash(
                    &self.app_data_root,
                    &self.starmap_id,
                    node_id,
                    &trash_root,
                    trash_prefix,
                ) {
                    Ok(Some((orig_rel, trash_rel))) => {
                        changed_paths.push(PathBuf::from(&orig_rel));
                        object_delete_facts.push(SyncDeleteFact {
                            original_path: orig_rel,
                            original_hash: String::new(),
                            deleted_at: chrono::Utc::now().timestamp(),
                            deleted_by: String::new(),
                            trash_path: trash_rel,
                        });
                        successful_deletes.deleted_nodes.insert(node_id.clone());
                    }
                    Ok(None) => {
                        // 文件已不存在，视为成功（幂等）。
                        successful_deletes.deleted_nodes.insert(node_id.clone());
                    }
                    Err(e) => {
                        self.record_delete_failure("node", node_id, &e);
                        succeeded = false;
                        break;
                    }
                }
            }
            if !succeeded {
                failed_types.push("DeleteNode".to_string());
            }
        }

        if !self.deleted_edge_ids.is_empty() {
            let ids: Vec<String> = self.deleted_edge_ids.iter().cloned().collect();
            let trash_prefix = object_delete_trash_prefix.as_ref().ok_or_else(|| {
                crate::error::Error::Other(
                    "flush_save_queue: trash prefix missing for edge deletes".to_string(),
                )
            })?;
            let trash_root = self.app_data_root.join(trash_prefix);
            let mut succeeded = true;
            for edge_id in &ids {
                match package_storage::delete_edge_file_to_trash(
                    &self.app_data_root,
                    &self.starmap_id,
                    edge_id,
                    &trash_root,
                    trash_prefix,
                ) {
                    Ok(Some((orig_rel, trash_rel))) => {
                        changed_paths.push(PathBuf::from(&orig_rel));
                        object_delete_facts.push(SyncDeleteFact {
                            original_path: orig_rel,
                            original_hash: String::new(),
                            deleted_at: chrono::Utc::now().timestamp(),
                            deleted_by: String::new(),
                            trash_path: trash_rel,
                        });
                        successful_deletes.deleted_edges.insert(edge_id.clone());
                    }
                    Ok(None) => {
                        successful_deletes.deleted_edges.insert(edge_id.clone());
                    }
                    Err(e) => {
                        self.record_delete_failure("edge", edge_id, &e);
                        succeeded = false;
                        break;
                    }
                }
            }
            if !succeeded {
                failed_types.push("DeleteEdge".to_string());
            }
        }

        if !self.deleted_embed_ids.is_empty() {
            let ids: Vec<String> = self.deleted_embed_ids.iter().cloned().collect();
            let trash_prefix = object_delete_trash_prefix.as_ref().ok_or_else(|| {
                crate::error::Error::Other(
                    "flush_save_queue: trash prefix missing for embed deletes".to_string(),
                )
            })?;
            let trash_root = self.app_data_root.join(trash_prefix);
            let mut succeeded = true;
            for instance_id in &ids {
                match package_storage::delete_embed_file_to_trash(
                    &self.app_data_root,
                    &self.starmap_id,
                    instance_id,
                    &trash_root,
                    trash_prefix,
                ) {
                    Ok(Some((orig_rel, trash_rel))) => {
                        changed_paths.push(PathBuf::from(&orig_rel));
                        object_delete_facts.push(SyncDeleteFact {
                            original_path: orig_rel,
                            original_hash: String::new(),
                            deleted_at: chrono::Utc::now().timestamp(),
                            deleted_by: String::new(),
                            trash_path: trash_rel,
                        });
                        successful_deletes
                            .deleted_embeds
                            .insert(instance_id.clone());
                    }
                    Ok(None) => {
                        successful_deletes
                            .deleted_embeds
                            .insert(instance_id.clone());
                    }
                    Err(e) => {
                        self.record_delete_failure("embed", instance_id, &e);
                        succeeded = false;
                        break;
                    }
                }
            }
            if !succeeded {
                failed_types.push("DeleteEmbed".to_string());
            }
        }

        if !self.deleted_link_ids.is_empty() {
            let ids: Vec<String> = self.deleted_link_ids.iter().cloned().collect();
            let trash_prefix = object_delete_trash_prefix.as_ref().ok_or_else(|| {
                crate::error::Error::Other(
                    "flush_save_queue: trash prefix missing for link deletes".to_string(),
                )
            })?;
            let trash_root = self.app_data_root.join(trash_prefix);
            let mut succeeded = true;
            for link_id in &ids {
                match package_storage::delete_link_file_to_trash(
                    &self.app_data_root,
                    &self.starmap_id,
                    link_id,
                    &trash_root,
                    trash_prefix,
                ) {
                    Ok(Some((orig_rel, trash_rel))) => {
                        changed_paths.push(PathBuf::from(&orig_rel));
                        object_delete_facts.push(SyncDeleteFact {
                            original_path: orig_rel,
                            original_hash: String::new(),
                            deleted_at: chrono::Utc::now().timestamp(),
                            deleted_by: String::new(),
                            trash_path: trash_rel,
                        });
                        successful_deletes.deleted_links.insert(link_id.clone());
                    }
                    Ok(None) => {
                        successful_deletes.deleted_links.insert(link_id.clone());
                    }
                    Err(e) => {
                        self.record_delete_failure("link", link_id, &e);
                        succeeded = false;
                        break;
                    }
                }
            }
            if !succeeded {
                failed_types.push("DeleteLink".to_string());
            }
        }

        if !self.deleted_hyperlink_ids.is_empty() {
            let ids: Vec<String> = self.deleted_hyperlink_ids.iter().cloned().collect();
            let trash_prefix = object_delete_trash_prefix.as_ref().ok_or_else(|| {
                crate::error::Error::Other(
                    "flush_save_queue: trash prefix missing for hyperlink deletes".to_string(),
                )
            })?;
            let trash_root = self.app_data_root.join(trash_prefix);
            let mut succeeded = true;
            for hl_id in &ids {
                match package_storage::delete_hyperlink_file_to_trash(
                    &self.app_data_root,
                    &self.starmap_id,
                    hl_id,
                    &trash_root,
                    trash_prefix,
                ) {
                    Ok(Some((orig_rel, trash_rel))) => {
                        changed_paths.push(PathBuf::from(&orig_rel));
                        object_delete_facts.push(SyncDeleteFact {
                            original_path: orig_rel,
                            original_hash: String::new(),
                            deleted_at: chrono::Utc::now().timestamp(),
                            deleted_by: String::new(),
                            trash_path: trash_rel,
                        });
                        successful_deletes.deleted_hyperlinks.insert(hl_id.clone());
                    }
                    Ok(None) => {
                        successful_deletes.deleted_hyperlinks.insert(hl_id.clone());
                    }
                    Err(e) => {
                        self.record_delete_failure("hyperlink", hl_id, &e);
                        succeeded = false;
                        break;
                    }
                }
            }
            if !succeeded {
                failed_types.push("DeleteHyperlink".to_string());
            }
        }

        // 幂等写 SyncState.tombstones：对象文件已 durable rename 到 trash，
        // 现在补 LWW tombstone，避免 "known file missing without tombstone"。
        // tombstone 写入失败不算 flush 失败（tombstone 可由后续 sync 补齐），
        // 但记入 recovery_log 供诊断。
        if !object_delete_facts.is_empty() {
            if let Err(e) =
                ensure_sync_tombstones_from_facts(&self.app_data_root, &object_delete_facts)
            {
                log::warn!(
                    "flush_save_queue: ensure_sync_tombstones_from_facts failed for starmap {}: {} \
                     — object files moved to trash but tombstone not persisted; sync may report missing files",
                    self.starmap_id,
                    e
                );
                self.recovery_log.push(LoadDiagnostic {
                    kind: LoadDiagnosticKind::Corrupt,
                    object_type: "tombstone".to_string(),
                    object_id: self.starmap_id.clone(),
                    detail: format!("ensure_sync_tombstones_from_facts failed: {:?}", e),
                });
            }
        }

        // Phase 2: 只有 Phase 1 全部成功才写 GraphMeta（commit record）。
        // 如果 Phase 1 有失败，不写 GraphMeta。失败时不 requeue——dirty_graph_meta 保持 true，
        // 下一轮会重新发现。
        let graph_meta_succeeded = if !failed_types.is_empty() {
            false
        } else if self.dirty_graph_meta {
            self.reload_graph_meta_if_stale()?;
            // 用 successful_writes 和 successful_deletes 生成本次 revision，
            // 因为只有真正写成功的对象才应该获得新 revision。
            let flush_dirty = FlushDirtySet {
                nodes: successful_writes.nodes.clone(),
                edges: successful_writes.edges.clone(),
                embeds: successful_writes.embeds.clone(),
                links: successful_writes.links.clone(),
                hyperlinks: successful_writes.hyperlinks.clone(),
                deleted_nodes: successful_deletes.deleted_nodes.clone(),
                deleted_edges: successful_deletes.deleted_edges.clone(),
                deleted_embeds: successful_deletes.deleted_embeds.clone(),
                deleted_links: successful_deletes.deleted_links.clone(),
                deleted_hyperlinks: successful_deletes.deleted_hyperlinks.clone(),
            };
            match self.update_graph_meta_file(&flush_dirty) {
                Ok((written_revision, rel_path)) => {
                    self.package_revision = written_revision;
                    changed_paths.push(rel_path);
                    true
                }
                Err(_) => {
                    failed_types.push("GraphMeta".to_string());
                    false
                }
            }
        } else {
            true
        };

        // Phase 3: GraphMeta commit 成功后，统一从正式 dirty/deleted 集合中清掉 successful set。
        // 失败时不做任何 requeue，dirty 集合保持原样，下一轮 flush_save_queue 会重新发现。
        if failed_types.is_empty() && graph_meta_succeeded {
            for node_id in &successful_writes.nodes {
                self.dirty_nodes.remove(node_id);
            }
            for edge_id in &successful_writes.edges {
                self.dirty_edges.remove(edge_id);
            }
            for instance_id in &successful_writes.embeds {
                self.dirty_embeds.remove(instance_id);
            }
            for link_id in &successful_writes.links {
                self.dirty_links.remove(link_id);
            }
            for hl_id in &successful_writes.hyperlinks {
                self.dirty_hyperlinks.remove(hl_id);
            }
            for node_id in &successful_deletes.deleted_nodes {
                self.deleted_node_ids.remove(node_id);
            }
            for edge_id in &successful_deletes.deleted_edges {
                self.deleted_edge_ids.remove(edge_id);
            }
            for instance_id in &successful_deletes.deleted_embeds {
                self.deleted_embed_ids.remove(instance_id);
            }
            for link_id in &successful_deletes.deleted_links {
                self.deleted_link_ids.remove(link_id);
            }
            for hl_id in &successful_deletes.deleted_hyperlinks {
                self.deleted_hyperlink_ids.remove(hl_id);
            }
            self.dirty_graph_meta = false;
        }

        if self.has_pending_deletes() || self.has_pending_writes() || !self.recovery_log.is_empty()
        {
            let recovery_path = self.flush_recovery_to_disk()?;
            changed_paths.push(recovery_path);
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

    pub fn flush(&mut self) -> Result<Vec<PathBuf>> {
        // 将所有 dirty/delete kind 入队，然后走统一的事务化 flush_save_queue。
        if !self.dirty_nodes.is_empty() {
            self.enqueue_save(SaveQueueEntry::Node);
        }
        if !self.dirty_edges.is_empty() {
            self.enqueue_save(SaveQueueEntry::Edge);
        }
        if !self.dirty_embeds.is_empty() {
            self.enqueue_save(SaveQueueEntry::Embed);
        }
        if !self.dirty_links.is_empty() {
            self.enqueue_save(SaveQueueEntry::Link);
        }
        if !self.dirty_hyperlinks.is_empty() {
            self.enqueue_save(SaveQueueEntry::Hyperlink);
        }
        if self.has_pending_deletes() {
            self.enqueue_save(SaveQueueEntry::DeleteNode);
            self.enqueue_save(SaveQueueEntry::DeleteEdge);
            self.enqueue_save(SaveQueueEntry::DeleteEmbed);
            self.enqueue_save(SaveQueueEntry::DeleteLink);
            self.enqueue_save(SaveQueueEntry::DeleteHyperlink);
        }
        if self.dirty_graph_meta {
            self.enqueue_save(SaveQueueEntry::GraphMeta);
        }
        self.flush_save_queue()
    }
}
