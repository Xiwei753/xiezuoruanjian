//! 星图超链接（hyperlink）的 `AppBackend` 领域方法。
//!
//! 从 `starmap_backend.rs` 拆出：超链接是独立于节点/边的第四类图元，
//! 带独立的增删改查与列表接口，单独成模块避免与图元操作混在一起。

use super::*;

impl AppBackend {
    // AppBackend::add_starmap_hyperlink_json
    pub(crate) fn add_starmap_hyperlink_json(
        &mut self,
        starmap_id: QString,
        hyperlink_json: QString,
    ) -> QString {
        let sid = starmap_id.to_string();
        let hj = hyperlink_json.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::add_starmap_hyperlink(&core, &sid, &hj).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::add_starmap_hyperlink
    pub(crate) fn add_starmap_hyperlink(
        &mut self,
        starmap_id: QString,
        hyperlink_json: QString,
    ) -> QJsonObject {
        let raw = self
            .add_starmap_hyperlink_json(starmap_id, hyperlink_json)
            .to_string();
        qjson_object_from_json(&raw)
    }

    // AppBackend::update_starmap_hyperlink_json
    pub(crate) fn update_starmap_hyperlink_json(
        &mut self,
        starmap_id: QString,
        hyperlink_id: QString,
        patch_json: QString,
    ) -> QString {
        let sid = starmap_id.to_string();
        let hid = hyperlink_id.to_string();
        let pj = patch_json.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::update_starmap_hyperlink(&core, &sid, &hid, &pj).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::update_starmap_hyperlink
    pub(crate) fn update_starmap_hyperlink(
        &mut self,
        starmap_id: QString,
        hyperlink_id: QString,
        patch_json: QString,
    ) -> QJsonObject {
        let raw = self
            .update_starmap_hyperlink_json(starmap_id, hyperlink_id, patch_json)
            .to_string();
        qjson_object_from_json(&raw)
    }

    // AppBackend::delete_starmap_hyperlink_json
    pub(crate) fn delete_starmap_hyperlink_json(
        &mut self,
        starmap_id: QString,
        hyperlink_id: QString,
    ) -> QString {
        let sid = starmap_id.to_string();
        let hid = hyperlink_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::delete_starmap_hyperlink(&core, &sid, &hid).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::delete_starmap_hyperlink
    pub(crate) fn delete_starmap_hyperlink(
        &mut self,
        starmap_id: QString,
        hyperlink_id: QString,
    ) -> QJsonObject {
        let raw = self
            .delete_starmap_hyperlink_json(starmap_id, hyperlink_id)
            .to_string();
        qjson_object_from_json(&raw)
    }

    // AppBackend::list_starmap_hyperlinks_json
    pub(crate) fn list_starmap_hyperlinks_json(&self, starmap_id: QString) -> QString {
        let sid = starmap_id.to_string();
        if let Some(core) = self.core_api() {
            starmap_bridge::list_starmap_hyperlinks(&core, &sid).into()
        } else {
            crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .into()
        }
    }

    // AppBackend::list_starmap_hyperlinks
    pub(crate) fn list_starmap_hyperlinks(&self, starmap_id: QString) -> QJsonObject {
        let raw = self.list_starmap_hyperlinks_json(starmap_id).to_string();
        qjson_object_from_json(&raw)
    }
}
