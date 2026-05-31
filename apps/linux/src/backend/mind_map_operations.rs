// =============================================================================
// mind_map_operations.rs — 脑图操作（从 starmap_backend.rs 拆分）
// =============================================================================

use super::*;

impl AppBackend {
    pub(crate) fn get_mind_map_snapshot_json(&self, project_id: QString) -> QString {
        let pid = project_id.to_string();
        if let Some(api) = self.core_api() {
            match api.get_mind_map_snapshot(&pid) {
                Ok(data) => serde_json::json!({ "success": true, "data": data, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }

    pub(crate) fn create_mind_map_graph_json(&mut self, project_id: QString, title: QString) -> QString {
        let pid = project_id.to_string();
        let t = title.to_string();
        if let Some(api) = self.core_api() {
            match api.create_mind_map_graph(&pid, &t) {
                Ok(data) => serde_json::json!({ "success": true, "data": data, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }

    pub(crate) fn list_mind_map_graphs_json(&self, project_id: QString) -> QString {
        let pid = project_id.to_string();
        if let Some(api) = self.core_api() {
            match api.list_mind_map_graphs(&pid) {
                Ok(data) => serde_json::json!({ "success": true, "data": data, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }

    pub(crate) fn set_default_mind_map_graph_json(&mut self, project_id: QString, graph_id: QString) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        if let Some(api) = self.core_api() {
            match api.set_default_mind_map_graph(&pid, &gid) {
                Ok(_) => serde_json::json!({ "success": true, "data": true, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }

    pub(crate) fn create_mind_map_node_json(&mut self, project_id: QString, graph_id: QString, node_json: QString) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let nj = node_json.to_string();

        let node: writer_core::api::types::MindMapGraphNodeDto = match serde_json::from_str(&nj) {
            Ok(n) => n,
            Err(e) => return serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("Invalid node JSON: {}", e) }).to_string().into(),
        };

        if let Some(api) = self.core_api() {
            match api.create_mind_map_node(&pid, &gid, node) {
                Ok(data) => serde_json::json!({ "success": true, "data": data, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }

    pub(crate) fn update_mind_map_node_json(&mut self, project_id: QString, graph_id: QString, node_id: QString, patch_json: QString) -> QString {
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
            Err(e) => return serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("Invalid patch JSON: {}", e) }).to_string().into(),
        };

        if let Some(api) = self.core_api() {
            match api.update_mind_map_node(&pid, &gid, &nid, writer_core::api::types::MindMapNodePatchDto { title: patch.title, kind: patch.kind, payload: patch.payload.map(Some), tags: patch.tags }) {
                Ok(data) => serde_json::json!({ "success": true, "data": data, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }

    pub(crate) fn delete_mind_map_node_json(&mut self, project_id: QString, graph_id: QString, node_id: QString, cascade: bool) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let nid = node_id.to_string();

        if let Some(api) = self.core_api() {
            match api.delete_mind_map_node(&pid, &gid, &nid, cascade) {
                Ok(_) => serde_json::json!({ "success": true, "data": true, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }

    pub(crate) fn create_mind_map_edge_json(&mut self, project_id: QString, graph_id: QString, edge_json: QString) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let ej = edge_json.to_string();

        let edge: writer_core::api::types::MindMapGraphEdgeDto = match serde_json::from_str(&ej) {
            Ok(e) => e,
            Err(e) => return serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("Invalid edge JSON: {}", e) }).to_string().into(),
        };

        if let Some(api) = self.core_api() {
            match api.create_mind_map_edge(&pid, &gid, edge) {
                Ok(data) => serde_json::json!({ "success": true, "data": data, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }

    pub(crate) fn update_mind_map_edge_json(&mut self, project_id: QString, graph_id: QString, edge_id: QString, patch_json: QString) -> QString {
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
            Err(e) => return serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("Invalid patch JSON: {}", e) }).to_string().into(),
        };

        if let Some(api) = self.core_api() {
            match api.update_mind_map_edge(&pid, &gid, &eid, writer_core::api::types::MindMapEdgePatchDto { kind: patch.kind, label: patch.label.map(Some), payload: patch.payload.map(Some) }) {
                Ok(data) => serde_json::json!({ "success": true, "data": data, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }

    pub(crate) fn delete_mind_map_edge_json(&mut self, project_id: QString, graph_id: QString, edge_id: QString) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let eid = edge_id.to_string();

        if let Some(api) = self.core_api() {
            match api.delete_mind_map_edge(&pid, &gid, &eid) {
                Ok(_) => serde_json::json!({ "success": true, "data": true, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }

    pub(crate) fn create_mind_map_anchor_json(&mut self, project_id: QString, graph_id: QString, anchor_json: QString) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let aj = anchor_json.to_string();

        let anchor: writer_core::api::types::MindMapAnchorDto = match serde_json::from_str(&aj) {
            Ok(a) => a,
            Err(e) => return serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("Invalid anchor JSON: {}", e) }).to_string().into(),
        };

        if let Some(api) = self.core_api() {
            match api.create_mind_map_anchor(&pid, &gid, anchor) {
                Ok(data) => serde_json::json!({ "success": true, "data": data, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }

    pub(crate) fn bind_mind_map_anchor_json(&mut self, project_id: QString, graph_id: QString, node_id: QString, anchor_id: QString, link_kind: QString) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let nid = node_id.to_string();
        let aid = anchor_id.to_string();
        let lk = link_kind.to_string();

        if let Some(api) = self.core_api() {
            match api.bind_mind_map_node_to_anchor(&pid, &gid, &nid, &aid, &lk) {
                Ok(data) => serde_json::json!({ "success": true, "data": data, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }

    pub(crate) fn save_mind_map_layout_json(&mut self, project_id: QString, graph_id: QString, layout_json: QString) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let lj = layout_json.to_string();

        let layout: writer_core::api::types::MindMapLayoutDto = match serde_json::from_str(&lj) {
            Ok(l) => l,
            Err(e) => return serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("Invalid layout JSON: {}", e) }).to_string().into(),
        };

        if let Some(api) = self.core_api() {
            match api.save_mind_map_layout(&pid, &gid, layout) {
                Ok(_) => serde_json::json!({ "success": true, "data": true, "error": serde_json::Value::Null }).to_string().into(),
                Err(e) => serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": format!("{}", e) }).to_string().into(),
            }
        } else {
            serde_json::json!({ "success": false, "data": serde_json::Value::Null, "error": "Core not initialized" }).to_string().into()
        }
    }
}
