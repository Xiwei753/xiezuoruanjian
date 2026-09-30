use std::path::PathBuf;

use crate::error::Result;
use crate::starmap::package_storage;
use crate::storage::journal::starmap_object_delete::{
    PlannedStarMapObjectDelete, StarMapObjectDeletePhase, StarMapObjectDeleteTarget,
    StarMapObjectKind,
};
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

        // ── 对象级删除 durable transaction（Issue #805 评论 5912394108）──
        // 在执行任何 rename 前：
        // 1. 从 deleted_*_ids 固定本轮删除集合
        // 2. 根据 ID 算出所有原文件路径（不碰磁盘）
        // 3. 一次加载 SyncState，用 known_files 填真实 original_hash，用真实 device_id 填 deleted_by
        // 4. 生成固定 trash path
        // 5. durable 写 journal（phase=Planned）
        // 6. 逐个 durable_rename
        // 7. 写 tombstone（失败返回 Err，不写 GraphMeta，不清 deleted_*_ids）
        // 8. 更新 journal phase=Tombstoned
        let has_object_deletes = !self.deleted_node_ids.is_empty()
            || !self.deleted_edge_ids.is_empty()
            || !self.deleted_embed_ids.is_empty()
            || !self.deleted_link_ids.is_empty()
            || !self.deleted_hyperlink_ids.is_empty();

        // 标记对象删除 journal 是否已 durable 写入并活跃。
        // 供 Phase 2 GraphMeta 成功后更新 phase=GraphMetaWritten、
        // Phase 3 清集合后删除 journal 用。
        let mut object_delete_journal_active = false;
        let mut object_delete_facts_for_tombstone: Vec<SyncDeleteFact> = Vec::new();

        if has_object_deletes {
            //   同进程重试改为 resume：先检查已有 journal。有则复用其
            // trash_token/trash_rel_path/sync_delete_facts，不生成新 token，
            // 不覆盖 journal（save_planned 也拒绝静默覆盖）。没有才生成新 plan。
            // 这避免第一次 flush 落盘旧 token journal、文件移到旧 trash、
            // tombstone 写失败后，同进程再次 flush 生成新 token 覆盖旧 journal、
            // 新 plan 指向不存在的新 trash 位置、旧恢复事实被覆盖。
            let existing_journal =
                PlannedStarMapObjectDelete::load(&self.app_data_root, &self.starmap_id)?;

            let trash_rel_path: String;

            if let Some(existing) = existing_journal {
                // resume：用 existing 的固定 trash 路径和 facts，不生成新 token。
                log::debug!(
                    "flush_save_queue: resuming existing object delete journal for starmap {} \
                     (token={}, phase={:?}) — reusing fixed trash path and facts, not \
                     overwriting",
                    self.starmap_id,
                    existing.token,
                    existing.phase
                );
                trash_rel_path = existing.trash_rel_path.clone();
                object_delete_journal_active = true;
                object_delete_facts_for_tombstone = existing.sync_delete_facts.clone();
            } else {
                // 没有已有 journal，生成新 plan。
                // 1. 从 deleted_*_ids 固定本轮删除集合，计算 original_path（不碰磁盘）。
                //    original_path 格式与 package_storage::delete_*_file_to_trash 返回的
                //    orig_rel 一致，供 SyncState.known_files 查找 hash。
                let starmap_id = &self.starmap_id;
                let mut targets: Vec<StarMapObjectDeleteTarget> = Vec::new();
                for node_id in &self.deleted_node_ids {
                    let rel = format!(
                        "starmaps/{starmap_id}/nodes/{}/{node_id}.json",
                        package_storage::bucket_for_id(node_id)
                    );
                    targets.push(StarMapObjectDeleteTarget {
                        kind: StarMapObjectKind::Node,
                        id: node_id.clone(),
                        original_path: rel,
                    });
                }
                for edge_id in &self.deleted_edge_ids {
                    let rel = format!(
                        "starmaps/{starmap_id}/edges/{}/{edge_id}.json",
                        package_storage::bucket_for_id(edge_id)
                    );
                    targets.push(StarMapObjectDeleteTarget {
                        kind: StarMapObjectKind::Edge,
                        id: edge_id.clone(),
                        original_path: rel,
                    });
                }
                for instance_id in &self.deleted_embed_ids {
                    let rel = format!(
                        "starmaps/{starmap_id}/embeds/{}/{instance_id}.json",
                        package_storage::bucket_for_id(instance_id)
                    );
                    targets.push(StarMapObjectDeleteTarget {
                        kind: StarMapObjectKind::Embed,
                        id: instance_id.clone(),
                        original_path: rel,
                    });
                }
                for link_id in &self.deleted_link_ids {
                    let rel = format!(
                        "starmaps/{starmap_id}/links/{}/{link_id}.json",
                        package_storage::bucket_for_id(link_id)
                    );
                    targets.push(StarMapObjectDeleteTarget {
                        kind: StarMapObjectKind::Link,
                        id: link_id.clone(),
                        original_path: rel,
                    });
                }
                for hl_id in &self.deleted_hyperlink_ids {
                    let rel = format!(
                        "starmaps/{starmap_id}/hyperlinks/{}/{hl_id}.json",
                        package_storage::bucket_for_id(hl_id)
                    );
                    targets.push(StarMapObjectDeleteTarget {
                        kind: StarMapObjectKind::Hyperlink,
                        id: hl_id.clone(),
                        original_path: rel,
                    });
                }

                // 2. 一次加载 SyncState，用 known_files 填真实 original_hash，
                //    用真实 device_id 填 deleted_by。生成固定 trash path。
                //    不再用空字符串填 original_hash 和 deleted_by。
                let trash_token = format!(
                    "{}_{}_objects",
                    chrono::Utc::now().timestamp_millis(),
                    uuid::Uuid::new_v4()
                );
                trash_rel_path = format!("sync/trash/{trash_token}");

                let (sync_facts, plan_prepared) =
                    match crate::sync::SyncService::load_sync_state(&self.app_data_root) {
                        Ok(state) => {
                            let device_id = state.device_id.clone();
                            let now = chrono::Utc::now().timestamp();
                            let facts: Vec<SyncDeleteFact> = targets
                                .iter()
                                .map(|t| {
                                    let original_hash = state
                                        .known_files
                                        .get(&t.original_path)
                                        .cloned()
                                        .unwrap_or_default();
                                    let trash_path =
                                        format!("{trash_rel_path}/{}", t.original_path);
                                    SyncDeleteFact {
                                        original_path: t.original_path.clone(),
                                        original_hash,
                                        deleted_at: now,
                                        deleted_by: device_id.clone(),
                                        trash_path,
                                    }
                                })
                                .collect();
                            (facts, true)
                        }
                        Err(e) => {
                            log::warn!(
                                "flush_save_queue: load_sync_state failed for starmap {}: {} \
                                 — cannot prepare object delete facts, skipping deletes",
                                self.starmap_id,
                                e
                            );
                            self.recovery_log.push(LoadDiagnostic {
                                kind: LoadDiagnosticKind::Corrupt,
                                object_type: "sync_state".to_string(),
                                object_id: self.starmap_id.clone(),
                                detail: format!(
                                    "load_sync_state for object delete plan failed: {:?}",
                                    e
                                ),
                            });
                            failed_types.push("ObjectDeletePlan".to_string());
                            (Vec::new(), false)
                        }
                    };

                // 3. durable 写 journal（phase=Planned），然后才逐个 durable_rename。
                //    不再在 rename 后临时构造 SyncDeleteFact，而是在 rename 前就
                //    构造好完整 facts 并 durable 写入 journal。
                if plan_prepared && !sync_facts.is_empty() {
                    let plan = PlannedStarMapObjectDelete {
                        token: trash_token.clone(),
                        starmap_id: self.starmap_id.clone(),
                        trash_rel_path: trash_rel_path.clone(),
                        objects: targets.clone(),
                        sync_delete_facts: sync_facts.clone(),
                        phase: StarMapObjectDeletePhase::Planned,
                    };
                    match PlannedStarMapObjectDelete::save_planned(&self.app_data_root, &plan) {
                        Ok(()) => {
                            object_delete_journal_active = true;
                            object_delete_facts_for_tombstone = sync_facts.clone();
                        }
                        Err(e) => {
                            log::warn!(
                                "flush_save_queue: save_planned journal failed for starmap {}: \
                                 {} — cannot guarantee durable delete, skipping deletes",
                                self.starmap_id,
                                e
                            );
                            self.recovery_log.push(LoadDiagnostic {
                                kind: LoadDiagnosticKind::Corrupt,
                                object_type: "object_delete_journal".to_string(),
                                object_id: self.starmap_id.clone(),
                                detail: format!("save_planned failed: {:?}", e),
                            });
                            failed_types.push("ObjectDeleteJournal".to_string());
                        }
                    }
                }
            }

            // 4. 逐个 durable_rename（只有 journal 写成功才执行）。
            if object_delete_journal_active {
                let trash_root = self.app_data_root.join(&trash_rel_path);

                if !self.deleted_node_ids.is_empty() {
                    let ids: Vec<String> = self.deleted_node_ids.iter().cloned().collect();
                    let mut succeeded = true;
                    for node_id in &ids {
                        match package_storage::delete_node_file_to_trash(
                            &self.app_data_root,
                            &self.starmap_id,
                            node_id,
                            &trash_root,
                            &trash_rel_path,
                        ) {
                            Ok(Some((orig_rel, _))) => {
                                changed_paths.push(PathBuf::from(&orig_rel));
                                successful_deletes.deleted_nodes.insert(node_id.clone());
                            }
                            Ok(None) => {
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
                    let mut succeeded = true;
                    for edge_id in &ids {
                        match package_storage::delete_edge_file_to_trash(
                            &self.app_data_root,
                            &self.starmap_id,
                            edge_id,
                            &trash_root,
                            &trash_rel_path,
                        ) {
                            Ok(Some((orig_rel, _))) => {
                                changed_paths.push(PathBuf::from(&orig_rel));
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
                    let mut succeeded = true;
                    for instance_id in &ids {
                        match package_storage::delete_embed_file_to_trash(
                            &self.app_data_root,
                            &self.starmap_id,
                            instance_id,
                            &trash_root,
                            &trash_rel_path,
                        ) {
                            Ok(Some((orig_rel, _))) => {
                                changed_paths.push(PathBuf::from(&orig_rel));
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
                    let mut succeeded = true;
                    for link_id in &ids {
                        match package_storage::delete_link_file_to_trash(
                            &self.app_data_root,
                            &self.starmap_id,
                            link_id,
                            &trash_root,
                            &trash_rel_path,
                        ) {
                            Ok(Some((orig_rel, _))) => {
                                changed_paths.push(PathBuf::from(&orig_rel));
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
                    let mut succeeded = true;
                    for hl_id in &ids {
                        match package_storage::delete_hyperlink_file_to_trash(
                            &self.app_data_root,
                            &self.starmap_id,
                            hl_id,
                            &trash_root,
                            &trash_rel_path,
                        ) {
                            Ok(Some((orig_rel, _))) => {
                                changed_paths.push(PathBuf::from(&orig_rel));
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

                // 5. 写 SyncState.tombstones（关键修复）。
                //    只有所有 rename 成功且有 facts 才写 tombstone。
                //    tombstone 写失败时 **必须返回 Err**，不能只写 recovery_log
                //    然后继续往下写 GraphMeta 并清 deleted_*_ids——
                //    这等于 tombstone 写失败时主动把可重试状态清掉。
                let rename_failed = failed_types.iter().any(|t| {
                    t == "DeleteNode"
                        || t == "DeleteEdge"
                        || t == "DeleteEmbed"
                        || t == "DeleteLink"
                        || t == "DeleteHyperlink"
                });
                if !rename_failed && !object_delete_facts_for_tombstone.is_empty() {
                    match ensure_sync_tombstones_from_facts(
                        &self.app_data_root,
                        &object_delete_facts_for_tombstone,
                    ) {
                        Ok(()) => {
                            // 6. 更新 journal phase=Tombstoned。
                            //    tombstone 已 durable 写入，后续 GraphMeta 成功后
                            //    再更新到 GraphMetaWritten。
                            if let Err(e) = PlannedStarMapObjectDelete::update_phase(
                                &self.app_data_root,
                                &self.starmap_id,
                                StarMapObjectDeletePhase::Tombstoned,
                            ) {
                                log::warn!(
                                    "flush_save_queue: update_phase to Tombstoned failed \
                                     for starmap {}: {} — tombstone already persisted, \
                                     journal phase update is best-effort",
                                    self.starmap_id,
                                    e
                                );
                            }
                        }
                        Err(e) => {
                            // tombstone 写失败：必须返回 Err，不写 GraphMeta，不清 deleted_*_ids。
                            // 将 "Tombstone" 加入 failed_types，Phase 2 检查 failed_types
                            // 非空就不会写 GraphMeta，Phase 3 也不会清集合。
                            // journal 保持 phase=Planned，重启后 bootstrap 可重新执行
                            // rename（幂等）+ tombstone。
                            log::error!(
                                "flush_save_queue: ensure_sync_tombstones_from_facts failed \
                                 for starmap {}: {} — object files moved to trash but tombstone \
                                 not persisted; refusing to write GraphMeta or clear deleted ids",
                                self.starmap_id,
                                e
                            );
                            self.recovery_log.push(LoadDiagnostic {
                                kind: LoadDiagnosticKind::Corrupt,
                                object_type: "tombstone".to_string(),
                                object_id: self.starmap_id.clone(),
                                detail: format!(
                                    "ensure_sync_tombstones_from_facts failed: {:?}",
                                    e
                                ),
                            });
                            failed_types.push("Tombstone".to_string());
                        }
                    }
                }
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
                    // 对象删除 journal 活跃时，GraphMeta 成功后更新
                    // phase=GraphMetaWritten。Phase 3 清集合后会删 journal。
                    if object_delete_journal_active {
                        if let Err(e) = PlannedStarMapObjectDelete::update_phase(
                            &self.app_data_root,
                            &self.starmap_id,
                            StarMapObjectDeletePhase::GraphMetaWritten,
                        ) {
                            log::warn!(
                                "flush_save_queue: update_phase to GraphMetaWritten \
                                 failed for starmap {}: {} — GraphMeta already persisted, \
                                 journal phase update is best-effort",
                                self.starmap_id,
                                e
                            );
                        }
                    }
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

            // 对象删除事务完成：清完 deleted_*_ids 后删除 journal。
            // 如果删除失败（best-effort），journal 残留，下次启动 bootstrap
            // 会看到 phase=GraphMetaWritten 并幂等清理。
            if object_delete_journal_active {
                if let Err(e) =
                    PlannedStarMapObjectDelete::clear(&self.app_data_root, &self.starmap_id)
                {
                    log::warn!(
                        "flush_save_queue: clear object delete journal failed for starmap {}: {} \
                         — transaction already committed, journal cleanup is best-effort",
                        self.starmap_id,
                        e
                    );
                }
            }
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
