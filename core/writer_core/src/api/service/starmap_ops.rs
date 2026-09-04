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

fn extract_node_dto_search_body(
    content: &crate::api::types::StarMapNodeContentDto,
    tags: &[String],
) -> String {
    let mut parts = Vec::new();
    match content.kind.as_str() {
        "inline" => {
            if let Some(ref s) = content.summary {
                if !s.is_empty() {
                    parts.push(s.clone());
                }
            }
            if let Some(ref b) = content.body {
                if !b.is_empty() {
                    parts.push(b.clone());
                }
            }
        }
        "chapterRef" => {
            if let Some(ref cid) = content.chapter_id {
                if !cid.is_empty() {
                    parts.push(cid.clone());
                }
            }
        }
        "entityRef" => {
            if let Some(ref et) = content.entity_type {
                if !et.is_empty() {
                    parts.push(et.clone());
                }
            }
            if let Some(ref eid) = content.entity_id {
                if !eid.is_empty() {
                    parts.push(eid.clone());
                }
            }
        }
        "externalRef" => {
            if let Some(ref l) = content.label {
                if !l.is_empty() {
                    parts.push(l.clone());
                }
            }
            if let Some(ref u) = content.uri {
                if !u.is_empty() {
                    parts.push(u.clone());
                }
            }
        }
        _ => {}
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
        let value = self
            .core_write()
            .create_starmap(title, desc, None)
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
        Self::json_string(&value)
    }

    pub fn get_starmap_graph_json(&self, starmap_id: &str) -> ApiResult<String> {
        let value = self
            .core_write()
            .get_starmap_graph(starmap_id)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn add_starmap_embed(
        &self,
        starmap_id: &str,
        embed: crate::api::types::StarMapEmbedDto,
    ) -> ApiResult<crate::api::types::StarMapEmbedDto> {
        let result = self
            .core_write()
            .add_starmap_embed(starmap_id, embed.into())
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

    pub fn update_starmap_embed(
        &self,
        starmap_id: &str,
        instance_id: &str,
        patch: crate::api::types::StarMapEmbedPatchDto,
    ) -> ApiResult<crate::api::types::StarMapEmbedDto> {
        let result = self
            .core_write()
            .update_starmap_embed(starmap_id, instance_id, patch.into())
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
            .add_starmap_link(starmap_id, link.into())
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
            .update_starmap_link(starmap_id, link_id, patch.into())
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

    pub fn find_starmap_references_json(&self, target_starmap_id: &str) -> ApiResult<String> {
        let value = self
            .core_write()
            .find_starmap_references(target_starmap_id)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
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

    pub fn get_starmap_motion_policy(
        &self,
    ) -> ApiResult<crate::api::types::StarMapMotionPolicyDto> {
        self.core_write()
            .get_motion_policy()
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn get_starmap_layout(
        &self,
        starmap_id: &str,
    ) -> ApiResult<crate::api::types::StarMapLayoutDto> {
        self.core_write()
            .get_starmap_layout(starmap_id)
            .map(Into::into)
            .map_err(Into::into)
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

    pub fn list_starmaps_for_project(
        &self,
        project_id: &str,
    ) -> ApiResult<Vec<crate::api::types::StarMapMetaDto>> {
        self.core_write()
            .list_starmaps_for_project(project_id)
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
        let result: crate::api::types::StarMapMetaDto = self
            .core_write()
            .create_starmap(title, desc, template_id)
            .map(Into::into)
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
        Ok(result)
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
            .add_starmap_node(starmap_id, node.into(), x, y)
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

    pub fn save_starmap_layout(
        &self,
        starmap_id: &str,
        layout: &crate::api::types::StarMapLayoutDto,
    ) -> ApiResult<bool> {
        let changed_paths = self
            .core_write()
            .save_starmap_layout(starmap_id, &layout.clone().into())
            .map_err(crate::api::error::WriterError::from)?;
        self.record_workspace_history(&changed_paths, "save_starmap_layout");
        Ok(true)
    }

    pub fn get_starmap_viewport(
        &self,
        starmap_id: &str,
    ) -> ApiResult<crate::api::types::StarMapViewportDto> {
        self.core_write()
            .get_starmap_viewport(starmap_id)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn save_starmap_viewport(
        &self,
        starmap_id: &str,
        viewport: crate::api::types::StarMapViewportDto,
    ) -> ApiResult<bool> {
        let changed_paths = self
            .core_write()
            .save_starmap_viewport(starmap_id, &viewport.into())
            .map_err(crate::api::error::WriterError::from)?;
        self.record_workspace_history(&changed_paths, "save_starmap_viewport");
        Ok(true)
    }

    pub fn compute_starmap_edge_renders(
        &self,
        graph: crate::api::types::StarMapGraphDto,
        layout: crate::api::types::StarMapLayoutDto,
    ) -> ApiResult<Vec<crate::api::types::StarMapEdgeRenderDto>> {
        let node_centers: HashMap<String, (f32, f32)> = layout
            .nodes
            .iter()
            .map(|node| {
                (
                    node.node_id.clone(),
                    (node.x + node.width / 2.0, node.y + node.height / 2.0),
                )
            })
            .collect();
        let edges: Vec<crate::starmap::render::EdgeInput> = graph
            .edges
            .into_iter()
            .filter_map(|edge| {
                let from = edge.from.filter(|id| !id.is_empty())?;
                let to = edge.to.filter(|id| !id.is_empty())?;
                Some(crate::starmap::render::EdgeInput {
                    id: edge.id,
                    from,
                    to,
                    label: edge.label,
                })
            })
            .collect();

        Ok(crate::starmap::render::compute_edge_renders(
            &edges,
            &node_centers,
            &crate::starmap::render::EdgeRenderParams::default(),
        )
        .into_iter()
        .map(Into::into)
        .collect())
    }

    pub fn hit_test_starmap_node(
        &self,
        layout: crate::api::types::StarMapLayoutDto,
        x: f32,
        y: f32,
    ) -> ApiResult<Option<String>> {
        let layout: crate::starmap::types::StarMapLayout = layout.into();
        Ok(crate::starmap::hittest::hit_test_nodes(x, y, &layout.nodes).map(|hit| hit.id))
    }

    pub fn rename_starmap(
        &self,
        starmap_id: &str,
        new_title: &str,
    ) -> ApiResult<crate::api::types::StarMapMetaDto> {
        let result = self
            .core_write()
            .rename_starmap(starmap_id, new_title)
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
        Ok(result.into())
    }

    pub fn delete_starmap(&self, starmap_id: &str) -> ApiResult<bool> {
        self.core_write().delete_starmap(starmap_id)?;
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
        self.core_write()
            .bind_starmap_to_project(starmap_id, project_id)?;
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
        self.core_write().unbind_starmap_from_project(starmap_id)?;
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
        Ok(true)
    }

    pub fn set_main_starmap_for_project(
        &self,
        starmap_id: &str,
        project_id: &str,
    ) -> ApiResult<bool> {
        self.core_write()
            .set_main_starmap_for_project(starmap_id, project_id)
            .map(|_| true)
            .map_err(crate::api::error::WriterError::from)?;
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

    pub fn create_child_starmap(
        &self,
        parent_id: &str,
        title: &str,
        desc: &str,
        accent_color: Option<&str>,
    ) -> ApiResult<crate::api::types::StarMapMetaDto> {
        let result: crate::api::types::StarMapMetaDto = self
            .core_write()
            .create_child_starmap(parent_id, title, desc, accent_color)
            .map(Into::into)
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
        Ok(result)
    }

    pub fn update_starmap_node(
        &self,
        starmap_id: &str,
        node_id: &str,
        patch: crate::api::types::StarMapNodePatchDto,
    ) -> ApiResult<crate::api::types::StarMapNodeDto> {
        let result = self
            .core_write()
            .update_starmap_node(starmap_id, node_id, patch.into())
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
            .add_starmap_edge(starmap_id, edge.into())
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
            .update_starmap_edge(starmap_id, edge_id, patch.into())
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

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn import_or_replace_starmap_package(
        &self,
        starmap_id: &str,
        graph: &crate::api::types::StarMapGraphDto,
        base_package_revision: u64,
    ) -> ApiResult<bool> {
        let old_graph = self.core_write().get_starmap_graph(starmap_id).ok();
        let old_node_ids: std::collections::HashSet<String> = old_graph
            .as_ref()
            .map(|g| g.nodes.iter().map(|n| n.id.clone()).collect())
            .unwrap_or_default();
        let old_edge_ids: std::collections::HashSet<String> = old_graph
            .as_ref()
            .map(|g| g.edges.iter().map(|e| e.id.clone()).collect())
            .unwrap_or_default();
        let old_link_ids: std::collections::HashSet<String> = old_graph
            .as_ref()
            .map(|g| g.links.iter().map(|l| l.link_id.clone()).collect())
            .unwrap_or_default();
        let old_embed_ids: std::collections::HashSet<String> = old_graph
            .as_ref()
            .map(|g| g.embeds.iter().map(|e| e.instance_id.clone()).collect())
            .unwrap_or_default();
        let old_hyperlink_ids: std::collections::HashSet<String> = old_graph
            .as_ref()
            .map(|g| {
                g.hyperlinks
                    .iter()
                    .map(|hl| hl.hyperlink_id.clone())
                    .collect()
            })
            .unwrap_or_default();

        let core = self.core_write();
        let changed_paths = core.import_or_replace_starmap_package(
            starmap_id,
            &graph.clone().into(),
            base_package_revision,
        )?;
        drop(core);
        self.record_workspace_history(&changed_paths, "import_or_replace_starmap_package");

        let project_id = get_starmap_project_id(self, starmap_id);

        let new_node_ids: std::collections::HashSet<String> =
            graph.nodes.iter().map(|n| n.id.clone()).collect();
        for node in &graph.nodes {
            let node_content = extract_node_dto_search_body(&node.content, &node.tags);
            let entry = crate::search::extractor::extract_starmap_node_entry(
                starmap_id,
                &node.id,
                project_id.as_deref(),
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
        for old_id in &old_node_ids - &new_node_ids {
            self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                action: crate::search::SearchIndexAction::Delete,
                object_id: format!("starmap_node:{}:{}", starmap_id, old_id),
                scope: crate::search::SearchScope::All,
                title: String::new(),
                body: String::new(),
                target: None,
            });
        }

        let new_edge_ids: std::collections::HashSet<String> =
            graph.edges.iter().map(|e| e.id.clone()).collect();
        for edge in &graph.edges {
            let label = edge.label.clone().unwrap_or_default();
            let entry = crate::search::extractor::extract_starmap_edge_entry(
                starmap_id,
                &edge.id,
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
        }
        for old_id in &old_edge_ids - &new_edge_ids {
            self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                action: crate::search::SearchIndexAction::Delete,
                object_id: format!("starmap_edge:{}:{}", starmap_id, old_id),
                scope: crate::search::SearchScope::All,
                title: String::new(),
                body: String::new(),
                target: None,
            });
        }

        let new_link_ids: std::collections::HashSet<String> =
            graph.links.iter().map(|l| l.link_id.clone()).collect();
        for link in &graph.links {
            let label = link.label.clone().unwrap_or_default();
            let entry = crate::search::extractor::extract_starmap_link_entry(
                starmap_id,
                &link.link_id,
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
        }
        for old_id in &old_link_ids - &new_link_ids {
            self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                action: crate::search::SearchIndexAction::Delete,
                object_id: format!("starmap_link:{}:{}", starmap_id, old_id),
                scope: crate::search::SearchScope::All,
                title: String::new(),
                body: String::new(),
                target: None,
            });
        }

        let new_embed_ids: std::collections::HashSet<String> =
            graph.embeds.iter().map(|e| e.instance_id.clone()).collect();
        for embed in &graph.embeds {
            let embed_label = embed.label.clone().unwrap_or_default();
            let entry = crate::search::extractor::extract_starmap_embed_entry(
                starmap_id,
                &embed.instance_id,
                project_id.as_deref(),
                &embed_label,
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
        for old_id in &old_embed_ids - &new_embed_ids {
            self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                action: crate::search::SearchIndexAction::Delete,
                object_id: format!("starmap_embed:{}:{}", starmap_id, old_id),
                scope: crate::search::SearchScope::All,
                title: String::new(),
                body: String::new(),
                target: None,
            });
        }

        let new_hyperlink_ids: std::collections::HashSet<String> = graph
            .hyperlinks
            .iter()
            .map(|hl| hl.hyperlink_id.clone())
            .collect();
        for hl in &graph.hyperlinks {
            let hl_label = hl.label.as_deref().unwrap_or("");
            let entry = crate::search::extractor::extract_starmap_hyperlink_entry(
                starmap_id,
                &hl.hyperlink_id,
                project_id.as_deref(),
                hl_label,
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
        for old_id in &old_hyperlink_ids - &new_hyperlink_ids {
            self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
                action: crate::search::SearchIndexAction::Delete,
                object_id: format!("starmap_hyperlink:{}:{}", starmap_id, old_id),
                scope: crate::search::SearchScope::All,
                title: String::new(),
                body: String::new(),
                target: None,
            });
        }

        let entry = crate::search::extractor::extract_starmap_title_entry(
            starmap_id,
            project_id.as_deref(),
            &graph.title,
        );
        self.enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        Ok(true)
    }

    pub fn flush_starmap_store(&self, starmap_id: &str) -> ApiResult<bool> {
        let changed_paths = self
            .core_write()
            .flush_starmap_store(starmap_id)
            .map_err(WriterError::from)?;
        self.record_workspace_history(&changed_paths, "flush_starmap_store");
        Ok(true)
    }

    pub fn close_starmap_store(&self, starmap_id: &str) -> ApiResult<bool> {
        let changed_paths = self
            .core_write()
            .close_starmap_store(starmap_id)
            .map_err(WriterError::from)?;
        self.record_workspace_history(&changed_paths, "close_starmap_store");
        Ok(true)
    }

    pub fn flush_all_starmap_stores(&self) -> ApiResult<bool> {
        let changed_paths = self
            .core_write()
            .flush_all_starmap_stores()
            .map_err(WriterError::from)?;
        self.record_workspace_history(&changed_paths, "flush_all_starmap_stores");
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
            .add_starmap_hyperlink(starmap_id, hl.into())
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
        let label = patch.label.as_ref().and_then(|opt| opt.as_deref());
        let target_uri = patch.target_uri.as_ref().and_then(|opt| opt.as_deref());
        let result = self
            .core_write()
            .update_starmap_hyperlink(starmap_id, hyperlink_id, label, target_uri)
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
