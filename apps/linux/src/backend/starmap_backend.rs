// =============================================================================
// starmap_backend.rs — 星图与创作脑图领域 QObject 后端适配层
// =============================================================================
//
// 引用了什么：
// - super::*：引入 AppBackend 核心后端的全部方法与结构体。
// - crate::backend::SafeAppPtr：用于安全访问全局 AppBackend 指针以读取/更新星图与脑图数据。
//
// 干什么的：
// - 实现 StarMapBackend 结构体，作为 QML 中 "starmapBackend" 对象的桥梁。
// - 提供星图管理交互（starmap_bridge::*），包括获取列表、新建、重命名、物理删除、作品绑定解绑。
// - 负责星图二维大画布节点（Nodes）的添加/更新/删除、连接线（Edges）的增删改查、以及高频拖拽节点后的坐标布局落盘（save_starmap_layout）。
// - 负责脑图（Mind Map）元数据与大纲树结构管理，包含脑图快照解析（get_mind_map_snapshot）、脑图节点创建、锚点绑定（bind_mind_map_anchor）与脑图坐标排版保存，协助用户绘制作品的创作知识图谱。
//
// 被什么引用：
// - 被 apps/linux/src/backend/mod.rs 引用，用于实例化星图后端并绑定为 QML 全局上下文属性。
// =============================================================================

use super::*;
use crate::backend::SafeAppPtr;

#[path = "mind_map_operations.rs"]
mod mind_map_operations;

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
