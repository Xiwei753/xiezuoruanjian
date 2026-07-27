use crate::api::{
    StarMapEdgeDto, StarMapEdgePatchInputDto, StarMapEdgeRenderDto, StarMapEmbedDto,
    StarMapEmbedPatchInputDto, StarMapGraphDto, StarMapHyperlinkDto,
    StarMapHyperlinkPatchInputDto, StarMapLayoutDto, StarMapLinkDto,
    StarMapLinkPatchInputDto, StarMapMetaDto, StarMapMotionPolicyDto, StarMapNodeDto,
    StarMapNodePatchInputDto, StarMapPhasedSnapshotDto, StarMapReferenceDto,
    StarMapViewportDto, WriterError,
};

impl super::WriterAppService {
    pub fn list_starmaps(&self) -> Result<Vec<StarMapMetaDto>, WriterError> {
        self.api.list_starmaps()
    }

    pub fn create_starmap(
        &self,
        title: String,
        desc: String,
    ) -> Result<StarMapMetaDto, WriterError> {
        self.api.create_starmap(&title, &desc, None)
    }

    pub fn get_starmap_graph(
        &self,
        starmap_id: String,
    ) -> Result<StarMapGraphDto, WriterError> {
        self.api.get_starmap_graph(&starmap_id)
    }

    pub fn add_starmap_node(
        &self,
        starmap_id: String,
        node: StarMapNodeDto,
        x: f32,
        y: f32,
    ) -> Result<StarMapNodeDto, WriterError> {
        self.api.add_starmap_node(&starmap_id, node, x, y)
    }

    pub fn update_starmap_node(
        &self,
        starmap_id: String,
        node_id: String,
        patch: StarMapNodePatchInputDto,
    ) -> Result<StarMapNodeDto, WriterError> {
        self.api.update_starmap_node(&starmap_id, &node_id, patch.into())
    }

    pub fn delete_starmap_node(
        &self,
        starmap_id: String,
        node_id: String,
    ) -> Result<bool, WriterError> {
        self.api.delete_starmap_node(&starmap_id, &node_id)
    }

    pub fn add_starmap_edge(
        &self,
        starmap_id: String,
        edge: StarMapEdgeDto,
    ) -> Result<StarMapEdgeDto, WriterError> {
        self.api.add_starmap_edge(&starmap_id, edge)
    }

    pub fn update_starmap_edge(
        &self,
        starmap_id: String,
        edge_id: String,
        patch: StarMapEdgePatchInputDto,
    ) -> Result<StarMapEdgeDto, WriterError> {
        self.api.update_starmap_edge(&starmap_id, &edge_id, patch.into())
    }

    pub fn delete_starmap_edge(
        &self,
        starmap_id: String,
        edge_id: String,
    ) -> Result<bool, WriterError> {
        self.api.delete_starmap_edge(&starmap_id, &edge_id)
    }

    pub fn save_starmap_graph(
        &self,
        starmap_id: String,
        graph: StarMapGraphDto,
    ) -> Result<bool, WriterError> {
        self.api.save_starmap_graph(&starmap_id, &graph)
    }

    pub fn save_starmap_layout(
        &self,
        starmap_id: String,
        layout: StarMapLayoutDto,
    ) -> Result<bool, WriterError> {
        self.api.save_starmap_layout(&starmap_id, &layout)
    }

    pub fn get_starmap_viewport(
        &self,
        starmap_id: String,
    ) -> Result<StarMapViewportDto, WriterError> {
        self.api.get_starmap_viewport(&starmap_id)
    }

    pub fn save_starmap_viewport(
        &self,
        starmap_id: String,
        viewport: StarMapViewportDto,
    ) -> Result<bool, WriterError> {
        self.api.save_starmap_viewport(&starmap_id, viewport)
    }

    pub fn compute_starmap_edge_renders(
        &self,
        graph: StarMapGraphDto,
        layout: StarMapLayoutDto,
    ) -> Result<Vec<StarMapEdgeRenderDto>, WriterError> {
        self.api.compute_starmap_edge_renders(graph, layout)
    }

    pub fn hit_test_starmap_node(
        &self,
        layout: StarMapLayoutDto,
        x: f32,
        y: f32,
    ) -> Result<Option<String>, WriterError> {
        self.api.hit_test_starmap_node(layout, x, y)
    }

    pub fn add_starmap_embed(
        &self,
        starmap_id: String,
        embed: StarMapEmbedDto,
    ) -> Result<StarMapEmbedDto, WriterError> {
        self.api.add_starmap_embed(&starmap_id, embed)
    }

    pub fn update_starmap_embed(
        &self,
        starmap_id: String,
        instance_id: String,
        patch: StarMapEmbedPatchInputDto,
    ) -> Result<StarMapEmbedDto, WriterError> {
        self.api
            .update_starmap_embed(&starmap_id, &instance_id, patch.into())
    }

    pub fn delete_starmap_embed(
        &self,
        starmap_id: String,
        instance_id: String,
    ) -> Result<bool, WriterError> {
        self.api.delete_starmap_embed(&starmap_id, &instance_id)
    }

    pub fn add_starmap_link(
        &self,
        starmap_id: String,
        link: StarMapLinkDto,
    ) -> Result<StarMapLinkDto, WriterError> {
        self.api.add_starmap_link(&starmap_id, link)
    }

    pub fn update_starmap_link(
        &self,
        starmap_id: String,
        link_id: String,
        patch: StarMapLinkPatchInputDto,
    ) -> Result<StarMapLinkDto, WriterError> {
        self.api
            .update_starmap_link(&starmap_id, &link_id, patch.into())
    }

    pub fn delete_starmap_link(
        &self,
        starmap_id: String,
        link_id: String,
    ) -> Result<bool, WriterError> {
        self.api.delete_starmap_link(&starmap_id, &link_id)
    }

    pub fn find_starmap_references(
        &self,
        target_starmap_id: String,
    ) -> Result<Vec<StarMapReferenceDto>, WriterError> {
        self.api.find_starmap_references(&target_starmap_id)
    }

    pub fn get_starmap_motion_policy(
        &self,
    ) -> Result<StarMapMotionPolicyDto, WriterError> {
        self.api.get_starmap_motion_policy()
    }

    pub fn flush_starmap_store(
        &self,
        starmap_id: String,
    ) -> Result<bool, WriterError> {
        self.api.flush_starmap_store(&starmap_id)
    }

    pub fn close_starmap_store(
        &self,
        starmap_id: String,
    ) -> Result<bool, WriterError> {
        self.api.close_starmap_store(&starmap_id)
    }

    pub fn flush_all_starmap_stores(&self) -> Result<bool, WriterError> {
        self.api.flush_all_starmap_stores()
    }

    pub fn list_starmap_links(
        &self,
        starmap_id: String,
    ) -> Result<crate::api::types::StarMapLinkListWithDiagnosticsDto, WriterError> {
        self.api.list_starmap_links(&starmap_id)
    }

    pub fn add_starmap_hyperlink(
        &self,
        starmap_id: String,
        hl: StarMapHyperlinkDto,
    ) -> Result<StarMapHyperlinkDto, WriterError> {
        self.api.add_starmap_hyperlink(&starmap_id, hl)
    }

    pub fn update_starmap_hyperlink(
        &self,
        starmap_id: String,
        hyperlink_id: String,
        patch: StarMapHyperlinkPatchInputDto,
    ) -> Result<StarMapHyperlinkDto, WriterError> {
        self.api.update_starmap_hyperlink(&starmap_id, &hyperlink_id, patch.into())
    }

    pub fn delete_starmap_hyperlink(
        &self,
        starmap_id: String,
        hyperlink_id: String,
    ) -> Result<bool, WriterError> {
        self.api.delete_starmap_hyperlink(&starmap_id, &hyperlink_id)
    }

    pub fn list_starmap_hyperlinks(
        &self,
        starmap_id: String,
    ) -> Result<crate::api::types::StarMapHyperlinkListWithDiagnosticsDto, WriterError> {
        self.api.list_starmap_hyperlinks(&starmap_id)
    }

    pub fn get_starmap_phased_snapshot(
        &self,
        starmap_id: String,
    ) -> Result<StarMapPhasedSnapshotDto, WriterError> {
        self.api.get_starmap_phased_snapshot(&starmap_id)
    }
}
