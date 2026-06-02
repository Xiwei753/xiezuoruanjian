// =============================================================================
// mind_map_operations.rs — 脑图操作（从 starmap_backend.rs 拆分）
// =============================================================================

use super::*;
use writer_core::api::{WriterCoreApi, WriterError};

fn envelope_not_initialized() -> QString {
    WriterCoreApi::envelope_json::<serde_json::Value>(Err(WriterError::InvalidWorkspace)).into()
}

fn envelope_err(e: WriterError) -> QString {
    WriterCoreApi::envelope_json::<serde_json::Value>(Err(e)).into()
}

impl AppBackend {
    pub(crate) fn get_mind_map_snapshot_json(&self, project_id: QString) -> QString {
        let pid = project_id.to_string();
        if let Some(api) = self.core_api() {
            WriterCoreApi::envelope_json(api.get_mind_map_snapshot(&pid)).into()
        } else {
            envelope_not_initialized()
        }
    }

    pub(crate) fn create_mind_map_graph_json(
        &mut self,
        project_id: QString,
        title: QString,
    ) -> QString {
        let pid = project_id.to_string();
        let t = title.to_string();
        if let Some(api) = self.core_api() {
            WriterCoreApi::envelope_json(api.create_mind_map_graph(&pid, &t)).into()
        } else {
            envelope_not_initialized()
        }
    }

    pub(crate) fn list_mind_map_graphs_json(&self, project_id: QString) -> QString {
        let pid = project_id.to_string();
        if let Some(api) = self.core_api() {
            WriterCoreApi::envelope_json(api.list_mind_map_graphs(&pid)).into()
        } else {
            envelope_not_initialized()
        }
    }

    pub(crate) fn set_default_mind_map_graph_json(
        &mut self,
        project_id: QString,
        graph_id: QString,
    ) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        if let Some(api) = self.core_api() {
            WriterCoreApi::envelope_json(api.set_default_mind_map_graph(&pid, &gid)).into()
        } else {
            envelope_not_initialized()
        }
    }

    pub(crate) fn create_mind_map_node_json(
        &mut self,
        project_id: QString,
        graph_id: QString,
        node_json: QString,
    ) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let nj = node_json.to_string();

        let node: writer_core::api::types::MindMapGraphNodeDto = match serde_json::from_str(&nj) {
            Ok(n) => n,
            Err(e) => return envelope_err(WriterError::Json(e.to_string())),
        };

        if let Some(api) = self.core_api() {
            WriterCoreApi::envelope_json(api.create_mind_map_node(&pid, &gid, node)).into()
        } else {
            envelope_not_initialized()
        }
    }

    pub(crate) fn update_mind_map_node_json(
        &mut self,
        project_id: QString,
        graph_id: QString,
        node_id: QString,
        patch_json: QString,
    ) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let nid = node_id.to_string();
        let pj = patch_json.to_string();

        #[derive(serde::Deserialize)]
        struct NodePatch {
            title: Option<String>,
            kind: Option<writer_core::api::types::MindMapNodeKindDto>,
            payload: Option<serde_json::Value>,
            tags: Option<Vec<String>>,
        }

        let patch: NodePatch = match serde_json::from_str(&pj) {
            Ok(p) => p,
            Err(e) => return envelope_err(WriterError::Json(e.to_string())),
        };

        if let Some(api) = self.core_api() {
            WriterCoreApi::envelope_json(
                api.update_mind_map_node(
                    &pid,
                    &gid,
                    &nid,
                    writer_core::api::types::MindMapNodePatchDto {
                        title: patch.title,
                        kind: patch.kind,
                        payload: patch
                            .payload
                            .map(|v| Some(serde_json::to_string(&v).unwrap_or_default())),
                        tags: patch.tags,
                    },
                ),
            )
            .into()
        } else {
            envelope_not_initialized()
        }
    }

    pub(crate) fn delete_mind_map_node_json(
        &mut self,
        project_id: QString,
        graph_id: QString,
        node_id: QString,
        cascade: bool,
    ) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let nid = node_id.to_string();

        if let Some(api) = self.core_api() {
            WriterCoreApi::envelope_json(api.delete_mind_map_node(&pid, &gid, &nid, cascade)).into()
        } else {
            envelope_not_initialized()
        }
    }

    pub(crate) fn create_mind_map_edge_json(
        &mut self,
        project_id: QString,
        graph_id: QString,
        edge_json: QString,
    ) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let ej = edge_json.to_string();

        let edge: writer_core::api::types::MindMapGraphEdgeDto = match serde_json::from_str(&ej) {
            Ok(e) => e,
            Err(e) => return envelope_err(WriterError::Json(e.to_string())),
        };

        if let Some(api) = self.core_api() {
            WriterCoreApi::envelope_json(api.create_mind_map_edge(&pid, &gid, edge)).into()
        } else {
            envelope_not_initialized()
        }
    }

    pub(crate) fn update_mind_map_edge_json(
        &mut self,
        project_id: QString,
        graph_id: QString,
        edge_id: QString,
        patch_json: QString,
    ) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let eid = edge_id.to_string();
        let pj = patch_json.to_string();

        #[derive(serde::Deserialize)]
        struct EdgePatch {
            kind: Option<writer_core::api::types::MindMapEdgeKindDto>,
            label: Option<String>,
            payload: Option<serde_json::Value>,
        }

        let patch: EdgePatch = match serde_json::from_str(&pj) {
            Ok(p) => p,
            Err(e) => return envelope_err(WriterError::Json(e.to_string())),
        };

        if let Some(api) = self.core_api() {
            WriterCoreApi::envelope_json(
                api.update_mind_map_edge(
                    &pid,
                    &gid,
                    &eid,
                    writer_core::api::types::MindMapEdgePatchDto {
                        kind: patch.kind,
                        label: patch
                            .label
                            .map(|v| Some(serde_json::to_string(&v).unwrap_or_default())),
                        payload: patch
                            .payload
                            .map(|v| Some(serde_json::to_string(&v).unwrap_or_default())),
                    },
                ),
            )
            .into()
        } else {
            envelope_not_initialized()
        }
    }

    pub(crate) fn delete_mind_map_edge_json(
        &mut self,
        project_id: QString,
        graph_id: QString,
        edge_id: QString,
    ) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let eid = edge_id.to_string();

        if let Some(api) = self.core_api() {
            WriterCoreApi::envelope_json(api.delete_mind_map_edge(&pid, &gid, &eid)).into()
        } else {
            envelope_not_initialized()
        }
    }

    pub(crate) fn create_mind_map_anchor_json(
        &mut self,
        project_id: QString,
        graph_id: QString,
        anchor_json: QString,
    ) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let aj = anchor_json.to_string();

        let anchor: writer_core::api::types::MindMapAnchorDto = match serde_json::from_str(&aj) {
            Ok(a) => a,
            Err(e) => return envelope_err(WriterError::Json(e.to_string())),
        };

        if let Some(api) = self.core_api() {
            WriterCoreApi::envelope_json(api.create_mind_map_anchor(&pid, &gid, anchor)).into()
        } else {
            envelope_not_initialized()
        }
    }

    pub(crate) fn bind_mind_map_anchor_json(
        &mut self,
        project_id: QString,
        graph_id: QString,
        node_id: QString,
        anchor_id: QString,
        link_kind: QString,
    ) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let nid = node_id.to_string();
        let aid = anchor_id.to_string();
        let lk = link_kind.to_string();

        if let Some(api) = self.core_api() {
            WriterCoreApi::envelope_json(
                api.bind_mind_map_node_to_anchor(&pid, &gid, &nid, &aid, &lk),
            )
            .into()
        } else {
            envelope_not_initialized()
        }
    }

    pub(crate) fn save_mind_map_layout_json(
        &mut self,
        project_id: QString,
        graph_id: QString,
        layout_json: QString,
    ) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let lj = layout_json.to_string();

        let layout: writer_core::api::types::MindMapLayoutDto = match serde_json::from_str(&lj) {
            Ok(l) => l,
            Err(e) => return envelope_err(WriterError::Json(e.to_string())),
        };

        if let Some(api) = self.core_api() {
            WriterCoreApi::envelope_json(api.save_mind_map_layout(&pid, &gid, layout)).into()
        } else {
            envelope_not_initialized()
        }
    }
}
