use super::*;

fn extract_node_search_body(content: &crate::starmap::semantic::StarMapNodeContent) -> String {
    match content {
        crate::starmap::semantic::StarMapNodeContent::Inline { summary, body } => {
            let mut parts = Vec::new();
            if let Some(ref s) = summary {
                if !s.is_empty() { parts.push(s.clone()); }
            }
            if let Some(ref b) = body {
                if !b.is_empty() { parts.push(b.clone()); }
            }
            parts.join(" ")
        }
        _ => String::new(),
    }
}

fn extract_node_dto_search_body(content: &crate::api::types::StarMapNodeContentDto) -> String {
    let mut parts = Vec::new();
    if let Some(ref s) = content.summary {
        if !s.is_empty() { parts.push(s.clone()); }
    }
    if let Some(ref b) = content.body {
        if !b.is_empty() { parts.push(b.clone()); }
    }
    parts.join(" ")
}

impl WriterCoreApi {
    pub fn list_starmaps_json(&self) -> ApiResult<String> {
        let value = self.core().list_starmaps().map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn create_starmap_json(&self, title: &str, desc: &str) -> ApiResult<String> {
        let value = self
            .core()
            .create_starmap(title, desc, None)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn get_starmap_graph_json(&self, starmap_id: &str) -> ApiResult<String> {
        let value = self
            .core()
            .get_starmap_graph(starmap_id)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn add_starmap_embed(
        &self,
        starmap_id: &str,
        embed: crate::api::types::StarMapEmbedDto,
    ) -> ApiResult<crate::api::types::StarMapEmbedDto> {
        let result = self.core()
            .add_starmap_embed(starmap_id, embed.into())
            .map_err(WriterError::from)?;
        self.core().enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: format!("starmap_embed:{}:{}", starmap_id, result.instance_id),
            scope: crate::search::SearchScope::StarmapNode,
            title: result.label.clone().unwrap_or_default(),
            body: result.label.clone().unwrap_or_default(),
            target: Some(crate::search::SearchTarget {
                project_id: None,
                volume_id: None,
                chapter_id: None,
                starmap_id: Some(starmap_id.to_string()),
                node_id: None,
                setting_key: None,
            }),
        });
        Ok(result.into())
    }

    pub fn update_starmap_embed(
        &self,
        starmap_id: &str,
        instance_id: &str,
        patch: crate::api::types::StarMapEmbedPatchDto,
    ) -> ApiResult<crate::api::types::StarMapEmbedDto> {
        let result = self.core()
            .update_starmap_embed(starmap_id, instance_id, patch.into())
            .map_err(WriterError::from)?;
        self.core().enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: format!("starmap_embed:{}:{}", starmap_id, instance_id),
            scope: crate::search::SearchScope::StarmapNode,
            title: result.label.clone().unwrap_or_default(),
            body: result.label.clone().unwrap_or_default(),
            target: Some(crate::search::SearchTarget {
                project_id: None,
                volume_id: None,
                chapter_id: None,
                starmap_id: Some(starmap_id.to_string()),
                node_id: None,
                setting_key: None,
            }),
        });
        Ok(result.into())
    }

    pub fn delete_starmap_embed(&self, starmap_id: &str, instance_id: &str) -> ApiResult<bool> {
        self.core()
            .delete_starmap_embed(starmap_id, instance_id)?;
        self.core().enqueue_search_index_update(crate::search::SearchIndexUpdate {
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
        let result = self.core()
            .add_starmap_link(starmap_id, link.into())
            .map_err(WriterError::from)?;
        let label = result.label.clone().unwrap_or_default();
        let entry = crate::search::extractor::extract_starmap_link_entry(
            starmap_id, &result.link_id, None, &label,
        );
        self.core().enqueue_search_index_update(crate::search::SearchIndexUpdate {
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
        let result = self.core()
            .update_starmap_link(starmap_id, link_id, patch.into())
            .map_err(WriterError::from)?;
        let label = result.label.clone().unwrap_or_default();
        let entry = crate::search::extractor::extract_starmap_link_entry(
            starmap_id, link_id, None, &label,
        );
        self.core().enqueue_search_index_update(crate::search::SearchIndexUpdate {
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
        self.core()
            .delete_starmap_link(starmap_id, link_id)?;
        self.core().enqueue_search_index_update(crate::search::SearchIndexUpdate {
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
            .core()
            .find_starmap_references(target_starmap_id)
            .map_err(WriterError::from)?;
        Self::json_string(&value)
    }

    pub fn find_starmap_references(
        &self,
        target_starmap_id: &str,
    ) -> ApiResult<Vec<crate::api::types::StarMapReferenceDto>> {
        self.core()
            .find_starmap_references(target_starmap_id)
            .map(|list| list.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn get_starmap_motion_policy(
        &self,
    ) -> ApiResult<crate::api::types::StarMapMotionPolicyDto> {
        self.core()
            .get_motion_policy()
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn get_starmap_layout(
        &self,
        starmap_id: &str,
    ) -> ApiResult<crate::api::types::StarMapLayoutDto> {
        self.core()
            .get_starmap_layout(starmap_id)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn get_starmap_graph(
        &self,
        starmap_id: &str,
    ) -> ApiResult<crate::api::types::StarMapGraphDto> {
        self.core()
            .get_starmap_graph(starmap_id)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn list_starmaps(&self) -> ApiResult<Vec<crate::api::types::StarMapMetaDto>> {
        self.core()
            .list_starmaps()
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn list_starmaps_for_project(
        &self,
        project_id: &str,
    ) -> ApiResult<Vec<crate::api::types::StarMapMetaDto>> {
        self.core()
            .list_starmaps_for_project(project_id)
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn get_starmap(&self, starmap_id: &str) -> ApiResult<crate::api::types::StarMapMetaDto> {
        self.core()
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
        self.core()
            .create_starmap(title, desc, template_id)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn add_starmap_node(
        &self,
        starmap_id: &str,
        node: crate::api::types::StarMapNodeDto,
        x: f32,
        y: f32,
    ) -> ApiResult<crate::api::types::StarMapNodeDto> {
        let result = self.core()
            .add_starmap_node(starmap_id, node.into(), x, y)
            .map_err(WriterError::from)?;
        let node_content = extract_node_search_body(&result.content);
        let entry = crate::search::extractor::extract_starmap_node_entry(
            starmap_id, &result.id, None, &result.title, &node_content,
        );
        self.core().enqueue_search_index_update(crate::search::SearchIndexUpdate {
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
        self.core()
            .save_starmap_layout(starmap_id, &layout.clone().into())
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn get_starmap_viewport(
        &self,
        starmap_id: &str,
    ) -> ApiResult<crate::api::types::StarMapViewportDto> {
        self.core()
            .get_starmap_viewport(starmap_id)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn save_starmap_viewport(
        &self,
        starmap_id: &str,
        viewport: crate::api::types::StarMapViewportDto,
    ) -> ApiResult<bool> {
        self.core()
            .save_starmap_viewport(starmap_id, &viewport.into())
            .map(|_| true)
            .map_err(Into::into)
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
        let result = self.core()
            .rename_starmap(starmap_id, new_title)
            .map_err(WriterError::from)?;
        let entry = crate::search::extractor::extract_starmap_title_entry(
            starmap_id, None, new_title,
        );
        self.core().enqueue_search_index_update(crate::search::SearchIndexUpdate {
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
        self.core()
            .delete_starmap(starmap_id)?;
        for prefix in &[
            format!("starmap:{}", starmap_id),
            format!("starmap_node:{}:", starmap_id),
            format!("starmap_edge:{}:", starmap_id),
            format!("starmap_hyperlink:{}:", starmap_id),
            format!("starmap_link:{}:", starmap_id),
            format!("starmap_embed:{}:", starmap_id),
        ] {
            self.core().remove_search_index_by_prefix(prefix);
        }
        Ok(true)
    }

    pub fn bind_starmap_to_project(&self, starmap_id: &str, project_id: &str) -> ApiResult<bool> {
        self.core()
            .bind_starmap_to_project(starmap_id, project_id)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn unbind_starmap_from_project(&self, starmap_id: &str) -> ApiResult<bool> {
        self.core()
            .unbind_starmap_from_project(starmap_id)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn set_main_starmap_for_project(
        &self,
        starmap_id: &str,
        project_id: &str,
    ) -> ApiResult<bool> {
        self.core()
            .set_main_starmap_for_project(starmap_id, project_id)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn get_main_starmap_for_project(
        &self,
        project_id: &str,
    ) -> ApiResult<Option<crate::api::types::StarMapMetaDto>> {
        self.core()
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
        self.core()
            .create_child_starmap(parent_id, title, desc, accent_color)
            .map(Into::into)
            .map_err(Into::into)
    }

    pub fn update_starmap_node(
        &self,
        starmap_id: &str,
        node_id: &str,
        patch: crate::api::types::StarMapNodePatchDto,
    ) -> ApiResult<crate::api::types::StarMapNodeDto> {
        let result = self.core()
            .update_starmap_node(starmap_id, node_id, patch.into())
            .map_err(WriterError::from)?;
        let node_content = extract_node_search_body(&result.content);
        let entry = crate::search::extractor::extract_starmap_node_entry(
            starmap_id, node_id, None, &result.title, &node_content,
        );
        self.core().enqueue_search_index_update(crate::search::SearchIndexUpdate {
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
        self.core()
            .delete_starmap_node(starmap_id, node_id)?;
        self.core().enqueue_search_index_update(crate::search::SearchIndexUpdate {
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
        let result = self.core()
            .add_starmap_edge(starmap_id, edge.into())
            .map_err(WriterError::from)?;
        let label = result.label.clone().unwrap_or_default();
        let entry = crate::search::extractor::extract_starmap_edge_entry(
            starmap_id, &result.id, None, &label,
        );
        self.core().enqueue_search_index_update(crate::search::SearchIndexUpdate {
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
        let result = self.core()
            .update_starmap_edge(starmap_id, edge_id, patch.into())
            .map_err(WriterError::from)?;
        let label = result.label.clone().unwrap_or_default();
        let entry = crate::search::extractor::extract_starmap_edge_entry(
            starmap_id, edge_id, None, &label,
        );
        self.core().enqueue_search_index_update(crate::search::SearchIndexUpdate {
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
        self.core()
            .delete_starmap_edge(starmap_id, edge_id)?;
        self.core().enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Delete,
            object_id: format!("starmap_edge:{}:{}", starmap_id, edge_id),
            scope: crate::search::SearchScope::All,
            title: String::new(),
            body: String::new(),
            target: None,
        });
        Ok(true)
    }

    pub fn save_starmap_graph(
        &self,
        starmap_id: &str,
        graph: &crate::api::types::StarMapGraphDto,
    ) -> ApiResult<bool> {
        self.core()
            .save_starmap_graph(starmap_id, &graph.clone().into())?;
        for node in &graph.nodes {
            let node_content = extract_node_dto_search_body(&node.content);
            let entry = crate::search::extractor::extract_starmap_node_entry(
                starmap_id, &node.id, None, &node.title, &node_content,
            );
            self.core().enqueue_search_index_update(crate::search::SearchIndexUpdate {
                action: crate::search::SearchIndexAction::Upsert,
                object_id: entry.object_id.clone(),
                scope: entry.scope,
                title: entry.title.clone(),
                body: entry.body.clone(),
                target: Some(entry.target.clone()),
            });
        }
        for edge in &graph.edges {
            let label = edge.label.clone().unwrap_or_default();
            let entry = crate::search::extractor::extract_starmap_edge_entry(
                starmap_id, &edge.id, None, &label,
            );
            self.core().enqueue_search_index_update(crate::search::SearchIndexUpdate {
                action: crate::search::SearchIndexAction::Upsert,
                object_id: entry.object_id.clone(),
                scope: entry.scope,
                title: entry.title.clone(),
                body: entry.body.clone(),
                target: Some(entry.target.clone()),
            });
        }
        for link in &graph.links {
            let label = link.label.clone().unwrap_or_default();
            let entry = crate::search::extractor::extract_starmap_link_entry(
                starmap_id, &link.link_id, None, &label,
            );
            self.core().enqueue_search_index_update(crate::search::SearchIndexUpdate {
                action: crate::search::SearchIndexAction::Upsert,
                object_id: entry.object_id.clone(),
                scope: entry.scope,
                title: entry.title.clone(),
                body: entry.body.clone(),
                target: Some(entry.target.clone()),
            });
        }
        let entry = crate::search::extractor::extract_starmap_title_entry(
            starmap_id, None, &graph.title,
        );
        self.core().enqueue_search_index_update(crate::search::SearchIndexUpdate {
            action: crate::search::SearchIndexAction::Upsert,
            object_id: entry.object_id.clone(),
            scope: entry.scope,
            title: entry.title.clone(),
            body: entry.body.clone(),
            target: Some(entry.target.clone()),
        });
        Ok(true)
    }
}
