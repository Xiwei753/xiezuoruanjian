use crate::error::Result;
use crate::starmap::graph::validation;
use crate::starmap::store::{SaveQueueEntry, StarMapStore};

impl super::WriterCore {
    pub fn list_starmaps(&self) -> Result<Vec<crate::starmap::StarMapMeta>> {
        crate::starmap::list_starmaps(&self.app_data_root)
    }

    pub fn list_starmaps_for_project(
        &self,
        project_id: &str,
    ) -> Result<Vec<crate::starmap::StarMapMeta>> {
        crate::starmap::list_starmaps_for_project(&self.app_data_root, project_id)
    }

    pub fn list_starmaps_bound_to_project(
        &self,
        project_id: &str,
    ) -> Result<Vec<crate::starmap::StarMapMeta>> {
        crate::starmap::list_starmaps_bound_to_project(&self.app_data_root, project_id)
    }

    pub fn get_starmap(&self, starmap_id: &str) -> Result<crate::starmap::StarMapMeta> {
        crate::starmap::get_starmap(&self.app_data_root, starmap_id)
    }

    pub fn create_starmap(
        &self,
        title: &str,
        description: &str,
        accent_color: Option<&str>,
    ) -> Result<crate::starmap::StarMapMeta> {
        crate::starmap::create_starmap(&self.app_data_root, title, description, accent_color)
    }

    ///   create_starmap 的变更集版本。
    pub fn create_starmap_with_changes(
        &self,
        title: &str,
        description: &str,
        accent_color: Option<&str>,
    ) -> Result<(
        crate::starmap::StarMapMeta,
        crate::storage::workspace_git::WorkspaceChangeSet,
    )> {
        crate::starmap::create_starmap_with_changes(
            &self.app_data_root,
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
        crate::starmap::rename_starmap(&self.app_data_root, starmap_id, new_title)
    }

    ///   rename_starmap 的变更集版本。
    pub fn rename_starmap_with_changes(
        &self,
        starmap_id: &str,
        new_title: &str,
    ) -> Result<(
        crate::starmap::StarMapMeta,
        crate::storage::workspace_git::WorkspaceChangeSet,
    )> {
        crate::starmap::rename_starmap_with_changes(&self.app_data_root, starmap_id, new_title)
    }

    #[allow(
        clippy::too_many_lines,
        clippy::cognitive_complexity,
        clippy::excessive_nesting,
        clippy::too_many_arguments,
        clippy::type_complexity
    )]
    pub fn delete_starmap(&self, starmap_id: &str) -> Result<()> {
        // Fix 6: 引用扫描前必须 flush 所有 dirty starmap stores，否则
        // find_starmap_references 读到的磁盘数据可能不含刚写入的引用，
        // 导致误删。先 flush 全部，再移除待删 store，最后落盘删除。
        self.flush_all_starmap_stores()?;
        {
            let mut stores = self
                .starmap_stores
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            stores.remove(starmap_id);
        }
        crate::starmap::delete_starmap(&self.app_data_root, starmap_id)
    }

    ///   delete_starmap 的变更集版本。
    pub fn delete_starmap_with_changes(
        &self,
        starmap_id: &str,
    ) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
        // 先 flush 全部 dirty stores（与 delete_starmap 同样的前置逻辑），
        // 再移除缓存并落盘删除。
        self.flush_all_starmap_stores()?;
        self.remove_starmap_store(starmap_id);
        crate::starmap::delete_starmap_with_changes(&self.app_data_root, starmap_id)
    }

    /// 从缓存中移除指定 starmap store（内部 helper）。
    fn remove_starmap_store(&self, starmap_id: &str) {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        stores.remove(starmap_id);
    }

    pub fn bind_starmap_to_project(&self, starmap_id: &str, project_id: &str) -> Result<()> {
        crate::starmap::bind_starmap_to_project(&self.app_data_root, starmap_id, project_id)
    }

    ///   bind_starmap_to_project 的变更集版本。
    pub fn bind_starmap_to_project_with_changes(
        &self,
        starmap_id: &str,
        project_id: &str,
    ) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
        crate::starmap::bind_starmap_to_project_with_changes(
            &self.app_data_root,
            starmap_id,
            project_id,
        )
    }

    pub fn set_main_starmap_for_project(&self, starmap_id: &str, project_id: &str) -> Result<()> {
        crate::starmap::set_main_starmap_for_project(&self.app_data_root, starmap_id, project_id)
    }

    ///   set_main_starmap_for_project 的变更集版本。
    pub fn set_main_starmap_for_project_with_changes(
        &self,
        starmap_id: &str,
        project_id: &str,
    ) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
        crate::starmap::set_main_starmap_for_project_with_changes(
            &self.app_data_root,
            starmap_id,
            project_id,
        )
    }

    pub fn get_main_starmap_for_project(
        &self,
        project_id: &str,
    ) -> Result<Option<crate::starmap::StarMapMeta>> {
        crate::starmap::get_main_starmap_for_project(&self.app_data_root, project_id)
    }

    pub fn unbind_starmap_from_project(&self, starmap_id: &str) -> Result<()> {
        crate::starmap::unbind_starmap_from_project(&self.app_data_root, starmap_id)
    }

    ///   unbind_starmap_from_project 的变更集版本。
    pub fn unbind_starmap_from_project_with_changes(
        &self,
        starmap_id: &str,
    ) -> Result<crate::storage::workspace_git::WorkspaceChangeSet> {
        crate::starmap::unbind_starmap_from_project_with_changes(&self.app_data_root, starmap_id)
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
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        store.ensure_fully_loaded()?;
        Ok(store.to_starmap_graph())
    }

    pub fn import_or_replace_starmap_package(
        &self,
        starmap_id: &str,
        graph: &crate::starmap::types::StarMapGraph,
        base_package_revision: u64,
    ) -> Result<Vec<std::path::PathBuf>> {
        // Fix 2: graph.starmap_id 必须与传入的 starmap_id 一致，
        // 否则 validate_graph 中的 path.starmap_id == graph.starmap_id
        // 不变量无法保证跨层路径的正确性。
        if graph.starmap_id != starmap_id {
            return Err(crate::error::Error::Other(format!(
                "graph.starmap_id ({}) does not match starmap_id ({})",
                graph.starmap_id, starmap_id
            )));
        }
        validation::validate_graph(&self.app_data_root, graph)?;

        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));

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
        store.flush_save_queue()
    }

    pub fn get_starmap_store_package_revision(&self, starmap_id: &str) -> u64 {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
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
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        store.ensure_fully_loaded()?;

        // 先在 candidate graph 上模拟 add，跑 validate_graph，再真正改 Store。
        let mut candidate = store.to_starmap_graph();
        candidate.nodes.push(node.clone());
        validation::validate_graph(&self.app_data_root, &candidate)?;

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
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        store.ensure_fully_loaded()?;
        store.ensure_object_loaded(node_id)?;

        // 先在 candidate graph 上模拟 update，跑 validate_graph，再真正改 Store。
        let mut candidate = store.to_starmap_graph();
        apply_node_patch_to_graph(&mut candidate, node_id, &patch)?;
        validation::validate_graph(&self.app_data_root, &candidate)?;

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
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        // 级联删除需要完整加载，否则会漏掉跨对象的引用关系。
        store.ensure_fully_loaded()?;
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
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        store.ensure_fully_loaded()?;

        let mut candidate = store.to_starmap_graph();
        candidate.edges.push(edge.clone());
        validation::validate_graph(&self.app_data_root, &candidate)?;

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
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        store.ensure_fully_loaded()?;
        store.ensure_edge_loaded(edge_id)?;

        let mut candidate = store.to_starmap_graph();
        apply_edge_patch_to_graph(&mut candidate, edge_id, &patch)?;
        validation::validate_graph(&self.app_data_root, &candidate)?;

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
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
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
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        store.ensure_loaded()?;
        Ok(store.get_layout().cloned().unwrap_or_default())
    }

    pub fn save_starmap_layout(
        &self,
        starmap_id: &str,
        layout: &crate::starmap::types::StarMapLayout,
    ) -> Result<Vec<std::path::PathBuf>> {
        validation::validate_layout(layout)?;

        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        store.ensure_loaded()?;
        store.set_layout(layout.clone());
        store.enqueue_save(SaveQueueEntry::Layout);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        store.flush_save_queue()
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
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        store.ensure_loaded()?;
        Ok(store.get_viewport().cloned().unwrap_or_default())
    }

    pub fn save_starmap_viewport(
        &self,
        starmap_id: &str,
        viewport: &crate::starmap::types::StarMapViewport,
    ) -> Result<Vec<std::path::PathBuf>> {
        validation::validate_viewport(viewport)?;

        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        store.ensure_loaded()?;
        store.set_viewport(viewport.clone());
        store.flush_viewport()
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
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        store.ensure_fully_loaded()?;

        let mut candidate = store.to_starmap_graph();
        candidate.embeds.push(embed.clone());
        validation::validate_graph(&self.app_data_root, &candidate)?;

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
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        store.ensure_fully_loaded()?;
        store.ensure_embed_loaded(instance_id)?;

        let mut candidate = store.to_starmap_graph();
        apply_embed_patch_to_graph(&mut candidate, instance_id, &patch)?;
        validation::validate_graph(&self.app_data_root, &candidate)?;

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
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
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
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        store.ensure_fully_loaded()?;

        let mut candidate = store.to_starmap_graph();
        candidate.links.push(link.clone());
        validation::validate_graph(&self.app_data_root, &candidate)?;

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
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        store.ensure_fully_loaded()?;
        store.ensure_link_loaded(link_id)?;

        let mut candidate = store.to_starmap_graph();
        apply_link_patch_to_graph(&mut candidate, link_id, &patch)?;
        validation::validate_graph(&self.app_data_root, &candidate)?;

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
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
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
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
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
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        store.ensure_fully_loaded()?;

        let mut candidate = store.to_starmap_graph();
        candidate.hyperlinks.push(hl.clone());
        validation::validate_graph(&self.app_data_root, &candidate)?;

        let result = store.add_hyperlink(hl)?;
        store.enqueue_save(SaveQueueEntry::Hyperlink);
        store.enqueue_save(SaveQueueEntry::GraphMeta);
        Ok(result)
    }

    pub fn update_starmap_hyperlink(
        &self,
        starmap_id: &str,
        hyperlink_id: &str,
        patch: &crate::starmap::types::StarMapHyperlinkPatch,
    ) -> Result<crate::starmap::types::StarMapHyperlink> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        store.ensure_fully_loaded()?;
        store.ensure_hyperlink_loaded(hyperlink_id)?;

        // hyperlink update 统一走 validate 以保持引用完整性（source 路径可能变）。
        let mut candidate = store.to_starmap_graph();
        apply_hyperlink_update_to_graph(&mut candidate, hyperlink_id, patch)?;
        validation::validate_graph(&self.app_data_root, &candidate)?;

        let result = store.update_hyperlink(hyperlink_id, patch)?;
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
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
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
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
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
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        store.get_phased_snapshot(request)
    }

    /// 确认星图删除 tombstone 已被同步方持久化，清理 `deleted_at_revision <=
    /// acknowledged_revision` 的 tombstone。清理结果在下次 flush 时写回磁盘。
    pub fn ack_starmap_deletions(
        &self,
        starmap_id: &str,
        acknowledged_revision: u64,
    ) -> Result<()> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let store = stores
            .entry(starmap_id.to_string())
            .or_insert_with(|| StarMapStore::new(&self.app_data_root, starmap_id));
        store.acknowledge_deletions(acknowledged_revision)
    }

    pub fn find_starmap_references(
        &self,
        target_starmap_id: &str,
    ) -> Result<Vec<crate::starmap::StarMapReference>> {
        crate::starmap::find_starmap_references(&self.app_data_root, target_starmap_id)
    }

    pub fn get_motion_policy(&self) -> Result<crate::starmap::types::StarMapMotionPolicyDto> {
        crate::starmap::get_motion_policy(&self.app_data_root)
    }

    pub fn close_starmap_store(&self, starmap_id: &str) -> Result<Vec<std::path::PathBuf>> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(store) = stores.get_mut(starmap_id) {
            if store.is_dirty() || store.has_pending_deletes() || store.save_queue_len() > 0 {
                return store.flush();
            }
        }
        stores.remove(starmap_id);
        Ok(Vec::new())
    }

    pub fn flush_starmap_store(&self, starmap_id: &str) -> Result<Vec<std::path::PathBuf>> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(store) = stores.get_mut(starmap_id) {
            if store.is_dirty() || store.has_pending_deletes() || store.save_queue_len() > 0 {
                return store.flush_save_queue();
            }
        }
        Ok(Vec::new())
    }

    pub fn flush_all_starmap_stores(&self) -> Result<Vec<std::path::PathBuf>> {
        let mut stores = self
            .starmap_stores
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut all_changed: Vec<std::path::PathBuf> = Vec::new();
        for store in stores.values_mut() {
            if store.is_dirty() || store.has_pending_deletes() || store.save_queue_len() > 0 {
                let paths = store.flush_save_queue()?;
                all_changed.extend(paths);
            }
        }
        Ok(all_changed)
    }
}

// ---------------------------------------------------------------------------
// Candidate graph patch 应用辅助函数
//
// 这些函数在 candidate `StarMapGraph` 上模拟 store 的 update 操作，
// 用于在真正修改 Store 前跑 `validate_graph`。它们必须与 store CRUD 的
// 字段更新语义保持一致（见 store/crud/*.rs）。
// ---------------------------------------------------------------------------

fn apply_node_patch_to_graph(
    graph: &mut crate::starmap::types::StarMapGraph,
    node_id: &str,
    patch: &crate::starmap::types::StarMapNodePatch,
) -> Result<()> {
    let node = graph
        .nodes
        .iter_mut()
        .find(|n| n.id == node_id)
        .ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Node not found",
            ))
        })?;
    if let Some(ref t) = patch.title {
        node.title = t.clone();
    }
    if let Some(ref k) = patch.kind {
        node.kind = k.clone();
    }
    if let Some(ref p) = patch.payload {
        node.payload = p.clone();
    }
    if let Some(ref t) = patch.tags {
        node.tags = t.clone();
    }
    if let Some(ref c) = patch.content {
        node.content = c.clone();
    }
    if let Some(ref a) = patch.anchors {
        node.anchors = a.clone();
    }
    if let Some(ref p) = patch.portal {
        node.portal = p.clone();
    }
    if let Some(ref dp) = patch.display_policy {
        node.display_policy = dp.clone();
    }
    if let Some(ref ob) = patch.open_behavior {
        node.open_behavior = ob.clone();
    }
    if let Some(ref p) = patch.provenance {
        node.provenance = p.clone();
    }
    Ok(())
}

fn apply_edge_patch_to_graph(
    graph: &mut crate::starmap::types::StarMapGraph,
    edge_id: &str,
    patch: &crate::starmap::types::StarMapEdgePatch,
) -> Result<()> {
    let edge = graph
        .edges
        .iter_mut()
        .find(|e| e.id == edge_id)
        .ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Edge not found",
            ))
        })?;
    if let Some(ref k) = patch.kind {
        edge.kind = k.clone();
    }
    if let Some(ref l) = patch.label {
        edge.label = l.clone();
    }
    if let Some(ref p) = patch.payload {
        edge.payload = p.clone();
    }
    if let Some(ref f) = patch.from {
        edge.from = f.clone();
    }
    if let Some(ref t) = patch.to {
        edge.to = t.clone();
    }
    Ok(())
}

fn apply_embed_patch_to_graph(
    graph: &mut crate::starmap::types::StarMapGraph,
    instance_id: &str,
    patch: &crate::starmap::types::StarMapEmbedPatch,
) -> Result<()> {
    let embed = graph
        .embeds
        .iter_mut()
        .find(|e| e.instance_id == instance_id)
        .ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Embed not found",
            ))
        })?;
    if let Some(ref l) = patch.label {
        embed.label = l.clone();
    }
    if let Some(ref dp) = patch.display_policy {
        embed.display_policy = dp.clone();
    }
    if let Some(ref ob) = patch.open_behavior {
        embed.open_behavior = ob.clone();
    }
    if let Some(Some(ref pl)) = patch.placement {
        embed.placement = pl.clone();
    }
    if let Some(Some(ref vp)) = patch.target_viewport {
        embed.target_viewport = vp.clone();
    }
    if let Some(ref hp) = patch.host_path {
        embed.host_path = hp.clone();
    }
    Ok(())
}

fn apply_link_patch_to_graph(
    graph: &mut crate::starmap::types::StarMapGraph,
    link_id: &str,
    patch: &crate::starmap::types::StarMapLinkPatch,
) -> Result<()> {
    let link = graph
        .links
        .iter_mut()
        .find(|l| l.link_id == link_id)
        .ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Link not found",
            ))
        })?;
    if let Some(ref s) = patch.source {
        link.source = s.clone();
    }
    if let Some(ref t) = patch.target {
        link.target = t.clone();
    }
    if let Some(ref l) = patch.label {
        link.label = l.clone();
    }
    Ok(())
}

fn apply_hyperlink_update_to_graph(
    graph: &mut crate::starmap::types::StarMapGraph,
    hyperlink_id: &str,
    patch: &crate::starmap::types::StarMapHyperlinkPatch,
) -> Result<()> {
    let hl = graph
        .hyperlinks
        .iter_mut()
        .find(|h| h.hyperlink_id == hyperlink_id)
        .ok_or_else(|| {
            crate::error::Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Hyperlink not found",
            ))
        })?;
    if let Some(ref l) = patch.label {
        hl.label = l.clone();
    }
    if let Some(ref u) = patch.target_uri {
        hl.target_uri = u.clone();
    }
    if let Some(ref s) = patch.source {
        hl.source = s.clone();
    }
    Ok(())
}
