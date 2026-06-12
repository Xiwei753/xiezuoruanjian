// =============================================================================
// mind_map_operations.rs — LEGACY MindMap stubs (removed at runtime)
// =============================================================================
//
// All MindMap CRUD operations have been removed from the runtime path.
// This file retains only error-returning stubs so that the struct fields
// in starmap_backend.rs still compile. Callers will receive
// "MindMap API removed; migrate to StarMap".

use super::*;
use writer_core::api::{WriterCoreApi, WriterError};

fn envelope_removed() -> QString {
    WriterCoreApi::envelope_json::<serde_json::Value>(Err(WriterError::Other(
        "MindMap API removed; migrate to StarMap".into(),
    )))
    .into()
}

impl AppBackend {
    pub(crate) fn get_mind_map_snapshot_json(&self, _project_id: QString) -> QString {
        envelope_removed()
    }

    pub(crate) fn create_mind_map_graph_json(
        &mut self,
        _project_id: QString,
        _title: QString,
    ) -> QString {
        envelope_removed()
    }

    pub(crate) fn list_mind_map_graphs_json(&self, _project_id: QString) -> QString {
        envelope_removed()
    }

    pub(crate) fn set_default_mind_map_graph_json(
        &mut self,
        _project_id: QString,
        _graph_id: QString,
    ) -> QString {
        envelope_removed()
    }

    pub(crate) fn create_mind_map_node_json(
        &mut self,
        _project_id: QString,
        _graph_id: QString,
        _node_json: QString,
    ) -> QString {
        envelope_removed()
    }

    pub(crate) fn update_mind_map_node_json(
        &mut self,
        _project_id: QString,
        _graph_id: QString,
        _node_id: QString,
        _patch_json: QString,
    ) -> QString {
        envelope_removed()
    }

    pub(crate) fn delete_mind_map_node_json(
        &mut self,
        _project_id: QString,
        _graph_id: QString,
        _node_id: QString,
        _cascade: bool,
    ) -> QString {
        envelope_removed()
    }

    pub(crate) fn create_mind_map_edge_json(
        &mut self,
        _project_id: QString,
        _graph_id: QString,
        _edge_json: QString,
    ) -> QString {
        envelope_removed()
    }

    pub(crate) fn update_mind_map_edge_json(
        &mut self,
        _project_id: QString,
        _graph_id: QString,
        _edge_id: QString,
        _patch_json: QString,
    ) -> QString {
        envelope_removed()
    }

    pub(crate) fn delete_mind_map_edge_json(
        &mut self,
        _project_id: QString,
        _graph_id: QString,
        _edge_id: QString,
    ) -> QString {
        envelope_removed()
    }

    pub(crate) fn create_mind_map_anchor_json(
        &mut self,
        _project_id: QString,
        _graph_id: QString,
        _anchor_json: QString,
    ) -> QString {
        envelope_removed()
    }

    pub(crate) fn bind_mind_map_anchor_json(
        &mut self,
        _project_id: QString,
        _graph_id: QString,
        _node_id: QString,
        _anchor_id: QString,
        _link_kind: QString,
    ) -> QString {
        envelope_removed()
    }

    pub(crate) fn save_mind_map_layout_json(
        &mut self,
        _project_id: QString,
        _graph_id: QString,
        _layout_json: QString,
    ) -> QString {
        envelope_removed()
    }
}
