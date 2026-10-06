//! 星图内部跳转链接（link）的 `AppBackend` 领域方法。
//!
//! 从 `starmap_backend.rs` 拆出：内部链接是独立于节点/边/超链接的第五类图元，
//! 表达图内二元关系（source + target 两条 StarMapTargetPath），带独立的增删改查
//! 与列表接口，单独成模块避免与图元操作混在一起。结构照 `hyperlinks.rs`。

use super::*;

impl AppBackend {
    // AppBackend::add_starmap_link_json
    pub(crate) fn add_starmap_link_json(
        &mut self,
        starmap_id: QString,
        link_json: QString,
    ) -> QString {
        let sid = starmap_id.to_string();
        let lj = link_json.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::add_starmap_link(&core, &sid, &lj).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::add_starmap_link
    pub(crate) fn add_starmap_link(
        &mut self,
        starmap_id: QString,
        link_json: QString,
    ) -> QJsonObject {
        let raw = self
            .add_starmap_link_json(starmap_id, link_json)
            .to_string();
        qjson_object_from_json(&raw)
    }

    // AppBackend::update_starmap_link_json
    pub(crate) fn update_starmap_link_json(
        &mut self,
        starmap_id: QString,
        link_id: QString,
        patch_json: QString,
    ) -> QString {
        let sid = starmap_id.to_string();
        let lid = link_id.to_string();
        let pj = patch_json.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::update_starmap_link(&core, &sid, &lid, &pj).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::update_starmap_link
    pub(crate) fn update_starmap_link(
        &mut self,
        starmap_id: QString,
        link_id: QString,
        patch_json: QString,
    ) -> QJsonObject {
        let raw = self
            .update_starmap_link_json(starmap_id, link_id, patch_json)
            .to_string();
        qjson_object_from_json(&raw)
    }

    // AppBackend::delete_starmap_link_json
    pub(crate) fn delete_starmap_link_json(
        &mut self,
        starmap_id: QString,
        link_id: QString,
    ) -> QString {
        let sid = starmap_id.to_string();
        let lid = link_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::delete_starmap_link(&core, &sid, &lid).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::delete_starmap_link
    pub(crate) fn delete_starmap_link(
        &mut self,
        starmap_id: QString,
        link_id: QString,
    ) -> QJsonObject {
        let raw = self
            .delete_starmap_link_json(starmap_id, link_id)
            .to_string();
        qjson_object_from_json(&raw)
    }

    // AppBackend::list_starmap_links_json
    pub(crate) fn list_starmap_links_json(&self, starmap_id: QString) -> QString {
        let sid = starmap_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::list_starmap_links(&core, &sid).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::list_starmap_links
    pub(crate) fn list_starmap_links(&self, starmap_id: QString) -> QJsonObject {
        let raw = self.list_starmap_links_json(starmap_id).to_string();
        qjson_object_from_json(&raw)
    }
}
