use crate::error::Result;

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
        crate::starmap::graph::get_starmap_graph(&self.workspace_path, starmap_id)
    }

    pub fn save_starmap_graph(
        &self,
        starmap_id: &str,
        graph: &crate::starmap::types::StarMapGraph,
    ) -> Result<()> {
        crate::starmap::graph::save_starmap_graph(&self.workspace_path, starmap_id, graph)
    }

    pub fn add_starmap_node(
        &self,
        starmap_id: &str,
        node: crate::starmap::types::StarMapNode,
        default_x: f32,
        default_y: f32,
    ) -> Result<crate::starmap::types::StarMapNode> {
        crate::starmap::graph::add_starmap_node(
            &self.workspace_path,
            starmap_id,
            node,
            default_x,
            default_y,
        )
    }

    pub fn update_starmap_node(
        &self,
        starmap_id: &str,
        node_id: &str,
        patch: crate::starmap::types::StarMapNodePatch,
    ) -> Result<crate::starmap::types::StarMapNode> {
        crate::starmap::graph::update_starmap_node(&self.workspace_path, starmap_id, node_id, patch)
    }

    pub fn delete_starmap_node(&self, starmap_id: &str, node_id: &str) -> Result<()> {
        crate::starmap::graph::delete_starmap_node(&self.workspace_path, starmap_id, node_id)
    }

    pub fn add_starmap_edge(
        &self,
        starmap_id: &str,
        edge: crate::starmap::types::StarMapEdge,
    ) -> Result<crate::starmap::types::StarMapEdge> {
        crate::starmap::graph::add_starmap_edge(&self.workspace_path, starmap_id, edge)
    }

    pub fn update_starmap_edge(
        &self,
        starmap_id: &str,
        edge_id: &str,
        patch: crate::starmap::types::StarMapEdgePatch,
    ) -> Result<crate::starmap::types::StarMapEdge> {
        crate::starmap::graph::update_starmap_edge(&self.workspace_path, starmap_id, edge_id, patch)
    }

    pub fn delete_starmap_edge(&self, starmap_id: &str, edge_id: &str) -> Result<()> {
        crate::starmap::graph::delete_starmap_edge(&self.workspace_path, starmap_id, edge_id)
    }

    pub fn get_starmap_layout(
        &self,
        starmap_id: &str,
    ) -> Result<crate::starmap::types::StarMapLayout> {
        crate::starmap::graph::get_starmap_layout(&self.workspace_path, starmap_id)
    }

    pub fn save_starmap_layout(
        &self,
        starmap_id: &str,
        layout: &crate::starmap::types::StarMapLayout,
    ) -> Result<()> {
        crate::starmap::graph::save_starmap_layout(&self.workspace_path, starmap_id, layout)
    }

    pub fn get_starmap_viewport(
        &self,
        starmap_id: &str,
    ) -> Result<crate::starmap::types::StarMapViewport> {
        crate::starmap::graph::get_starmap_viewport(&self.workspace_path, starmap_id)
    }

    pub fn save_starmap_viewport(
        &self,
        starmap_id: &str,
        viewport: &crate::starmap::types::StarMapViewport,
    ) -> Result<()> {
        crate::starmap::graph::save_starmap_viewport(&self.workspace_path, starmap_id, viewport)
    }

    pub fn add_starmap_embed(
        &self,
        starmap_id: &str,
        embed: crate::starmap::types::StarMapEmbed,
    ) -> Result<crate::starmap::types::StarMapEmbed> {
        crate::starmap::graph::add_starmap_embed(&self.workspace_path, starmap_id, embed)
    }

    pub fn update_starmap_embed(
        &self,
        starmap_id: &str,
        instance_id: &str,
        patch: crate::starmap::types::StarMapEmbedPatch,
    ) -> Result<crate::starmap::types::StarMapEmbed> {
        crate::starmap::graph::update_starmap_embed(
            &self.workspace_path,
            starmap_id,
            instance_id,
            patch,
        )
    }

    pub fn delete_starmap_embed(&self, starmap_id: &str, instance_id: &str) -> Result<()> {
        crate::starmap::graph::delete_starmap_embed(&self.workspace_path, starmap_id, instance_id)
    }

    pub fn add_starmap_link(
        &self,
        starmap_id: &str,
        link: crate::starmap::types::StarMapLink,
    ) -> Result<crate::starmap::types::StarMapLink> {
        crate::starmap::graph::add_starmap_link(&self.workspace_path, starmap_id, link)
    }

    pub fn update_starmap_link(
        &self,
        starmap_id: &str,
        link_id: &str,
        patch: crate::starmap::types::StarMapLinkPatch,
    ) -> Result<crate::starmap::types::StarMapLink> {
        crate::starmap::graph::update_starmap_link(&self.workspace_path, starmap_id, link_id, patch)
    }

    pub fn delete_starmap_link(&self, starmap_id: &str, link_id: &str) -> Result<()> {
        crate::starmap::graph::delete_starmap_link(&self.workspace_path, starmap_id, link_id)
    }

    pub fn find_starmap_references(
        &self,
        target_starmap_id: &str,
    ) -> Result<Vec<crate::starmap::StarMapReference>> {
        crate::starmap::find_starmap_references(&self.workspace_path, target_starmap_id)
    }
}