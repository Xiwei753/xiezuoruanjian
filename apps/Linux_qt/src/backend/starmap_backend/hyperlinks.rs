//! 星图超链接（hyperlink）的 `AppBackend` 领域方法。
//!
//! 从 `starmap_backend.rs` 拆出：超链接是独立于节点/边的第四类图元，
//! 带独立的增删改查与列表接口，单独成模块避免与图元操作混在一起。

use super::*;
use writer_core::api::types::StarMapHyperlinkDto;
use writer_core::api::WriterCoreApi;
use writer_core::search::extractor;
use writer_core::search::SearchIndexUpdate;

fn hyperlink_search_update(
    core: &WriterCoreApi,
    starmap_id: &str,
    hyperlink: StarMapHyperlinkDto,
) -> Option<SearchIndexUpdate> {
    let project_scope = AppBackend::starmap_project_scope(core, starmap_id)?;
    let entry = extractor::extract_starmap_hyperlink_entry(
        starmap_id,
        &hyperlink.hyperlink_id,
        project_scope.as_deref(),
        hyperlink.label.as_deref().unwrap_or(""),
        &hyperlink.target_uri,
    );
    Some(AppBackend::search_upsert_from_entry(entry))
}

impl AppBackend {
    // AppBackend::add_starmap_hyperlink_json
    pub(crate) fn add_starmap_hyperlink_json(
        &mut self,
        starmap_id: QString,
        hyperlink_json: QString,
    ) -> QString {
        let sid = starmap_id.to_string();
        let hj = hyperlink_json.to_string();
        let core = self.core_api();
        let raw = match core.as_ref() {
            Some(core) => starmap_bridge::add_starmap_hyperlink(core, &sid, &hj),
            None => crate::backend::json_utils::envelope_error_json(
                writer_core::api::WriterError::Other("core api not available".to_string()),
            ),
        };
        if Self::starmap_mutation_succeeded(&raw) {
            let updates = core.as_ref().and_then(|core| {
                let hyperlink = Self::starmap_mutation_data(&raw)
                    .and_then(|data| serde_json::from_value::<StarMapHyperlinkDto>(data).ok())?;
                hyperlink_search_update(core, &sid, hyperlink).map(|update| vec![update])
            });
            self.finish_starmap_search_update(&sid, updates);
        }
        raw.into()
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
        let core = self.core_api();
        let raw = match core.as_ref() {
            Some(core) => starmap_bridge::update_starmap_hyperlink(core, &sid, &hid, &pj),
            None => crate::backend::json_utils::envelope_error_json(
                writer_core::api::WriterError::Other("core api not available".to_string()),
            ),
        };
        if Self::starmap_mutation_succeeded(&raw) {
            let updates = core.as_ref().and_then(|core| {
                let hyperlink = Self::starmap_mutation_data(&raw)
                    .and_then(|data| serde_json::from_value::<StarMapHyperlinkDto>(data).ok())?;
                hyperlink_search_update(core, &sid, hyperlink).map(|update| vec![update])
            });
            self.finish_starmap_search_update(&sid, updates);
        }
        raw.into()
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
        expected_revision: u64,
    ) -> QString {
        let sid = starmap_id.to_string();
        let hid = hyperlink_id.to_string();
        let core = self.core_api();
        let raw = match core.as_ref() {
            Some(core) => {
                starmap_bridge::delete_starmap_hyperlink(core, &sid, &hid, Some(expected_revision))
            }
            None => crate::backend::json_utils::envelope_error_json(
                writer_core::api::WriterError::Other("core api not available".to_string()),
            ),
        };
        if Self::starmap_mutation_succeeded(&raw) {
            self.submit_search_updates(vec![Self::search_delete_update(format!(
                "starmap_hyperlink:{}:{}",
                sid, hid
            ))]);
        }
        raw.into()
    }

    // AppBackend::delete_starmap_hyperlink
    pub(crate) fn delete_starmap_hyperlink(
        &mut self,
        starmap_id: QString,
        hyperlink_id: QString,
        expected_revision: u64,
    ) -> QJsonObject {
        let raw = self
            .delete_starmap_hyperlink_json(starmap_id, hyperlink_id, expected_revision)
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
