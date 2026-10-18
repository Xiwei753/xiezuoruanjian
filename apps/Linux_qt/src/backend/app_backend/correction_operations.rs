//! Linux Qt proofreading adapter.
//!
//! The editor remains owned by its existing `SujianEditorItem` Core kernel.
//! This module only sends immutable text snapshots to WriterCoreApi. QML maps
//! the returned UTF-8 byte-range endpoints onto Qt's UTF-16 string positions.

use super::*;
use writer_core::api::types::CorrectionIgnoreScopeDto;

const CORRECTION_DICTIONARY_FILES: [&str; 8] = [
    "word_freq.txt",
    "same_pinyin.txt",
    "same_stroke.txt",
    "custom_confusion.txt",
    "person_name.txt",
    "place_name.txt",
    "proper_name.txt",
    "ignored_words.txt",
];

pub(crate) type CorrectionQueryCallback =
    Box<dyn FnOnce(u32, u64, Result<String, String>) + Send + Sync>;

impl AppBackend {
    pub(crate) fn correction_dictionary_resource_status(
        &self,
    ) -> Result<Vec<(String, bool)>, String> {
        if !self.current_has_data_root || self.current_data_root.is_empty() {
            return Err("Core 尚未打开工作区".to_string());
        }
        let api = self
            .core_api()
            .ok_or_else(|| "Core 尚未打开工作区".to_string())?;
        let status = api
            .get_correction_resource_status()
            .map_err(|error| error.to_string())?;
        let mut resources = CORRECTION_DICTIONARY_FILES
            .iter()
            .map(|name| {
                let ready = status
                    .installed_dictionary_files
                    .iter()
                    .any(|installed| installed == name);
                ((*name).to_string(), ready)
            })
            .collect::<Vec<_>>();
        resources.push(("内置基础词库".to_string(), status.bundled_lexicon_available));
        Ok(resources)
    }

    pub(crate) fn import_correction_dictionary(
        &self,
        source_path: std::path::PathBuf,
    ) -> Result<String, String> {
        if !self.current_has_data_root || self.current_data_root.is_empty() {
            return Err("Core 尚未打开工作区".to_string());
        }

        let source = source_path.as_path();
        let file_name = source
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "所选词库文件名无效".to_string())?;
        if !CORRECTION_DICTIONARY_FILES.contains(&file_name) {
            return Err(format!("不支持导入词库文件：{file_name}"));
        }

        let source = source
            .canonicalize()
            .map_err(|error| format!("无法读取所选词库文件: {error}"))?;
        if !source.is_file() {
            return Err("所选词库路径不是文件".to_string());
        }

        // This is the same data-root string passed to WriterCoreApi; Core reads
        // its correction resources from app_data_root/correction-data.
        let resource_root = std::path::Path::new(&self.current_data_root).join("correction-data");
        std::fs::create_dir_all(&resource_root)
            .map_err(|error| format!("无法创建校对词库目录: {error}"))?;
        let destination = resource_root.join(file_name);

        if let Ok(metadata) = std::fs::symlink_metadata(&destination) {
            if metadata.file_type().is_symlink() {
                return Err("目标词库文件是符号链接，已取消覆盖".to_string());
            }
            if !metadata.is_file() {
                return Err("目标词库路径不是普通文件".to_string());
            }
            if destination.canonicalize().ok().as_deref() == Some(source.as_path()) {
                return Ok(file_name.to_string());
            }
        }

        std::fs::copy(&source, &destination)
            .map_err(|error| format!("复制词库文件失败: {error}"))?;
        Ok(file_name.to_string())
    }

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
