use crate::error::Result;
use crate::starmap::graph::validation;
use crate::starmap::store::{SaveQueueEntry, StarMapStore};

impl super::WriterCore {
    pub fn list_starmaps(&self) -> Result<Vec<crate::starmap::StarMapMeta>> {
        crate::starmap::list_starmaps(&self.workspace_path)
    }

    pub fn list_starmaps_for_project(
        &self,
        project_id: &str,
    ) -> Result<Vec<crate::starmap::StarMapMeta>> {
        crate::starmap::list_starmaps_for_project(&self.workspace_path, project_id)
    }

    pub fn list_starmaps_bound_to_project(
        &self,
        project_id: &str,
    ) -> Result<Vec<crate::starmap::StarMapMeta>> {
        crate::starmap::list_starmaps_bound_to_project(&self.workspace_path, project_id)
    }

    pub fn get_starmap(&self, starmap_id: &str) -> Result<crate::starmap::StarMapMeta> {
        crate::starmap::get_starmap(&self.workspace_path, starmap_id)
    }

    pub fn create_starmap(
        &self,
        title: &str,
        description: &str,
        accent_color: Option<&str>,
    ) -> Result<crate::starmap::StarMapMeta> {
        crate::starmap::create_starmap(&self.workspace_path, title, description, accent_color)
    }

    pub fn create_child_starmap(
        &self,
        parent_id: &str,
        title: &str,
        description: &str,
        accent_color: Option<&str>,
    ) -> Result<crate::starmap::StarMapMeta> {
        crate::starmap::create_child_starmap(
            &self.workspace_path,
            parent_id,
            title,
            description,
            accent_color,
        )
    }

    pub fn rename_starmap(
        &self,
        starmap_id: &str,
        new_title: &str,
    ) -> Result<crate::starmap::StarMapMeta> {
        crate::starmap::rename_starmap(&self.workspace_path, starmap_id, new_title)
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn delete_starmap(&self, starmap_id: &str) -> Result<()> {
        {
            let mut stores = self
                .starmap_stores
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(store) = stores.get_mut(starmap_id) {
                if store.is_dirty() || store.has_pending_deletes() {
                    store.flush()?;
                }
            }
        }
        {
            let mut stores = self
                .starmap_stores
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            stores.remove(starmap_id);
        }
        crate::starmap::delete_starmap(&self.workspace_path, starmap_id)
    }

    pub fn bind_starmap_to_project(&self, starmap_id: &str, project_id: &str) -> Result<()> {
        crate::starmap::bind_starmap_to_project(&self.workspace_path, starmap_id, project_id)
    }

    pub fn set_main_starmap_for_project(&self, starmap_id: &str, project_id: &str) -> Result<()> {
        crate::starmap::set_main_starmap_for_project(&self.workspace_path, starmap_id, project_id)
    }

    pub fn get_main_starmap_for_project(
        &self,
        project_id: &str,
    ) -> Result<Option<crate::starmap::StarMapMeta>> {
        crate::starmap::get_main_starmap_for_project(&self.workspace_path, project_id)
    }

    pub fn unbind_starmap_from_project(&self, starmap_id: &str) -> Result<()> {
        crate::starmap::unbind_starmap_from_project(&self.workspace_path, starmap_id)
    }

    pub fn get_starmap_graph(
        &self,
        starmap_id: &str,
    ) -> Result<crate::starmap::types::StarMapGraph> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.workspace_path, starmap_id));
        store.ensure_loaded()?;
        Ok(store.to_starmap_graph())
    }

    pub fn import_or_replace_starmap_package(
        &self,
        starmap_id: &str,
        graph: &crate::starmap::types::StarMapGraph,
        base_package_revision: u64,
    ) -> Result<()> {
        validation::validate_graph(&self.workspace_path, graph)?;

        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.workspace_path, starmap_id));

        store.ensure_fully_loaded()?;

        let current_revision = store.package_revision();
        if base_package_revision != current_revision {
            return Err(crate::error::Error::Other(format!(
                "package revision mismatch: base={}, current={}",
                base_package_revision, current_revision
            )));
        }

        let old_node_ids: std::collections::HashSet<String> =
            store.all_nodes().map(|n| n.id.clone()).collect();
        let old_edge_ids: std::collections::HashSet<String> =
            store.all_edges().map(|e| e.id.clone()).collect();
        let old_embed_ids: std::collections::HashSet<String> =
            store.all_embeds().map(|e| e.instance_id.clone()).collect();
        let old_link_ids: std::collections::HashSet<String> =
            store.all_links().map(|l| l.link_id.clone()).collect();
        let old_hyperlink_ids: std::collections::HashSet<String> = store
            .all_hyperlinks()
            .map(|hl| hl.hyperlink_id.clone())
            .collect();

        let new_node_ids: std::collections::HashSet<String> =
            graph.nodes.iter().map(|n| n.id.clone()).collect();
        let new_edge_ids: std::collections::HashSet<String> =
            graph.edges.iter().map(|e| e.id.clone()).collect();
        let new_embed_ids: std::collections::HashSet<String> =
            graph.embeds.iter().map(|e| e.instance_id.clone()).collect();
        let new_link_ids: std::collections::HashSet<String> =
            graph.links.iter().map(|l| l.link_id.clone()).collect();
        let new_hyperlink_ids: std::collections::HashSet<String> = graph
            .hyperlinks
            .iter()
            .map(|hl| hl.hyperlink_id.clone())
            .collect();

        for node in &graph.nodes {
            store.upsert_node(node.clone());
        }
        for edge in &graph.edges {
            store.upsert_edge(edge.clone());
        }
        for embed in &graph.embeds {
            store.upsert_embed(embed.clone());
        }
        for link in &graph.links {
            store.upsert_link(link.clone());
        }
        for hl in &graph.hyperlinks {
            store.upsert_hyperlink(hl.clone());
        }

        for old_id in &old_node_ids {
            if !new_node_ids.contains(old_id) {
                let _ = store.delete_node(old_id);
            }
        }
        for old_id in &old_edge_ids {
            if !new_edge_ids.contains(old_id) {
                store.remove_edge(old_id);
            }
        }
        for old_id in &old_embed_ids {
            if !new_embed_ids.contains(old_id) {
                store.remove_embed(old_id);
            }
        }
        for old_id in &old_link_ids {
            if !new_link_ids.contains(old_id) {
                store.remove_link(old_id);
            }
        }
        for old_id in &old_hyperlink_ids {
            if !new_hyperlink_ids.contains(old_id) {
                store.remove_hyperlink(old_id);
            }
        }

        store.enqueue_save(SaveQueueEntry::Node);
        store.enqueue_save(SaveQueueEntry::Edge);
        store.enqueue_save(SaveQueueEntry::Embed);
        store.enqueue_save(SaveQueueEntry::Link);
        store.enqueue_save(SaveQueueEntry::Hyperlink);
        store.enqueue_save(SaveQueueEntry::DeleteNode);
        store.enqueue_save(SaveQueueEntry::DeleteEdge);
        store.enqueue_save(SaveQueueEntry::DeleteEmbed);
        store.enqueue_save(SaveQueueEntry::DeleteLink);
        store.enqueue_save(SaveQueueEntry::DeleteHyperlink);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        store.flush_save_queue()?;

        Ok(())
    }

    pub fn get_starmap_store_package_revision(&self, starmap_id: &str) -> u64 {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.workspace_path, starmap_id));
        let _ = store.ensure_loaded();
        store.package_revision()
    }

    pub fn add_starmap_node(
        &self,
        starmap_id: &str,
        node: crate::starmap::types::StarMapNode,
        default_x: f32,
        default_y: f32,
    ) -> Result<crate::starmap::types::StarMapNode> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.workspace_path, starmap_id));
        store.ensure_loaded()?;
        let result = store.add_node(node, default_x, default_y);
        store.enqueue_save(SaveQueueEntry::Node);
        store.enqueue_save(SaveQueueEntry::Layout);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        Ok(result)
    }

    pub fn update_starmap_node(
        &self,
        starmap_id: &str,
        node_id: &str,
        patch: crate::starmap::types::StarMapNodePatch,
    ) -> Result<crate::starmap::types::StarMapNode> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.workspace_path, starmap_id));
        store.ensure_loaded()?;
        store.ensure_object_loaded(node_id)?;
        let result = store.update_node(node_id, &patch)?;
        store.enqueue_save(SaveQueueEntry::Node);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        Ok(result)
    }

    pub fn delete_starmap_node(&self, starmap_id: &str, node_id: &str) -> Result<()> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.workspace_path, starmap_id));
        store.ensure_loaded()?;
        store.ensure_object_loaded(node_id)?;
        store.delete_node(node_id)?;
        store.enqueue_save(SaveQueueEntry::DeleteNode);
        store.enqueue_save(SaveQueueEntry::DeleteEdge);
        store.enqueue_save(SaveQueueEntry::DeleteEmbed);
        store.enqueue_save(SaveQueueEntry::DeleteLink);
        store.enqueue_save(SaveQueueEntry::DeleteHyperlink);
        store.enqueue_save(SaveQueueEntry::Layout);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        Ok(())
    }

    pub fn add_starmap_edge(
        &self,
        starmap_id: &str,
        edge: crate::starmap::types::StarMapEdge,
    ) -> Result<crate::starmap::types::StarMapEdge> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.workspace_path, starmap_id));
        store.ensure_loaded()?;
        let result = store.add_edge(edge)?;
        store.enqueue_save(SaveQueueEntry::Edge);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        Ok(result)
    }

    pub fn update_starmap_edge(
        &self,
        starmap_id: &str,
        edge_id: &str,
        patch: crate::starmap::types::StarMapEdgePatch,
    ) -> Result<crate::starmap::types::StarMapEdge> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.workspace_path, starmap_id));
        store.ensure_loaded()?;
        store.ensure_edge_loaded(edge_id)?;
        let result = store.update_edge(edge_id, &patch)?;
        store.enqueue_save(SaveQueueEntry::Edge);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        Ok(result)
    }

    pub fn delete_starmap_edge(&self, starmap_id: &str, edge_id: &str) -> Result<()> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.workspace_path, starmap_id));
        store.ensure_loaded()?;
        store.ensure_edge_loaded(edge_id)?;
        store.delete_edge(edge_id)?;
        store.enqueue_save(SaveQueueEntry::DeleteEdge);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        Ok(())
    }

    pub fn get_starmap_layout(
        &self,
        starmap_id: &str,
    ) -> Result<crate::starmap::types::StarMapLayout> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.workspace_path, starmap_id));
        store.ensure_loaded()?;
        Ok(store.get_layout().cloned().unwrap_or_default())
    }

    pub fn save_starmap_layout(
        &self,
        starmap_id: &str,
        layout: &crate::starmap::types::StarMapLayout,
    ) -> Result<()> {
        validation::validate_layout(layout)?;

        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.workspace_path, starmap_id));
        store.ensure_loaded()?;
        store.set_layout(layout.clone());
        store.enqueue_save(SaveQueueEntry::Layout);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        store.flush_save_queue()?;
        Ok(())
    }

    pub fn get_starmap_viewport(
        &self,
        starmap_id: &str,
    ) -> Result<crate::starmap::types::StarMapViewport> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.workspace_path, starmap_id));
        store.ensure_loaded()?;
        Ok(store.get_viewport().cloned().unwrap_or_default())
    }

    pub fn save_starmap_viewport(
        &self,
        starmap_id: &str,
        viewport: &crate::starmap::types::StarMapViewport,
    ) -> Result<()> {
        validation::validate_viewport(viewport)?;

        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.workspace_path, starmap_id));
        store.ensure_loaded()?;
        store.set_viewport(viewport.clone());
        store.flush_viewport()?;
        Ok(())
    }

    pub fn add_starmap_embed(
        &self,
        starmap_id: &str,
        embed: crate::starmap::types::StarMapEmbed,
    ) -> Result<crate::starmap::types::StarMapEmbed> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.workspace_path, starmap_id));
        store.ensure_loaded()?;
        let result = store.add_embed(embed)?;
        store.enqueue_save(SaveQueueEntry::Embed);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        Ok(result)
    }

    pub fn update_starmap_embed(
        &self,
        starmap_id: &str,
        instance_id: &str,
        patch: crate::starmap::types::StarMapEmbedPatch,
    ) -> Result<crate::starmap::types::StarMapEmbed> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.workspace_path, starmap_id));
        store.ensure_loaded()?;
        store.ensure_embed_loaded(instance_id)?;
        let result = store.update_embed(instance_id, &patch)?;
        store.enqueue_save(SaveQueueEntry::Embed);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        Ok(result)
    }

    pub fn delete_starmap_embed(&self, starmap_id: &str, instance_id: &str) -> Result<()> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.workspace_path, starmap_id));
        store.ensure_loaded()?;
        store.ensure_embed_loaded(instance_id)?;
        store.delete_embed(instance_id)?;
        store.enqueue_save(SaveQueueEntry::DeleteEmbed);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        Ok(())
    }

    pub fn add_starmap_link(
        &self,
        starmap_id: &str,
        link: crate::starmap::types::StarMapLink,
    ) -> Result<crate::starmap::types::StarMapLink> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.workspace_path, starmap_id));
        store.ensure_loaded()?;
        let result = store.add_link(link)?;
        store.enqueue_save(SaveQueueEntry::Link);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        Ok(result)
    }

    pub fn update_starmap_link(
        &self,
        starmap_id: &str,
        link_id: &str,
        patch: crate::starmap::types::StarMapLinkPatch,
    ) -> Result<crate::starmap::types::StarMapLink> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.workspace_path, starmap_id));
        store.ensure_loaded()?;
        store.ensure_link_loaded(link_id)?;
        let result = store.update_link(link_id, &patch)?;
        store.enqueue_save(SaveQueueEntry::Link);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        Ok(result)
    }

    pub fn delete_starmap_link(&self, starmap_id: &str, link_id: &str) -> Result<()> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.workspace_path, starmap_id));
        store.ensure_loaded()?;
        store.ensure_link_loaded(link_id)?;
        store.delete_link(link_id)?;
        store.enqueue_save(SaveQueueEntry::DeleteLink);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        Ok(())
    }

    pub fn list_starmap_hyperlinks(
        &self,
        starmap_id: &str,
    ) -> Result<crate::starmap::store::ListWithDiagnostics<crate::starmap::types::StarMapHyperlink>>
    {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.workspace_path, starmap_id));
        store.ensure_fully_loaded()?;
        Ok(store.list_hyperlinks_with_diagnostics())
    }

    pub fn add_starmap_hyperlink(
        &self,
        starmap_id: &str,
        hl: crate::starmap::types::StarMapHyperlink,
    ) -> Result<crate::starmap::types::StarMapHyperlink> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.workspace_path, starmap_id));
        store.ensure_loaded()?;
        let result = store.add_hyperlink(hl)?;
        store.enqueue_save(SaveQueueEntry::Hyperlink);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        Ok(result)
    }

    pub fn update_starmap_hyperlink(
        &self,
        starmap_id: &str,
        hyperlink_id: &str,
        label: Option<&str>,
        target_uri: Option<&str>,
    ) -> Result<crate::starmap::types::StarMapHyperlink> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.workspace_path, starmap_id));
        store.ensure_loaded()?;
        store.ensure_hyperlink_loaded(hyperlink_id)?;
        let result = store.update_hyperlink(hyperlink_id, label, target_uri)?;
        store.enqueue_save(SaveQueueEntry::Hyperlink);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        Ok(result)
    }

    pub fn delete_starmap_hyperlink(&self, starmap_id: &str, hyperlink_id: &str) -> Result<()> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.workspace_path, starmap_id));
        store.ensure_loaded()?;
        store.ensure_hyperlink_loaded(hyperlink_id)?;
        store.delete_hyperlink(hyperlink_id)?;
        store.enqueue_save(SaveQueueEntry::DeleteHyperlink);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        Ok(())
    }

    pub fn list_starmap_links(
        &self,
        starmap_id: &str,
    ) -> Result<crate::starmap::store::ListWithDiagnostics<crate::starmap::types::StarMapLink>>
    {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.workspace_path, starmap_id));
        store.ensure_fully_loaded()?;
        Ok(store.list_links_with_diagnostics())
    }

    pub fn get_starmap_phased_snapshot(
        &self,
        starmap_id: &str,
        request: &crate::starmap::store::PhasedSnapshotRequest,
    ) -> Result<crate::starmap::store::StarMapPhasedSnapshot> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.workspace_path, starmap_id));
        store.get_phased_snapshot(request)
    }

    pub fn find_starmap_references(
        &self,
        target_starmap_id: &str,
    ) -> Result<Vec<crate::starmap::StarMapReference>> {
        crate::starmap::find_starmap_references(&self.workspace_path, target_starmap_id)
    }

    pub fn get_motion_policy(&self) -> Result<crate::starmap::types::StarMapMotionPolicyDto> {
        crate::starmap::get_motion_policy(&self.workspace_path)
    }

    pub fn close_starmap_store(&self, starmap_id: &str) -> Result<()> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(store) = stores.get_mut(starmap_id) {
            if store.is_dirty() || store.has_pending_deletes() || store.save_queue_len() > 0 {
                store.flush()?;
            }
        }
        stores.remove(starmap_id);
        Ok(())
    }

    pub fn flush_starmap_store(&self, starmap_id: &str) -> Result<()> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(store) = stores.get_mut(starmap_id) {
            if store.is_dirty() || store.has_pending_deletes() || store.save_queue_len() > 0 {
                store.flush_save_queue()?;
            }
        }
        Ok(())
    }

    pub fn flush_all_starmap_stores(&self) -> Result<()> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        for store in stores.values_mut() {
            if store.is_dirty() || store.has_pending_deletes() || store.save_queue_len() > 0 {
                store.flush_save_queue()?;
            }
        }
        Ok(())
    }
}
