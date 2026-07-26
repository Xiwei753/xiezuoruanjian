use crate::error::Result;
use crate::starmap::store::StarMapStore;
use crate::starmap::types::*;
use crate::starmap::graph::validation;

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

    pub fn delete_starmap(&self, starmap_id: &str) -> Result<()> {
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
        let mut store = StarMapStore::new(&self.workspace_path, starmap_id);
        store.load_full()?;
        Ok(store.to_starmap_graph())
    }

    pub fn save_starmap_graph(
        &self,
        starmap_id: &str,
        graph: &crate::starmap::types::StarMapGraph,
    ) -> Result<()> {
        validation::validate_graph(&self.workspace_path, graph)?;

        let mut store = StarMapStore::new(&self.workspace_path, starmap_id);
        let _ = store.load_full();

        let old_node_ids: std::collections::HashSet<String> = store.all_nodes().map(|n| n.id.clone()).collect();
        let old_edge_ids: std::collections::HashSet<String> = store.all_edges().map(|e| e.id.clone()).collect();
        let old_embed_ids: std::collections::HashSet<String> = store.all_embeds().map(|e| e.instance_id.clone()).collect();
        let old_link_ids: std::collections::HashSet<String> = store.all_links().map(|l| l.link_id.clone()).collect();

        let new_node_ids: std::collections::HashSet<String> = graph.nodes.iter().map(|n| n.id.clone()).collect();
        let new_edge_ids: std::collections::HashSet<String> = graph.edges.iter().map(|e| e.id.clone()).collect();
        let new_embed_ids: std::collections::HashSet<String> = graph.embeds.iter().map(|e| e.instance_id.clone()).collect();
        let new_link_ids: std::collections::HashSet<String> = graph.links.iter().map(|l| l.link_id.clone()).collect();

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

        for old_id in &old_node_ids {
            if !new_node_ids.contains(old_id) {
                store.remove_node(old_id);
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

        store.flush()?;
        Ok(())
    }

    pub fn add_starmap_node(
        &self,
        starmap_id: &str,
        node: crate::starmap::types::StarMapNode,
        default_x: f32,
        default_y: f32,
    ) -> Result<crate::starmap::types::StarMapNode> {
        let mut store = StarMapStore::new(&self.workspace_path, starmap_id);
        store.load_full()?;
        let result = store.add_node(node, default_x, default_y);
        store.flush()?;
        Ok(result)
    }

    pub fn update_starmap_node(
        &self,
        starmap_id: &str,
        node_id: &str,
        patch: crate::starmap::types::StarMapNodePatch,
    ) -> Result<crate::starmap::types::StarMapNode> {
        let mut store = StarMapStore::new(&self.workspace_path, starmap_id);
        store.load_full()?;
        let result = store.update_node(node_id, &patch)?;
        store.flush()?;
        Ok(result)
    }

    pub fn delete_starmap_node(&self, starmap_id: &str, node_id: &str) -> Result<()> {
        let mut store = StarMapStore::new(&self.workspace_path, starmap_id);
        store.load_full()?;
        store.delete_node(node_id)?;
        store.flush()?;
        Ok(())
    }

    pub fn add_starmap_edge(
        &self,
        starmap_id: &str,
        edge: crate::starmap::types::StarMapEdge,
    ) -> Result<crate::starmap::types::StarMapEdge> {
        let mut store = StarMapStore::new(&self.workspace_path, starmap_id);
        store.load_full()?;
        let result = store.add_edge(edge)?;
        store.flush()?;
        Ok(result)
    }

    pub fn update_starmap_edge(
        &self,
        starmap_id: &str,
        edge_id: &str,
        patch: crate::starmap::types::StarMapEdgePatch,
    ) -> Result<crate::starmap::types::StarMapEdge> {
        let mut store = StarMapStore::new(&self.workspace_path, starmap_id);
        store.load_full()?;
        let result = store.update_edge(edge_id, &patch)?;
        store.flush()?;
        Ok(result)
    }

    pub fn delete_starmap_edge(&self, starmap_id: &str, edge_id: &str) -> Result<()> {
        let mut store = StarMapStore::new(&self.workspace_path, starmap_id);
        store.load_full()?;
        store.delete_edge(edge_id)?;
        store.flush()?;
        Ok(())
    }

    pub fn get_starmap_layout(
        &self,
        starmap_id: &str,
    ) -> Result<crate::starmap::types::StarMapLayout> {
        let mut store = StarMapStore::new(&self.workspace_path, starmap_id);
        store.load_full()?;
        Ok(store.get_layout().cloned().unwrap_or_default())
    }

    pub fn save_starmap_layout(
        &self,
        starmap_id: &str,
        layout: &crate::starmap::types::StarMapLayout,
    ) -> Result<()> {
        validation::validate_layout(layout)?;

        let mut store = StarMapStore::new(&self.workspace_path, starmap_id);
        store.load_full()?;
        store.set_layout(layout.clone());
        store.flush()?;
        Ok(())
    }

    pub fn get_starmap_viewport(
        &self,
        starmap_id: &str,
    ) -> Result<crate::starmap::types::StarMapViewport> {
        let mut store = StarMapStore::new(&self.workspace_path, starmap_id);
        store.load_full()?;
        Ok(store.get_viewport().cloned().unwrap_or_default())
    }

    pub fn save_starmap_viewport(
        &self,
        starmap_id: &str,
        viewport: &crate::starmap::types::StarMapViewport,
    ) -> Result<()> {
        validation::validate_viewport(viewport)?;

        let mut store = StarMapStore::new(&self.workspace_path, starmap_id);
        store.load_full()?;
        store.set_viewport(viewport.clone());
        store.flush_viewport()?;
        Ok(())
    }

    pub fn add_starmap_embed(
        &self,
        starmap_id: &str,
        embed: crate::starmap::types::StarMapEmbed,
    ) -> Result<crate::starmap::types::StarMapEmbed> {
        let mut store = StarMapStore::new(&self.workspace_path, starmap_id);
        store.load_full()?;
        let result = store.add_embed(embed)?;
        store.flush()?;
        Ok(result)
    }

    pub fn update_starmap_embed(
        &self,
        starmap_id: &str,
        instance_id: &str,
        patch: crate::starmap::types::StarMapEmbedPatch,
    ) -> Result<crate::starmap::types::StarMapEmbed> {
        let mut store = StarMapStore::new(&self.workspace_path, starmap_id);
        store.load_full()?;
        let result = store.update_embed(instance_id, &patch)?;
        store.flush()?;
        Ok(result)
    }

    pub fn delete_starmap_embed(&self, starmap_id: &str, instance_id: &str) -> Result<()> {
        let mut store = StarMapStore::new(&self.workspace_path, starmap_id);
        store.load_full()?;
        store.delete_embed(instance_id)?;
        store.flush()?;
        Ok(())
    }

    pub fn add_starmap_link(
        &self,
        starmap_id: &str,
        link: crate::starmap::types::StarMapLink,
    ) -> Result<crate::starmap::types::StarMapLink> {
        let mut store = StarMapStore::new(&self.workspace_path, starmap_id);
        store.load_full()?;
        let result = store.add_link(link)?;
        store.flush()?;
        Ok(result)
    }

    pub fn update_starmap_link(
        &self,
        starmap_id: &str,
        link_id: &str,
        patch: crate::starmap::types::StarMapLinkPatch,
    ) -> Result<crate::starmap::types::StarMapLink> {
        let mut store = StarMapStore::new(&self.workspace_path, starmap_id);
        store.load_full()?;
        let result = store.update_link(link_id, &patch)?;
        store.flush()?;
        Ok(result)
    }

    pub fn delete_starmap_link(&self, starmap_id: &str, link_id: &str) -> Result<()> {
        let mut store = StarMapStore::new(&self.workspace_path, starmap_id);
        store.load_full()?;
        store.delete_link(link_id)?;
        store.flush()?;
        Ok(())
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
}
