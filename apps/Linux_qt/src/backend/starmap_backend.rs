// =============================================================================
// starmap_backend.rs — 星图领域 QObject 后端适配层
// =============================================================================
//
// 引用了什么：
// - super::*：引入 AppBackend 核心后端的全部方法与结构体。
// - crate::backend::AppRef：用于安全访问全局 AppBackend 指针以读取/更新星图数据。
//
// 干什么的：
// - 实现 StarMapBackend 结构体，作为 QML 中 "starmapBackend" 对象的桥梁。
// - 提供星图管理交互（starmap_bridge::*），包括获取列表、新建、重命名、物理删除、作品绑定解绑。
// - 负责星图二维大画布节点（Nodes）的添加/更新/删除、连接线（Edges）的增删改查、以及高频拖拽节点后的坐标布局落盘（save_starmap_layout）。
//
// 被什么引用：
// - 被 apps/Linux_qt/src/backend/mod.rs 引用，用于实例化星图后端并绑定为 QML 全局上下文属性。
// =============================================================================

use super::*;
use crate::backend::AppRef;

#[allow(non_snake_case)]
#[derive(QObject, Default)]
pub struct StarMapBackend {
    base: qt_base_class!(trait QObject),
    list_starmaps_json: qt_method!(fn(&self) -> QString),
    list_starmaps: qt_method!(fn(&self) -> QJsonArray),
    list_root_starmaps_json: qt_method!(fn(&self) -> QString),
    list_root_starmaps: qt_method!(fn(&self) -> QJsonArray),
    list_starmaps_for_project_json: qt_method!(fn(&self, project_id: QString) -> QString),
    get_starmap_json: qt_method!(fn(&self, starmap_id: QString) -> QString),
    create_starmap_json: qt_method!(
        fn(&mut self, title: QString, description: QString, accent_color: QString) -> QString
    ),
    create_starmap: qt_method!(
        fn(&mut self, title: QString, description: QString, accent_color: QString) -> QJsonObject
    ),

    rename_starmap_json:
        qt_method!(fn(&mut self, starmap_id: QString, new_title: QString) -> QString),
    delete_starmap_json: qt_method!(fn(&mut self, starmap_id: QString) -> QString),
    delete_starmap: qt_method!(fn(&mut self, starmap_id: QString) -> QJsonObject),
    bind_starmap_to_project_json:
        qt_method!(fn(&mut self, starmap_id: QString, project_id: QString) -> QString),
    set_main_starmap_json:
        qt_method!(fn(&mut self, starmap_id: QString, project_id: QString) -> QString),
    get_main_starmap_json: qt_method!(fn(&self, project_id: QString) -> QString),
    unbind_starmap_json: qt_method!(fn(&mut self, starmap_id: QString) -> QString),
    get_starmap_graph_json: qt_method!(fn(&self, starmap_id: QString) -> QString),
    get_starmap_graph: qt_method!(fn(&self, starmap_id: QString) -> QJsonObject),
    create_starmap_node_json: qt_method!(
        fn(
            &mut self,
            starmap_id: QString,
            title: QString,
            kind: QString,
            x: f64,
            y: f64,
        ) -> QString
    ),
    create_starmap_node: qt_method!(
        fn(
            &mut self,
            starmap_id: QString,
            title: QString,
            kind: QString,
            x: f64,
            y: f64,
        ) -> QJsonObject
    ),
    update_starmap_node_json: qt_method!(
        fn(&mut self, starmap_id: QString, node_id: QString, patch_json: QString) -> QString
    ),
    update_starmap_node: qt_method!(
        fn(&mut self, starmap_id: QString, node_id: QString, patch_json: QString) -> QJsonObject
    ),
    delete_starmap_node_json:
        qt_method!(fn(&mut self, starmap_id: QString, node_id: QString) -> QString),
    delete_starmap_node:
        qt_method!(fn(&mut self, starmap_id: QString, node_id: QString) -> QJsonObject),
    create_starmap_edge_json: qt_method!(
        fn(
            &mut self,
            starmap_id: QString,
            from_node_id: QString,
            to_node_id: QString,
            kind: QString,
            label: QString,
        ) -> QString
    ),
    create_starmap_edge: qt_method!(
        fn(
            &mut self,
            starmap_id: QString,
            from_node_id: QString,
            to_node_id: QString,
            kind: QString,
            label: QString,
        ) -> QJsonObject
    ),
    create_starmap_edge_with_paths_json: qt_method!(
        fn(
            &mut self,
            starmap_id: QString,
            from_path_json: QString,
            to_path_json: QString,
            kind: QString,
            label: QString,
        ) -> QString
    ),
    create_starmap_edge_with_paths: qt_method!(
        fn(
            &mut self,
            starmap_id: QString,
            from_path_json: QString,
            to_path_json: QString,
            kind: QString,
            label: QString,
        ) -> QJsonObject
    ),
    update_starmap_edge_json: qt_method!(
        fn(&mut self, starmap_id: QString, edge_id: QString, patch_json: QString) -> QString
    ),
    update_starmap_edge: qt_method!(
        fn(&mut self, starmap_id: QString, edge_id: QString, patch_json: QString) -> QJsonObject
    ),
    delete_starmap_edge_json:
        qt_method!(fn(&mut self, starmap_id: QString, edge_id: QString) -> QString),
    delete_starmap_edge:
        qt_method!(fn(&mut self, starmap_id: QString, edge_id: QString) -> QJsonObject),
    create_starmap_embed_json: qt_method!(
        fn(
            &mut self,
            starmap_id: QString,
            target_starmap_id: QString,
            label: QString,
            x: f64,
            y: f64,
        ) -> QString
    ),
    create_starmap_embed: qt_method!(
        fn(
            &mut self,
            starmap_id: QString,
            target_starmap_id: QString,
            label: QString,
            x: f64,
            y: f64,
        ) -> QJsonObject
    ),
    update_starmap_embed_json: qt_method!(
        fn(&mut self, starmap_id: QString, instance_id: QString, patch_json: QString) -> QString
    ),
    update_starmap_embed: qt_method!(
        fn(
            &mut self,
            starmap_id: QString,
            instance_id: QString,
            patch_json: QString,
        ) -> QJsonObject
    ),
    delete_starmap_embed_json:
        qt_method!(fn(&mut self, starmap_id: QString, instance_id: QString) -> QString),
    delete_starmap_embed:
        qt_method!(fn(&mut self, starmap_id: QString, instance_id: QString) -> QJsonObject),
    save_starmap_layout_json:
        qt_method!(fn(&mut self, starmap_id: QString, layout_json: QString) -> QString),
    save_starmap_layout:
        qt_method!(fn(&mut self, starmap_id: QString, layout_json: QString) -> QJsonObject),
    compute_edge_renders_json: qt_method!(
        fn(&self, starmap_id: QString, nodes_json: QString, embeds_json: QString) -> QString
    ),
    compute_edge_renders: qt_method!(
        fn(&self, starmap_id: QString, nodes_json: QString, embeds_json: QString) -> QJsonObject
    ),
    hit_test_edge_renders_json:
        qt_method!(fn(&self, renders_json: QString, x: f64, y: f64) -> QString),
    hit_test_edge_renders:
        qt_method!(fn(&self, renders_json: QString, x: f64, y: f64) -> QJsonObject),
    hit_test_nodes_json: qt_method!(fn(&self, nodes_json: QString, x: f64, y: f64) -> QString),
    hit_test_nodes: qt_method!(fn(&self, nodes_json: QString, x: f64, y: f64) -> QJsonObject),
    calculate_grid_layout_json:
        qt_method!(fn(&self, node_ids_json: QString, existing_layout_json: QString) -> QString),
    calculate_grid_layout:
        qt_method!(fn(&self, node_ids_json: QString, existing_layout_json: QString) -> QJsonObject),
    add_starmap_hyperlink:
        qt_method!(fn(&mut self, starmap_id: QString, hyperlink_json: QString) -> QJsonObject),
    update_starmap_hyperlink: qt_method!(
        fn(
            &mut self,
            starmap_id: QString,
            hyperlink_id: QString,
            patch_json: QString,
        ) -> QJsonObject
    ),
    delete_starmap_hyperlink:
        qt_method!(fn(&mut self, starmap_id: QString, hyperlink_id: QString) -> QJsonObject),
    list_starmap_hyperlinks: qt_method!(fn(&self, starmap_id: QString) -> QJsonObject),
    app: AppRef,
}

impl StarMapBackend {
    pub fn new(app: AppRef) -> Self {
        Self {
            app,
            ..Default::default()
        }
    }
    fn with_app<R>(
        &self,
        f: impl FnOnce(&AppBackend) -> R,
    ) -> Result<R, crate::backend::AppBorrowError> {
        self.app.with_app(f)
    }
    fn with_app_mut<R>(
        &self,
        f: impl FnOnce(&mut AppBackend) -> R,
    ) -> Result<R, crate::backend::AppBorrowError> {
        self.app.with_app_mut(f)
    }

    /// 记录星图写操作的真实业务结果（按 JSON envelope 的 success/errorCode/messageKey/rawError/data.* 字段），
    /// 不再用 with_app_mut().is_ok() 代表业务成功。
    fn log_starmap_envelope(operation: &str, starmap_id: &str, object_id: &str, raw_json: &str) {
        let v: serde_json::Value = match serde_json::from_str(raw_json) {
            Ok(v) => v,
            Err(_) => {
                log::info!(
                    "starmap_op operation={} starmapId={} success=false error=invalid_envelope_json",
                    operation, starmap_id
                );
                return;
            }
        };
        let success = v.get("success").and_then(|s| s.as_bool()).unwrap_or(false);
        let error_code = v.get("errorCode").and_then(|x| x.as_str()).unwrap_or("");
        let message_key = v.get("messageKey").and_then(|x| x.as_str()).unwrap_or("");
        let raw_error = v.get("rawError").and_then(|x| x.as_str()).unwrap_or("");
        // data.id / data.nodeId / data.edgeId 都尝试读
        let data_id = v
            .get("data")
            .and_then(|d| {
                d.get("id")
                    .or_else(|| d.get("nodeId"))
                    .or_else(|| d.get("edgeId"))
                    .or_else(|| d.get("starmapId"))
            })
            .and_then(|i| i.as_str())
            .unwrap_or("");
        log::info!(
            "starmap_op operation={} starmapId={} objectId={} dataId={} success={} errorCode={} messageKey={} rawError={}",
            operation, starmap_id, object_id, data_id, success, error_code, message_key, raw_error
        );
    }

    fn list_starmaps_json(&self) -> QString {
        self.with_app(|app| app.list_starmaps_json())
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn list_starmaps(&self) -> QJsonArray {
        match self.with_app(|app| app.list_starmaps()) {
            Ok(arr) => arr,
            Err(_) => crate::backend::json_utils::serde_to_qjson_array(
                serde_json::from_str(&crate::backend::json_utils::borrow_conflict_error_json())
                    .unwrap_or(serde_json::json!([])),
            ),
        }
    }
    fn list_root_starmaps_json(&self) -> QString {
        self.with_app(|app| app.list_root_starmaps_json())
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn list_root_starmaps(&self) -> QJsonArray {
        match self.with_app(|app| app.list_root_starmaps()) {
            Ok(arr) => arr,
            Err(_) => crate::backend::json_utils::serde_to_qjson_array(
                serde_json::from_str(&crate::backend::json_utils::borrow_conflict_error_json())
                    .unwrap_or(serde_json::json!([])),
            ),
        }
    }
    fn list_starmaps_for_project_json(&self, project_id: QString) -> QString {
        self.with_app(|app| app.list_starmaps_for_project_json(project_id))
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn get_starmap_json(&self, starmap_id: QString) -> QString {
        self.with_app(|app| app.get_starmap_json(starmap_id))
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn create_starmap_json(
        &mut self,
        title: QString,
        description: QString,
        accent_color: QString,
    ) -> QString {
        self.with_app_mut(|app| app.create_starmap_json(title, description, accent_color))
            .unwrap_or_else(|_| {
                QString::from(crate::backend::json_utils::borrow_conflict_error_json())
            })
    }
    fn create_starmap(
        &mut self,
        title: QString,
        description: QString,
        accent_color: QString,
    ) -> QJsonObject {
        self.with_app_mut(|app| app.create_starmap(title, description, accent_color))
            .unwrap_or_else(|_| {
                crate::backend::json_utils::qjson_object_from_json(
                    &crate::backend::json_utils::borrow_conflict_error_json(),
                )
            })
    }

    fn rename_starmap_json(&mut self, starmap_id: QString, new_title: QString) -> QString {
        self.with_app_mut(|app| app.rename_starmap_json(starmap_id, new_title))
            .unwrap_or_else(|_| {
                QString::from(crate::backend::json_utils::borrow_conflict_error_json())
            })
    }
    fn delete_starmap_json(&mut self, starmap_id: QString) -> QString {
        self.with_app_mut(|app| app.delete_starmap_json(starmap_id))
            .unwrap_or_else(|_| {
                QString::from(crate::backend::json_utils::borrow_conflict_error_json())
            })
    }
    fn delete_starmap(&mut self, starmap_id: QString) -> QJsonObject {
        let raw = self.delete_starmap_json(starmap_id).to_string();
        crate::backend::json_utils::qjson_object_from_json(&raw)
    }
    fn bind_starmap_to_project_json(
        &mut self,
        starmap_id: QString,
        project_id: QString,
    ) -> QString {
        self.with_app_mut(|app| app.bind_starmap_to_project_json(starmap_id, project_id))
            .unwrap_or_else(|_| {
                QString::from(crate::backend::json_utils::borrow_conflict_error_json())
            })
    }
    fn set_main_starmap_json(&mut self, starmap_id: QString, project_id: QString) -> QString {
        self.with_app_mut(|app| app.set_main_starmap_json(starmap_id, project_id))
            .unwrap_or_else(|_| {
                QString::from(crate::backend::json_utils::borrow_conflict_error_json())
            })
    }
    fn get_main_starmap_json(&self, project_id: QString) -> QString {
        self.with_app(|app| app.get_main_starmap_json(project_id))
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn unbind_starmap_json(&mut self, starmap_id: QString) -> QString {
        self.with_app_mut(|app| app.unbind_starmap_json(starmap_id))
            .unwrap_or_else(|_| {
                QString::from(crate::backend::json_utils::borrow_conflict_error_json())
            })
    }
    fn compute_edge_renders_json(
        &self,
        starmap_id: QString,
        nodes_json: QString,
        embeds_json: QString,
    ) -> QString {
        let sid = starmap_id.to_string();
        let nj = nodes_json.to_string();
        let ej = embeds_json.to_string();
        match self.with_app(|app| app.core_api()) {
            Ok(Some(core)) => {
                match core.get_starmap_graph(&sid) {
                    Ok(graph_dto) => {
                        match <writer_core::starmap::types::StarMapGraph as std::convert::TryFrom<
                            _,
                        >>::try_from(graph_dto)
                        {
                            Ok(graph) => crate::starmap_view::bridge::compute_edge_renders_json(
                                &graph, &nj, &ej,
                            )
                            .into(),
                            Err(e) => crate::backend::json_utils::envelope_error_json(
                                writer_core::api::WriterError::Other(e.to_string()),
                            )
                            .into(),
                        }
                    }
                    Err(e) => crate::backend::json_utils::envelope_error_json(
                        writer_core::api::WriterError::Other(e.to_string()),
                    )
                    .into(),
                }
            }
            _ => crate::backend::json_utils::borrow_conflict_error_json().into(),
        }
    }
    fn hit_test_edge_renders_json(&self, renders_json: QString, x: f64, y: f64) -> QString {
        let rj = renders_json.to_string();
        crate::starmap_view::bridge::hit_test_edge_renders_json(&rj, x as f32, y as f32).into()
    }
    fn hit_test_nodes_json(&self, nodes_json: QString, x: f64, y: f64) -> QString {
        let nj = nodes_json.to_string();
        crate::starmap_view::bridge::hit_test_nodes_json(&nj, x as f32, y as f32).into()
    }
    fn calculate_grid_layout_json(
        &self,
        node_ids_json: QString,
        existing_layout_json: QString,
    ) -> QString {
        let ni = node_ids_json.to_string();
        let el = existing_layout_json.to_string();
        crate::starmap_view::bridge::calculate_grid_layout_json(&ni, &el).into()
    }
    fn compute_edge_renders(
        &self,
        starmap_id: QString,
        nodes_json: QString,
        embeds_json: QString,
    ) -> QJsonObject {
        let raw = self
            .compute_edge_renders_json(starmap_id, nodes_json, embeds_json)
            .to_string();
        crate::backend::json_utils::qjson_object_from_json(&raw)
    }
    fn hit_test_edge_renders(&self, renders_json: QString, x: f64, y: f64) -> QJsonObject {
        let raw = self
            .hit_test_edge_renders_json(renders_json, x, y)
            .to_string();
        crate::backend::json_utils::qjson_object_from_json(&raw)
    }
    fn hit_test_nodes(&self, nodes_json: QString, x: f64, y: f64) -> QJsonObject {
        let raw = self.hit_test_nodes_json(nodes_json, x, y).to_string();
        crate::backend::json_utils::qjson_object_from_json(&raw)
    }
    fn calculate_grid_layout(
        &self,
        node_ids_json: QString,
        existing_layout_json: QString,
    ) -> QJsonObject {
        let raw = self
            .calculate_grid_layout_json(node_ids_json, existing_layout_json)
            .to_string();
        crate::backend::json_utils::qjson_object_from_json(&raw)
    }
    fn get_starmap_graph_json(&self, starmap_id: QString) -> QString {
        self.with_app(|app| app.get_starmap_graph_json(starmap_id))
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn get_starmap_graph(&self, starmap_id: QString) -> QJsonObject {
        self.with_app(|app| app.get_starmap_graph(starmap_id))
            .unwrap_or_else(|_| {
                crate::backend::json_utils::qjson_object_from_json(
                    &crate::backend::json_utils::borrow_conflict_error_json(),
                )
            })
    }
    fn create_starmap_node_json(
        &mut self,
        starmap_id: QString,
        title: QString,
        kind: QString,
        x: f64,
        y: f64,
    ) -> QString {
        self.with_app_mut(|app| app.create_starmap_node_json(starmap_id, title, kind, x, y))
            .unwrap_or_else(|_| {
                QString::from(crate::backend::json_utils::borrow_conflict_error_json())
            })
    }
    fn create_starmap_node(
        &mut self,
        starmap_id: QString,
        title: QString,
        kind: QString,
        x: f64,
        y: f64,
    ) -> QJsonObject {
        let sid = starmap_id.to_string();
        let raw = self
            .create_starmap_node_json(starmap_id, title, kind, x, y)
            .to_string();
        Self::log_starmap_envelope("create_starmap_node", &sid, "", &raw);
        crate::backend::json_utils::qjson_object_from_json(&raw)
    }
    fn update_starmap_node_json(
        &mut self,
        starmap_id: QString,
        node_id: QString,
        patch_json: QString,
    ) -> QString {
        self.with_app_mut(|app| app.update_starmap_node_json(starmap_id, node_id, patch_json))
            .unwrap_or_else(|_| {
                QString::from(crate::backend::json_utils::borrow_conflict_error_json())
            })
    }
    fn update_starmap_node(
        &mut self,
        starmap_id: QString,
        node_id: QString,
        patch_json: QString,
    ) -> QJsonObject {
        let sid = starmap_id.to_string();
        let nid = node_id.to_string();
        let raw = self
            .update_starmap_node_json(starmap_id, node_id, patch_json)
            .to_string();
        Self::log_starmap_envelope("update_starmap_node", &sid, &nid, &raw);
        crate::backend::json_utils::qjson_object_from_json(&raw)
    }
    fn delete_starmap_node_json(&mut self, starmap_id: QString, node_id: QString) -> QString {
        self.with_app_mut(|app| app.delete_starmap_node_json(starmap_id, node_id))
            .unwrap_or_else(|_| {
                QString::from(crate::backend::json_utils::borrow_conflict_error_json())
            })
    }
    fn delete_starmap_node(&mut self, starmap_id: QString, node_id: QString) -> QJsonObject {
        let sid = starmap_id.to_string();
        let nid = node_id.to_string();
        let raw = self
            .delete_starmap_node_json(starmap_id, node_id)
            .to_string();
        Self::log_starmap_envelope("delete_starmap_node", &sid, &nid, &raw);
        crate::backend::json_utils::qjson_object_from_json(&raw)
    }
    fn create_starmap_edge_json(
        &mut self,
        starmap_id: QString,
        from_node_id: QString,
        to_node_id: QString,
        kind: QString,
        label: QString,
    ) -> QString {
        self.with_app_mut(|app| {
            app.create_starmap_edge_json(starmap_id, from_node_id, to_node_id, kind, label)
        })
        .unwrap_or_else(|_| QString::from(crate::backend::json_utils::borrow_conflict_error_json()))
    }
    fn create_starmap_edge(
        &mut self,
        starmap_id: QString,
        from_node_id: QString,
        to_node_id: QString,
        kind: QString,
        label: QString,
    ) -> QJsonObject {
        let sid = starmap_id.to_string();
        let raw = self
            .create_starmap_edge_json(starmap_id, from_node_id, to_node_id, kind, label)
            .to_string();
        Self::log_starmap_envelope("create_starmap_edge", &sid, "", &raw);
        crate::backend::json_utils::qjson_object_from_json(&raw)
    }
    fn create_starmap_edge_with_paths_json(
        &mut self,
        starmap_id: QString,
        from_path_json: QString,
        to_path_json: QString,
        kind: QString,
        label: QString,
    ) -> QString {
        self.with_app_mut(|app| {
            app.create_starmap_edge_with_paths_json(
                starmap_id,
                from_path_json,
                to_path_json,
                kind,
                label,
            )
        })
        .unwrap_or_else(|_| QString::from(crate::backend::json_utils::borrow_conflict_error_json()))
    }
    fn create_starmap_edge_with_paths(
        &mut self,
        starmap_id: QString,
        from_path_json: QString,
        to_path_json: QString,
        kind: QString,
        label: QString,
    ) -> QJsonObject {
        let sid = starmap_id.to_string();
        let raw = self
            .create_starmap_edge_with_paths_json(
                starmap_id,
                from_path_json,
                to_path_json,
                kind,
                label,
            )
            .to_string();
        Self::log_starmap_envelope("create_starmap_edge_with_paths", &sid, "", &raw);
        crate::backend::json_utils::qjson_object_from_json(&raw)
    }
    fn update_starmap_edge_json(
        &mut self,
        starmap_id: QString,
        edge_id: QString,
        patch_json: QString,
    ) -> QString {
        self.with_app_mut(|app| app.update_starmap_edge_json(starmap_id, edge_id, patch_json))
            .unwrap_or_else(|_| {
                QString::from(crate::backend::json_utils::borrow_conflict_error_json())
            })
    }
    fn update_starmap_edge(
        &mut self,
        starmap_id: QString,
        edge_id: QString,
        patch_json: QString,
    ) -> QJsonObject {
        let sid = starmap_id.to_string();
        let eid = edge_id.to_string();
        let raw = self
            .update_starmap_edge_json(starmap_id, edge_id, patch_json)
            .to_string();
        Self::log_starmap_envelope("update_starmap_edge", &sid, &eid, &raw);
        crate::backend::json_utils::qjson_object_from_json(&raw)
    }
    fn delete_starmap_edge_json(&mut self, starmap_id: QString, edge_id: QString) -> QString {
        self.with_app_mut(|app| app.delete_starmap_edge_json(starmap_id, edge_id))
            .unwrap_or_else(|_| {
                QString::from(crate::backend::json_utils::borrow_conflict_error_json())
            })
    }
    fn delete_starmap_edge(&mut self, starmap_id: QString, edge_id: QString) -> QJsonObject {
        let sid = starmap_id.to_string();
        let eid = edge_id.to_string();
        let raw = self
            .delete_starmap_edge_json(starmap_id, edge_id)
            .to_string();
        Self::log_starmap_envelope("delete_starmap_edge", &sid, &eid, &raw);
        crate::backend::json_utils::qjson_object_from_json(&raw)
    }
    fn create_starmap_embed_json(
        &mut self,
        starmap_id: QString,
        target_starmap_id: QString,
        label: QString,
        x: f64,
        y: f64,
    ) -> QString {
        self.with_app_mut(|app| {
            app.create_starmap_embed_json(starmap_id, target_starmap_id, label, x, y)
        })
        .unwrap_or_else(|_| QString::from(crate::backend::json_utils::borrow_conflict_error_json()))
    }
    fn create_starmap_embed(
        &mut self,
        starmap_id: QString,
        target_starmap_id: QString,
        label: QString,
        x: f64,
        y: f64,
    ) -> QJsonObject {
        let sid = starmap_id.to_string();
        let raw = self
            .create_starmap_embed_json(starmap_id, target_starmap_id, label, x, y)
            .to_string();
        Self::log_starmap_envelope("create_starmap_embed", &sid, "", &raw);
        crate::backend::json_utils::qjson_object_from_json(&raw)
    }
    fn update_starmap_embed_json(
        &mut self,
        starmap_id: QString,
        instance_id: QString,
        patch_json: QString,
    ) -> QString {
        self.with_app_mut(|app| app.update_starmap_embed_json(starmap_id, instance_id, patch_json))
            .unwrap_or_else(|_| {
                QString::from(crate::backend::json_utils::borrow_conflict_error_json())
            })
    }
    fn update_starmap_embed(
        &mut self,
        starmap_id: QString,
        instance_id: QString,
        patch_json: QString,
    ) -> QJsonObject {
        let sid = starmap_id.to_string();
        let iid = instance_id.to_string();
        let raw = self
            .update_starmap_embed_json(starmap_id, instance_id, patch_json)
            .to_string();
        Self::log_starmap_envelope("update_starmap_embed", &sid, &iid, &raw);
        crate::backend::json_utils::qjson_object_from_json(&raw)
    }
    fn delete_starmap_embed_json(&mut self, starmap_id: QString, instance_id: QString) -> QString {
        self.with_app_mut(|app| app.delete_starmap_embed_json(starmap_id, instance_id))
            .unwrap_or_else(|_| {
                QString::from(crate::backend::json_utils::borrow_conflict_error_json())
            })
    }
    fn delete_starmap_embed(&mut self, starmap_id: QString, instance_id: QString) -> QJsonObject {
        let sid = starmap_id.to_string();
        let iid = instance_id.to_string();
        let raw = self
            .delete_starmap_embed_json(starmap_id, instance_id)
            .to_string();
        Self::log_starmap_envelope("delete_starmap_embed", &sid, &iid, &raw);
        crate::backend::json_utils::qjson_object_from_json(&raw)
    }
    fn save_starmap_layout_json(&mut self, starmap_id: QString, layout_json: QString) -> QString {
        self.with_app_mut(|app| app.save_starmap_layout_json(starmap_id, layout_json))
            .unwrap_or_else(|_| {
                QString::from(crate::backend::json_utils::borrow_conflict_error_json())
            })
    }
    fn save_starmap_layout(&mut self, starmap_id: QString, layout_json: QString) -> QJsonObject {
        self.with_app_mut(|app| app.save_starmap_layout(starmap_id, layout_json))
            .unwrap_or_else(|_| {
                crate::backend::json_utils::qjson_object_from_json(
                    &crate::backend::json_utils::borrow_conflict_error_json(),
                )
            })
    }
    fn add_starmap_hyperlink(
        &mut self,
        starmap_id: QString,
        hyperlink_json: QString,
    ) -> QJsonObject {
        self.with_app_mut(|app| app.add_starmap_hyperlink(starmap_id, hyperlink_json))
            .unwrap_or_else(|_| {
                crate::backend::json_utils::qjson_object_from_json(
                    &crate::backend::json_utils::borrow_conflict_error_json(),
                )
            })
    }
    fn update_starmap_hyperlink(
        &mut self,
        starmap_id: QString,
        hyperlink_id: QString,
        patch_json: QString,
    ) -> QJsonObject {
        self.with_app_mut(|app| app.update_starmap_hyperlink(starmap_id, hyperlink_id, patch_json))
            .unwrap_or_else(|_| {
                crate::backend::json_utils::qjson_object_from_json(
                    &crate::backend::json_utils::borrow_conflict_error_json(),
                )
            })
    }
    fn delete_starmap_hyperlink(
        &mut self,
        starmap_id: QString,
        hyperlink_id: QString,
    ) -> QJsonObject {
        self.with_app_mut(|app| app.delete_starmap_hyperlink(starmap_id, hyperlink_id))
            .unwrap_or_else(|_| {
                crate::backend::json_utils::qjson_object_from_json(
                    &crate::backend::json_utils::borrow_conflict_error_json(),
                )
            })
    }
    fn list_starmap_hyperlinks(&self, starmap_id: QString) -> QJsonObject {
        self.with_app(|app| app.list_starmap_hyperlinks(starmap_id))
            .unwrap_or_else(|_| {
                crate::backend::json_utils::qjson_object_from_json(
                    &crate::backend::json_utils::borrow_conflict_error_json(),
                )
            })
    }
}

// 原本内联在 `impl AppBackend`（定义在 app_backend.rs）里的星图领域方法，
// 按关注点拆成三个子模块：
//   - documents.rs：星图本体 CRUD + 作品绑定 / 主星图
//   - graph.rs：graph 读取、节点与边增删改、坐标布局落盘
//   - hyperlinks.rs：超链接增删改查
// 本文件保留 QObject 桥接层（`StarMapBackend` 的 qt_method 实现）与共享的
// with_app / with_app_mut / log_starmap_envelope 工具方法。
// 本模块在 app_backend.rs 里是用 `#[path = "starmap_backend.rs"]` 声明的，
// 这种声明下子模块默认被解析到 backend/ 同级目录，所以这里必须显式写 #[path]。
#[path = "starmap_backend/documents.rs"]
mod documents;
#[path = "starmap_backend/graph.rs"]
mod graph;
#[path = "starmap_backend/hyperlinks.rs"]
mod hyperlinks;
