//! Linux Qt proofreading adapter.
//!
//! The editor remains owned by its existing `SujianEditorItem` Core kernel.
//! This module only sends immutable text snapshots to WriterCoreApi. QML maps
//! the returned UTF-8 byte-range endpoints onto Qt's UTF-16 string positions.

use super::*;
use writer_core::api::types::CorrectionIgnoreScopeDto;

pub(crate) type CorrectionQueryCallback =
    Box<dyn FnOnce(u32, u64, Result<String, String>) + Send + Sync>;

impl AppBackend {
    pub(crate) fn set_correction_model_path(&self, path: String) -> Result<(), String> {
        let path = path.trim();
        if path.is_empty() {
            return Err("模型路径不能为空".to_string());
        }
        let api = self
            .core_api()
            .ok_or_else(|| "Core 尚未打开工作区".to_string())?;
        api.set_correction_model_path(path.to_string())
            .map_err(|error| error.to_string())
    }

    pub(crate) fn ignore_correction(
        &self,
        project_id: String,
        chapter_id: String,
        rule_id: String,
        original: String,
        scope: String,
        text_revision: u64,
        start_byte: u32,
        end_byte: u32,
    ) -> Result<bool, String> {
        let scope = match scope.as_str() {
            "once" => CorrectionIgnoreScopeDto::Once,
            "chapter" => CorrectionIgnoreScopeDto::Chapter,
            "rule" => CorrectionIgnoreScopeDto::Rule,
            _ => return Err("不支持的忽略范围".to_string()),
        };
        let api = self
            .core_api()
            .ok_or_else(|| "Core 尚未打开工作区".to_string())?;
        api.ignore_correction(
            project_id,
            chapter_id,
            rule_id,
            original,
            scope,
            text_revision,
            start_byte,
            end_byte,
        )
        .map_err(|error| error.to_string())
    }

    pub(crate) fn add_correction_dictionary_entry(
        &self,
        project_id: String,
        entry: String,
    ) -> Result<bool, String> {
        let api = self
            .core_api()
            .ok_or_else(|| "Core 尚未打开工作区".to_string())?;
        api.add_correction_dictionary_entry(project_id, entry)
            .map_err(|error| error.to_string())
    }

    /// Queue one whole-chapter proofreading request without blocking the Qt UI.
    ///
    /// Results carry the submitted revision. `EditorController` drops them if
    /// the active chapter or the `SujianEditorItem` revision has since changed.
    pub(crate) fn request_correction_chapter(
        &self,
        request_id: u32,
        project_id: String,
        chapter_id: String,
        text_revision: u64,
        text: String,
        completion: CorrectionQueryCallback,
    ) -> Result<(), String> {
        let (data_root, projects_root, layout) =
            if self.current_has_data_root && !self.current_data_root.is_empty() {
                let layout = self
                    .current_workspace_git_layout
                    .clone()
                    .ok_or_else(|| "当前工作区缺少 Core 存储布局".to_string())?;
                (
                    self.current_data_root.clone(),
                    self.current_projects_root.clone(),
                    layout,
                )
            } else {
                return Err("Core 尚未打开工作区".to_string());
            };

        std::thread::Builder::new()
            .name("correction-check".to_string())
            .spawn(move || {
                let api = with_layout_core_api(&data_root, &projects_root, &layout);
                // The Core API receives an owned snapshot so model loading and
                // chapter scanning never block typing, layout, or saving.
                let result = api
                    .check_correction_chapter(project_id, chapter_id, text_revision, text)
                    .map_err(|error| error.to_string())
                    .and_then(|suggestions| {
                        serde_json::to_string(&suggestions)
                            .map_err(|error| format!("无法编码校对结果: {error}"))
                    });
                completion(request_id, text_revision, result);
            })
            .map(|_| ())
            .map_err(|error| format!("无法启动校对任务: {error}"))
    }
}
