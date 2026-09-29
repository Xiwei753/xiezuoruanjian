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
        let _ = self.record_workspace_change_set_history(&change_set, "create_starmap");
        Self::json_string(&value)
    }

    pub fn add_starmap_embed(
        &self,
        starmap_id: &str,
        embed: crate::api::types::StarMapEmbedDto,
    ) -> ApiResult<crate::api::types::StarMapEmbedDto> {
        let result = self
            .core_write()
            .add_starmap_embed(starmap_id, embed.try_into().map_err(WriterError::from)?)
            .map_err(WriterError::from)?;
        let project_id = get_starmap_project_id(self, starmap_id);
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
        let core_position: crate::starmap::types::StarMapPoint = position.into();
        let (starmap_meta, embed) = self
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

        Ok(crate::api::types::CreateStarMapChildEmbedResultDto {
            starmap: starmap_meta.into(),
            embed: embed.into(),
            pending: false,
        })
    }

    pub fn update_starmap_embed(
        &self,
        starmap_id: &str,
        instance_id: &str,
        patch: crate::api::types::StarMapEmbedPatchDto,
    ) -> ApiResult<crate::api::types::StarMapEmbedDto> {
        let result = self
            .core_write()
            .update_starmap_embed(
                starmap_id,
                instance_id,
                patch.try_into().map_err(WriterError::from)?,
            )
            .map_err(WriterError::from)?;
        let project_id = get_starmap_project_id(self, starmap_id);
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
        self.core_write()
            .delete_starmap_embed(starmap_id, instance_id)?;
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
        let result = self
            .core_write()
            .add_starmap_link(starmap_id, link.try_into().map_err(WriterError::from)?)
            .map_err(WriterError::from)?;
        let label = result.label.clone().unwrap_or_default();
        let project_id = get_starmap_project_id(self, starmap_id);
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
        let result = self
            .core_write()
            .update_starmap_link(
                starmap_id,
                link_id,
                patch.try_into().map_err(WriterError::from)?,
            )
            .map_err(WriterError::from)?;
        let label = result.label.clone().unwrap_or_default();
        let project_id = get_starmap_project_id(self, starmap_id);
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
        self.core_write().delete_starmap_link(starmap_id, link_id)?;
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
        self.core_write()
            .ack_starmap_deletions(starmap_id, acknowledged_revision)
            .map_err(WriterError::from)
    }

    pub fn get_starmap_graph(
        &self,
        starmap_id: &str,
    ) -> ApiResult<crate::api::types::StarMapGraphDto> {
        self.core_write()
            .get_starmap_graph(starmap_id)
            .map(Into::into)
            .map_err(Into::into)
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
        let result_dto: crate::api::types::StarMapMetaDto = result.into();
        let _ = self.record_workspace_change_set_history(&change_set, "create_starmap");
        Ok(result_dto)
    }

    pub fn add_starmap_node(
        &self,
        starmap_id: &str,
        node: crate::api::types::StarMapNodeDto,
        x: f32,
        y: f32,
    ) -> ApiResult<crate::api::types::StarMapNodeDto> {
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
        Ok(result.into())
    }

    pub fn rename_starmap(
        &self,
        starmap_id: &str,
        new_title: &str,
    ) -> ApiResult<crate::api::types::StarMapMetaDto> {
        //   用 _with_changes 版本记录本地历史。
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
        let result_dto: crate::api::types::StarMapMetaDto = result.into();
        let _ = self.record_workspace_change_set_history(&change_set, "rename_starmap");
        Ok(result_dto)
    }

    pub fn delete_starmap(&self, starmap_id: &str) -> ApiResult<bool> {
        //   用 _with_changes 版本记录本地历史。
        let change_set = self.core_write().delete_starmap_with_changes(starmap_id)?;
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
        let _ = self.record_workspace_change_set_history(&change_set, "delete_starmap");
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
        let _ = self.record_workspace_change_set_history(&change_set, "bind_starmap_to_project");
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
            self.record_workspace_change_set_history(&change_set, "unbind_starmap_from_project");
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
        //   用 _with_changes 版本记录本地历史。
        let change_set = self
            .core_write()
            .set_main_starmap_for_project_with_changes(starmap_id, project_id)
            .map_err(crate::api::error::WriterError::from)?;
        let _ =
            self.record_workspace_change_set_history(&change_set, "set_main_starmap_for_project");
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
        Ok(result.into())
    }

    pub fn delete_starmap_node(&self, starmap_id: &str, node_id: &str) -> ApiResult<bool> {
        self.core_write().delete_starmap_node(starmap_id, node_id)?;
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Delete,
            object_id: format!("starmap_node:{}:{}", starmap_id, node_id),
            scope: crate::search::SearchScope::All,
            title: String::new(),
            body: String::new(),
            target: None,
        });
        Ok(true)
    }

    pub fn add_starmap_edge(
        &self,
        starmap_id: &str,
        edge: crate::api::types::StarMapEdgeDto,
    ) -> ApiResult<crate::api::types::StarMapEdgeDto> {
        let result = self
            .core_write()
            .add_starmap_edge(starmap_id, edge.try_into().map_err(WriterError::from)?)
            .map_err(WriterError::from)?;
        let label = result.label.clone().unwrap_or_default();
        let project_id = get_starmap_project_id(self, starmap_id);
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
        Ok(result.into())
    }

    pub fn delete_starmap_edge(&self, starmap_id: &str, edge_id: &str) -> ApiResult<bool> {
        self.core_write().delete_starmap_edge(starmap_id, edge_id)?;
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Delete,
            object_id: format!("starmap_edge:{}:{}", starmap_id, edge_id),
            scope: crate::search::SearchScope::All,
            title: String::new(),
            body: String::new(),
            target: None,
        });
        Ok(true)
    }

    pub fn flush_starmap_store(&self, starmap_id: &str) -> ApiResult<bool> {
        let changed_paths = self
            .core_write()
            .flush_starmap_store(starmap_id)
            .map_err(WriterError::from)?;
        self.record_workspace_paths_history(&changed_paths, "flush_starmap_store");
        Ok(true)
    }

    pub fn close_starmap_store(&self, starmap_id: &str) -> ApiResult<bool> {
        let changed_paths = self
            .core_write()
            .close_starmap_store(starmap_id)
            .map_err(WriterError::from)?;
        self.record_workspace_paths_history(&changed_paths, "close_starmap_store");
        Ok(true)
    }

    pub fn flush_all_starmap_stores(&self) -> ApiResult<bool> {
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
        let result = self
            .core_write()
            .add_starmap_hyperlink(starmap_id, hl.try_into().map_err(WriterError::from)?)
            .map_err(WriterError::from)?;
        let project_id = get_starmap_project_id(self, starmap_id);
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
        let core_patch: crate::starmap::types::StarMapHyperlinkPatch =
            patch.try_into().map_err(WriterError::from)?;
        let result = self
            .core_write()
            .update_starmap_hyperlink(starmap_id, hyperlink_id, &core_patch)
            .map_err(WriterError::from)?;
        let project_id = get_starmap_project_id(self, starmap_id);
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
        self.core_write()
            .delete_starmap_hyperlink(starmap_id, hyperlink_id)?;
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
        let core_request: crate::starmap::store::PhasedSnapshotRequest = request.clone().into();
        self.core_write()
            .get_starmap_phased_snapshot(starmap_id, &core_request)
            .map(crate::api::types::StarMapPhasedSnapshotDto::from)
            .map_err(Into::into)
    }
}
