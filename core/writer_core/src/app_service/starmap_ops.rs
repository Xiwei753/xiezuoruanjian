use crate::api::{
    CreateStarMapChildEmbedResultDto, StarMapConnectionInfoDto, StarMapEdgeDto,
    StarMapEdgePatchInputDto, StarMapEmbedDto, StarMapEmbedPatchInputDto,
    StarMapEndpointSelectionDto, StarMapGraphDto, StarMapHyperlinkDto,
    StarMapHyperlinkPatchInputDto, StarMapLinkDto, StarMapLinkPatchInputDto, StarMapMetaDto,
    StarMapNavigationLocationDto, StarMapNavigationRouteDto, StarMapNodeDto,
    StarMapNodePatchInputDto, StarMapPathSegmentDto, StarMapPhasedSnapshotDto, StarMapPointDto,
    StarMapReferenceDto, StarMapResolvedPathDto, StarMapTargetDetailDto, StarMapTargetPathDto,
    WriterError,
};

impl super::WriterAppService {
    pub fn list_starmaps(&self) -> Result<Vec<StarMapMetaDto>, WriterError> {
        self.api.list_starmaps()
    }

    pub fn list_root_starmaps(&self) -> Result<Vec<StarMapMetaDto>, WriterError> {
        self.api.list_root_starmaps()
    }

    pub fn create_starmap(
        &self,
        title: String,
        desc: String,
    ) -> Result<StarMapMetaDto, WriterError> {
        self.api.create_starmap(&title, &desc, None)
    }

    pub fn get_starmap_graph(&self, starmap_id: String) -> Result<StarMapGraphDto, WriterError> {
        self.api.get_starmap_graph(&starmap_id)
    }

    /// 按 `root starmap + 路径段` 解析当前层星图，转发到 Core API。
    ///
    /// 层级身份由 root + segments 逐段穿越决定，解析失败返回 `Err`，
    /// 调用方（FFI / 平台端）不得回退到点击事件传来的裸目标 ID。
    pub fn resolve_starmap_path(
        &self,
        root_starmap_id: &str,
        segments: Vec<StarMapPathSegmentDto>,
    ) -> Result<StarMapResolvedPathDto, WriterError> {
        self.api.resolve_starmap_path(root_starmap_id, segments)
    }

    pub fn add_starmap_node(
        &self,
        starmap_id: String,
        node: StarMapNodeDto,
        x: f64,
        y: f64,
    ) -> Result<StarMapNodeDto, WriterError> {
        self.api.add_starmap_node(&starmap_id, node, x, y)
    }

    pub fn update_starmap_node(
        &self,
        starmap_id: String,
        node_id: String,
        patch: StarMapNodePatchInputDto,
    ) -> Result<StarMapNodeDto, WriterError> {
        self.api
            .update_starmap_node(&starmap_id, &node_id, patch.into())
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
        self.api
            .update_starmap_edge(&starmap_id, &edge_id, patch.into())
    }

    pub fn delete_starmap_edge(
        &self,
        starmap_id: String,
        edge_id: String,
    ) -> Result<bool, WriterError> {
        self.api.delete_starmap_edge(&starmap_id, &edge_id)
    }

    pub fn add_starmap_embed(
        &self,
        starmap_id: String,
        embed: StarMapEmbedDto,
    ) -> Result<StarMapEmbedDto, WriterError> {
        self.api.add_starmap_embed(&starmap_id, embed)
    }

    pub fn create_starmap_child_embed(
        &self,
        host_starmap_id: String,
        title: String,
        position: StarMapPointDto,
    ) -> Result<CreateStarMapChildEmbedResultDto, WriterError> {
        self.api
            .create_starmap_child_embed(&host_starmap_id, &title, position)
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

    /// Fix 4: 确认星图删除 tombstone 已被同步方持久化。
    pub fn ack_starmap_deletions(
        &self,
        starmap_id: String,
        acknowledged_revision: u64,
    ) -> Result<(), WriterError> {
        self.api
            .ack_starmap_deletions(&starmap_id, acknowledged_revision)
    }

    pub fn flush_starmap_store(&self, starmap_id: String) -> Result<bool, WriterError> {
        self.api.flush_starmap_store(&starmap_id)
    }

    pub fn close_starmap_store(&self, starmap_id: String) -> Result<bool, WriterError> {
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
        self.api
            .update_starmap_hyperlink(&starmap_id, &hyperlink_id, patch.into())
    }

    pub fn delete_starmap_hyperlink(
        &self,
        starmap_id: String,
        hyperlink_id: String,
    ) -> Result<bool, WriterError> {
        self.api
            .delete_starmap_hyperlink(&starmap_id, &hyperlink_id)
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
        request: crate::api::types::PhasedSnapshotRequestDto,
    ) -> Result<StarMapPhasedSnapshotDto, WriterError> {
        self.api.get_starmap_phased_snapshot(&starmap_id, &request)
    }

    /// 解析星图导航路由，返回当前画布位置。转发到 Core API。
    pub fn resolve_starmap_navigation(
        &self,
        root_starmap_id: String,
        segments: Vec<StarMapPathSegmentDto>,
    ) -> Result<StarMapNavigationLocationDto, WriterError> {
        self.api
            .resolve_starmap_navigation(root_starmap_id, segments)
    }

    /// 通过 Embed 实例进入子画布。转发到 Core API。
    pub fn enter_starmap_embed(
        &self,
        root_starmap_id: String,
        segments: Vec<StarMapPathSegmentDto>,
        instance_id: String,
    ) -> Result<crate::api::types::EnterStarmapResultDto, WriterError> {
        self.api
            .enter_starmap_embed(root_starmap_id, segments, instance_id)
    }

    /// 通过 Portal 节点进入子画布。转发到 Core API。
    pub fn enter_starmap_portal(
        &self,
        root_starmap_id: String,
        segments: Vec<StarMapPathSegmentDto>,
        node_id: String,
    ) -> Result<crate::api::types::EnterStarmapResultDto, WriterError> {
        self.api
            .enter_starmap_portal(root_starmap_id, segments, node_id)
    }

    /// 退出当前画布，返回上一层导航路由。转发到 Core API。
    pub fn exit_starmap_canvas(
        &self,
        route: StarMapNavigationRouteDto,
    ) -> Result<StarMapNavigationRouteDto, WriterError> {
        self.api.exit_starmap_canvas(route)
    }

    /// 跳到指定祖先层级，返回截断后的导航路由。转发到 Core API。
    pub fn jump_starmap_ancestor(
        &self,
        route: StarMapNavigationRouteDto,
        depth: u32,
    ) -> Result<StarMapNavigationRouteDto, WriterError> {
        self.api.jump_starmap_ancestor(route, depth)
    }

    /// 创建跨画布边并真正写入宿主图 Store（事务操作）。转发到 Core API。
    ///
    /// 返回 `CreateCrossCanvasEdgeResultDto`（`ownerStarmapId` + 完整 `edge`），
    /// 供平台端后续修改/删除原始边。
    pub fn create_cross_canvas_edge(
        &self,
        from: StarMapEndpointSelectionDto,
        to: StarMapEndpointSelectionDto,
        kind: crate::api::types::StarMapEdgeKindDto,
        label: Option<String>,
        payload: Option<String>,
    ) -> Result<crate::api::types::CreateCrossCanvasEdgeResultDto, WriterError> {
        self.api
            .create_cross_canvas_edge(from, to, kind, label, payload)
    }

    /// 列出与特定画布或目标相关的所有连接。转发到 Core API。
    pub fn list_starmap_connections(
        &self,
        route: StarMapNavigationRouteDto,
        target: Option<StarMapTargetDetailDto>,
    ) -> Result<Vec<StarMapConnectionInfoDto>, WriterError> {
        self.api.list_starmap_connections(route, target)
    }

    /// 规划跨层连线应当落在哪张图上。转发到 Core API。
    ///
    /// 输入是两条从同一根星图出发的绝对端点路径，返回宿主星图、宿主 Scene
    /// 路径和两条相对宿主的端点路径；自环/不可解析/不同根都返回 `Err`。
    pub fn plan_cross_layer_connection(
        &self,
        from: StarMapTargetPathDto,
        to: StarMapTargetPathDto,
    ) -> Result<crate::api::types::CrossLayerConnectionPlanDto, WriterError> {
        self.api.plan_cross_layer_connection(from, to)
    }
}
