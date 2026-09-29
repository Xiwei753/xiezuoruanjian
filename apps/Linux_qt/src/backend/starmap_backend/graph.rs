//! 星图图元（graph / 节点 / 边）与坐标布局落盘的 `AppBackend` 领域方法。
//!
//! 从 `starmap_backend.rs` 拆出：图的读取、节点与边的增删改、以及高频拖拽
//! 节点后的坐标批量落盘（`save_starmap_layout`）属于同一组图元操作。
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

    // AppBackend::save_starmap_layout_json
    pub(crate) fn save_starmap_layout_json(
        &mut self,
        starmap_id: QString,
        layout_json: QString,
    ) -> QString {
        let sid = starmap_id.to_string();
        let lj = layout_json.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::save_starmap_layout(&core, &sid, &lj).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::save_starmap_layout
    pub(crate) fn save_starmap_layout(
        &mut self,
        starmap_id: QString,
        layout_json: QString,
    ) -> QJsonObject {
        let raw = self
            .save_starmap_layout_json(starmap_id, layout_json)
            .to_string();
        qjson_object_from_json(&raw)
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
