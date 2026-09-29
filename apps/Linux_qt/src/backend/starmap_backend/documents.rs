//! 星图本体（documents）与作品绑定的 `AppBackend` 领域方法。
//!
//! 从 `starmap_backend.rs` 拆出：文档级 CRUD（列表 / 读取 / 新建 / 重命名 /
//! 删除）与作品绑定关系（绑定 / 解绑 / 主星图）是一组关注点，和节点、边、
//! 超链接的图元操作无关。所有方法都只是把 QString 参数转成字符串后转调
//! `starmap_bridge` 自由函数，真正的业务真相仍在 Core。

use super::*;

impl AppBackend {
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

    // AppBackend::list_root_starmaps_json
    /// 一级星图页：只列根星图（未被嵌入且非 legacy child）。
    pub(crate) fn list_root_starmaps_json(&self) -> QString {
        if let Some(core) = self.core_api() {
            starmap_bridge::list_root_starmaps_json(&core).into()
        } else {
            "[]".into()
        }
    }

    // AppBackend::list_root_starmaps
    pub(crate) fn list_root_starmaps(&self) -> QJsonArray {
        if let Some(core) = self.core_api() {
            qjson_array_data_from_json(&starmap_bridge::list_root_starmaps_json(&core))
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
    pub(crate) fn create_starmap_json(
        &mut self,
        title: QString,
        description: QString,
        accent_color: QString,
    ) -> QString {
        let t = title.to_string();
        let d = description.to_string();
        let ac = accent_color.to_string();
        let color_ref = if ac.is_empty() {
            None
        } else {
            Some(ac.as_str())
        };
        if let Some(core) = self.core_api() {
            starmap_bridge::create_starmap(&core, &t, &d, color_ref).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::create_starmap
    pub(crate) fn create_starmap(
        &mut self,
        title: QString,
        description: QString,
        accent_color: QString,
    ) -> QJsonObject {
        let raw = self
            .create_starmap_json(title, description, accent_color)
            .to_string();
        qjson_object_from_json(&raw)
    }

    // AppBackend::rename_starmap_json
    pub(crate) fn rename_starmap_json(
        &mut self,
        starmap_id: QString,
        new_title: QString,
    ) -> QString {
        let sid = starmap_id.to_string();
        let t = new_title.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::rename_starmap(&core, &sid, &t).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::delete_starmap_json
    pub(crate) fn delete_starmap_json(&mut self, starmap_id: QString) -> QString {
        let sid = starmap_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::delete_starmap(&core, &sid).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::get_starmap_graph_json
    pub(crate) fn get_starmap_graph_json(&self, starmap_id: QString) -> QString {
        let sid = starmap_id.to_string();
        if let Some(core) = self.core_api() {
            match core.get_starmap_graph(&sid) {
                Ok(g) => {
                    // 真正有用的图快照日志：记录各类图元数量，便于排查"图空了""embed 丢失"等问题。
                    // StarMapGraphDto 同时持有 `links`（普通连线）和 `hyperlinks`（超链接）两个字段，
                    // 分别记录两者，避免把 hyperlinks 误当成 links 输出到日志。
                    log::debug!(
                        "starmap graph snapshot: id={} nodes={} edges={} embeds={} links={} hyperlinks={}",
                        sid,
                        g.nodes.len(),
                        g.edges.len(),
                        g.embeds.len(),
                        g.links.len(),
                        g.hyperlinks.len()
                    );
                    writer_core::api::ResultEnvelope::success(serde_json::json!({
                        "graph": g
                    }))
                    .to_json_string()
                    .into()
                }
                Err(e) => crate::backend::json_utils::envelope_error_json(
                    writer_core::api::WriterError::Other(e.to_string()),
                )
                .into(),
            }
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::get_starmap_graph
    pub(crate) fn get_starmap_graph(&self, starmap_id: QString) -> QJsonObject {
        let raw = self.get_starmap_graph_json(starmap_id).to_string();
        qjson_object_from_json(&raw)
    }

    // AppBackend::bind_starmap_to_project_json
    pub(crate) fn bind_starmap_to_project_json(
        &mut self,
        starmap_id: QString,
        project_id: QString,
    ) -> QString {
        let sid = starmap_id.to_string();
        let pid = project_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::bind_starmap_to_project(&core, &sid, &pid).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::set_main_starmap_json
    pub(crate) fn set_main_starmap_json(
        &mut self,
        starmap_id: QString,
        project_id: QString,
    ) -> QString {
        let sid = starmap_id.to_string();
        let pid = project_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::set_main_starmap(&core, &sid, &pid).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
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
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }
}
