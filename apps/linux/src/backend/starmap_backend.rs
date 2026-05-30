use super::*;
use crate::backend::SafeAppPtr;

#[allow(non_snake_case)]
#[derive(QObject, Default)]
pub struct StarMapBackend {
    base: qt_base_class!(trait QObject),
    list_starmaps_json: qt_method!(fn(&self) -> QString),
    list_starmaps: qt_method!(fn(&self) -> QJsonArray),
    list_starmaps_for_project_json: qt_method!(fn(&self, project_id: QString) -> QString),
    get_starmap_json: qt_method!(fn(&self, starmap_id: QString) -> QString),
    create_starmap_json: qt_method!(fn(&mut self, title: QString, description: QString, accent_color: QString) -> QString),
    create_starmap: qt_method!(fn(&mut self, title: QString, description: QString, accent_color: QString) -> QJsonObject),
    create_child_starmap_legacy_json: qt_method!(fn(&mut self, parent_id: QString, title: QString, description: QString, accent_color: QString) -> QString),
    rename_starmap_json: qt_method!(fn(&mut self, starmap_id: QString, new_title: QString) -> QString),
    delete_starmap_json: qt_method!(fn(&mut self, starmap_id: QString) -> QString),
    bind_starmap_to_project_json: qt_method!(fn(&mut self, starmap_id: QString, project_id: QString) -> QString),
    set_main_starmap_json: qt_method!(fn(&mut self, starmap_id: QString, project_id: QString) -> QString),
    get_main_starmap_json: qt_method!(fn(&self, project_id: QString) -> QString),
    unbind_starmap_json: qt_method!(fn(&mut self, starmap_id: QString) -> QString),
    get_starmap_graph_json: qt_method!(fn(&self, starmap_id: QString) -> QString),
    get_starmap_graph: qt_method!(fn(&self, starmap_id: QString) -> QJsonObject),
    create_starmap_node_json: qt_method!(fn(&mut self, starmap_id: QString, title: QString, kind: QString, x: f64, y: f64) -> QString),
    create_starmap_node: qt_method!(fn(&mut self, starmap_id: QString, title: QString, kind: QString, x: f64, y: f64) -> QJsonObject),
    update_starmap_node_json: qt_method!(fn(&mut self, starmap_id: QString, node_id: QString, patch_json: QString) -> QString),
    update_starmap_node: qt_method!(fn(&mut self, starmap_id: QString, node_id: QString, patch_json: QString) -> QJsonObject),
    delete_starmap_node_json: qt_method!(fn(&mut self, starmap_id: QString, node_id: QString) -> QString),
    delete_starmap_node: qt_method!(fn(&mut self, starmap_id: QString, node_id: QString) -> QJsonObject),
    create_starmap_edge_json: qt_method!(fn(&mut self, starmap_id: QString, from_node_id: QString, to_node_id: QString, kind: QString, label: QString) -> QString),
    create_starmap_edge: qt_method!(fn(&mut self, starmap_id: QString, from_node_id: QString, to_node_id: QString, kind: QString, label: QString) -> QJsonObject),
    update_starmap_edge_json: qt_method!(fn(&mut self, starmap_id: QString, edge_id: QString, patch_json: QString) -> QString),
    update_starmap_edge: qt_method!(fn(&mut self, starmap_id: QString, edge_id: QString, patch_json: QString) -> QJsonObject),
    delete_starmap_edge_json: qt_method!(fn(&mut self, starmap_id: QString, edge_id: QString) -> QString),
    delete_starmap_edge: qt_method!(fn(&mut self, starmap_id: QString, edge_id: QString) -> QJsonObject),
    save_starmap_layout_json: qt_method!(fn(&mut self, starmap_id: QString, layout_json: QString) -> QString),
    save_starmap_layout: qt_method!(fn(&mut self, starmap_id: QString, layout_json: QString) -> QJsonObject),
    get_mind_map_snapshot_json: qt_method!(fn(&self, project_id: QString) -> QString),
    create_mind_map_graph_json: qt_method!(fn(&mut self, project_id: QString, title: QString) -> QString),
    list_mind_map_graphs_json: qt_method!(fn(&self, project_id: QString) -> QString),
    set_default_mind_map_graph_json: qt_method!(fn(&mut self, project_id: QString, graph_id: QString) -> QString),
    create_mind_map_node_json: qt_method!(fn(&mut self, project_id: QString, graph_id: QString, node_json: QString) -> QString),
    update_mind_map_node_json: qt_method!(fn(&mut self, project_id: QString, graph_id: QString, node_id: QString, patch_json: QString) -> QString),
    delete_mind_map_node_json: qt_method!(fn(&mut self, project_id: QString, graph_id: QString, node_id: QString, cascade: bool) -> QString),
    create_mind_map_edge_json: qt_method!(fn(&mut self, project_id: QString, graph_id: QString, edge_json: QString) -> QString),
    update_mind_map_edge_json: qt_method!(fn(&mut self, project_id: QString, graph_id: QString, edge_id: QString, patch_json: QString) -> QString),
    delete_mind_map_edge_json: qt_method!(fn(&mut self, project_id: QString, graph_id: QString, edge_id: QString) -> QString),
    create_mind_map_anchor_json: qt_method!(fn(&mut self, project_id: QString, graph_id: QString, anchor_json: QString) -> QString),
    bind_mind_map_anchor_json: qt_method!(fn(&mut self, project_id: QString, graph_id: QString, node_id: QString, anchor_id: QString, link_kind: QString) -> QString),
    save_mind_map_layout_json: qt_method!(fn(&mut self, project_id: QString, graph_id: QString, layout_json: QString) -> QString),
    app: SafeAppPtr,
}

impl StarMapBackend {
    pub fn new(app: SafeAppPtr) -> Self { Self { app, ..Default::default() } }
    fn with_app<R>(&self, default: R, f: impl FnOnce(&AppBackend) -> R) -> R {
        if let Some(app) = self.app.get() {
            unsafe { f(&*app) }
        } else {
            crate::backend::app_backend::debug_error_static("starmap", "BACKEND_LINK_BROKEN", "app pointer is null");
            default
        }
    }
    fn with_app_mut<R>(&mut self, default: R, f: impl FnOnce(&mut AppBackend) -> R) -> R {
        if let Some(app) = self.app.get() {
            unsafe { f(&mut *app) }
        } else {
            crate::backend::app_backend::debug_error_static("starmap", "BACKEND_LINK_BROKEN", "app pointer is null");
            default
        }
    }
    
    fn list_starmaps_json(&self) -> QString { self.with_app("[]".into(), |app| app.list_starmaps_json()) }
    fn list_starmaps(&self) -> QJsonArray { self.with_app(QJsonArray::default(), |app| app.list_starmaps()) }
    fn list_starmaps_for_project_json(&self, project_id: QString) -> QString { self.with_app("[]".into(), |app| app.list_starmaps_for_project_json(project_id)) }
    fn get_starmap_json(&self, starmap_id: QString) -> QString { self.with_app("{}".into(), |app| app.get_starmap_json(starmap_id)) }
    fn create_starmap_json(&mut self, title: QString, description: QString, accent_color: QString) -> QString { self.with_app_mut("{}".into(), |app| app.create_starmap_json(title, description, accent_color)) }
    fn create_starmap(&mut self, title: QString, description: QString, accent_color: QString) -> QJsonObject { self.with_app_mut(QJsonObject::default(), |app| app.create_starmap(title, description, accent_color)) }
    fn create_child_starmap_legacy_json(&mut self, parent_id: QString, title: QString, description: QString, accent_color: QString) -> QString { self.with_app_mut("{}".into(), |app| app.create_child_starmap_legacy_json(parent_id, title, description, accent_color)) }
    fn rename_starmap_json(&mut self, starmap_id: QString, new_title: QString) -> QString { self.with_app_mut("{}".into(), |app| app.rename_starmap_json(starmap_id, new_title)) }
    fn delete_starmap_json(&mut self, starmap_id: QString) -> QString { self.with_app_mut("{}".into(), |app| app.delete_starmap_json(starmap_id)) }
    fn bind_starmap_to_project_json(&mut self, starmap_id: QString, project_id: QString) -> QString { self.with_app_mut("{}".into(), |app| app.bind_starmap_to_project_json(starmap_id, project_id)) }
    fn set_main_starmap_json(&mut self, starmap_id: QString, project_id: QString) -> QString { self.with_app_mut("{}".into(), |app| app.set_main_starmap_json(starmap_id, project_id)) }
    fn get_main_starmap_json(&self, project_id: QString) -> QString { self.with_app("{}".into(), |app| app.get_main_starmap_json(project_id)) }
    fn unbind_starmap_json(&mut self, starmap_id: QString) -> QString { self.with_app_mut("{}".into(), |app| app.unbind_starmap_json(starmap_id)) }
    fn get_starmap_graph_json(&self, starmap_id: QString) -> QString { self.with_app("{}".into(), |app| app.get_starmap_graph_json(starmap_id)) }
    fn get_starmap_graph(&self, starmap_id: QString) -> QJsonObject { self.with_app(QJsonObject::default(), |app| app.get_starmap_graph(starmap_id)) }
    fn create_starmap_node_json(&mut self, starmap_id: QString, title: QString, kind: QString, x: f64, y: f64) -> QString { self.with_app_mut("{}".into(), |app| app.create_starmap_node_json(starmap_id, title, kind, x, y)) }
    fn create_starmap_node(&mut self, starmap_id: QString, title: QString, kind: QString, x: f64, y: f64) -> QJsonObject { self.with_app_mut(QJsonObject::default(), |app| app.create_starmap_node(starmap_id, title, kind, x, y)) }
    fn update_starmap_node_json(&mut self, starmap_id: QString, node_id: QString, patch_json: QString) -> QString { self.with_app_mut("{}".into(), |app| app.update_starmap_node_json(starmap_id, node_id, patch_json)) }
    fn update_starmap_node(&mut self, starmap_id: QString, node_id: QString, patch_json: QString) -> QJsonObject { self.with_app_mut(QJsonObject::default(), |app| app.update_starmap_node(starmap_id, node_id, patch_json)) }
    fn delete_starmap_node_json(&mut self, starmap_id: QString, node_id: QString) -> QString { self.with_app_mut("{}".into(), |app| app.delete_starmap_node_json(starmap_id, node_id)) }
    fn delete_starmap_node(&mut self, starmap_id: QString, node_id: QString) -> QJsonObject { self.with_app_mut(QJsonObject::default(), |app| app.delete_starmap_node(starmap_id, node_id)) }
    fn create_starmap_edge_json(&mut self, starmap_id: QString, from_node_id: QString, to_node_id: QString, kind: QString, label: QString) -> QString { self.with_app_mut("{}".into(), |app| app.create_starmap_edge_json(starmap_id, from_node_id, to_node_id, kind, label)) }
    fn create_starmap_edge(&mut self, starmap_id: QString, from_node_id: QString, to_node_id: QString, kind: QString, label: QString) -> QJsonObject { self.with_app_mut(QJsonObject::default(), |app| app.create_starmap_edge(starmap_id, from_node_id, to_node_id, kind, label)) }
    fn update_starmap_edge_json(&mut self, starmap_id: QString, edge_id: QString, patch_json: QString) -> QString { self.with_app_mut("{}".into(), |app| app.update_starmap_edge_json(starmap_id, edge_id, patch_json)) }
    fn update_starmap_edge(&mut self, starmap_id: QString, edge_id: QString, patch_json: QString) -> QJsonObject { self.with_app_mut(QJsonObject::default(), |app| app.update_starmap_edge(starmap_id, edge_id, patch_json)) }
    fn delete_starmap_edge_json(&mut self, starmap_id: QString, edge_id: QString) -> QString { self.with_app_mut("{}".into(), |app| app.delete_starmap_edge_json(starmap_id, edge_id)) }
    fn delete_starmap_edge(&mut self, starmap_id: QString, edge_id: QString) -> QJsonObject { self.with_app_mut(QJsonObject::default(), |app| app.delete_starmap_edge(starmap_id, edge_id)) }
    fn save_starmap_layout_json(&mut self, starmap_id: QString, layout_json: QString) -> QString { self.with_app_mut("{}".into(), |app| app.save_starmap_layout_json(starmap_id, layout_json)) }
    fn save_starmap_layout(&mut self, starmap_id: QString, layout_json: QString) -> QJsonObject { self.with_app_mut(QJsonObject::default(), |app| app.save_starmap_layout(starmap_id, layout_json)) }
    fn get_mind_map_snapshot_json(&self, project_id: QString) -> QString { self.with_app("{}".into(), |app| app.get_mind_map_snapshot_json(project_id)) }
    fn create_mind_map_graph_json(&mut self, project_id: QString, title: QString) -> QString { self.with_app_mut("{}".into(), |app| app.create_mind_map_graph_json(project_id, title)) }
    fn list_mind_map_graphs_json(&self, project_id: QString) -> QString { self.with_app("[]".into(), |app| app.list_mind_map_graphs_json(project_id)) }
    fn set_default_mind_map_graph_json(&mut self, project_id: QString, graph_id: QString) -> QString { self.with_app_mut("{}".into(), |app| app.set_default_mind_map_graph_json(project_id, graph_id)) }
    fn create_mind_map_node_json(&mut self, project_id: QString, graph_id: QString, node_json: QString) -> QString { self.with_app_mut("{}".into(), |app| app.create_mind_map_node_json(project_id, graph_id, node_json)) }
    fn update_mind_map_node_json(&mut self, project_id: QString, graph_id: QString, node_id: QString, patch_json: QString) -> QString { self.with_app_mut("{}".into(), |app| app.update_mind_map_node_json(project_id, graph_id, node_id, patch_json)) }
    fn delete_mind_map_node_json(&mut self, project_id: QString, graph_id: QString, node_id: QString, cascade: bool) -> QString { self.with_app_mut("{}".into(), |app| app.delete_mind_map_node_json(project_id, graph_id, node_id, cascade)) }
    fn create_mind_map_edge_json(&mut self, project_id: QString, graph_id: QString, edge_json: QString) -> QString { self.with_app_mut("{}".into(), |app| app.create_mind_map_edge_json(project_id, graph_id, edge_json)) }
    fn update_mind_map_edge_json(&mut self, project_id: QString, graph_id: QString, edge_id: QString, patch_json: QString) -> QString { self.with_app_mut("{}".into(), |app| app.update_mind_map_edge_json(project_id, graph_id, edge_id, patch_json)) }
    fn delete_mind_map_edge_json(&mut self, project_id: QString, graph_id: QString, edge_id: QString) -> QString { self.with_app_mut("{}".into(), |app| app.delete_mind_map_edge_json(project_id, graph_id, edge_id)) }
    fn create_mind_map_anchor_json(&mut self, project_id: QString, graph_id: QString, anchor_json: QString) -> QString { self.with_app_mut("{}".into(), |app| app.create_mind_map_anchor_json(project_id, graph_id, anchor_json)) }
    fn bind_mind_map_anchor_json(&mut self, project_id: QString, graph_id: QString, node_id: QString, anchor_id: QString, link_kind: QString) -> QString { self.with_app_mut("{}".into(), |app| app.bind_mind_map_anchor_json(project_id, graph_id, node_id, anchor_id, link_kind)) }
    fn save_mind_map_layout_json(&mut self, project_id: QString, graph_id: QString, layout_json: QString) -> QString { self.with_app_mut("{}".into(), |app| app.save_mind_map_layout_json(project_id, graph_id, layout_json)) }
}

impl AppBackend {
// Included inside impl AppBackend from app_backend.rs.
// Deprecated compatibility methods for this Linux backend domain.

// AppBackend::get_mind_map_snapshot_json
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

// AppBackend::create_mind_map_graph_json
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

// AppBackend::list_mind_map_graphs_json
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

// AppBackend::set_default_mind_map_graph_json
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

// AppBackend::create_mind_map_node_json
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

// AppBackend::update_mind_map_node_json
    pub(crate) fn update_mind_map_node_json(&mut self, project_id: QString, graph_id: QString, node_id: QString, patch_json: QString) -> QString {
        let pid = project_id.to_string();
        let gid = graph_id.to_string();
        let nid = node_id.to_string();
        let pj = patch_json.to_string();

        // Define patch struct inline since we need to deserialize
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

// AppBackend::delete_mind_map_node_json
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

// AppBackend::create_mind_map_edge_json
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

// AppBackend::update_mind_map_edge_json
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

// AppBackend::delete_mind_map_edge_json
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

// AppBackend::create_mind_map_anchor_json
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

// AppBackend::bind_mind_map_anchor_json
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

// AppBackend::save_mind_map_layout_json
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

// AppBackend::list_starmaps_json
    pub(crate) fn list_starmaps_json(&self) -> QString {
        if let Some(core) = self.core_api() {
            starmap_bridge::list_starmaps(&core).into()
        } else {
            "[]".into()
        }
    }

// AppBackend::list_starmaps
    pub(crate) fn list_starmaps(&self) -> QJsonArray {
        if let Some(core) = self.core_api() {
            qjson_array_data_from_json(&starmap_bridge::list_starmaps(&core))
        } else {
            QJsonArray::default()
        }
    }

// AppBackend::list_starmaps_for_project_json
    pub(crate) fn list_starmaps_for_project_json(&self, project_id: QString) -> QString {
        let pid = project_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::list_starmaps_for_project(&core, &pid).into()
        } else {
            "[]".into()
        }
    }

// AppBackend::get_starmap_json
    pub(crate) fn get_starmap_json(&self, starmap_id: QString) -> QString {
        let sid = starmap_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::get_starmap(&core, &sid).into()
        } else {
            "{}".into()
        }
    }

// AppBackend::create_starmap_json
    pub(crate) fn create_starmap_json(&mut self, title: QString, description: QString, accent_color: QString) -> QString {
        let t = title.to_string();
        let d = description.to_string();
        let ac = accent_color.to_string();
        let color_ref = if ac.is_empty() { None } else { Some(ac.as_str()) };
        if let Some(core) = self.core_api() {
            starmap_bridge::create_starmap(&core, &t, &d, color_ref).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

// AppBackend::create_starmap
    pub(crate) fn create_starmap(&mut self, title: QString, description: QString, accent_color: QString) -> QJsonObject {
        let raw = self.create_starmap_json(title, description, accent_color).to_string();
        qjson_object_from_json(&raw)
    }

// AppBackend::create_child_starmap_legacy_json
    pub(crate) fn create_child_starmap_legacy_json(&mut self, parent_id: QString, title: QString, description: QString, accent_color: QString) -> QString {
        let pid = parent_id.to_string();
        let t = title.to_string();
        let d = description.to_string();
        let ac = accent_color.to_string();
        let color_ref = if ac.is_empty() { None } else { Some(ac.as_str()) };
        if let Some(core) = self.core_api() {
            starmap_bridge::create_child_starmap_legacy(&core, &pid, &t, &d, color_ref).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

// AppBackend::rename_starmap_json
    pub(crate) fn rename_starmap_json(&mut self, starmap_id: QString, new_title: QString) -> QString {
        let sid = starmap_id.to_string();
        let t = new_title.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::rename_starmap(&core, &sid, &t).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

// AppBackend::delete_starmap_json
    pub(crate) fn delete_starmap_json(&mut self, starmap_id: QString) -> QString {
        let sid = starmap_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::delete_starmap(&core, &sid).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

// AppBackend::get_starmap_graph_json
    pub(crate) fn get_starmap_graph_json(&self, starmap_id: QString) -> QString {
        let sid = starmap_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::get_starmap_graph_and_layout(&core, &sid).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

// AppBackend::get_starmap_graph
    pub(crate) fn get_starmap_graph(&self, starmap_id: QString) -> QJsonObject {
        let raw = self.get_starmap_graph_json(starmap_id).to_string();
        qjson_object_from_json(&raw)
    }

// AppBackend::create_starmap_node_json
    pub(crate) fn create_starmap_node_json(&mut self, starmap_id: QString, title: QString, kind: QString, x: f64, y: f64) -> QString {
        let sid = starmap_id.to_string();
        let t = title.to_string();
        let k = kind.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::create_starmap_node(&core, &sid, &t, &k, x, y).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

// AppBackend::create_starmap_node
    pub(crate) fn create_starmap_node(&mut self, starmap_id: QString, title: QString, kind: QString, x: f64, y: f64) -> QJsonObject {
        let raw = self.create_starmap_node_json(starmap_id, title, kind, x, y).to_string();
        qjson_object_from_json(&raw)
    }

// AppBackend::update_starmap_node_json
    pub(crate) fn update_starmap_node_json(&mut self, starmap_id: QString, node_id: QString, patch_json: QString) -> QString {
        let sid = starmap_id.to_string();
        let nid = node_id.to_string();
        let p = patch_json.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::update_starmap_node(&core, &sid, &nid, &p).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

// AppBackend::update_starmap_node
    pub(crate) fn update_starmap_node(&mut self, starmap_id: QString, node_id: QString, patch_json: QString) -> QJsonObject {
        let raw = self.update_starmap_node_json(starmap_id, node_id, patch_json).to_string();
        qjson_object_from_json(&raw)
    }

// AppBackend::delete_starmap_node_json
    pub(crate) fn delete_starmap_node_json(&mut self, starmap_id: QString, node_id: QString) -> QString {
        let sid = starmap_id.to_string();
        let nid = node_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::delete_starmap_node(&core, &sid, &nid).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

// AppBackend::delete_starmap_node
    pub(crate) fn delete_starmap_node(&mut self, starmap_id: QString, node_id: QString) -> QJsonObject {
        let raw = self.delete_starmap_node_json(starmap_id, node_id).to_string();
        qjson_object_from_json(&raw)
    }

// AppBackend::create_starmap_edge_json
    pub(crate) fn create_starmap_edge_json(&mut self, starmap_id: QString, from_node_id: QString, to_node_id: QString, kind: QString, label: QString) -> QString {
        let sid = starmap_id.to_string();
        let from_id = from_node_id.to_string();
        let to_id = to_node_id.to_string();
        let k = kind.to_string();
        let l = label.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::create_starmap_edge(&core, &sid, &from_id, &to_id, &k, &l).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

// AppBackend::create_starmap_edge
    pub(crate) fn create_starmap_edge(&mut self, starmap_id: QString, from_node_id: QString, to_node_id: QString, kind: QString, label: QString) -> QJsonObject {
        let raw = self.create_starmap_edge_json(starmap_id, from_node_id, to_node_id, kind, label).to_string();
        qjson_object_from_json(&raw)
    }

// AppBackend::update_starmap_edge_json
    pub(crate) fn update_starmap_edge_json(&mut self, starmap_id: QString, edge_id: QString, patch_json: QString) -> QString {
        let sid = starmap_id.to_string();
        let eid = edge_id.to_string();
        let p = patch_json.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::update_starmap_edge(&core, &sid, &eid, &p).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

// AppBackend::update_starmap_edge
    pub(crate) fn update_starmap_edge(&mut self, starmap_id: QString, edge_id: QString, patch_json: QString) -> QJsonObject {
        let raw = self.update_starmap_edge_json(starmap_id, edge_id, patch_json).to_string();
        qjson_object_from_json(&raw)
    }

// AppBackend::delete_starmap_edge_json
    pub(crate) fn delete_starmap_edge_json(&mut self, starmap_id: QString, edge_id: QString) -> QString {
        let sid = starmap_id.to_string();
        let eid = edge_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::delete_starmap_edge(&core, &sid, &eid).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

// AppBackend::delete_starmap_edge
    pub(crate) fn delete_starmap_edge(&mut self, starmap_id: QString, edge_id: QString) -> QJsonObject {
        let raw = self.delete_starmap_edge_json(starmap_id, edge_id).to_string();
        qjson_object_from_json(&raw)
    }

// AppBackend::save_starmap_layout_json
    pub(crate) fn save_starmap_layout_json(&mut self, starmap_id: QString, layout_json: QString) -> QString {
        let sid = starmap_id.to_string();
        let lj = layout_json.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::save_starmap_layout(&core, &sid, &lj).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

// AppBackend::save_starmap_layout
    pub(crate) fn save_starmap_layout(&mut self, starmap_id: QString, layout_json: QString) -> QJsonObject {
        let raw = self.save_starmap_layout_json(starmap_id, layout_json).to_string();
        qjson_object_from_json(&raw)
    }

// AppBackend::bind_starmap_to_project_json
    pub(crate) fn bind_starmap_to_project_json(&mut self, starmap_id: QString, project_id: QString) -> QString {
        let sid = starmap_id.to_string();
        let pid = project_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::bind_starmap_to_project(&core, &sid, &pid).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

// AppBackend::set_main_starmap_json
    pub(crate) fn set_main_starmap_json(&mut self, starmap_id: QString, project_id: QString) -> QString {
        let sid = starmap_id.to_string();
        let pid = project_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::set_main_starmap(&core, &sid, &pid).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

// AppBackend::get_main_starmap_json
    pub(crate) fn get_main_starmap_json(&self, project_id: QString) -> QString {
        let pid = project_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::get_main_starmap(&core, &pid).into()
        } else {
            "{}".into()
        }
    }

// AppBackend::unbind_starmap_json
    pub(crate) fn unbind_starmap_json(&mut self, starmap_id: QString) -> QString {
        let sid = starmap_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::unbind_starmap(&core, &sid).into()
        } else {
            serde_json::json!({"success": false, "message": "Core not initialized"}).to_string().into()
        }
    }

}
