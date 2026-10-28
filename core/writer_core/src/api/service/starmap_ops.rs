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

impl WriterCoreApi {
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
        //   走 _with_changes 入口，记录本地 history。
        // 之前调用 create_starmap(title, desc, None) 没有走 change set，
        // 导致 create_starmap_json 写的 meta/index 文件不进本地 Git history。
        let (value, change_set) = self
            .core_write()
            .create_starmap_with_changes(title, desc, None)
            .map_err(WriterError::from)?;
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
        let _ = self.record_workspace_change_set_history(&change_set, "create_starmap")?;
        self.record_contribution_event(
            project_id.as_deref().unwrap_or_default(),
            crate::writing_stats::ContributionObjectType::StarMap,
            &starmap_id,
            crate::writing_stats::ContributionOperation::Create,
            None,
            None,
            Some(Self::starmap_meta_snapshot(&value)),
        )?;
        Self::json_string(&value)
    }

    pub fn add_starmap_embed(
        &self,
        starmap_id: &str,
        embed: crate::api::types::StarMapEmbedDto,
    ) -> ApiResult<crate::api::types::StarMapEmbedDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
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
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
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
        let mut contribution_batch = self.begin_contribution_batch();
        let core_position: crate::starmap::types::StarMapPoint = position.into();
        let (starmap_meta, embed, change_set, tx_id) = self
            .core_write()
            .create_starmap_child_embed(host_starmap_id, title, core_position)
            .map_err(WriterError::from)?;

        let starmap_id = starmap_meta.starmap_id.clone();
        let project_id = starmap_meta.project_id.as_deref().map(|s| s.to_string());

        // 更新搜索索引：新创建的 starmap title
        let starmap_entry = crate::search::extractor::extract_starmap_title_entry(
            &starmap_id,
            project_id.as_deref(),
            &starmap_meta.title,
        );
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
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
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: embed_entry.object_id.clone(),
            scope: embed_entry.scope,
            title: embed_entry.title.clone(),
            body: embed_entry.body.clone(),
            target: Some(embed_entry.target.clone()),
        });

        // 先记录 history，成功后 ack journal。
        // history 失败时不返回 Err——journal 保留在 EmbedAdded，
        // 下次启动 recover 会补记 history。返回 pending: true 让调用方知道
        // 数据已 durable 但 history 待补。
        let pending = match self
            .record_workspace_change_set_history(&change_set, "create_starmap_child_embed")
        {
            Ok(()) => {
                // history 成功，ack 推进 journal 到 Completed 并清理。
                // ack 失败只 log::warn——数据已 durable，journal 残留不影响正确性，
                // 下次启动 recover 会清理。
                if let Err(e) =
                    crate::storage::journal::starmap_child_embed::ack_child_embed_history(
                        &self.app_data_root,
                        &tx_id,
                    )
                {
                    log::warn!(
                        "create_starmap_child_embed: ack_child_embed_history failed for tx_id={}: {} \
                         — journal retained, will be recovered on next startup",
                        tx_id,
                        e
                    );
                }
                false
            }
            Err(e) => {
                // history 失败：不传播错误，journal 保留在 EmbedAdded，
                // 下次启动 recovery 会补记 history。
                log::warn!(
                    "create_starmap_child_embed: record_workspace_change_set_history failed for \
                     tx_id={}: {} — journal retained at EmbedAdded, will be recovered on next \
                     startup",
                    tx_id,
                    e
                );
                true
            }
        };

        let context = crate::writing_stats::current_mutation_context();
        self.record_contribution_event_with_context(
            &context,
            project_id.as_deref().unwrap_or_default(),
            crate::writing_stats::ContributionObjectType::StarMap,
            &starmap_id,
            crate::writing_stats::ContributionOperation::Create,
            None,
            None,
            Some(Self::starmap_meta_snapshot(&starmap_meta)),
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
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let before = self.get_starmap_graph_snapshot(starmap_id);
        let project_id = get_starmap_project_id(self, starmap_id);
        let result = self
            .core_write()
            .update_starmap_embed(
                starmap_id,
                instance_id,
                patch.try_into().map_err(WriterError::from)?,
            )
            .map_err(WriterError::from)?;
        let after = self.get_starmap_graph_snapshot(starmap_id);
        self.record_starmap_object(
            project_id.as_deref(),
            starmap_id,
            crate::writing_stats::ContributionObjectType::StarMapEmbed,
            instance_id,
            crate::writing_stats::ContributionOperation::Update,
            before.as_ref(),
            after.as_ref(),
        )?;
        let entry = crate::search::extractor::extract_starmap_embed_entry(
            starmap_id,
            instance_id,
            project_id.as_deref(),
            &result.label.clone().unwrap_or_default(),
        );
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        Ok(result.into())
    }

    pub fn delete_starmap_embed(&self, starmap_id: &str, instance_id: &str) -> ApiResult<bool> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let before = self.get_starmap_graph_snapshot(starmap_id);
        let project_id = get_starmap_project_id(self, starmap_id);
        self.core_write()
            .delete_starmap_embed(starmap_id, instance_id)?;
        self.record_starmap_object(
            project_id.as_deref(),
            starmap_id,
            crate::writing_stats::ContributionObjectType::StarMapEmbed,
            instance_id,
            crate::writing_stats::ContributionOperation::Delete,
            before.as_ref(),
            None,
        )?;
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Delete,
            object_id: format!("starmap_embed:{}:{}", starmap_id, instance_id),
            scope: crate::search::SearchScope::All,
            title: String::new(),
            body: String::new(),
            target: None,
        });
        Ok(true)
    }

    pub fn add_starmap_link(
        &self,
        starmap_id: &str,
        link: crate::api::types::StarMapLinkDto,
    ) -> ApiResult<crate::api::types::StarMapLinkDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
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
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        Ok(result.into())
    }

    pub fn update_starmap_link(
        &self,
        starmap_id: &str,
        link_id: &str,
        patch: crate::api::types::StarMapLinkPatchDto,
    ) -> ApiResult<crate::api::types::StarMapLinkDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let before = self.get_starmap_graph_snapshot(starmap_id);
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
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        Ok(result.into())
    }

    pub fn delete_starmap_link(&self, starmap_id: &str, link_id: &str) -> ApiResult<bool> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let before = self.get_starmap_graph_snapshot(starmap_id);
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
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Delete,
            object_id: format!("starmap_link:{}:{}", starmap_id, link_id),
            scope: crate::search::SearchScope::All,
            title: String::new(),
            body: String::new(),
            target: None,
        });
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
        self.core_write()
            .get_starmap_graph(starmap_id)
            .map(Into::into)
            .map_err(Into::into)
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
        //   用 _with_changes 版本拿变更集，
        // 调 record_workspace_change_set_history 记录本地历史。
        let (result, change_set) = self
            .core_write()
            .create_starmap_with_changes(title, desc, template_id)
            .map_err(WriterError::from)?;
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
        self.record_contribution_event_with_context(
            &context,
            project_id.unwrap_or(""),
            crate::writing_stats::ContributionObjectType::StarMap,
            &result.starmap_id,
            crate::writing_stats::ContributionOperation::Create,
            None,
            None,
            Some(Self::starmap_meta_snapshot(&result)),
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
            for hyperlink in &graph.hyperlinks {
                self.record_starmap_object_with_context(
                    &context,
                    project_id,
                    &result.starmap_id,
                    crate::writing_stats::ContributionObjectType::StarMapHyperlink,
                    &hyperlink.hyperlink_id,
                    crate::writing_stats::ContributionOperation::Create,
                    None,
                    Some(&graph),
                )?;
            }
        }
        self.commit_contribution_batch(&mut contribution_batch)?;
        let result_dto: crate::api::types::StarMapMetaDto = result.into();
        let _ = self.record_workspace_change_set_history(&change_set, "create_starmap")?;
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
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
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
        Ok(result.into())
    }

    pub fn rename_starmap(
        &self,
        starmap_id: &str,
        new_title: &str,
    ) -> ApiResult<crate::api::types::StarMapMetaDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        //   用 _with_changes 版本记录本地历史。
        let before = self
            .core_write()
            .get_starmap(starmap_id)
            .ok()
            .map(|meta| Self::starmap_meta_snapshot(&meta));
        let (result, change_set) = self
            .core_write()
            .rename_starmap_with_changes(starmap_id, new_title)
            .map_err(WriterError::from)?;
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
        let after = Self::starmap_meta_snapshot(&result);
        self.record_contribution_event(
            project_id.as_deref().unwrap_or_default(),
            crate::writing_stats::ContributionObjectType::StarMap,
            starmap_id,
            crate::writing_stats::ContributionOperation::Update,
            None,
            before,
            Some(after),
        )?;
        let result_dto: crate::api::types::StarMapMetaDto = result.into();
        let _ = self.record_workspace_change_set_history(&change_set, "rename_starmap")?;
        Ok(result_dto)
    }

    pub fn delete_starmap(&self, starmap_id: &str) -> ApiResult<bool> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let mut contribution_batch = self.begin_contribution_batch();
        //   durable 删除事务：先 plan（不碰磁盘，构造完整 PlannedStarmapDelete
        // 含固定 trash 路径和完整 sync_delete_facts），再 save_pending 落盘 journal，
        // 再 apply planned delete（rename + 写 tombstone），再推进 journal 阶段。
        // 只有 save_pending 成功后才允许动本地文件，保证删除事实在物理删除前已持久化。
        // history 失败时保留 journal 并返回错误，与 delete_volume/delete_chapter 对称。
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
        let before_snapshot = before_meta.as_ref().map(Self::starmap_meta_snapshot);
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
            crate::storage::journal::workspace_change::WorkspaceChangeJournal::save_pending(
                &self.app_data_root,
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
            format!("starmap_hyperlink:{}:", starmap_id),
            format!("starmap_link:{}:", starmap_id),
            format!("starmap_embed:{}:", starmap_id),
        ] {
            self.remove_search_index_by_prefix(prefix);
        }
        // 写 workspace history，成功后清 journal；失败时 journal 保留供下次 bootstrap 补记。
        // 不再丢弃 history 错误（把 `let _ =` 改成实际错误处理）。
        match self.record_workspace_change_set_history(&change_set, "delete_starmap") {
            Ok(()) => {
                if let Err(e) = journal.mark_history_recorded(&self.app_data_root) {
                    log::warn!(
                        "delete_starmap: mark_history_recorded failed: {} — journal retained",
                        e
                    );
                } else if let Err(e) = journal.clear_journal(&self.app_data_root) {
                    log::warn!("delete_starmap: clear_journal failed: {}", e);
                }
            }
            Err(e) => {
                log::warn!(
                    "delete_starmap: record_workspace_change_set_history failed: {} — \
                     journal retained for recovery, history will be补 on next startup",
                    e
                );
            }
        }
        let context = crate::writing_stats::current_mutation_context();
        self.record_contribution_event_with_context(
            &context,
            &contribution_project,
            crate::writing_stats::ContributionObjectType::StarMap,
            starmap_id,
            crate::writing_stats::ContributionOperation::Delete,
            None,
            before_snapshot,
            None,
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
            for hyperlink in &graph.hyperlinks {
                self.record_starmap_object_with_context(
                    &context,
                    Some(&contribution_project),
                    starmap_id,
                    crate::writing_stats::ContributionObjectType::StarMapHyperlink,
                    &hyperlink.hyperlink_id,
                    crate::writing_stats::ContributionOperation::Delete,
                    Some(&graph),
                    None,
                )?;
            }
        }
        self.commit_contribution_batch(&mut contribution_batch)?;
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
        let before_meta = self.core_write().get_starmap(starmap_id).ok();
        //   用 _with_changes 版本记录本地历史。
        let change_set = self
            .core_write()
            .bind_starmap_to_project_with_changes(starmap_id, project_id)?;
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
        let hl_result = self.core_write().list_starmap_hyperlinks(starmap_id);
        if let Ok(result) = hl_result {
            for hl in &result.items {
                let hl_title = hl.label.as_deref().unwrap_or("");
                let entry = crate::search::extractor::extract_starmap_hyperlink_entry(
                    starmap_id,
                    &hl.hyperlink_id,
                    Some(project_id),
                    hl_title,
                    &hl.target_uri,
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
        let _ = self.record_workspace_change_set_history(&change_set, "bind_starmap_to_project")?;
        self.record_contribution_event(
            project_id,
            crate::writing_stats::ContributionObjectType::StarMap,
            starmap_id,
            crate::writing_stats::ContributionOperation::Link,
            None,
            before_meta.as_ref().map(Self::starmap_meta_snapshot),
            Some(Self::starmap_meta_snapshot(&meta)),
        )?;
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
        let before_meta = self.core_write().get_starmap(starmap_id).ok();
        //   用 _with_changes 版本记录本地历史。
        let change_set = self
            .core_write()
            .unbind_starmap_from_project_with_changes(starmap_id)?;
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
        let hl_result = self.core_write().list_starmap_hyperlinks(starmap_id);
        if let Ok(result) = hl_result {
            for hl in &result.items {
                let hl_title = hl.label.as_deref().unwrap_or("");
                let entry = crate::search::extractor::extract_starmap_hyperlink_entry(
                    starmap_id,
                    &hl.hyperlink_id,
                    None,
                    hl_title,
                    &hl.target_uri,
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
        let _ =
            self.record_workspace_change_set_history(&change_set, "unbind_starmap_from_project")?;
        let contribution_project = before_meta
            .as_ref()
            .and_then(|meta| meta.project_id.clone())
            .unwrap_or_default();
        self.record_contribution_event(
            &contribution_project,
            crate::writing_stats::ContributionObjectType::StarMap,
            starmap_id,
            crate::writing_stats::ContributionOperation::Unlink,
            None,
            before_meta.as_ref().map(Self::starmap_meta_snapshot),
            Some(Self::starmap_meta_snapshot(&meta)),
        )?;
        Ok(true)
    }

    ///   刷新单个 starmap 在搜索索引中的条目。
    ///
    /// 只更新搜索索引，不调用 core 层 unbind，不记录 history。
    /// 供 `delete_project` 在 core 层 `unbind_starmaps` 成功后刷新搜索索引使用。
    /// 读取最新的 starmap meta + graph，把 title/node/edge/link/embed/hyperlink
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
        let hl_result = self.core_write().list_starmap_hyperlinks(starmap_id);
        if let Ok(result) = hl_result {
            for hl in &result.items {
                let hl_title = hl.label.as_deref().unwrap_or("");
                let entry = crate::search::extractor::extract_starmap_hyperlink_entry(
                    starmap_id,
                    &hl.hyperlink_id,
                    None,
                    hl_title,
                    &hl.target_uri,
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
        let mut contribution_batch = self.begin_contribution_batch();
        //   用 _with_changes 版本记录本地历史。
        let previous = self.get_main_starmap_for_project(project_id).ok().flatten();
        let change_set = self
            .core_write()
            .set_main_starmap_for_project_with_changes(starmap_id, project_id)
            .map_err(crate::api::error::WriterError::from)?;
        let _ =
            self.record_workspace_change_set_history(&change_set, "set_main_starmap_for_project")?;
        let context = crate::writing_stats::current_mutation_context();
        if let Some(previous) = previous.filter(|meta| meta.starmap_id != starmap_id) {
            let before = self
                .core_write()
                .get_starmap(&previous.starmap_id)
                .ok()
                .map(|meta| Self::starmap_meta_snapshot(&meta));
            self.record_contribution_event_with_context(
                &context,
                project_id,
                crate::writing_stats::ContributionObjectType::StarMap,
                &previous.starmap_id,
                crate::writing_stats::ContributionOperation::Unlink,
                None,
                before,
                None,
            )?;
        }
        let after = self
            .core_write()
            .get_starmap(starmap_id)
            .ok()
            .map(|meta| Self::starmap_meta_snapshot(&meta));
        self.record_contribution_event_with_context(
            &context,
            project_id,
            crate::writing_stats::ContributionObjectType::StarMap,
            starmap_id,
            crate::writing_stats::ContributionOperation::Link,
            None,
            None,
            after,
        )?;
        self.commit_contribution_batch(&mut contribution_batch)?;
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
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let before = self.get_starmap_graph_snapshot(starmap_id);
        let result = self
            .core_write()
            .update_starmap_node(
                starmap_id,
                node_id,
                patch.try_into().map_err(WriterError::from)?,
            )
            .map_err(WriterError::from)?;
        let node_content = extract_node_search_body(&result.content, &result.tags);
        let project_id = get_starmap_project_id(self, starmap_id);
        let entry = crate::search::extractor::extract_starmap_node_entry(
            starmap_id,
            node_id,
            project_id.as_deref(),
            &result.title,
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
        let after = self.get_starmap_graph_snapshot(starmap_id);
        self.record_starmap_object(
            project_id.as_deref(),
            starmap_id,
            crate::writing_stats::ContributionObjectType::StarMapNode,
            node_id,
            crate::writing_stats::ContributionOperation::Update,
            before.as_ref(),
            after.as_ref(),
        )?;
        Ok(result.into())
    }

    pub fn delete_starmap_node(&self, starmap_id: &str, node_id: &str) -> ApiResult<bool> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let before = self.get_starmap_graph_snapshot(starmap_id);
        let project_id = get_starmap_project_id(self, starmap_id);
        self.core_write().delete_starmap_node(starmap_id, node_id)?;
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
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
        Ok(true)
    }

    pub fn add_starmap_edge(
        &self,
        starmap_id: &str,
        edge: crate::api::types::StarMapEdgeDto,
    ) -> ApiResult<crate::api::types::StarMapEdgeDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
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
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        Ok(result.into())
    }

    pub fn update_starmap_edge(
        &self,
        starmap_id: &str,
        edge_id: &str,
        patch: crate::api::types::StarMapEdgePatchDto,
    ) -> ApiResult<crate::api::types::StarMapEdgeDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let before = self.get_starmap_graph_snapshot(starmap_id);
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
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
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
        Ok(result.into())
    }

    pub fn delete_starmap_edge(&self, starmap_id: &str, edge_id: &str) -> ApiResult<bool> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let before = self.get_starmap_graph_snapshot(starmap_id);
        let project_id = get_starmap_project_id(self, starmap_id);
        self.core_write().delete_starmap_edge(starmap_id, edge_id)?;
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
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

    pub fn add_starmap_hyperlink(
        &self,
        starmap_id: &str,
        hl: crate::api::types::StarMapHyperlinkDto,
    ) -> ApiResult<crate::api::types::StarMapHyperlinkDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let result = self
            .core_write()
            .add_starmap_hyperlink(starmap_id, hl.try_into().map_err(WriterError::from)?)
            .map_err(WriterError::from)?;
        let project_id = get_starmap_project_id(self, starmap_id);
        let after = self.get_starmap_graph_snapshot(starmap_id);
        self.record_starmap_object(
            project_id.as_deref(),
            starmap_id,
            crate::writing_stats::ContributionObjectType::StarMapHyperlink,
            &result.hyperlink_id,
            crate::writing_stats::ContributionOperation::Create,
            None,
            after.as_ref(),
        )?;
        let hl_label = result.label.as_deref().unwrap_or("");
        let entry = crate::search::extractor::extract_starmap_hyperlink_entry(
            starmap_id,
            &result.hyperlink_id,
            project_id.as_deref(),
            hl_label,
            &result.target_uri,
        );
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        Ok(result.into())
    }

    pub fn update_starmap_hyperlink(
        &self,
        starmap_id: &str,
        hyperlink_id: &str,
        patch: crate::api::types::StarMapHyperlinkPatchDto,
    ) -> ApiResult<crate::api::types::StarMapHyperlinkDto> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let core_patch: crate::starmap::types::StarMapHyperlinkPatch =
            patch.try_into().map_err(WriterError::from)?;
        let before = self.get_starmap_graph_snapshot(starmap_id);
        let project_id = get_starmap_project_id(self, starmap_id);
        let result = self
            .core_write()
            .update_starmap_hyperlink(starmap_id, hyperlink_id, &core_patch)
            .map_err(WriterError::from)?;
        let after = self.get_starmap_graph_snapshot(starmap_id);
        self.record_starmap_object(
            project_id.as_deref(),
            starmap_id,
            crate::writing_stats::ContributionObjectType::StarMapHyperlink,
            hyperlink_id,
            crate::writing_stats::ContributionOperation::Update,
            before.as_ref(),
            after.as_ref(),
        )?;
        let hl_label = result.label.as_deref().unwrap_or("");
        let entry = crate::search::extractor::extract_starmap_hyperlink_entry(
            starmap_id,
            hyperlink_id,
            project_id.as_deref(),
            hl_label,
            &result.target_uri,
        );
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        Ok(result.into())
    }

    pub fn delete_starmap_hyperlink(
        &self,
        starmap_id: &str,
        hyperlink_id: &str,
    ) -> ApiResult<bool> {
        let _workspace_write_lock =
            crate::storage::WorkspaceWriteLock::acquire(&self.app_data_root)?;
        let before = self.get_starmap_graph_snapshot(starmap_id);
        let project_id = get_starmap_project_id(self, starmap_id);
        self.core_write()
            .delete_starmap_hyperlink(starmap_id, hyperlink_id)?;
        self.record_starmap_object(
            project_id.as_deref(),
            starmap_id,
            crate::writing_stats::ContributionObjectType::StarMapHyperlink,
            hyperlink_id,
            crate::writing_stats::ContributionOperation::Delete,
            before.as_ref(),
            None,
        )?;
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Delete,
            object_id: format!("starmap_hyperlink:{}:{}", starmap_id, hyperlink_id),
            scope: crate::search::SearchScope::All,
            title: String::new(),
            body: String::new(),
            target: None,
        });
        Ok(true)
    }

    pub fn list_starmap_hyperlinks(
        &self,
        starmap_id: &str,
    ) -> ApiResult<crate::api::types::StarMapHyperlinkListWithDiagnosticsDto> {
        self.core_write()
            .list_starmap_hyperlinks(starmap_id)
            .map(crate::api::types::StarMapHyperlinkListWithDiagnosticsDto::from)
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
        self.core_write()
            .get_starmap_phased_snapshot(starmap_id, &core_request)
            .map(crate::api::types::StarMapPhasedSnapshotDto::from)
            .map_err(Into::into)
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
}
