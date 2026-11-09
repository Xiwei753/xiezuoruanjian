use super::*;

fn extract_node_search_body(
    content: &crate::starmap::semantic::StarMapNodeContent,
    tags: &[String],
) -> String {
    let mut parts = Vec::new();
    let text = content.search_text();
    if !text.is_empty() {
        parts.push(text);
    }
    for tag in tags {
        if !tag.is_empty() {
            parts.push(tag.clone());
        }
    }
    parts.join(" ")
}

fn get_starmap_project_id(api: &WriterCoreApi, starmap_id: &str) -> Option<String> {
    api.core_write()
        .get_starmap(starmap_id)
        .ok()
        .and_then(|meta| meta.project_id)
}

fn node_patch_has_shared_fields(patch: &crate::api::types::StarMapNodePatchDto) -> bool {
    patch.title.is_some()
        || patch.kind.is_some()
        || patch.payload.is_some()
        || patch.tags.is_some()
        || patch.content.is_some()
        || patch.anchors.is_some()
        || patch.portal.is_some()
        || patch.style.is_some()
        || patch.provenance.is_some()
}

/// 把贡献事务与受影响的已加载画布绑定。
///
/// 事务在 Prepared 阶段回滚文件后，Guard 会立即淘汰这些 Store，禁止把失败
/// 修改留在内存中；ObjectApplied 阶段仍由贡献事务清单负责补写事件，不淘汰
/// 已落盘的 Store。图形对象的搜索索引更新也随本 Guard 暂存，仅在对象真正
/// 持久化后发布，避免回滚后搜索仍能命中已删除的对象。
struct StarmapStoreTransaction<'a> {
    api: &'a WriterCoreApi,
    transaction: crate::writing_stats::contribution_transaction::ContributionTransaction,
    pending_search_updates: Vec<crate::search::SearchIndexUpdate>,
}

impl<'a> StarmapStoreTransaction<'a> {
    fn begin(api: &'a WriterCoreApi) -> ApiResult<Self> {
        Ok(Self {
            api,
            transaction: api.begin_starmaps_contribution_transaction()?,
            pending_search_updates: Vec::new(),
        })
    }

    fn from_transaction(
        api: &'a WriterCoreApi,
        transaction: crate::writing_stats::contribution_transaction::ContributionTransaction,
    ) -> Self {
        Self {
            api,
            transaction,
            pending_search_updates: Vec::new(),
        }
    }

    fn include_starmaps(&mut self, starmap_ids: &[String]) {
        self.transaction.register_affected_starmaps(starmap_ids);
    }

    /// 图形对象对应的搜索索引更新先随事务暂存，不能立即写入全局索引。
    /// 提交成功或对象已持久化（ObjectApplied）后才发布；Prepared 回滚时丢弃。
    fn enqueue_search_update(&mut self, update: crate::search::SearchIndexUpdate) {
        self.pending_search_updates.push(update);
    }
}

impl std::ops::Deref for StarmapStoreTransaction<'_> {
    type Target = crate::writing_stats::contribution_transaction::ContributionTransaction;

    fn deref(&self) -> &Self::Target {
        &self.transaction
    }
}

impl std::ops::DerefMut for StarmapStoreTransaction<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.transaction
    }
}

impl Drop for StarmapStoreTransaction<'_> {
    fn drop(&mut self) {
        let affected = self.transaction.affected_starmaps().to_vec();
        let pending_search_updates = std::mem::take(&mut self.pending_search_updates);
        match self.transaction.rollback_prepared() {
            Ok(true) if !affected.is_empty() => {
                self.api
                    .core_write()
                    .evict_starmap_stores_after_rollback(&affected);
            }
            Ok(true) => {}
            Ok(false) => {
                // Prepared 回滚会返回 true 并丢弃索引更新；只有对象真正持久化
                // （Committed，或事件待补写的 ObjectApplied）才发布搜索索引。
                if self.transaction.objects_persisted() && !pending_search_updates.is_empty() {
                    self.api
                        .core_write()
                        .enqueue_search_index_updates(pending_search_updates);
                }
            }
            Err(error) => {
                log::error!("starmap contribution rollback failed: {error}");
            }
        }
    }
}

impl WriterCoreApi {
    /// 所有会修改 `StarMapStore` 内存态的来源事务统一从这里开始：
    /// 创建 Guard 时立即登记宿主画布；跨画布影响再用 `include_starmaps` 追加。
    fn begin_starmap_store_transaction(
        &self,
        starmap_id: &str,
    ) -> ApiResult<StarmapStoreTransaction<'_>> {
        let mut transaction = StarmapStoreTransaction::begin(self)?;
        transaction.include_starmaps(&[starmap_id.to_string()]);
        Ok(transaction)
    }

    pub fn list_starmaps_json(&self) -> ApiResult<String> {
        let value = self
            .core_write()
            .list_starmaps()
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn create_starmap_json(&self, title: &str, desc: &str) -> ApiResult<String> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let starmap_id = format!("sm_{}", uuid::Uuid::new_v4());
        let mut contribution_batch = self.begin_contribution_batch();
        let mut contribution_txn =
            self.begin_starmap_creation_contribution_transaction(&starmap_id)?;
        let history_token = crate::storage::journal::workspace_change::new_token();
        self.register_starmap_history_journal(&mut contribution_txn, &history_token)?;
        //   走 _with_changes 入口，记录本地 history。
        // 之前调用 create_starmap(title, desc, None) 没有走 change set，
        // 导致 create_starmap_json 写的 meta/index 文件不进本地 Git history。
        let (value, change_set) = self
            .core_write()
            .create_starmap_with_id_with_changes(&starmap_id, title, desc, None)
            .map_err(WriterError::from)?;
        let history_journal = self.save_starmap_history_journal(
            &history_token,
            &change_set,
            crate::storage::journal::workspace_change::WorkspaceChangeOpType::CreateStarMap,
        )?;
        let starmap_id = value.starmap_id.clone();
        let project_id = value.project_id.as_deref().map(|s| s.to_string());
        let entry = crate::search::extractor::extract_starmap_title_entry(
            &starmap_id,
            project_id.as_deref(),
            title,
        );
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        self.record_starmap_meta_with_context(
            &crate::writing_stats::current_mutation_context(),
            project_id.as_deref(),
            &starmap_id,
            crate::writing_stats::ContributionOperation::Create,
            None,
            self.core_write().get_starmap(&starmap_id).ok().as_ref(),
            crate::starmap::load_index(&self.app_data_root)
                .ok()
                .map(|index| index.root_starmap_ids),
        )?;
        self.commit_contribution_batch(&mut contribution_batch)?;
        self.finish_starmap_contribution_transaction(&starmap_id, &mut contribution_txn)?;
        if let Err(e) = history_journal.mark_local_applied(&self.app_data_root) {
            log::warn!(
                "create_starmap_json: mark_local_applied failed for {}: {} — journal retained",
                history_journal.token,
                e
            );
        }
        self.finish_starmap_history_journal(&history_journal, "create_starmap");
        if let Err(error) = crate::storage::workspace_events::append_structure_event(
            &self.app_data_root,
            &starmap_id,
            "starmap",
            &starmap_id,
        ) {
            log::warn!("failed to append create_starmap structure event: {error}");
        }
        Self::json_string(&value)
    }

    pub fn add_starmap_embed(
        &self,
        starmap_id: &str,
        embed: crate::api::types::StarMapEmbedDto,
    ) -> ApiResult<crate::api::types::StarMapEmbedDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_txn = self.begin_starmap_store_transaction(starmap_id)?;
        let result = self
            .core_write()
            .add_starmap_embed(starmap_id, embed.try_into().map_err(WriterError::from)?)
            .map_err(WriterError::from)?;
        let project_id = get_starmap_project_id(self, starmap_id);
        let after = self.get_starmap_graph_snapshot(starmap_id);
        self.record_starmap_object(
            project_id.as_deref(),
            starmap_id,
            crate::writing_stats::ContributionObjectType::StarMapEmbed,
            &result.instance_id,
            crate::writing_stats::ContributionOperation::Create,
            None,
            after.as_ref(),
        )?;
        let entry = crate::search::extractor::extract_starmap_embed_entry(
            starmap_id,
            &result.instance_id,
            project_id.as_deref(),
            &result.label.clone().unwrap_or_default(),
        );
        contribution_txn.enqueue_search_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        self.finish_starmap_contribution_transaction(starmap_id, &mut contribution_txn)?;
        if let Err(error) = crate::storage::workspace_events::append_structure_event(
            &self.app_data_root,
            starmap_id,
            "starmap_embed",
            &result.instance_id,
        ) {
            log::warn!("failed to append add_starmap_embed structure event: {error}");
        }
        Ok(result.into())
    }

    /// 将一个已存在的一级星图嵌入宿主星图，并在同一 Workspace 写事务内
    /// 把它从一级列表移出。返回新增的 Embed。
    pub fn embed_existing_starmap(
        &self,
        host_starmap_id: &str,
        child_starmap_id: &str,
        position: crate::api::types::StarMapPointDto,
    ) -> ApiResult<crate::api::types::StarMapEmbedDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_txn = self.begin_starmap_store_transaction(host_starmap_id)?;
        // 根身份与 Embed 同属一次来源事务：index 在修改前登记备份，
        // 宿主图文件在 add 之后、flush 之前登记，保证失败时可整体回滚。
        contribution_txn.prepare_paths(&[self.app_data_root.join("starmaps/index.json")])?;
        let project_id = get_starmap_project_id(self, host_starmap_id);
        let result = self
            .core_write()
            .embed_existing_root_starmap(host_starmap_id, child_starmap_id, position.into())
            .map_err(WriterError::from)?;
        let after = self.get_starmap_graph_snapshot(host_starmap_id);
        self.record_starmap_object(
            project_id.as_deref(),
            host_starmap_id,
            crate::writing_stats::ContributionObjectType::StarMapEmbed,
            &result.instance_id,
            crate::writing_stats::ContributionOperation::Create,
            None,
            after.as_ref(),
        )?;
        let entry = crate::search::extractor::extract_starmap_embed_entry(
            host_starmap_id,
            &result.instance_id,
            project_id.as_deref(),
            result.label.as_deref().unwrap_or_default(),
        );
        contribution_txn.enqueue_search_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        let mut change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
            .add_upsert(std::path::PathBuf::from("starmaps/index.json"));
        let mut pending_paths = self
            .core_read()
            .pending_starmap_flush_paths(host_starmap_id);
        pending_paths.push(self.app_data_root.join("starmaps/index.json"));
        contribution_txn.prepare_paths(&pending_paths)?;
        for path in self
            .core_write()
            .flush_starmap_store(host_starmap_id)
            .map_err(WriterError::from)?
        {
            change_set = change_set.add_upsert(path);
        }
        if let Err(error) =
            self.record_workspace_change_set_history(&change_set, "embed_existing_starmap")
        {
            log::warn!(
                "embed_existing_starmap: record_workspace_change_set_history failed: {}",
                error
            );
        }
        self.finish_starmap_contribution_transaction(host_starmap_id, &mut contribution_txn)?;
        if let Err(error) = crate::storage::workspace_events::append_structure_event(
            &self.app_data_root,
            host_starmap_id,
            "starmap_embed",
            &result.instance_id,
        ) {
            log::warn!("failed to append embed_existing_starmap structure event: {error}");
        }
        Ok(result.into())
    }

    /// 原子组合操作：创建子星图并嵌入父图。
    ///
    /// 调用 facade 的 `create_starmap_child_embed`，将领域类型转换为 DTO，
    /// 更新搜索索引（对新创建的 starmap 和 embed 都做 Upsert），
    /// 记录本地 Git history。
    pub fn create_starmap_child_embed(
        &self,
        host_starmap_id: &str,
        title: &str,
        position: crate::api::types::StarMapPointDto,
    ) -> ApiResult<crate::api::types::CreateStarMapChildEmbedResultDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let child_starmap_id = format!("sm_{}", uuid::Uuid::new_v4());
        let embed_instance_id =
            crate::storage::journal::starmap_child_embed::new_embed_instance_id();
        let journal_tx_id = crate::storage::journal::starmap_child_embed::new_journal_token();
        let contribution_transaction = self.begin_starmap_child_embed_contribution_transaction(
            host_starmap_id,
            &child_starmap_id,
            &embed_instance_id,
            &journal_tx_id,
        )?;
        let mut contribution_txn =
            StarmapStoreTransaction::from_transaction(self, contribution_transaction);
        contribution_txn.include_starmaps(&[
            host_starmap_id.to_string(),
            child_starmap_id.clone(),
        ]);
        let mut contribution_batch = self.begin_contribution_batch();
        let core_position: crate::starmap::types::StarMapPoint = position.into();
        let (starmap_meta, embed, change_set, tx_id) = self
            .core_write()
            .create_starmap_child_embed_with_id(
                host_starmap_id,
                &child_starmap_id,
                &embed_instance_id,
                &journal_tx_id,
                title,
                core_position,
            )
            .map_err(WriterError::from)?;

        let starmap_id = starmap_meta.starmap_id.clone();
        let project_id = starmap_meta.project_id.as_deref().map(|s| s.to_string());

        // 更新搜索索引：新创建的 starmap title
        let starmap_entry = crate::search::extractor::extract_starmap_title_entry(
            &starmap_id,
            project_id.as_deref(),
            &starmap_meta.title,
        );
        contribution_txn.enqueue_search_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: starmap_entry.object_id.clone(),
            scope: starmap_entry.scope,
            title: starmap_entry.title.clone(),
            body: starmap_entry.body.clone(),
            target: Some(starmap_entry.target.clone()),
        });

        // 更新搜索索引：新创建的 embed
        let embed_entry = crate::search::extractor::extract_starmap_embed_entry(
            host_starmap_id,
            &embed.instance_id,
            project_id.as_deref(),
            &embed.label.clone().unwrap_or_default(),
        );
        contribution_txn.enqueue_search_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: embed_entry.object_id.clone(),
            scope: embed_entry.scope,
            title: embed_entry.title.clone(),
            body: embed_entry.body.clone(),
            target: Some(embed_entry.target.clone()),
        });

        let context = crate::writing_stats::current_mutation_context();
        self.record_starmap_meta_with_context(
            &context,
            project_id.as_deref(),
            &starmap_id,
            crate::writing_stats::ContributionOperation::Create,
            None,
            Some(&starmap_meta),
            None,
        )?;
        let after = self.get_starmap_graph_snapshot(host_starmap_id);
        self.record_starmap_object_with_context(
            &context,
            project_id.as_deref(),
            host_starmap_id,
            crate::writing_stats::ContributionObjectType::StarMapEmbed,
            &embed.instance_id,
            crate::writing_stats::ContributionOperation::Create,
            None,
            after.as_ref(),
        )?;

        self.commit_contribution_batch(&mut contribution_batch)?;
        self.finish_starmap_contribution_transaction(host_starmap_id, &mut contribution_txn)?;

        // 对象、宿主 Embed 与来源事件都 durable 后才写 Git history；
        // 只有 history 成功后才能 ack 子图 journal。history 失败时保留
        // EmbedAdded journal，下次启动用同一 tx_id 幂等补记。
        let pending = match self
            .record_workspace_change_set_history(&change_set, "create_starmap_child_embed")
        {
            Ok(()) => {
                if let Err(e) =
                    crate::storage::journal::starmap_child_embed::ack_child_embed_history(
                        &self.app_data_root,
                        &tx_id,
                    )
                {
                    log::warn!(
                        "create_starmap_child_embed: ack_child_embed_history failed for \
                             tx_id={}: {} — journal retained, will be recovered on next startup",
                        tx_id,
                        e
                    );
                }
                false
            }
            Err(e) => {
                log::warn!(
                    "create_starmap_child_embed: record_workspace_change_set_history failed \
                         for tx_id={}: {} — journal retained at EmbedAdded, will be recovered on \
                         next startup",
                    tx_id,
                    e
                );
                true
            }
        };
        if let Err(error) = crate::storage::workspace_events::append_structure_event(
            &self.app_data_root,
            &starmap_meta.starmap_id,
            "starmap_embed",
            &embed.instance_id,
        ) {
            log::warn!("failed to append child starmap structure event: {error}");
        }
        Ok(crate::api::types::CreateStarMapChildEmbedResultDto {
            starmap: starmap_meta.into(),
            embed: embed.into(),
            pending,
        })
    }

    pub fn update_starmap_embed(
        &self,
        starmap_id: &str,
        instance_id: &str,
        patch: crate::api::types::StarMapEmbedPatchDto,
    ) -> ApiResult<crate::api::types::StarMapEmbedDto> {
        self.update_starmap_embed_expected(starmap_id, instance_id, patch, None)
    }

    pub fn update_starmap_embed_expected(
        &self,
        starmap_id: &str,
        instance_id: &str,
        patch: crate::api::types::StarMapEmbedPatchDto,
        expected_revision: Option<u64>,
    ) -> ApiResult<crate::api::types::StarMapEmbedDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_txn = self.begin_starmap_store_transaction(starmap_id)?;
        let before = self.get_starmap_graph_snapshot(starmap_id);
        ensure_starmap_revision(
            before
                .as_ref()
                .and_then(|graph| {
                    graph
                        .embeds
                        .iter()
                        .find(|embed| embed.instance_id == instance_id)
                })
                .map(|embed| embed.updated_at),
            expected_revision,
            "embed",
            instance_id,
        )?;
        let project_id = get_starmap_project_id(self, starmap_id);
        let trajectory = patch.trajectory.clone();
        let result = self
            .core_write()
            .update_starmap_embed(
                starmap_id,
                instance_id,
                patch.try_into().map_err(WriterError::from)?,
            )
            .map_err(WriterError::from)?;
        let after = self.get_starmap_graph_snapshot(starmap_id);
        let context = crate::writing_stats::current_mutation_context();
        self.record_starmap_object_with_context_and_trajectory(
            &context,
            project_id.as_deref(),
            starmap_id,
            crate::writing_stats::ContributionObjectType::StarMapEmbed,
            instance_id,
            crate::writing_stats::ContributionOperation::Update,
            before.as_ref(),
            after.as_ref(),
            &trajectory,
        )?;
        let entry = crate::search::extractor::extract_starmap_embed_entry(
            starmap_id,
            instance_id,
            project_id.as_deref(),
            &result.label.clone().unwrap_or_default(),
        );
        contribution_txn.enqueue_search_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        self.finish_starmap_contribution_transaction(starmap_id, &mut contribution_txn)?;
        Ok(result.into())
    }

    pub fn delete_starmap_embed(&self, starmap_id: &str, instance_id: &str) -> ApiResult<bool> {
        self.delete_starmap_embed_expected(starmap_id, instance_id, None)
    }

    pub fn delete_starmap_embed_expected(
        &self,
        starmap_id: &str,
        instance_id: &str,
        expected_revision: Option<u64>,
    ) -> ApiResult<bool> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_txn = self.begin_starmap_store_transaction(starmap_id)?;
        // 删除最后一个入口可能改写根身份 index；先登记备份，再修改图与 index。
        contribution_txn.prepare_paths(&[self.app_data_root.join("starmaps/index.json")])?;
        let before = self.get_starmap_graph_snapshot(starmap_id);
        ensure_starmap_revision(
            before
                .as_ref()
                .and_then(|graph| {
                    graph
                        .embeds
                        .iter()
                        .find(|embed| embed.instance_id == instance_id)
                })
                .map(|embed| embed.updated_at),
            expected_revision,
            "embed",
            instance_id,
        )?;
        let project_id = get_starmap_project_id(self, starmap_id);
        let embedded_target = before.as_ref().and_then(|graph| {
            graph
                .embeds
                .iter()
                .find(|embed| embed.instance_id == instance_id)
                .map(|embed| embed.target_starmap_id.clone())
        });
        self.core_write()
            .delete_starmap_embed(starmap_id, instance_id)?;
        if let Some(target_starmap_id) = embedded_target {
            // 在内存态上检查剩余入口；真正 flush 交给来源事务统一完成。
            self.core_write()
                .restore_root_starmap_if_unembedded(&target_starmap_id)?;
        }
        self.record_starmap_object(
            project_id.as_deref(),
            starmap_id,
            crate::writing_stats::ContributionObjectType::StarMapEmbed,
            instance_id,
            crate::writing_stats::ContributionOperation::Delete,
            before.as_ref(),
            None,
        )?;
        contribution_txn.enqueue_search_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Delete,
            object_id: format!("starmap_embed:{}:{}", starmap_id, instance_id),
            scope: crate::search::SearchScope::All,
            title: String::new(),
            body: String::new(),
            target: None,
        });
        self.finish_starmap_contribution_transaction(starmap_id, &mut contribution_txn)?;
        if let Err(error) = crate::storage::workspace_events::append_structure_event(
            &self.app_data_root,
            starmap_id,
            "starmap_embed",
            instance_id,
        ) {
            log::warn!("failed to append delete_starmap_embed structure event: {error}");
        }
        Ok(true)
    }

    pub fn add_starmap_link(
        &self,
        starmap_id: &str,
        link: crate::api::types::StarMapLinkDto,
    ) -> ApiResult<crate::api::types::StarMapLinkDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_txn = self.begin_starmap_store_transaction(starmap_id)?;
        let result = self
            .core_write()
            .add_starmap_link(starmap_id, link.try_into().map_err(WriterError::from)?)
            .map_err(WriterError::from)?;
        let label = result.label.clone().unwrap_or_default();
        let project_id = get_starmap_project_id(self, starmap_id);
        let after = self.get_starmap_graph_snapshot(starmap_id);
        self.record_starmap_object(
            project_id.as_deref(),
            starmap_id,
            crate::writing_stats::ContributionObjectType::StarMapLink,
            &result.link_id,
            crate::writing_stats::ContributionOperation::Create,
            None,
            after.as_ref(),
        )?;
        let entry = crate::search::extractor::extract_starmap_link_entry(
            starmap_id,
            &result.link_id,
            project_id.as_deref(),
            &label,
        );
        contribution_txn.enqueue_search_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        self.finish_starmap_contribution_transaction(starmap_id, &mut contribution_txn)?;
        Ok(result.into())
    }

    pub fn update_starmap_link(
        &self,
        starmap_id: &str,
        link_id: &str,
        patch: crate::api::types::StarMapLinkPatchDto,
    ) -> ApiResult<crate::api::types::StarMapLinkDto> {
        self.update_starmap_link_expected(starmap_id, link_id, patch, None)
    }

    pub fn update_starmap_link_expected(
        &self,
        starmap_id: &str,
        link_id: &str,
        patch: crate::api::types::StarMapLinkPatchDto,
        expected_revision: Option<u64>,
    ) -> ApiResult<crate::api::types::StarMapLinkDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_txn = self.begin_starmap_store_transaction(starmap_id)?;
        let before = self.get_starmap_graph_snapshot(starmap_id);
        ensure_starmap_revision(
            before
                .as_ref()
                .and_then(|graph| graph.links.iter().find(|link| link.link_id == link_id))
                .map(|link| link.updated_at),
            expected_revision,
            "link",
            link_id,
        )?;
        let project_id = get_starmap_project_id(self, starmap_id);
        let result = self
            .core_write()
            .update_starmap_link(
                starmap_id,
                link_id,
                patch.try_into().map_err(WriterError::from)?,
            )
            .map_err(WriterError::from)?;
        let after = self.get_starmap_graph_snapshot(starmap_id);
        self.record_starmap_object(
            project_id.as_deref(),
            starmap_id,
            crate::writing_stats::ContributionObjectType::StarMapLink,
            link_id,
            crate::writing_stats::ContributionOperation::Update,
            before.as_ref(),
            after.as_ref(),
        )?;
        let label = result.label.clone().unwrap_or_default();
        let entry = crate::search::extractor::extract_starmap_link_entry(
            starmap_id,
            link_id,
            project_id.as_deref(),
            &label,
        );
        contribution_txn.enqueue_search_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        self.finish_starmap_contribution_transaction(starmap_id, &mut contribution_txn)?;
        Ok(result.into())
    }

    pub fn delete_starmap_link(&self, starmap_id: &str, link_id: &str) -> ApiResult<bool> {
        self.delete_starmap_link_expected(starmap_id, link_id, None)
    }

    pub fn delete_starmap_link_expected(
        &self,
        starmap_id: &str,
        link_id: &str,
        expected_revision: Option<u64>,
    ) -> ApiResult<bool> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_txn = self.begin_starmap_store_transaction(starmap_id)?;
        let before = self.get_starmap_graph_snapshot(starmap_id);
        ensure_starmap_revision(
            before
                .as_ref()
                .and_then(|graph| graph.links.iter().find(|link| link.link_id == link_id))
                .map(|link| link.updated_at),
            expected_revision,
            "link",
            link_id,
        )?;
        let project_id = get_starmap_project_id(self, starmap_id);
        self.core_write().delete_starmap_link(starmap_id, link_id)?;
        self.record_starmap_object(
            project_id.as_deref(),
            starmap_id,
            crate::writing_stats::ContributionObjectType::StarMapLink,
            link_id,
            crate::writing_stats::ContributionOperation::Delete,
            before.as_ref(),
            None,
        )?;
        contribution_txn.enqueue_search_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Delete,
            object_id: format!("starmap_link:{}:{}", starmap_id, link_id),
            scope: crate::search::SearchScope::All,
            title: String::new(),
            body: String::new(),
            target: None,
        });
        self.finish_starmap_contribution_transaction(starmap_id, &mut contribution_txn)?;
        Ok(true)
    }

    pub fn find_starmap_references(
        &self,
        target_starmap_id: &str,
    ) -> ApiResult<Vec<crate::api::types::StarMapReferenceDto>> {
        self.core_write()
            .find_starmap_references(target_starmap_id)
            .map(|list| list.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    /// Fix 4: 确认星图删除 tombstone 已被同步方持久化。
    pub fn ack_starmap_deletions(
        &self,
        starmap_id: &str,
        acknowledged_revision: u64,
    ) -> ApiResult<()> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        self.core_write()
            .ack_starmap_deletions(starmap_id, acknowledged_revision)
            .map_err(WriterError::from)
    }

    pub fn get_starmap_graph(
        &self,
        starmap_id: &str,
    ) -> ApiResult<crate::api::types::StarMapGraphDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut graph: crate::api::types::StarMapGraphDto = self
            .core_write()
            .get_starmap_graph(starmap_id)
            .map(Into::into)
            .map_err(WriterError::from)?;
        graph.asset_bindings = self.starmap_asset_bindings(starmap_id)?;
        Ok(graph)
    }

    fn starmap_asset_bindings(
        &self,
        starmap_id: &str,
    ) -> ApiResult<Vec<crate::api::types::StarMapNodeAssetBindingDto>> {
        crate::starmap::library::instance_asset_bindings_for_starmap(
            &self.app_data_root,
            starmap_id,
        )
        .map(|bindings| {
            bindings
                .into_iter()
                .map(
                    |(instance_id, asset_id)| crate::api::types::StarMapNodeAssetBindingDto {
                        instance_id,
                        asset_id,
                    },
                )
                .collect()
        })
        .map_err(WriterError::from)
    }

    // ── 全局 / 作品素材库 ──

    fn parse_library_scope(
        scope_json: &str,
    ) -> ApiResult<crate::starmap::types::StarMapAssetScope> {
        serde_json::from_str(scope_json).map_err(WriterError::from)
    }

    /// 素材库写入同样必须进入来源事务和 Workspace 变更链。库索引与派生
    /// 引用索引的路径在修改前登记；对象文件由各操作显式加入 change set。
    fn commit_library_transaction(
        &self,
        transaction: &mut crate::writing_stats::contribution_transaction::ContributionTransaction,
        change_set: &crate::storage::workspace_git::WorkspaceChangeSet,
        message: &str,
    ) -> ApiResult<()> {
        transaction.commit()?;
        if let Err(error) = self.record_workspace_change_set_history(change_set, message) {
            log::warn!("{message}: record_workspace_change_set_history failed: {error}");
        }
        Ok(())
    }

    fn prepare_library_paths(
        &self,
        transaction: &mut crate::writing_stats::contribution_transaction::ContributionTransaction,
        include_definitions: bool,
    ) -> ApiResult<()> {
        let mut paths = vec![
            crate::starmap::library::library_index_abs_path(&self.app_data_root),
            crate::starmap::library::reference_index_abs_path(&self.app_data_root),
        ];
        if include_definitions {
            paths.push(crate::starmap::library::library_dir(&self.app_data_root).join("nodes"));
        }
        transaction.prepare_paths(&paths)?;
        Ok(())
    }

    pub fn list_starmap_library_json(&self, scope_json: &str) -> ApiResult<String> {
        let scope = Self::parse_library_scope(scope_json)?;
        let (entries, folders) = self
            .core_write()
            .list_starmap_library(&scope)
            .map_err(WriterError::from)?;
        let entries = entries
            .into_iter()
            .map(|entry| {
                let title =
                    crate::starmap::library::asset_title(&self.app_data_root, &entry.target)
                        .ok()
                        .flatten()
                        .unwrap_or_default();
                serde_json::json!({ "entry": entry, "title": title })
            })
            .collect::<Vec<_>>();
        Self::json_string(&serde_json::json!({
            "scope": scope,
            "entries": entries,
            "folders": folders,
        }))
    }

    pub fn create_starmap_library_folder_json(
        &self,
        scope_json: &str,
        parent_folder_id: Option<String>,
        name: &str,
    ) -> ApiResult<String> {
        let scope = Self::parse_library_scope(scope_json)?;
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_txn = self.begin_starmaps_contribution_transaction()?;
        self.prepare_library_paths(&mut contribution_txn, false)?;
        let folder = self
            .core_write()
            .create_starmap_library_folder(&scope, parent_folder_id, name)
            .map_err(WriterError::from)?;
        let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
            .add_upsert(crate::starmap::library::library_index_rel_path());
        self.commit_library_transaction(
            &mut contribution_txn,
            &change_set,
            "create_starmap_library_folder",
        )?;
        Self::json_string(&folder)
    }

    pub fn move_starmap_library_entry_json(
        &self,
        entry_id: &str,
        scope_json: &str,
        parent_folder_id: Option<String>,
    ) -> ApiResult<String> {
        let scope = Self::parse_library_scope(scope_json)?;
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_txn = self.begin_starmaps_contribution_transaction()?;
        self.prepare_library_paths(&mut contribution_txn, false)?;
        let entry = self
            .core_write()
            .move_starmap_library_entry(entry_id, &scope, parent_folder_id)
            .map_err(WriterError::from)?;
        let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
            .add_upsert(crate::starmap::library::library_index_rel_path());
        self.commit_library_transaction(
            &mut contribution_txn,
            &change_set,
            "move_starmap_library_entry",
        )?;
        Self::json_string(&entry)
    }

    pub fn add_starmap_node_reference(
        &self,
        starmap_id: &str,
        asset_id: &str,
        x: f64,
        y: f64,
    ) -> ApiResult<crate::api::types::StarMapNodeDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_txn = self.begin_starmap_store_transaction(starmap_id)?;
        let instance_id = format!("n_{}", uuid::Uuid::new_v4());
        contribution_txn.prepare_paths(&[
            self.app_data_root
                .join(crate::starmap::package_storage::node_rel_path(
                    starmap_id,
                    &instance_id,
                )),
            crate::starmap::library::library_index_abs_path(&self.app_data_root),
            crate::starmap::library::reference_index_abs_path(&self.app_data_root),
        ])?;
        let position = crate::starmap::types::StarMapPoint { x, y };
        let result = self
            .core_write()
            .add_starmap_node_reference(starmap_id, asset_id, position, None, Some(instance_id.clone()))
            .map_err(WriterError::from)?;
        let node_content = extract_node_search_body(&result.content, &result.tags);
        let project_id = get_starmap_project_id(self, starmap_id);
        let entry = crate::search::extractor::extract_starmap_node_entry(
            starmap_id,
            &result.id,
            project_id.as_deref(),
            &result.title,
            &node_content,
        );
        contribution_txn.enqueue_search_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        let after = self.get_starmap_graph_snapshot(starmap_id);
        self.record_starmap_object(
            project_id.as_deref(),
            starmap_id,
            crate::writing_stats::ContributionObjectType::StarMapNode,
            &result.id,
            crate::writing_stats::ContributionOperation::Link,
            None,
            after.as_ref(),
        )?;
        self.finish_starmap_contribution_transaction(starmap_id, &mut contribution_txn)?;
        let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
            .add_upsert(crate::starmap::package_storage::node_rel_path(
                starmap_id,
                &result.id,
            ))
            .add_upsert(crate::starmap::package_storage::graph_meta_rel_path(
                starmap_id,
            ))
            .add_upsert(crate::starmap::library::library_index_rel_path())
            .add_upsert(crate::starmap::library::reference_index_rel_path());
        if let Err(error) = self.record_workspace_change_set_history(
            &change_set,
            "add_starmap_node_reference",
        ) {
            log::warn!("add_starmap_node_reference history failed: {error}");
        }
        Ok(result.into())
    }

    pub fn add_starmap_reference(
        &self,
        host_starmap_id: &str,
        target_starmap_id: &str,
        x: f64,
        y: f64,
    ) -> ApiResult<crate::api::types::StarMapEmbedDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_txn = self.begin_starmap_store_transaction(host_starmap_id)?;
        let embed_instance_id =
            crate::storage::journal::starmap_child_embed::new_embed_instance_id();
        contribution_txn.prepare_paths(&[
            self.app_data_root
                .join(crate::starmap::package_storage::embed_rel_path(
                    host_starmap_id,
                    &embed_instance_id,
                )),
            crate::starmap::library::library_index_abs_path(&self.app_data_root),
        ])?;
        let result = self
            .core_write()
            .add_starmap_reference(
                host_starmap_id,
                target_starmap_id,
                crate::starmap::types::StarMapPoint { x, y },
                Some(embed_instance_id),
            )
            .map_err(WriterError::from)?;
        let project_id = get_starmap_project_id(self, host_starmap_id);
        let after = self.get_starmap_graph_snapshot(host_starmap_id);
        self.record_starmap_object(
            project_id.as_deref(),
            host_starmap_id,
            crate::writing_stats::ContributionObjectType::StarMapEmbed,
            &result.instance_id,
            crate::writing_stats::ContributionOperation::Create,
            None,
            after.as_ref(),
        )?;
        let entry = crate::search::extractor::extract_starmap_embed_entry(
            host_starmap_id,
            &result.instance_id,
            project_id.as_deref(),
            result.label.as_deref().unwrap_or_default(),
        );
        contribution_txn.enqueue_search_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        self.finish_starmap_contribution_transaction(host_starmap_id, &mut contribution_txn)?;
        let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
            .add_upsert(crate::starmap::package_storage::embed_rel_path(
                host_starmap_id,
                &result.instance_id,
            ))
            .add_upsert(crate::starmap::package_storage::graph_meta_rel_path(
                host_starmap_id,
            ))
            .add_upsert(crate::starmap::library::library_index_rel_path());
        if let Err(error) =
            self.record_workspace_change_set_history(&change_set, "add_starmap_reference")
        {
            log::warn!("add_starmap_reference history failed: {error}");
        }
        if let Err(error) = crate::storage::workspace_events::append_structure_event(
            &self.app_data_root,
            host_starmap_id,
            "starmap_embed",
            &result.instance_id,
        ) {
            log::warn!("failed to append add_starmap_reference structure event: {error}");
        }
        Ok(result.into())
    }

    pub fn copy_starmap_library_asset_json(
        &self,
        target_json: &str,
        scope_json: &str,
        title: Option<String>,
    ) -> ApiResult<String> {
        let target = serde_json::from_str(target_json).map_err(WriterError::from)?;
        let scope = Self::parse_library_scope(scope_json)?;
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_txn = StarmapStoreTransaction::begin(self)?;
        let copied = {
            let mut prepare_paths = |paths: &[std::path::PathBuf]| {
                contribution_txn.prepare_paths(paths)
            };
            self.core_write().copy_starmap_library_asset(
                &target,
                &scope,
                title,
                &mut prepare_paths,
            )
            .map_err(WriterError::from)?
        };
        // 复制不是新的手写输入；沿用当前事务 ID，但以系统来源记录，避免把
        // 副本正文再次计入人类输入速度。
        let mutation_context = crate::writing_stats::current_mutation_context();
        let context = crate::writing_stats::MutationContext::system()
            .with_transaction_id(mutation_context.transaction_id().to_string());
        match copied {
            crate::facade::StarMapAssetCopyResult::Node { asset, definition } => {
                let mut copy_batch = self.begin_contribution_batch();
                // 素材本体创建独立于画布：记录一次 Copy，容器为空。Copy 只
                // 进入证明链，不进入写作字数投影；后续拖入画布只记录 Link。
                let mut graph = crate::starmap::types::StarMapGraph::default();
                graph.starmap_id = String::new();
                graph.nodes.push(definition.instantiate(
                    &asset.asset_id,
                    crate::starmap::types::StarMapPoint { x: 0.0, y: 0.0 },
                ));
                let delta = crate::writing_stats::starmap_proof::StarMapProofDelta::for_object(
                    "",
                    crate::writing_stats::ContributionObjectType::StarMapNode,
                    &asset.asset_id,
                    crate::writing_stats::ContributionOperation::Copy,
                    None,
                    Some(&graph),
                );
                self.record_contribution_event_with_context_and_delta(
                    &context,
                    scope.project_id().unwrap_or_default(),
                    crate::writing_stats::ContributionObjectType::StarMapNode,
                    &asset.asset_id,
                    crate::writing_stats::ContributionOperation::Copy,
                    None,
                    None,
                    Some(crate::writing_stats::ContributionObjectSnapshot::object(
                        definition.updated_at.to_string(),
                        definition.title.clone(),
                    )),
                    Some(delta),
                )?;
                self.commit_contribution_batch(&mut copy_batch)?;
                let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
                    .add_upsert(crate::starmap::library::node_definition_rel_path(
                        &asset.asset_id,
                    ))
                    .add_upsert(crate::starmap::library::library_index_rel_path());
                self.commit_library_transaction(
                    &mut contribution_txn,
                    &change_set,
                    "copy_starmap_library_asset",
                )?;
                Self::json_string(&asset)
            }
            crate::facade::StarMapAssetCopyResult::StarMap(result) => {
                let copied_starmaps = result
                    .starmaps
                    .iter()
                    .map(|meta| meta.starmap_id.clone())
                    .collect::<Vec<_>>();
                contribution_txn.include_starmaps(&copied_starmaps);
                let mut copy_batch = self.begin_contribution_batch();
                let mut graphs = std::collections::HashMap::new();
                let mut projects = std::collections::HashMap::new();
                let roots_after = crate::starmap::load_index(&self.app_data_root)
                    .ok()
                    .map(|index| index.root_starmap_ids);
                for meta in &result.starmaps {
                    let graph = self
                        .get_starmap_graph_snapshot(&meta.starmap_id)
                        .ok_or_else(|| {
                            WriterError::Other(format!(
                                "copied starmap graph missing: {}",
                                meta.starmap_id
                            ))
                        })?;
                    graphs.insert(meta.starmap_id.clone(), graph);
                    projects.insert(meta.starmap_id.clone(), meta.project_id.clone());
                }
                // 先记录每张新画布的 Meta/Create，再按实际创建顺序记录对象。
                for meta in &result.starmaps {
                    self.record_starmap_meta_with_context(
                        &context,
                        meta.project_id.as_deref(),
                        &meta.starmap_id,
                        crate::writing_stats::ContributionOperation::Create,
                        None,
                        Some(meta),
                        roots_after.clone(),
                    )?;
                }
                let object_lists = [
                    (
                        crate::writing_stats::ContributionObjectType::StarMapNode,
                        &result.nodes,
                    ),
                    (
                        crate::writing_stats::ContributionObjectType::StarMapEmbed,
                        &result.embeds,
                    ),
                    (
                        crate::writing_stats::ContributionObjectType::StarMapEdge,
                        &result.edges,
                    ),
                    (
                        crate::writing_stats::ContributionObjectType::StarMapLink,
                        &result.links,
                    ),
                ];
                for (object_type, objects) in object_lists {
                    for (starmap_id, object_id) in objects {
                        let graph = graphs.get(starmap_id).ok_or_else(|| {
                            WriterError::Other(format!(
                                "copied starmap graph missing for object {object_id}: {starmap_id}"
                            ))
                        })?;
                        self.record_starmap_object_with_context(
                            &context,
                            projects.get(starmap_id).and_then(|p| p.as_deref()),
                            starmap_id,
                            object_type,
                            object_id,
                            crate::writing_stats::ContributionOperation::Copy,
                            None,
                            Some(graph),
                        )?;
                    }
                }
                self.commit_contribution_batch(&mut copy_batch)?;
                let mut change_set = crate::storage::workspace_git::WorkspaceChangeSet::new();
                for path in &result.change_paths {
                    change_set = change_set.add_upsert(path.clone());
                }
                let asset =
                    crate::starmap::types::StarMapAssetRef::starmap(result.root.starmap_id);
                self.commit_library_transaction(
                    &mut contribution_txn,
                    &change_set,
                    "copy_starmap_library_asset",
                )?;
                Self::json_string(&asset)
            }
        }
    }

    pub fn list_starmap_asset_references_json(&self, target_json: &str) -> ApiResult<String> {
        let target = serde_json::from_str(target_json).map_err(WriterError::from)?;
        let refs = self
            .core_write()
            .list_starmap_asset_references(&target)
            .map_err(WriterError::from)?;
        Self::json_string(&refs)
    }

    pub fn delete_starmap_library_asset_json(&self, target_json: &str) -> ApiResult<bool> {
        let target: crate::starmap::types::StarMapAssetRef =
            serde_json::from_str(target_json).map_err(WriterError::from)?;
        if matches!(
            target.kind,
            crate::starmap::types::StarMapAssetKind::StarMap
        ) {
            // 星图本体删除必须走完整 durable 删除事务；该入口自身会做引用保护，
            // 并在成功后清理素材库归属与实例映射。
            self.delete_starmap(&target.asset_id)?;
            return Ok(true);
        }
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_txn = self.begin_starmaps_contribution_transaction()?;
        self.prepare_library_paths(&mut contribution_txn, false)?;
        let definition_rel =
            crate::starmap::library::node_definition_rel_path(&target.asset_id);
        contribution_txn
            .prepare_paths(&[self.app_data_root.join(&definition_rel)])?;
        self.core_write()
            .delete_starmap_library_asset(&target)
            .map_err(WriterError::from)?;
        let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
            .add_delete(definition_rel)
            .add_upsert(crate::starmap::library::library_index_rel_path())
            .add_upsert(crate::starmap::library::reference_index_rel_path());
        self.commit_library_transaction(
            &mut contribution_txn,
            &change_set,
            "delete_starmap_library_asset",
        )?;
        Ok(true)
    }

    pub fn remove_starmap_instance(
        &self,
        starmap_id: &str,
        instance_id: &str,
        is_embed: bool,
    ) -> ApiResult<bool> {
        // 必须经过 API 层的删除入口，才能同时完成贡献事件、搜索索引和
        // Workspace 事务；直接调用 facade 会绕过这些副作用。删除入口自己
        // 获取 WorkspaceWriteLock，这里不能先持锁再调用。
        if is_embed {
            self.delete_starmap_embed(starmap_id, instance_id)
        } else {
            self.delete_starmap_node(starmap_id, instance_id)
        }
    }

    /// 按 `root starmap + 路径段` 解析当前层星图。
    ///
    /// 无级星图套星图的当前层身份由 root + segments 决定：逐段走 Core
    /// resolver（`EnterEmbed` / `EnterPortal`），返回最终星图 ID。
    /// 解析失败返回错误，平台端不得回退到点击事件传来的裸目标 ID。
    pub fn resolve_starmap_path(
        &self,
        root_starmap_id: &str,
        segments: Vec<crate::api::types::StarMapPathSegmentDto>,
    ) -> ApiResult<crate::api::types::StarMapResolvedPathDto> {
        let segments = segments
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>, crate::error::Error>>()
            .map_err(WriterError::from)?;
        let resolved = self
            .core_write()
            .resolve_starmap_path(root_starmap_id, segments)?;
        Ok(resolved.into())
    }

    pub fn list_starmaps(&self) -> ApiResult<Vec<crate::api::types::StarMapMetaDto>> {
        self.core_write()
            .list_starmaps()
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn list_root_starmaps(&self) -> ApiResult<Vec<crate::api::types::StarMapMetaDto>> {
        self.core_write()
            .list_root_starmaps()
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn list_starmaps_for_project(
        &self,
        project_id: &str,
    ) -> ApiResult<Vec<crate::api::types::StarMapMetaDto>> {
        self.core_write()
            .list_starmaps_bound_to_project(project_id)
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn get_starmap(&self, starmap_id: &str) -> ApiResult<crate::api::types::StarMapMetaDto> {
        self.core_write()
            .get_starmap(starmap_id)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn create_starmap(
        &self,
        title: &str,
        desc: &str,
        template_id: Option<&str>,
    ) -> ApiResult<crate::api::types::StarMapMetaDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_batch = self.begin_contribution_batch();
        let starmap_id = format!("sm_{}", uuid::Uuid::new_v4());
        let mut contribution_txn =
            self.begin_starmap_creation_contribution_transaction(&starmap_id)?;
        let history_token = crate::storage::journal::workspace_change::new_token();
        self.register_starmap_history_journal(&mut contribution_txn, &history_token)?;
        //   用 _with_changes 版本拿变更集，
        // 调 record_workspace_change_set_history 记录本地历史。
        let (result, change_set) = self
            .core_write()
            .create_starmap_with_id_with_changes(&starmap_id, title, desc, template_id)
            .map_err(WriterError::from)?;
        let history_journal = self.save_starmap_history_journal(
            &history_token,
            &change_set,
            crate::storage::journal::workspace_change::WorkspaceChangeOpType::CreateStarMap,
        )?;
        let project_id = result.project_id.as_deref();
        let entry = crate::search::extractor::extract_starmap_title_entry(
            &result.starmap_id,
            project_id,
            title,
        );
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        let context = crate::writing_stats::current_mutation_context();
        self.record_starmap_meta_with_context(
            &context,
            project_id,
            &result.starmap_id,
            crate::writing_stats::ContributionOperation::Create,
            None,
            self.core_write()
                .get_starmap(&result.starmap_id)
                .ok()
                .as_ref(),
            crate::starmap::load_index(&self.app_data_root)
                .ok()
                .map(|index| index.root_starmap_ids),
        )?;
        if let Some(graph) = self.get_starmap_graph_snapshot(&result.starmap_id) {
            for node in &graph.nodes {
                self.record_starmap_object_with_context(
                    &context,
                    project_id,
                    &result.starmap_id,
                    crate::writing_stats::ContributionObjectType::StarMapNode,
                    &node.id,
                    crate::writing_stats::ContributionOperation::Create,
                    None,
                    Some(&graph),
                )?;
            }
            for edge in &graph.edges {
                self.record_starmap_object_with_context(
                    &context,
                    project_id,
                    &result.starmap_id,
                    crate::writing_stats::ContributionObjectType::StarMapEdge,
                    &edge.id,
                    crate::writing_stats::ContributionOperation::Create,
                    None,
                    Some(&graph),
                )?;
            }
            for embed in &graph.embeds {
                self.record_starmap_object_with_context(
                    &context,
                    project_id,
                    &result.starmap_id,
                    crate::writing_stats::ContributionObjectType::StarMapEmbed,
                    &embed.instance_id,
                    crate::writing_stats::ContributionOperation::Create,
                    None,
                    Some(&graph),
                )?;
            }
            for link in &graph.links {
                self.record_starmap_object_with_context(
                    &context,
                    project_id,
                    &result.starmap_id,
                    crate::writing_stats::ContributionObjectType::StarMapLink,
                    &link.link_id,
                    crate::writing_stats::ContributionOperation::Create,
                    None,
                    Some(&graph),
                )?;
            }
        }
        self.commit_contribution_batch(&mut contribution_batch)?;
        self.finish_starmap_contribution_transaction(&result.starmap_id, &mut contribution_txn)?;
        if let Err(e) = history_journal.mark_local_applied(&self.app_data_root) {
            log::warn!(
                "create_starmap: mark_local_applied failed for {}: {} — journal retained",
                history_journal.token,
                e
            );
        }
        self.finish_starmap_history_journal(&history_journal, "create_starmap");
        let result_dto: crate::api::types::StarMapMetaDto = result.into();
        if let Err(error) = crate::storage::workspace_events::append_structure_event(
            &self.app_data_root,
            &result_dto.starmap_id,
            "starmap",
            &result_dto.starmap_id,
        ) {
            log::warn!("failed to append create_starmap structure event: {error}");
        }
        Ok(result_dto)
    }

    pub fn add_starmap_node(
        &self,
        starmap_id: &str,
        node: crate::api::types::StarMapNodeDto,
        x: f64,
        y: f64,
    ) -> ApiResult<crate::api::types::StarMapNodeDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_txn = self.begin_starmap_store_transaction(starmap_id)?;
        contribution_txn.prepare_paths(&[
            crate::starmap::library::library_index_abs_path(&self.app_data_root),
            crate::starmap::library::reference_index_abs_path(&self.app_data_root),
            crate::starmap::library::node_definition_abs_path(
                &self.app_data_root,
                &node.id,
            ),
        ])?;
        let result = self
            .core_write()
            .add_starmap_node(
                starmap_id,
                node.try_into().map_err(WriterError::from)?,
                x,
                y,
            )
            .map_err(WriterError::from)?;
        let node_content = extract_node_search_body(&result.content, &result.tags);
        let project_id = get_starmap_project_id(self, starmap_id);
        let entry = crate::search::extractor::extract_starmap_node_entry(
            starmap_id,
            &result.id,
            project_id.as_deref(),
            &result.title,
            &node_content,
        );
        contribution_txn.enqueue_search_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        let after = self.get_starmap_graph_snapshot(starmap_id);
        self.record_starmap_object(
            project_id.as_deref(),
            starmap_id,
            crate::writing_stats::ContributionObjectType::StarMapNode,
            &result.id,
            crate::writing_stats::ContributionOperation::Create,
            None,
            after.as_ref(),
        )?;
        self.finish_starmap_contribution_transaction(starmap_id, &mut contribution_txn)?;
        let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
            .add_upsert(crate::starmap::package_storage::node_rel_path(
                starmap_id,
                &result.id,
            ))
            .add_upsert(crate::starmap::package_storage::graph_meta_rel_path(
                starmap_id,
            ))
            .add_upsert(crate::starmap::library::library_index_rel_path())
            .add_upsert(crate::starmap::library::reference_index_rel_path())
            .add_upsert(crate::starmap::library::node_definition_rel_path(
                &result.id,
            ));
        if let Err(error) = self.record_workspace_change_set_history(&change_set, "add_starmap_node")
        {
            log::warn!("add_starmap_node history failed: {error}");
        }
        Ok(result.into())
    }

    pub fn rename_starmap(
        &self,
        starmap_id: &str,
        new_title: &str,
    ) -> ApiResult<crate::api::types::StarMapMetaDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_txn = self.begin_starmap_contribution_transaction(starmap_id)?;
        let history_token = crate::storage::journal::workspace_change::new_token();
        // rename 直接写 meta：必须在修改前登记具体文件，来源事务才能回滚。
        contribution_txn.prepare_paths(&[self
            .app_data_root
            .join("starmaps")
            .join(format!("{}.meta.json", starmap_id))])?;
        self.register_starmap_history_journal(&mut contribution_txn, &history_token)?;
        //   用 _with_changes 版本记录本地历史。
        let before_meta = self.core_write().get_starmap(starmap_id).ok();
        let (result, change_set) = self
            .core_write()
            .rename_starmap_with_changes(starmap_id, new_title)
            .map_err(WriterError::from)?;
        let history_journal = self.save_starmap_history_journal(
            &history_token,
            &change_set,
            crate::storage::journal::workspace_change::WorkspaceChangeOpType::RenameStarMap,
        )?;
        let project_id = get_starmap_project_id(self, starmap_id);
        let entry = crate::search::extractor::extract_starmap_title_entry(
            starmap_id,
            project_id.as_deref(),
            new_title,
        );
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        self.record_starmap_meta_with_context(
            &crate::writing_stats::current_mutation_context(),
            project_id.as_deref(),
            starmap_id,
            crate::writing_stats::ContributionOperation::Update,
            before_meta.as_ref(),
            self.core_write().get_starmap(starmap_id).ok().as_ref(),
            None,
        )?;
        self.finish_starmap_contribution_transaction(starmap_id, &mut contribution_txn)?;
        if let Err(e) = history_journal.mark_local_applied(&self.app_data_root) {
            log::warn!(
                "rename_starmap: mark_local_applied failed for {}: {} — journal retained",
                history_journal.token,
                e
            );
        }
        self.finish_starmap_history_journal(&history_journal, "rename_starmap");
        let result_dto: crate::api::types::StarMapMetaDto = result.into();
        Ok(result_dto)
    }

    pub fn delete_starmap(&self, starmap_id: &str) -> ApiResult<bool> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_txn =
            self.begin_starmap_delete_contribution_transaction(starmap_id)?;
        let history_token = crate::storage::journal::workspace_change::new_token();
        // 删除 journal 本身也是本次来源事务的产物：先登记为“原本不存在”，
        // 来源回滚时随对象一起删除，避免恢复流程重放已回滚的删除。
        self.register_starmap_history_journal(&mut contribution_txn, &history_token)?;
        let mut contribution_batch = self.begin_contribution_batch();
        //   durable 删除事务：先 plan（不碰磁盘，构造完整 PlannedStarmapDelete
        // 含固定 trash 路径和完整 sync_delete_facts），再 save_pending 落盘 journal，
        // 再 apply planned delete（rename + 写 tombstone），再推进 journal 阶段。
        // 只有 save_pending 成功后才允许动本地文件，保证删除事实在物理删除前已持久化。
        // Git history 只在本来源事务提交后补记；history 失败时 journal 保留，
        // 由启动恢复用同一 token 幂等补记。
        //
        //   搜索索引是派生数据，必须在删除事实 durable 成功后才删除。
        // 之前一进函数就删搜索索引，如果 plan_delete_starmap 因外部引用拒绝删除、
        // 或 save_pending 失败，星图还在但搜索索引已经没了。现在把搜索索引删除
        // 移到 apply_planned_delete_starmap 成功 + mark_local_applied 之后。
        let before_meta = self.core_write().get_starmap(starmap_id).ok();
        let contribution_project = before_meta
            .as_ref()
            .and_then(|meta| meta.project_id.clone())
            .unwrap_or_default();
        let before_graph = self.get_starmap_graph_snapshot(starmap_id);
        let device_id = crate::settings::load_device_info(&self.app_data_root)
            .map(|i| i.device_id)
            .unwrap_or_default();
        // 先 flush 全部 dirty starmap stores，确保引用扫描读到最新磁盘数据。
        self.core_write().flush_all_starmap_stores()?;
        self.core_write().remove_starmap_store(starmap_id);
        let (change_set, planned) = crate::storage::journal::starmap_delete::plan_delete_starmap(
            &self.app_data_root,
            starmap_id,
            &device_id,
        )
        .map_err(WriterError::from)?;
        // 先落盘 journal（phase=Pending，包含完整 facts/固定 trash path），确保崩溃后能恢复。
        let planned_workspace = planned.to_workspace_planned();
        let journal =
            crate::storage::journal::workspace_change::WorkspaceChangeJournal::save_pending_with_token(
                &self.app_data_root,
                &history_token,
                &change_set,
                &device_id,
                crate::storage::journal::workspace_change::WorkspaceChangeOpType::DeleteStarMap,
                Some(planned_workspace.delete_target.clone()),
                Some(planned_workspace.clone()),
            )
            .map_err(|e| {
                log::warn!(
                    "delete_starmap: save_pending journal failed: {} — aborting before physical delete",
                    e
                );
                WriterError::Other(format!("delete_starmap: save_pending journal failed: {e}"))
            })?;
        // journal 落盘成功，执行物理删除（消费 plan 中固定的 trash 路径和 facts）。
        if let Err(e) = crate::storage::journal::starmap_delete::apply_planned_delete_starmap(
            &self.app_data_root,
            starmap_id,
            &planned,
        )
        .map_err(WriterError::from)
        {
            log::warn!(
                "delete_starmap: physical delete failed: {} — journal retained for recovery",
                e
            );
            return Err(e);
        }
        // 物理删除成功后失效路径依赖索引：remove_starmap_store 已在删除前失效一次，
        // 但期间可能有其他线程惰性重建了索引。物理删除后磁盘数据已变，再次失效
        // 确保索引不持有已删除图的 entry（问题1）。
        self.core_write().invalidate_path_dependency_index();
        // 本地删除已完成，推进到 LocalApplied。
        if let Err(e) = journal.mark_local_applied(&self.app_data_root) {
            log::warn!(
                "delete_starmap: mark_local_applied failed: {} — journal retained for recovery",
                e
            );
        }
        //   搜索索引删除：本地删除已 durable 成功（apply + mark_local_applied），
        // 现在才清派生搜索索引。搜索索引是派生数据，不能先于删除事实改变。
        for prefix in &[
            format!("starmap:{}", starmap_id),
            format!("starmap_node:{}:", starmap_id),
            format!("starmap_edge:{}:", starmap_id),
            format!("starmap_link:{}:", starmap_id),
            format!("starmap_embed:{}:", starmap_id),
        ] {
            self.remove_search_index_by_prefix(prefix);
        }
        let context = crate::writing_stats::current_mutation_context();
        self.record_starmap_meta_with_context(
            &context,
            Some(&contribution_project),
            starmap_id,
            crate::writing_stats::ContributionOperation::Delete,
            before_meta.as_ref(),
            None,
            crate::starmap::load_index(&self.app_data_root)
                .ok()
                .map(|index| index.root_starmap_ids),
        )?;
        if let Some(graph) = before_graph {
            for node in &graph.nodes {
                self.record_starmap_object_with_context(
                    &context,
                    Some(&contribution_project),
                    starmap_id,
                    crate::writing_stats::ContributionObjectType::StarMapNode,
                    &node.id,
                    crate::writing_stats::ContributionOperation::Delete,
                    Some(&graph),
                    None,
                )?;
            }
            for edge in &graph.edges {
                self.record_starmap_object_with_context(
                    &context,
                    Some(&contribution_project),
                    starmap_id,
                    crate::writing_stats::ContributionObjectType::StarMapEdge,
                    &edge.id,
                    crate::writing_stats::ContributionOperation::Delete,
                    Some(&graph),
                    None,
                )?;
            }
            for embed in &graph.embeds {
                self.record_starmap_object_with_context(
                    &context,
                    Some(&contribution_project),
                    starmap_id,
                    crate::writing_stats::ContributionObjectType::StarMapEmbed,
                    &embed.instance_id,
                    crate::writing_stats::ContributionOperation::Delete,
                    Some(&graph),
                    None,
                )?;
            }
            for link in &graph.links {
                self.record_starmap_object_with_context(
                    &context,
                    Some(&contribution_project),
                    starmap_id,
                    crate::writing_stats::ContributionObjectType::StarMapLink,
                    &link.link_id,
                    crate::writing_stats::ContributionOperation::Delete,
                    Some(&graph),
                    None,
                )?;
            }
        }
        self.commit_contribution_batch(&mut contribution_batch)?;
        // 对象删除与来源事件 durable 后才允许写 Git history；journal 处于
        // LocalApplied，history 失败会在下次启动用同一 token 幂等补记。
        contribution_txn.commit()?;
        self.finish_starmap_history_journal(&journal, "delete_starmap");
        if let Err(error) = crate::storage::workspace_events::append_structure_event(
            &self.app_data_root,
            starmap_id,
            "starmap",
            starmap_id,
        ) {
            log::warn!("failed to append delete_starmap structure event: {error}");
        }
        let library_target =
            crate::starmap::types::StarMapAssetRef::starmap(starmap_id.to_string());
        if let Err(error) =
            crate::starmap::library::remove_entries_for_asset(&self.app_data_root, &library_target)
        {
            log::warn!("delete_starmap: failed to remove library entry: {error}");
        }
        if let Err(error) = crate::starmap::library::remove_starmap_instance_records(
            &self.app_data_root,
            starmap_id,
        ) {
            log::warn!("delete_starmap: failed to remove library node instances: {error}");
        }
        Ok(true)
    }

    // TODO(#597): 既有代码可读性技术债，待后续重构拆分
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn bind_starmap_to_project(&self, starmap_id: &str, project_id: &str) -> ApiResult<bool> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_txn = self.begin_starmap_contribution_transaction(starmap_id)?;
        let history_token = crate::storage::journal::workspace_change::new_token();
        // bind 直接写 meta，并可能先写 index；两者都要进入来源事务备份清单。
        contribution_txn.prepare_paths(&[
            self.app_data_root
                .join("starmaps")
                .join(format!("{}.meta.json", starmap_id)),
            self.app_data_root.join("starmaps/index.json"),
        ])?;
        self.register_starmap_history_journal(&mut contribution_txn, &history_token)?;
        let before_meta = self.core_write().get_starmap(starmap_id).ok();
        //   用 _with_changes 版本记录本地历史。
        let change_set = self
            .core_write()
            .bind_starmap_to_project_with_changes(starmap_id, project_id)?;
        let history_journal = self.save_starmap_history_journal(
            &history_token,
            &change_set,
            crate::storage::journal::workspace_change::WorkspaceChangeOpType::BindStarMap,
        )?;
        let meta = self
            .core_write()
            .get_starmap(starmap_id)
            .map_err(WriterError::from)?;
        let entry = crate::search::extractor::extract_starmap_title_entry(
            starmap_id,
            Some(project_id),
            &meta.title,
        );
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        let graph_result = self.core_write().get_starmap_graph(starmap_id);
        if let Ok(graph) = graph_result {
            for node in &graph.nodes {
                let node_content = extract_node_search_body(&node.content, &node.tags);
                let entry = crate::search::extractor::extract_starmap_node_entry(
                    starmap_id,
                    &node.id,
                    Some(project_id),
                    &node.title,
                    &node_content,
                );
                self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                    action: crate::search::SearchIndexAction::Upsert,
                    object_id: entry.object_id.clone(),
                    scope: entry.scope,
                    title: entry.title.clone(),
                    body: entry.body.clone(),
                    target: Some(entry.target.clone()),
                });
            }
            for edge in &graph.edges {
                let label = edge.label.as_deref().unwrap_or("");
                if !label.is_empty() {
                    let entry = crate::search::extractor::extract_starmap_edge_entry(
                        starmap_id,
                        &edge.id,
                        Some(project_id),
                        label,
                    );
                    self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                        action: crate::search::SearchIndexAction::Upsert,
                        object_id: entry.object_id.clone(),
                        scope: entry.scope,
                        title: entry.title.clone(),
                        body: entry.body.clone(),
                        target: Some(entry.target.clone()),
                    });
                }
            }
            for link in &graph.links {
                let label = link.label.as_deref().unwrap_or("");
                if !label.is_empty() {
                    let entry = crate::search::extractor::extract_starmap_link_entry(
                        starmap_id,
                        &link.link_id,
                        Some(project_id),
                        label,
                    );
                    self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                        action: crate::search::SearchIndexAction::Upsert,
                        object_id: entry.object_id.clone(),
                        scope: entry.scope,
                        title: entry.title.clone(),
                        body: entry.body.clone(),
                        target: Some(entry.target.clone()),
                    });
                }
            }
            for embed in &graph.embeds {
                let label = embed.label.as_deref().unwrap_or("");
                let entry = crate::search::extractor::extract_starmap_embed_entry(
                    starmap_id,
                    &embed.instance_id,
                    Some(project_id),
                    label,
                );
                self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                    action: crate::search::SearchIndexAction::Upsert,
                    object_id: entry.object_id.clone(),
                    scope: entry.scope,
                    title: entry.title.clone(),
                    body: entry.body.clone(),
                    target: Some(entry.target.clone()),
                });
            }
        }
        self.record_starmap_meta_with_context(
            &crate::writing_stats::current_mutation_context(),
            Some(project_id),
            starmap_id,
            crate::writing_stats::ContributionOperation::Link,
            before_meta.as_ref(),
            Some(&meta),
            None,
        )?;
        self.finish_starmap_contribution_transaction(starmap_id, &mut contribution_txn)?;
        if let Err(e) = history_journal.mark_local_applied(&self.app_data_root) {
            log::warn!(
                "bind_starmap_to_project: mark_local_applied failed for {}: {} — journal retained",
                history_journal.token,
                e
            );
        }
        self.finish_starmap_history_journal(&history_journal, "bind_starmap_to_project");
        if let Err(error) = crate::storage::workspace_events::append_structure_event(
            &self.app_data_root,
            starmap_id,
            "starmap_binding",
            project_id,
        ) {
            log::warn!("failed to append bind structure event: {error}");
        }
        Ok(true)
    }

    // TODO(#597): 既有代码可读性技术债，待后续重构拆分
    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn unbind_starmap_from_project(&self, starmap_id: &str) -> ApiResult<bool> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_txn = self.begin_starmap_contribution_transaction(starmap_id)?;
        let history_token = crate::storage::journal::workspace_change::new_token();
        // unbind 直接写 meta，并可能先写 index；两者都要进入来源事务备份清单。
        contribution_txn.prepare_paths(&[
            self.app_data_root
                .join("starmaps")
                .join(format!("{}.meta.json", starmap_id)),
            self.app_data_root.join("starmaps/index.json"),
        ])?;
        self.register_starmap_history_journal(&mut contribution_txn, &history_token)?;
        let before_meta = self.core_write().get_starmap(starmap_id).ok();
        //   用 _with_changes 版本记录本地历史。
        let change_set = self
            .core_write()
            .unbind_starmap_from_project_with_changes(starmap_id)?;
        let history_journal = self.save_starmap_history_journal(
            &history_token,
            &change_set,
            crate::storage::journal::workspace_change::WorkspaceChangeOpType::UnbindStarMap,
        )?;
        let meta = self
            .core_write()
            .get_starmap(starmap_id)
            .map_err(WriterError::from)?;
        let entry =
            crate::search::extractor::extract_starmap_title_entry(starmap_id, None, &meta.title);
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        let graph_result = self.core_write().get_starmap_graph(starmap_id);
        if let Ok(graph) = graph_result {
            for node in &graph.nodes {
                let node_content = extract_node_search_body(&node.content, &node.tags);
                let entry = crate::search::extractor::extract_starmap_node_entry(
                    starmap_id,
                    &node.id,
                    None,
                    &node.title,
                    &node_content,
                );
                self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                    action: crate::search::SearchIndexAction::Upsert,
                    object_id: entry.object_id.clone(),
                    scope: entry.scope,
                    title: entry.title.clone(),
                    body: entry.body.clone(),
                    target: Some(entry.target.clone()),
                });
            }
            for edge in &graph.edges {
                let label = edge.label.as_deref().unwrap_or("");
                if !label.is_empty() {
                    let entry = crate::search::extractor::extract_starmap_edge_entry(
                        starmap_id, &edge.id, None, label,
                    );
                    self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                        action: crate::search::SearchIndexAction::Upsert,
                        object_id: entry.object_id.clone(),
                        scope: entry.scope,
                        title: entry.title.clone(),
                        body: entry.body.clone(),
                        target: Some(entry.target.clone()),
                    });
                }
            }
            for link in &graph.links {
                let label = link.label.as_deref().unwrap_or("");
                if !label.is_empty() {
                    let entry = crate::search::extractor::extract_starmap_link_entry(
                        starmap_id,
                        &link.link_id,
                        None,
                        label,
                    );
                    self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                        action: crate::search::SearchIndexAction::Upsert,
                        object_id: entry.object_id.clone(),
                        scope: entry.scope,
                        title: entry.title.clone(),
                        body: entry.body.clone(),
                        target: Some(entry.target.clone()),
                    });
                }
            }
            for embed in &graph.embeds {
                let label = embed.label.as_deref().unwrap_or("");
                let entry = crate::search::extractor::extract_starmap_embed_entry(
                    starmap_id,
                    &embed.instance_id,
                    None,
                    label,
                );
                self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                    action: crate::search::SearchIndexAction::Upsert,
                    object_id: entry.object_id.clone(),
                    scope: entry.scope,
                    title: entry.title.clone(),
                    body: entry.body.clone(),
                    target: Some(entry.target.clone()),
                });
            }
        }
        let contribution_project = before_meta
            .as_ref()
            .and_then(|meta| meta.project_id.clone())
            .unwrap_or_default();
        self.record_starmap_meta_with_context(
            &crate::writing_stats::current_mutation_context(),
            Some(&contribution_project),
            starmap_id,
            crate::writing_stats::ContributionOperation::Unlink,
            before_meta.as_ref(),
            Some(&meta),
            None,
        )?;
        self.finish_starmap_contribution_transaction(starmap_id, &mut contribution_txn)?;
        if let Err(e) = history_journal.mark_local_applied(&self.app_data_root) {
            log::warn!(
                "unbind_starmap_from_project: mark_local_applied failed for {}: {} — \
                 journal retained",
                history_journal.token,
                e
            );
        }
        self.finish_starmap_history_journal(&history_journal, "unbind_starmap_from_project");
        if let Err(error) = crate::storage::workspace_events::append_structure_event(
            &self.app_data_root,
            starmap_id,
            "starmap_unbind",
            starmap_id,
        ) {
            log::warn!("failed to append unbind structure event: {error}");
        }
        Ok(true)
    }

    ///   刷新单个 starmap 在搜索索引中的条目。
    ///
    /// 只更新搜索索引，不调用 core 层 unbind，不记录 history。
    /// 供 `delete_project` 在 core 层 `unbind_starmaps` 成功后刷新搜索索引使用。
    /// 读取最新的 starmap meta + graph，把 title/node/edge/link/embed
    /// 条目重新 upsert 到搜索索引（project_id 已被 core 层清成 None）。
    #[allow(clippy::too_many_lines, clippy::excessive_nesting)]
    pub(crate) fn refresh_starmap_search_index(&self, starmap_id: &str) {
        let meta = match self.core_write().get_starmap(starmap_id) {
            Ok(m) => m,
            Err(_) => return,
        };
        let entry =
            crate::search::extractor::extract_starmap_title_entry(starmap_id, None, &meta.title);
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        let graph_result = self.core_write().get_starmap_graph(starmap_id);
        if let Ok(graph) = graph_result {
            for node in &graph.nodes {
                let node_content = extract_node_search_body(&node.content, &node.tags);
                let entry = crate::search::extractor::extract_starmap_node_entry(
                    starmap_id,
                    &node.id,
                    None,
                    &node.title,
                    &node_content,
                );
                self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                    action: crate::search::SearchIndexAction::Upsert,
                    object_id: entry.object_id.clone(),
                    scope: entry.scope,
                    title: entry.title.clone(),
                    body: entry.body.clone(),
                    target: Some(entry.target.clone()),
                });
            }
            for edge in &graph.edges {
                let label = edge.label.as_deref().unwrap_or("");
                if !label.is_empty() {
                    let entry = crate::search::extractor::extract_starmap_edge_entry(
                        starmap_id, &edge.id, None, label,
                    );
                    self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                        action: crate::search::SearchIndexAction::Upsert,
                        object_id: entry.object_id.clone(),
                        scope: entry.scope,
                        title: entry.title.clone(),
                        body: entry.body.clone(),
                        target: Some(entry.target.clone()),
                    });
                }
            }
            for link in &graph.links {
                let label = link.label.as_deref().unwrap_or("");
                if !label.is_empty() {
                    let entry = crate::search::extractor::extract_starmap_link_entry(
                        starmap_id,
                        &link.link_id,
                        None,
                        label,
                    );
                    self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                        action: crate::search::SearchIndexAction::Upsert,
                        object_id: entry.object_id.clone(),
                        scope: entry.scope,
                        title: entry.title.clone(),
                        body: entry.body.clone(),
                        target: Some(entry.target.clone()),
                    });
                }
            }
            for embed in &graph.embeds {
                let label = embed.label.as_deref().unwrap_or("");
                let entry = crate::search::extractor::extract_starmap_embed_entry(
                    starmap_id,
                    &embed.instance_id,
                    None,
                    label,
                );
                self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                    action: crate::search::SearchIndexAction::Upsert,
                    object_id: entry.object_id.clone(),
                    scope: entry.scope,
                    title: entry.title.clone(),
                    body: entry.body.clone(),
                    target: Some(entry.target.clone()),
                });
            }
        }
    }

    pub fn set_main_starmap_for_project(
        &self,
        starmap_id: &str,
        project_id: &str,
    ) -> ApiResult<bool> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_txn = self.begin_starmap_contribution_transaction(starmap_id)?;
        let history_token = crate::storage::journal::workspace_change::new_token();
        // set_main 直接写 starmaps/index.json；先登记备份并按统一阶段补 history。
        contribution_txn.prepare_paths(&[self.app_data_root.join("starmaps/index.json")])?;
        self.register_starmap_history_journal(&mut contribution_txn, &history_token)?;
        let mut contribution_batch = self.begin_contribution_batch();
        //   用 _with_changes 版本记录本地历史。
        let previous = self.get_main_starmap_for_project(project_id).ok().flatten();
        let change_set = self
            .core_write()
            .set_main_starmap_for_project_with_changes(starmap_id, project_id)
            .map_err(crate::api::error::WriterError::from)?;
        let history_journal = self.save_starmap_history_journal(
            &history_token,
            &change_set,
            crate::storage::journal::workspace_change::WorkspaceChangeOpType::SetMainStarMap,
        )?;
        let context = crate::writing_stats::current_mutation_context();
        if let Some(previous) = previous.filter(|meta| meta.starmap_id != starmap_id) {
            let before = self.core_write().get_starmap(&previous.starmap_id).ok();
            self.record_starmap_meta_with_context(
                &context,
                Some(project_id),
                &previous.starmap_id,
                crate::writing_stats::ContributionOperation::Unlink,
                before.as_ref(),
                None,
                None,
            )?;
        }
        let after = self.core_write().get_starmap(starmap_id).ok();
        self.record_starmap_meta_with_context(
            &context,
            Some(project_id),
            starmap_id,
            crate::writing_stats::ContributionOperation::Link,
            None,
            after.as_ref(),
            None,
        )?;
        self.commit_contribution_batch(&mut contribution_batch)?;
        self.finish_starmap_contribution_transaction(starmap_id, &mut contribution_txn)?;
        if let Err(e) = history_journal.mark_local_applied(&self.app_data_root) {
            log::warn!(
                "set_main_starmap_for_project: mark_local_applied failed for {}: {} — \
                 journal retained",
                history_journal.token,
                e
            );
        }
        self.finish_starmap_history_journal(&history_journal, "set_main_starmap_for_project");
        Ok(true)
    }

    pub fn get_main_starmap_for_project(
        &self,
        project_id: &str,
    ) -> ApiResult<Option<crate::api::types::StarMapMetaDto>> {
        self.core_write()
            .get_main_starmap_for_project(project_id)
            .map(|opt| opt.map(Into::into))
            .map_err(Into::into)
    }

    pub fn update_starmap_node(
        &self,
        starmap_id: &str,
        node_id: &str,
        patch: crate::api::types::StarMapNodePatchDto,
    ) -> ApiResult<crate::api::types::StarMapNodeDto> {
        self.update_starmap_node_expected(starmap_id, node_id, patch, None)
    }

    /// 更新节点，并校验调用方读取到的对象版本。
    ///
    /// `expected_revision` 来自 `StarMapNodeDto.updated_at`。版本不一致时返回
    /// `WriterError::Conflict`，调用方必须重新读取后再提交，不能覆盖别人的修改。
    pub fn update_starmap_node_expected(
        &self,
        starmap_id: &str,
        node_id: &str,
        patch: crate::api::types::StarMapNodePatchDto,
        expected_revision: Option<u64>,
    ) -> ApiResult<crate::api::types::StarMapNodeDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_txn = self.begin_starmap_store_transaction(starmap_id)?;
        let before = self.get_starmap_graph_snapshot(starmap_id);
        ensure_starmap_revision(
            before
                .as_ref()
                .and_then(|graph| graph.nodes.iter().find(|node| node.id == node_id))
                .map(|node| node.updated_at),
            expected_revision,
            "node",
            node_id,
        )?;
        let trajectory = patch.trajectory.clone();
        let shared_changed = node_patch_has_shared_fields(&patch);
        let position_changed = patch.position.is_some();
        let definition_asset = if shared_changed {
            Some(
                self.starmap_asset_bindings(starmap_id)?
                    .into_iter()
                    .find(|binding| binding.instance_id == node_id)
                    .map(|binding| binding.asset_id)
                    .unwrap_or_else(|| node_id.to_string()),
            )
        } else {
            None
        };
        let affected_instances = if shared_changed {
            self.shared_node_instances(starmap_id, node_id)?
        } else {
            vec![(starmap_id.to_string(), node_id.to_string())]
        };
        let mut affected_starmaps: Vec<String> = affected_instances
            .iter()
            .map(|(owner, _)| owner.clone())
            .collect();
        affected_starmaps.sort();
        affected_starmaps.dedup();
        contribution_txn.include_starmaps(&affected_starmaps);
        let mut before_graphs = std::collections::HashMap::new();
        if let Some(graph) = before.as_ref() {
            before_graphs.insert(starmap_id.to_string(), graph.clone());
        }
        for affected_starmap in &affected_starmaps {
            if before_graphs.contains_key(affected_starmap) {
                continue;
            }
            let graph = self.read_contribution_starmap_graph(affected_starmap)?;
            before_graphs.insert(affected_starmap.clone(), graph);
        }
        if let Some(definition_asset) = definition_asset.as_deref() {
            // 共享定义是唯一内容源；实例文件不因内容修改而重写。共享定义、
            // 库归属索引和反向索引都会在 facade 更新阶段直接落盘，必须在
            // 调用前登记，提交失败时才能一起恢复。
            contribution_txn.prepare_paths(&[
                crate::starmap::library::node_definition_abs_path(
                    &self.app_data_root,
                    definition_asset,
                ),
                crate::starmap::library::reference_index_abs_path(&self.app_data_root),
                crate::starmap::library::library_index_abs_path(&self.app_data_root),
            ])?;
        }
        let result = self
            .core_write()
            .update_starmap_node(
                starmap_id,
                node_id,
                patch.try_into().map_err(WriterError::from)?,
            )
            .map_err(WriterError::from)?;
        let project_id = get_starmap_project_id(self, starmap_id);
        let after = self.get_starmap_graph_snapshot(starmap_id);
        let mut after_graphs = std::collections::HashMap::new();
        if let Some(graph) = after.as_ref() {
            after_graphs.insert(starmap_id.to_string(), graph.clone());
        }
        for affected_starmap in &affected_starmaps {
            if after_graphs.contains_key(affected_starmap) {
                continue;
            }
            let graph = self.read_contribution_starmap_graph(affected_starmap)?;
            after_graphs.insert(affected_starmap.clone(), graph);
        }
        let context = crate::writing_stats::current_mutation_context();
        let propagated_context = crate::writing_stats::MutationContext::system()
            .with_transaction_id(context.transaction_id().to_string());
        let mut origin_recorded = false;
        for (affected_starmap, affected_instance) in &affected_instances {
            let Some(after_graph) = after_graphs.get(affected_starmap) else {
                return Err(WriterError::Other(format!(
                    "shared node update: missing updated graph for starmap {affected_starmap}"
                )));
            };
            let Some(after_node) = after_graph
                .nodes
                .iter()
                .find(|node| node.id == *affected_instance)
            else {
                return Err(WriterError::Other(format!(
                    "shared node update: instance {affected_instance} missing from starmap \
                     {affected_starmap} after update"
                )));
            };
            self.enqueue_starmap_node_search_update(
                &mut contribution_txn,
                affected_starmap,
                after_node,
            );
            let before_graph = before_graphs.get(affected_starmap);
            if affected_starmap == starmap_id && affected_instance == node_id {
                self.record_starmap_object_with_context_and_trajectory(
                    &context,
                    project_id.as_deref(),
                    starmap_id,
                    crate::writing_stats::ContributionObjectType::StarMapNode,
                    node_id,
                    crate::writing_stats::ContributionOperation::Update,
                    before_graph,
                    Some(after_graph),
                    &trajectory,
                )?;
                origin_recorded = true;
            } else {
                let affected_project = get_starmap_project_id(self, affected_starmap);
                self.record_starmap_object_with_context_and_trajectory(
                    &propagated_context,
                    affected_project.as_deref(),
                    affected_starmap,
                    crate::writing_stats::ContributionObjectType::StarMapNode,
                    affected_instance,
                    crate::writing_stats::ContributionOperation::Link,
                    before_graph,
                    Some(after_graph),
                    &[],
                )?;
            }
        }
        if !origin_recorded {
            self.enqueue_starmap_node_search_update(&mut contribution_txn, starmap_id, &result);
            self.record_starmap_object_with_context_and_trajectory(
                &context,
                project_id.as_deref(),
                starmap_id,
                crate::writing_stats::ContributionObjectType::StarMapNode,
                node_id,
                crate::writing_stats::ContributionOperation::Update,
                before.as_ref(),
                after.as_ref(),
                &trajectory,
            )?;
        }
        self.finish_starmaps_contribution_transaction(&affected_starmaps, &mut contribution_txn)?;
        let mut change_set = crate::storage::workspace_git::WorkspaceChangeSet::new();
        if position_changed {
            change_set = change_set
                .add_upsert(crate::starmap::package_storage::node_rel_path(
                    starmap_id, node_id,
                ))
                .add_upsert(crate::starmap::package_storage::graph_meta_rel_path(
                    starmap_id,
                ));
        }
        if let Some(definition_asset) = definition_asset.as_deref() {
            change_set = change_set
                .add_upsert(crate::starmap::library::node_definition_rel_path(
                    definition_asset,
                ))
                .add_upsert(crate::starmap::library::library_index_rel_path());
        }
        if let Err(error) =
            self.record_workspace_change_set_history(&change_set, "update_starmap_node")
        {
            log::warn!("update_starmap_node history failed: {error}");
        }
        Ok(result.into())
    }

    pub fn delete_starmap_node(&self, starmap_id: &str, node_id: &str) -> ApiResult<bool> {
        self.delete_starmap_node_expected(starmap_id, node_id, None)
    }

    pub fn delete_starmap_node_expected(
        &self,
        starmap_id: &str,
        node_id: &str,
        expected_revision: Option<u64>,
    ) -> ApiResult<bool> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_txn = self.begin_starmap_store_transaction(starmap_id)?;
        contribution_txn.prepare_paths(&[
            crate::starmap::library::reference_index_abs_path(&self.app_data_root),
        ])?;
        let before = self.get_starmap_graph_snapshot(starmap_id);
        ensure_starmap_revision(
            before
                .as_ref()
                .and_then(|graph| graph.nodes.iter().find(|node| node.id == node_id))
                .map(|node| node.updated_at),
            expected_revision,
            "node",
            node_id,
        )?;
        let project_id = get_starmap_project_id(self, starmap_id);
        self.core_write().delete_starmap_node(starmap_id, node_id)?;
        contribution_txn.enqueue_search_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Delete,
            object_id: format!("starmap_node:{}:{}", starmap_id, node_id),
            scope: crate::search::SearchScope::All,
            title: String::new(),
            body: String::new(),
            target: None,
        });
        self.record_starmap_object(
            project_id.as_deref(),
            starmap_id,
            crate::writing_stats::ContributionObjectType::StarMapNode,
            node_id,
            crate::writing_stats::ContributionOperation::Delete,
            before.as_ref(),
            None,
        )?;
        self.finish_starmap_contribution_transaction(starmap_id, &mut contribution_txn)?;
        let change_set = crate::storage::workspace_git::WorkspaceChangeSet::new()
            .add_delete(crate::starmap::package_storage::node_rel_path(
                starmap_id,
                node_id,
            ))
            .add_upsert(crate::starmap::package_storage::graph_meta_rel_path(
                starmap_id,
            ))
            .add_upsert(crate::starmap::library::reference_index_rel_path());
        if let Err(error) =
            self.record_workspace_change_set_history(&change_set, "delete_starmap_node")
        {
            log::warn!("delete_starmap_node history failed: {error}");
        }
        Ok(true)
    }

    pub fn add_starmap_edge(
        &self,
        starmap_id: &str,
        edge: crate::api::types::StarMapEdgeDto,
    ) -> ApiResult<crate::api::types::StarMapEdgeDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_txn = self.begin_starmap_store_transaction(starmap_id)?;
        let result = self
            .core_write()
            .add_starmap_edge(starmap_id, edge.try_into().map_err(WriterError::from)?)
            .map_err(WriterError::from)?;
        let label = result.label.clone().unwrap_or_default();
        let project_id = get_starmap_project_id(self, starmap_id);
        let after = self.get_starmap_graph_snapshot(starmap_id);
        self.record_starmap_object(
            project_id.as_deref(),
            starmap_id,
            crate::writing_stats::ContributionObjectType::StarMapEdge,
            &result.id,
            crate::writing_stats::ContributionOperation::Create,
            None,
            after.as_ref(),
        )?;
        let entry = crate::search::extractor::extract_starmap_edge_entry(
            starmap_id,
            &result.id,
            project_id.as_deref(),
            &label,
        );
        contribution_txn.enqueue_search_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        self.finish_starmap_contribution_transaction(starmap_id, &mut contribution_txn)?;
        Ok(result.into())
    }

    pub fn update_starmap_edge(
        &self,
        starmap_id: &str,
        edge_id: &str,
        patch: crate::api::types::StarMapEdgePatchDto,
    ) -> ApiResult<crate::api::types::StarMapEdgeDto> {
        self.update_starmap_edge_expected(starmap_id, edge_id, patch, None)
    }

    pub fn update_starmap_edge_expected(
        &self,
        starmap_id: &str,
        edge_id: &str,
        patch: crate::api::types::StarMapEdgePatchDto,
        expected_revision: Option<u64>,
    ) -> ApiResult<crate::api::types::StarMapEdgeDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_txn = self.begin_starmap_store_transaction(starmap_id)?;
        let before = self.get_starmap_graph_snapshot(starmap_id);
        ensure_starmap_revision(
            before
                .as_ref()
                .and_then(|graph| graph.edges.iter().find(|edge| edge.id == edge_id))
                .map(|edge| edge.updated_at),
            expected_revision,
            "edge",
            edge_id,
        )?;
        let result = self
            .core_write()
            .update_starmap_edge(
                starmap_id,
                edge_id,
                patch.try_into().map_err(WriterError::from)?,
            )
            .map_err(WriterError::from)?;
        let label = result.label.clone().unwrap_or_default();
        let project_id = get_starmap_project_id(self, starmap_id);
        let entry = crate::search::extractor::extract_starmap_edge_entry(
            starmap_id,
            edge_id,
            project_id.as_deref(),
            &label,
        );
        contribution_txn.enqueue_search_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        let after = self.get_starmap_graph_snapshot(starmap_id);
        self.record_starmap_object(
            project_id.as_deref(),
            starmap_id,
            crate::writing_stats::ContributionObjectType::StarMapEdge,
            edge_id,
            crate::writing_stats::ContributionOperation::Update,
            before.as_ref(),
            after.as_ref(),
        )?;
        self.finish_starmap_contribution_transaction(starmap_id, &mut contribution_txn)?;
        Ok(result.into())
    }

    pub fn delete_starmap_edge(&self, starmap_id: &str, edge_id: &str) -> ApiResult<bool> {
        self.delete_starmap_edge_expected(starmap_id, edge_id, None)
    }

    pub fn delete_starmap_edge_expected(
        &self,
        starmap_id: &str,
        edge_id: &str,
        expected_revision: Option<u64>,
    ) -> ApiResult<bool> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_txn = self.begin_starmap_store_transaction(starmap_id)?;
        let before = self.get_starmap_graph_snapshot(starmap_id);
        ensure_starmap_revision(
            before
                .as_ref()
                .and_then(|graph| graph.edges.iter().find(|edge| edge.id == edge_id))
                .map(|edge| edge.updated_at),
            expected_revision,
            "edge",
            edge_id,
        )?;
        let project_id = get_starmap_project_id(self, starmap_id);
        self.core_write().delete_starmap_edge(starmap_id, edge_id)?;
        contribution_txn.enqueue_search_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Delete,
            object_id: format!("starmap_edge:{}:{}", starmap_id, edge_id),
            scope: crate::search::SearchScope::All,
            title: String::new(),
            body: String::new(),
            target: None,
        });
        self.record_starmap_object(
            project_id.as_deref(),
            starmap_id,
            crate::writing_stats::ContributionObjectType::StarMapEdge,
            edge_id,
            crate::writing_stats::ContributionOperation::Delete,
            before.as_ref(),
            None,
        )?;
        self.finish_starmap_contribution_transaction(starmap_id, &mut contribution_txn)?;
        Ok(true)
    }

    pub fn flush_starmap_store(&self, starmap_id: &str) -> ApiResult<bool> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let changed_paths = self
            .core_write()
            .flush_starmap_store(starmap_id)
            .map_err(WriterError::from)?;
        self.record_workspace_paths_history(&changed_paths, "flush_starmap_store");
        Ok(true)
    }

    pub fn close_starmap_store(&self, starmap_id: &str) -> ApiResult<bool> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let changed_paths = self
            .core_write()
            .close_starmap_store(starmap_id)
            .map_err(WriterError::from)?;
        self.record_workspace_paths_history(&changed_paths, "close_starmap_store");
        Ok(true)
    }

    pub fn flush_all_starmap_stores(&self) -> ApiResult<bool> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let changed_paths = self
            .core_write()
            .flush_all_starmap_stores()
            .map_err(WriterError::from)?;
        self.record_workspace_paths_history(&changed_paths, "flush_all_starmap_stores");
        Ok(true)
    }

    pub fn list_starmap_links(
        &self,
        starmap_id: &str,
    ) -> ApiResult<crate::api::types::StarMapLinkListWithDiagnosticsDto> {
        self.core_write()
            .list_starmap_links(starmap_id)
            .map(crate::api::types::StarMapLinkListWithDiagnosticsDto::from)
            .map_err(Into::into)
    }

    pub fn get_starmap_phased_snapshot(
        &self,
        starmap_id: &str,
        request: &crate::api::types::PhasedSnapshotRequestDto,
    ) -> ApiResult<crate::api::types::StarMapPhasedSnapshotDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let core_request: crate::starmap::store::PhasedSnapshotRequest = request.clone().into();
        let mut snapshot: crate::api::types::StarMapPhasedSnapshotDto = self
            .core_write()
            .get_starmap_phased_snapshot(starmap_id, &core_request)
            .map(crate::api::types::StarMapPhasedSnapshotDto::from)
            .map_err(WriterError::from)?;
        snapshot.asset_bindings = self.starmap_asset_bindings(starmap_id)?;
        Ok(snapshot)
    }

    // -----------------------------------------------------------------
    // 星图导航与跨画布连接 API（Issue #902 §5）
    // -----------------------------------------------------------------

    /// 构建 `GraphResolverContext`，包含当前所有已完成后台全量加载的
    /// Store 的 `to_starmap_graph()` 快照作为 overlays。
    ///
    /// 先确保 `root_starmap_id` 的 Store 完全加载（`ensure_fully_loaded`），
    /// 这样它尚未 flush 的 Embed/Portal 变更会作为 overlay 进入 resolver；
    /// 其余已全量加载的 Store 同样作为 overlay，避免读到旧磁盘状态。
    fn build_resolver_context(
        &self,
        root_starmap_id: &str,
    ) -> Result<crate::starmap::graph::resolve::GraphResolverContext, WriterError> {
        let core = self.core_write();
        let mut stores = core
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        // 确保 root starmap 的 Store 完全加载
        {
            let store = stores
                .entry(root_starmap_id.to_string())
                .or_insert_with(|| {
                    crate::starmap::store::StarMapStore::new(&core.app_data_root, root_starmap_id)
                });
            store.ensure_fully_loaded().map_err(|e| {
                WriterError::Other(format!(
                    "build_resolver_context: ensure_fully_loaded failed: {e:?}"
                ))
            })?;
        }

        // 只放入已完成后台全量加载的 Store 的图快照
        let mut overlays = std::collections::HashMap::new();
        for (id, store) in stores.iter() {
            if store.current_load_phase()
                == Some(crate::starmap::store::LoadPhase::BackgroundFullLoad)
            {
                overlays.insert(id.clone(), store.to_starmap_graph());
            }
        }

        Ok(crate::starmap::graph::resolve::GraphResolverContext {
            app_data_root: self.app_data_root.clone(),
            overlays,
        })
    }

    /// 解析星图导航路由，返回当前画布位置。
    ///
    /// 构造 `StarMapNavigationRoute`（root + segments），通过 `GraphResolverContext`
    /// 调用 `resolve_navigation_route`，返回 `StarMapNavigationLocation`。
    /// 解析失败返回错误，平台端不得回退到点击事件传来的裸目标 ID。
    pub fn resolve_starmap_navigation(
        &self,
        root_starmap_id: String,
        segments: Vec<crate::api::types::StarMapPathSegmentDto>,
    ) -> Result<crate::api::types::StarMapNavigationLocationDto, WriterError> {
        let context = self.build_resolver_context(&root_starmap_id)?;

        let core_segments = segments
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>, crate::error::Error>>()
            .map_err(|e| {
                WriterError::Other(format!(
                    "resolve_starmap_navigation: segment conversion failed: {e:?}"
                ))
            })?;

        let route = crate::starmap::navigation::StarMapNavigationRoute {
            root_starmap_id,
            segments: core_segments,
        };

        let location = crate::starmap::navigation::resolve_navigation_route(&context, &route)
            .map_err(|e| {
                WriterError::Other(format!("resolve_starmap_navigation: resolve failed: {e:?}"))
            })?;

        Ok(location.into())
    }

    /// 通过 Embed 实例进入子画布。
    ///
    /// 先解析当前 route 确认当前位置有效，再追加 `EnterEmbed` 段，
    /// 最后重新解析新 route 验证整体有效性。
    /// 返回 `EnterStarmapResultDto`（更新后的 route + location）。
    pub fn enter_starmap_embed(
        &self,
        root_starmap_id: String,
        segments: Vec<crate::api::types::StarMapPathSegmentDto>,
        instance_id: String,
    ) -> Result<crate::api::types::EnterStarmapResultDto, WriterError> {
        let context = self.build_resolver_context(&root_starmap_id)?;

        let core_segments = segments
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>, crate::error::Error>>()
            .map_err(|e| {
                WriterError::Other(format!(
                    "enter_starmap_embed: segment conversion failed: {e:?}"
                ))
            })?;

        let route = crate::starmap::navigation::StarMapNavigationRoute {
            root_starmap_id,
            segments: core_segments,
        };

        let (new_route, new_location) =
            crate::starmap::navigation::enter_embed(&context, &route, &instance_id).map_err(
                |e| WriterError::Other(format!("enter_starmap_embed: enter failed: {e:?}")),
            )?;

        Ok(crate::api::types::EnterStarmapResultDto {
            route: new_route.into(),
            location: new_location.into(),
        })
    }

    /// 通过 Portal 节点进入子画布。
    ///
    /// 先解析当前 route 确认当前位置有效，再追加 `EnterPortal` 段，
    /// 最后重新解析新 route 验证整体有效性。
    /// 返回 `EnterStarmapResultDto`（更新后的 route + location）。
    pub fn enter_starmap_portal(
        &self,
        root_starmap_id: String,
        segments: Vec<crate::api::types::StarMapPathSegmentDto>,
        node_id: String,
    ) -> Result<crate::api::types::EnterStarmapResultDto, WriterError> {
        let context = self.build_resolver_context(&root_starmap_id)?;

        let core_segments = segments
            .into_iter()
            .map(TryInto::try_into)
            .collect::<Result<Vec<_>, crate::error::Error>>()
            .map_err(|e| {
                WriterError::Other(format!(
                    "enter_starmap_portal: segment conversion failed: {e:?}"
                ))
            })?;

        let route = crate::starmap::navigation::StarMapNavigationRoute {
            root_starmap_id,
            segments: core_segments,
        };

        let (new_route, new_location) =
            crate::starmap::navigation::enter_portal(&context, &route, &node_id).map_err(|e| {
                WriterError::Other(format!("enter_starmap_portal: enter failed: {e:?}"))
            })?;

        Ok(crate::api::types::EnterStarmapResultDto {
            route: new_route.into(),
            location: new_location.into(),
        })
    }

    /// 退出当前画布，返回上一层导航路由。
    ///
    /// 纯路径操作：弹出 `route.segments` 最后一个段。已在根图（`segments` 为空）
    /// 时返回原路由。对同一目标星图的多个 Embed，返回的仍是**原进入实例**，
    /// 因为路径段记录了具体的 `instance_id`。
    ///
    /// 不访问磁盘，不需要 resolver context；与 `enter_starmap_embed` /
    /// `enter_starmap_portal` 一起构成 #902 要求的"返回层级"统一 Core API。
    pub fn exit_starmap_canvas(
        &self,
        route: crate::api::types::StarMapNavigationRouteDto,
    ) -> Result<crate::api::types::StarMapNavigationRouteDto, WriterError> {
        let core_route: crate::starmap::navigation::StarMapNavigationRoute =
            route.try_into().map_err(|e: crate::error::Error| {
                WriterError::Other(format!(
                    "exit_starmap_canvas: route conversion failed: {e:?}"
                ))
            })?;

        Ok(crate::starmap::navigation::exit_canvas(&core_route).into())
    }

    /// 跳到指定祖先层级，返回截断后的导航路由。
    ///
    /// `depth` 表示截断后保留的路径段数量：`0` = 根图，`1` = 第一层子图，
    /// `n` = 保留前 `n` 个 segments。`depth >= segments.len()` 时返回原路由。
    ///
    /// 纯路径操作，不访问磁盘；与 `exit_starmap_canvas` 一起补齐
    /// #902 的"返回层级"对外 API。
    pub fn jump_starmap_ancestor(
        &self,
        route: crate::api::types::StarMapNavigationRouteDto,
        depth: u32,
    ) -> Result<crate::api::types::StarMapNavigationRouteDto, WriterError> {
        let core_route: crate::starmap::navigation::StarMapNavigationRoute =
            route.try_into().map_err(|e: crate::error::Error| {
                WriterError::Other(format!(
                    "jump_starmap_ancestor: route conversion failed: {e:?}"
                ))
            })?;

        Ok(crate::starmap::navigation::jump_to_ancestor(&core_route, depth as usize).into())
    }

    /// 创建跨画布边并真正写入宿主图 Store（事务操作）。
    ///
    /// 转发到 facade 的 [`WriterCore::create_cross_canvas_edge`]，后者在同一次
    /// 写操作内完成 plan + 构造 Edge + 写入 Store + validate。返回
    /// `CreateCrossCanvasEdgeResultDto`（`ownerStarmapId` + 完整 `edge`），
    /// 供平台端后续修改/删除原始边。
    pub fn create_cross_canvas_edge(
        &self,
        from: crate::api::types::StarMapEndpointSelectionDto,
        to: crate::api::types::StarMapEndpointSelectionDto,
        kind: crate::api::types::StarMapEdgeKindDto,
        label: Option<String>,
        payload: Option<String>,
    ) -> Result<crate::api::types::CreateCrossCanvasEdgeResultDto, WriterError> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        // 跨画布边同样先建立来源事务 Guard；宿主图要等 Core 完成规划后才能确定，
        // 因此在写入 Store 前先保留一个空 Guard，拿到结果后立即登记宿主并备份
        // 尚未 flush 的 Edge/GraphMeta 路径。
        let mut contribution_txn = StarmapStoreTransaction::begin(self)?;
        let from_core: crate::starmap::connections::StarMapEndpointSelection =
            from.try_into().map_err(|e: crate::error::Error| {
                WriterError::Other(format!(
                    "create_cross_canvas_edge: from conversion failed: {e:?}"
                ))
            })?;
        let to_core: crate::starmap::connections::StarMapEndpointSelection =
            to.try_into().map_err(|e: crate::error::Error| {
                WriterError::Other(format!(
                    "create_cross_canvas_edge: to conversion failed: {e:?}"
                ))
            })?;

        let payload_core = payload
            .map(|s| {
                serde_json::from_str::<serde_json::Value>(&s).map_err(crate::error::Error::from)
            })
            .transpose()
            .map_err(|e| {
                WriterError::Other(format!(
                    "create_cross_canvas_edge: payload parse failed: {e:?}"
                ))
            })?;

        let result = self
            .core_write()
            .create_cross_canvas_edge(from_core, to_core, kind.into(), label, payload_core)
            .map_err(|e| {
                WriterError::Other(format!("create_cross_canvas_edge: write failed: {e:?}"))
            })?;

        let project_id = get_starmap_project_id(self, &result.owner_starmap_id);
        let edge_id = result.edge.id.clone();
        contribution_txn.include_starmaps(std::slice::from_ref(&result.owner_starmap_id));
        contribution_txn.prepare_paths(&[
            self.app_data_root.join(
                crate::starmap::package_storage::edge_rel_path(
                    &result.owner_starmap_id,
                    &edge_id,
                ),
            ),
            self.app_data_root.join(
                crate::starmap::package_storage::graph_meta_rel_path(
                    &result.owner_starmap_id,
                ),
            ),
        ])?;
        let after_graph = self.get_starmap_graph_snapshot(&result.owner_starmap_id);
        self.record_starmap_object(
            project_id.as_deref(),
            &result.owner_starmap_id,
            crate::writing_stats::ContributionObjectType::StarMapEdge,
            &edge_id,
            crate::writing_stats::ContributionOperation::Create,
            None,
            after_graph.as_ref(),
        )?;
        self.finish_starmap_contribution_transaction(
            &result.owner_starmap_id,
            &mut contribution_txn,
        )?;

        Ok(crate::api::types::CreateCrossCanvasEdgeResultDto {
            owner_starmap_id: result.owner_starmap_id,
            edge: result.edge.into(),
        })
    }

    /// 规划跨层连线应当落在哪张图上（Core 唯一事实来源）。
    ///
    /// Edge 与 Link 共用同一条宿主规划规则：平台端提交两条从同一根星图出发的
    /// 绝对端点路径，Core 解析两端所在 Scene，取最长公共前缀得到宿主星图与
    /// 两条相对宿主的端点路径。自环、不可解析路径、不同根星图都会被拒绝。
    pub fn plan_cross_layer_connection(
        &self,
        from: crate::api::types::StarMapTargetPathDto,
        to: crate::api::types::StarMapTargetPathDto,
    ) -> Result<crate::api::types::CrossLayerConnectionPlanDto, WriterError> {
        let from_core: crate::starmap::types::reference::StarMapTargetPath =
            from.try_into().map_err(|e: crate::error::Error| {
                WriterError::Other(format!(
                    "plan_cross_layer_connection: from conversion failed: {e:?}"
                ))
            })?;
        let to_core: crate::starmap::types::reference::StarMapTargetPath =
            to.try_into().map_err(|e: crate::error::Error| {
                WriterError::Other(format!(
                    "plan_cross_layer_connection: to conversion failed: {e:?}"
                ))
            })?;
        let plan = self
            .core_write()
            .plan_cross_layer_connection(&from_core, &to_core)?;
        Ok(plan.into())
    }

    /// 列出与特定画布或目标相关的所有连接。
    ///
    /// `target` 为 `Some` 时走 `query_starmap_connections_for_target`，为 `None`
    /// 时走 `query_starmap_connections_for_canvas`，两者都从持久化路径依赖索引
    /// 收集候选祖先图，返回当前画布中的所有 Edge 以及存于祖先图但至少一端
    /// route-aware 落在当前画布的 Edge，而非逐 Edge 加载所有祖先图（问题4）。
    pub fn list_starmap_connections(
        &self,
        route: crate::api::types::StarMapNavigationRouteDto,
        target: Option<crate::api::types::StarMapTargetDetailDto>,
    ) -> Result<Vec<crate::api::types::StarMapConnectionInfoDto>, WriterError> {
        let root_starmap_id = route.root_starmap_id.clone();

        let core_route: crate::starmap::navigation::StarMapNavigationRoute =
            route.try_into().map_err(|e: crate::error::Error| {
                WriterError::Other(format!(
                    "list_starmap_connections: route conversion failed: {e:?}"
                ))
            })?;

        let connections = if let Some(target_dto) = target {
            let core_target: crate::starmap::semantic::StarMapTargetDetail =
                target_dto.try_into().map_err(|e: crate::error::Error| {
                    WriterError::Other(format!(
                        "list_starmap_connections: target conversion failed: {e:?}"
                    ))
                })?;
            // target=Some：走索引查询，从反向依赖索引获取候选 Edge 来源，
            // 只加载相关祖先图的边文件，而非遍历所有祖先图。
            self.core_write()
                .query_starmap_connections_for_target(&core_route, &core_target, &root_starmap_id)
                .map_err(|e| {
                    WriterError::Other(format!(
                        "list_starmap_connections: index query failed: {e:?}"
                    ))
                })?
        } else {
            // target=None：走索引查询，从反向依赖索引获取候选 Edge 来源，
            // 只加载相关祖先图的边文件，而非遍历所有祖先图（问题4）。
            self.core_write()
                .query_starmap_connections_for_canvas(&core_route, &root_starmap_id)
                .map_err(|e| {
                    WriterError::Other(format!(
                        "list_starmap_connections: index query failed: {e:?}"
                    ))
                })?
        };

        Ok(connections.into_iter().map(Into::into).collect())
    }

    /// 把预生成的 workspace history journal 路径登记进来源事务。
    ///
    /// 必须在写 meta/index 前调用：来源事务失败回滚时，该路径按“原本不存在”
    /// 恢复，不会留下可被启动恢复误补 history 的孤立 journal。
    pub(crate) fn register_starmap_history_journal(
        &self,
        transaction: &mut crate::writing_stats::contribution_transaction::ContributionTransaction,
        token: &str,
    ) -> ApiResult<()> {
        let path = crate::storage::journal::workspace_change::journal_file_path(
            &self.app_data_root,
            token,
        );
        transaction.prepare_paths(&[path])?;
        Ok(())
    }

    /// 在对象文件已经落盘、来源事务提交前写入 history journal。
    ///
    /// journal 记录本次确定的 change set；只有来源事务提交成功后，调用方
    /// 才允许记录 Git history 并清理 journal。
    pub(crate) fn save_starmap_history_journal(
        &self,
        token: &str,
        change_set: &crate::storage::workspace_git::WorkspaceChangeSet,
        op_type: crate::storage::journal::workspace_change::WorkspaceChangeOpType,
    ) -> ApiResult<crate::storage::journal::workspace_change::WorkspaceChangeJournal> {
        let device_id = crate::settings::load_device_info(&self.app_data_root)
            .map(|info| info.device_id)
            .unwrap_or_default();
        crate::storage::journal::workspace_change::WorkspaceChangeJournal::save_pending_with_token(
            &self.app_data_root,
            token,
            change_set,
            &device_id,
            op_type,
            None,
            None,
        )
        .map_err(Into::into)
    }

    /// 在来源事务已 durable 后补记 Git history。
    ///
    /// history 失败时保留 journal，由 bootstrap 用同一 token 幂等补记；
    /// 不把对象已提交的写操作反向报告为失败，也不回滚已经提交的对象。
    pub(crate) fn finish_starmap_history_journal(
        &self,
        journal: &crate::storage::journal::workspace_change::WorkspaceChangeJournal,
        message: &str,
    ) {
        match self.record_workspace_change_set_history(&journal.change_set, message) {
            Ok(()) => {
                if let Err(e) = journal.mark_history_recorded(&self.app_data_root) {
                    log::warn!(
                        "{}: mark_history_recorded failed for {}: {} — journal retained",
                        message,
                        journal.token,
                        e
                    );
                } else if let Err(e) = journal.clear_journal(&self.app_data_root) {
                    log::warn!(
                        "{}: clear_journal failed for {}: {}",
                        message,
                        journal.token,
                        e
                    );
                }
            }
            Err(e) => {
                log::warn!(
                    "{}: record_workspace_change_set_history failed for {}: {} — \
                     journal retained, history will be补 on next startup",
                    message,
                    journal.token,
                    e
                );
            }
        }
    }

    /// 读取受影响画布的贡献事件前后态：优先使用已加载 Store 的当前内存图，
    /// 只有画布尚未加载时才用临时 Store 从磁盘只读重建，避免预热未加载画布。
    /// 两种来源都必须按同一实例的真实状态读取，不能把磁盘旧图与内存新图混用。
    fn read_contribution_starmap_graph(
        &self,
        starmap_id: &str,
    ) -> ApiResult<crate::starmap::types::StarMapGraph> {
        if let Some(graph) = self
            .core_read()
            .get_loaded_starmap_graph(starmap_id)
            .map_err(WriterError::from)?
        {
            return Ok(graph);
        }
        let mut store = crate::starmap::store::StarMapStore::new(&self.app_data_root, starmap_id);
        store.ensure_fully_loaded().map_err(WriterError::from)?;
        Ok(store.to_starmap_graph())
    }

    /// 枚举共享节点素材在全部画布中的实例；来源画布自身必须包含在内。
    ///
    /// 反向索引可能因旧数据或中断写入损坏。此时必须从画布实例文件重建后
    /// 重试，不能在索引读取失败时退化成“只更新来源实例”，否则其它画布的
    /// 回放链会缺少这次共享内容变更。
    fn shared_node_instances(
        &self,
        starmap_id: &str,
        node_id: &str,
    ) -> ApiResult<Vec<(String, String)>> {
        let bindings = match crate::starmap::library::instance_asset_bindings_for_starmap(
            &self.app_data_root,
            starmap_id,
        ) {
            Ok(bindings) => bindings,
            Err(error) => {
                log::warn!(
                    "shared node bindings unreadable for {starmap_id}, rebuilding references: {error}"
                );
                crate::starmap::library::rebuild_reference_index(&self.app_data_root)
                    .map_err(WriterError::from)?;
                crate::starmap::library::instance_asset_bindings_for_starmap(
                    &self.app_data_root,
                    starmap_id,
                )
                .map_err(WriterError::from)?
            }
        };
        let asset_id = bindings
            .into_iter()
            .find(|(instance_id, _)| instance_id == node_id)
            .map(|(_, asset_id)| asset_id)
            .unwrap_or_else(|| node_id.to_string());
        let mut instances = match crate::starmap::library::instances_for_asset(
            &self.app_data_root,
            &asset_id,
        ) {
            Ok(instances) => instances,
            Err(error) => {
                log::warn!(
                    "shared node references unreadable for {asset_id}, rebuilding references: {error}"
                );
                crate::starmap::library::rebuild_reference_index(&self.app_data_root)
                    .map_err(WriterError::from)?;
                crate::starmap::library::instances_for_asset(&self.app_data_root, &asset_id)
                    .map_err(WriterError::from)?
            }
        }
        .into_iter()
        .map(|instance| (instance.starmap_id, instance.instance_id))
        .collect::<Vec<_>>();
        if !instances
            .iter()
            .any(|(owner, instance)| owner == starmap_id && instance == node_id)
        {
            // 索引缺失或陈旧时同样会表现为“找不到来源实例”。重建后再失败
            // 说明画布实例文件本身不一致，必须中止共享内容写入，避免其它
            // 画布静默缺少本次变更事件。
            log::warn!(
                "shared node source {starmap_id}/{node_id} absent from references, rebuilding"
            );
            crate::starmap::library::rebuild_reference_index(&self.app_data_root)
                .map_err(WriterError::from)?;
            instances = crate::starmap::library::instances_for_asset(
                &self.app_data_root,
                &asset_id,
            )
            .map_err(WriterError::from)?
            .into_iter()
            .map(|instance| (instance.starmap_id, instance.instance_id))
            .collect();
            if !instances
                .iter()
                .any(|(owner, instance)| owner == starmap_id && instance == node_id)
            {
                return Err(WriterError::Other(format!(
                    "shared node source {starmap_id}/{node_id} missing after rebuilding references"
                )));
            }
        }
        instances.sort();
        instances.dedup();
        Ok(instances)
    }

    fn enqueue_starmap_node_search_update(
        &self,
        transaction: &mut StarmapStoreTransaction<'_>,
        starmap_id: &str,
        node: &crate::starmap::types::StarMapNode,
    ) {
        let project_id = get_starmap_project_id(self, starmap_id);
        let node_content = extract_node_search_body(&node.content, &node.tags);
        let entry = crate::search::extractor::extract_starmap_node_entry(
            starmap_id,
            &node.id,
            project_id.as_deref(),
            &node.title,
            &node_content,
        );
        transaction.enqueue_search_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
    }

    /// 共享内容传播会同时修改多个画布；每个画布都在提交前 flush，保证
    /// 同一来源事务能覆盖全部派生写入。
    fn finish_starmaps_contribution_transaction(
        &self,
        starmap_ids: &[String],
        transaction: &mut crate::writing_stats::contribution_transaction::ContributionTransaction,
    ) -> ApiResult<()> {
        for starmap_id in starmap_ids {
            let pending_paths = self.core_read().pending_starmap_flush_paths(starmap_id);
            if !pending_paths.is_empty() {
                let mut paths = pending_paths;
                paths.push(self.app_data_root.join("starmaps/index.json"));
                paths.push(self.app_data_root.join("app-meta/sync/state.local.json"));
                transaction.prepare_paths(&paths)?;
            }
            if let Err(error) = self.flush_starmap_store(starmap_id) {
                let _ = self.core_write().remove_starmap_store(starmap_id);
                return Err(error);
            }
        }
        transaction.commit()?;
        Ok(())
    }

    pub(crate) fn finish_starmap_contribution_transaction(
        &self,
        starmap_id: &str,
        transaction: &mut crate::writing_stats::contribution_transaction::ContributionTransaction,
    ) -> ApiResult<()> {
        // 先把本次 flush 会写到的具体文件登记进事务，再真正写盘；不再整图备份。
        let pending_paths = {
            let core = self.core_read();
            core.pending_starmap_flush_paths(starmap_id)
        };
        if !pending_paths.is_empty() {
            let mut paths = pending_paths;
            paths.push(self.app_data_root.join("starmaps/index.json"));
            paths.push(self.app_data_root.join("app-meta/sync/state.local.json"));
            transaction.prepare_paths(&paths)?;
        }
        if let Err(error) = self.flush_starmap_store(starmap_id) {
            let _ = self.core_write().remove_starmap_store(starmap_id);
            return Err(error);
        }
        transaction.commit()?;
        Ok(())
    }
}

fn ensure_starmap_revision(
    actual: Option<u64>,
    expected: Option<u64>,
    object_type: &str,
    object_id: &str,
) -> ApiResult<()> {
    let Some(expected) = expected else {
        return Ok(());
    };
    match actual {
        Some(actual) if actual == expected => Ok(()),
        Some(actual) => Err(WriterError::Conflict(format!(
            "revision conflict: {object_type} {object_id} expected={expected} actual={actual}"
        ))),
        None => Err(WriterError::Conflict(format!(
            "revision conflict: {object_type} {object_id} no longer exists"
        ))),
    }
}
