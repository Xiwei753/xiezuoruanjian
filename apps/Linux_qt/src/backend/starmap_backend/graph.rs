//! 星图图元（graph / 节点 / 边）的 `AppBackend` 领域方法。
//!
//! 从 `starmap_backend.rs` 拆出：图的读取、节点与边的增删改属于同一组图元操作。
//! 布局算法本身在 `crate::starmap_view`（Linux 平台端），Core 只保存节点坐标。

use super::*;

impl AppBackend {
    // AppBackend::create_starmap_node_json
    pub(crate) fn create_starmap_node_json(
        &mut self,
        starmap_id: QString,
        title: QString,
        kind: QString,
        x: f64,
        y: f64,
    ) -> QString {
        let sid = starmap_id.to_string();
        let t = title.to_string();
        let k = kind.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::create_starmap_node(&core, &sid, &t, &k, x, y).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::update_starmap_node_json
    pub(crate) fn update_starmap_node_json(
        &mut self,
        starmap_id: QString,
        node_id: QString,
        patch_json: QString,
    ) -> QString {
        let sid = starmap_id.to_string();
        let nid = node_id.to_string();
        let p = patch_json.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::update_starmap_node(&core, &sid, &nid, &p).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::delete_starmap_node_json
    pub(crate) fn delete_starmap_node_json(
        &mut self,
        starmap_id: QString,
        node_id: QString,
    ) -> QString {
        let sid = starmap_id.to_string();
        let nid = node_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::delete_starmap_node(&core, &sid, &nid).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::resolve_starmap_path_json
    /// 按 `root starmap + 路径段` 解析当前层星图（无级星图套星图的加载入口）。
    pub(crate) fn resolve_starmap_path_json(
        &self,
        root_starmap_id: QString,
        segments_json: QString,
    ) -> QString {
        let root = root_starmap_id.to_string();
        let segs = segments_json.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::resolve_starmap_path(&core, &root, &segs).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::resolve_starmap_path
    pub(crate) fn resolve_starmap_path(
        &self,
        root_starmap_id: QString,
        segments_json: QString,
    ) -> QJsonObject {
        let raw = self
            .resolve_starmap_path_json(root_starmap_id, segments_json)
            .to_string();
        qjson_object_from_json(&raw)
    }

    // -------------------------------------------------------------------------
    // 星图导航路由（独立画布）— Issue #902 / #929
    // -------------------------------------------------------------------------

    // AppBackend::resolve_starmap_navigation_json
    /// 按 root + segments 定位当前活动画布（独立画布协议，Core 是路由唯一真相）。
    pub(crate) fn resolve_starmap_navigation_json(
        &self,
        root_starmap_id: QString,
        segments_json: QString,
    ) -> QString {
        let root = root_starmap_id.to_string();
        let segs = segments_json.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::resolve_starmap_navigation(&core, &root, &segs).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::resolve_starmap_navigation
    pub(crate) fn resolve_starmap_navigation(
        &self,
        root_starmap_id: QString,
        segments_json: QString,
    ) -> QJsonObject {
        let raw = self
            .resolve_starmap_navigation_json(root_starmap_id, segments_json)
            .to_string();
        qjson_object_from_json(&raw)
    }

    // AppBackend::enter_starmap_embed_json
    /// 从当前 route 追加一个 Embed 段进入子画布，返回 EnterStarmapResultDto。
    pub(crate) fn enter_starmap_embed_json(
        &self,
        root_starmap_id: QString,
        segments_json: QString,
        instance_id: QString,
    ) -> QString {
        let root = root_starmap_id.to_string();
        let segs = segments_json.to_string();
        let iid = instance_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::enter_starmap_embed(&core, &root, &segs, &iid).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::enter_starmap_embed
    pub(crate) fn enter_starmap_embed(
        &self,
        root_starmap_id: QString,
        segments_json: QString,
        instance_id: QString,
    ) -> QJsonObject {
        let raw = self
            .enter_starmap_embed_json(root_starmap_id, segments_json, instance_id)
            .to_string();
        qjson_object_from_json(&raw)
    }

    // AppBackend::exit_starmap_canvas_json
    /// 弹出 route 最后一个段回到父画布（纯路径操作）。
    pub(crate) fn exit_starmap_canvas_json(&self, route_json: QString) -> QString {
        let route = route_json.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::exit_starmap_canvas(&core, &route).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::exit_starmap_canvas
    pub(crate) fn exit_starmap_canvas(&self, route_json: QString) -> QJsonObject {
        let raw = self.exit_starmap_canvas_json(route_json).to_string();
        qjson_object_from_json(&raw)
    }

    // AppBackend::plan_cross_layer_connection_json
    /// 跨层连线宿主规划：两条从同一根星图出发的绝对端点路径交给 Core，
    /// 返回宿主星图、宿主 Scene 路径和两条相对宿主的端点路径。
    pub(crate) fn plan_cross_layer_connection_json(
        &self,
        from_path_json: QString,
        to_path_json: QString,
    ) -> QString {
        let fp = from_path_json.to_string();
        let tp = to_path_json.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::plan_cross_layer_connection(&core, &fp, &tp).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::plan_cross_layer_connection
    pub(crate) fn plan_cross_layer_connection(
        &self,
        from_path_json: QString,
        to_path_json: QString,
    ) -> QJsonObject {
        let raw = self
            .plan_cross_layer_connection_json(from_path_json, to_path_json)
            .to_string();
        qjson_object_from_json(&raw)
    }

    // AppBackend::create_starmap_edge_json
    pub(crate) fn create_starmap_edge_json(
        &mut self,
        starmap_id: QString,
        from_node_id: QString,
        to_node_id: QString,
        kind: QString,
        label: QString,
    ) -> QString {
        let sid = starmap_id.to_string();
        let from_id = from_node_id.to_string();
        let to_id = to_node_id.to_string();
        let k = kind.to_string();
        let l = label.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::create_starmap_edge(&core, &sid, &from_id, &to_id, &k, &l).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::create_starmap_edge_with_paths_json
    pub(crate) fn create_starmap_edge_with_paths_json(
        &mut self,
        starmap_id: QString,
        from_path_json: QString,
        to_path_json: QString,
        kind: QString,
        label: QString,
    ) -> QString {
        let sid = starmap_id.to_string();
        let fp = from_path_json.to_string();
        let tp = to_path_json.to_string();
        let k = kind.to_string();
        let l = label.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::create_starmap_edge_with_paths(&core, &sid, &fp, &tp, &k, &l).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::update_starmap_edge_json
    pub(crate) fn update_starmap_edge_json(
        &mut self,
        starmap_id: QString,
        edge_id: QString,
        patch_json: QString,
    ) -> QString {
        let sid = starmap_id.to_string();
        let eid = edge_id.to_string();
        let p = patch_json.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::update_starmap_edge(&core, &sid, &eid, &p).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::delete_starmap_edge_json
    pub(crate) fn delete_starmap_edge_json(
        &mut self,
        starmap_id: QString,
        edge_id: QString,
    ) -> QString {
        let sid = starmap_id.to_string();
        let eid = edge_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::delete_starmap_edge(&core, &sid, &eid).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // -------------------------------------------------------------------------
    // 星图子星图嵌入（embed）— Embed 是图元，放在 graph.rs
    // -------------------------------------------------------------------------

    // AppBackend::create_starmap_embed_json
    pub(crate) fn create_starmap_embed_json(
        &mut self,
        starmap_id: QString,
        target_starmap_id: QString,
        label: QString,
        x: f64,
        y: f64,
    ) -> QString {
        let sid = starmap_id.to_string();
        let tid = target_starmap_id.to_string();
        let l = label.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::create_starmap_embed(&core, &sid, &tid, &l, x, y).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::update_starmap_embed_json
    pub(crate) fn update_starmap_embed_json(
        &mut self,
        starmap_id: QString,
        instance_id: QString,
        patch_json: QString,
    ) -> QString {
        let sid = starmap_id.to_string();
        let iid = instance_id.to_string();
        let p = patch_json.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::update_starmap_embed(&core, &sid, &iid, &p).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::create_starmap_child_embed_json
    // 原子创建子星图并嵌入父图（Issue #805 评论 5907045450 第 5 部分）。
    // 替代 QML 侧 create_starmap -> create_starmap_embed -> 失败时 delete_starmap 拼接。
    pub(crate) fn create_starmap_child_embed_json(
        &mut self,
        host_starmap_id: QString,
        title: QString,
        x: f64,
        y: f64,
    ) -> QString {
        let hid = host_starmap_id.to_string();
        let t = title.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::create_starmap_child_embed(&core, &hid, &t, x, y).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::delete_starmap_embed_json
    pub(crate) fn delete_starmap_embed_json(
        &mut self,
        starmap_id: QString,
        instance_id: QString,
    ) -> QString {
        let sid = starmap_id.to_string();
        let iid = instance_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::delete_starmap_embed(&core, &sid, &iid).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }
}
