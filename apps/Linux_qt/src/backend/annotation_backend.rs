//! Qt adapter for Core's shared tags, text anchors, named entries and links.
//!
//! All annotation state is owned by `WriterCoreApi`; this QObject only converts
//! JSON arguments/results for QML and never keeps a second copy of that state.

use qmetaobject::prelude::*;
use qmetaobject::QString;
use serde::{de::DeserializeOwned, Serialize};
use writer_core::api::types::{
    AnnotationEditDto, AnnotationObjectRefDto, AnnotationTargetDto, EditorTransactionCauseDto,
};
use writer_core::api::{ResultEnvelope, WriterCoreApi, WriterError};

use super::AppRef;

#[allow(non_snake_case)]
#[derive(QObject, Default)]
pub struct AnnotationBackend {
    base: qt_base_class!(trait QObject),
    list_tags_json: qt_method!(fn(&self, project_id: QString) -> QString),
    list_named_entries_json: qt_method!(fn(&self, project_id: QString) -> QString),
    create_tag_json: qt_method!(fn(&self, project_id: QString, name: QString, color_json: QString) -> QString),
    rename_tag_json: qt_method!(fn(&self, project_id: QString, tag_id: QString, name: QString) -> QString),
    delete_tag_json: qt_method!(fn(&self, project_id: QString, tag_id: QString) -> QString),
    merge_tags_json: qt_method!(fn(&self, project_id: QString, source_tag_id: QString, target_tag_id: QString) -> QString),
    set_tag_bindings_json: qt_method!(fn(&self, project_id: QString, target_json: QString, tag_ids_json: QString) -> QString),
    list_bindings_for_target_json: qt_method!(fn(&self, project_id: QString, target_json: QString) -> QString),
    create_text_anchor_json: qt_method!(fn(&self, project_id: QString, chapter_id: QString, start_byte: u32, end_byte: u32, text_revision: u64, text: QString) -> QString),
    create_named_entry_json: qt_method!(fn(&self, project_id: QString, canonical_name: QString, aliases_json: QString, misspellings_json: QString, auto_link_enabled: bool) -> QString),
    update_named_entry_json: qt_method!(fn(&self, project_id: QString, entry_id: QString, canonical_name: QString, aliases_json: QString, misspellings_json: QString, auto_link_enabled: bool) -> QString),
    delete_named_entry_json: qt_method!(fn(&self, project_id: QString, entry_id: QString) -> QString),
    create_link_json: qt_method!(fn(&self, source_json: QString, target_json: QString) -> QString),
    update_link_json: qt_method!(fn(&self, project_id: QString, link_id: QString, source_json: QString, target_json: QString) -> QString),
    delete_link_json: qt_method!(fn(&self, project_id: QString, link_id: QString) -> QString),
    resolve_link_json: qt_method!(fn(&self, target_json: QString) -> QString),
    find_backlinks_json: qt_method!(fn(&self, target_json: QString) -> QString),
    list_links_for_source_json: qt_method!(fn(&self, source_json: QString) -> QString),
    get_text_anchor_json: qt_method!(fn(&self, project_id: QString, anchor_id: QString) -> QString),
    get_chapter_annotations_json: qt_method!(fn(&self, project_id: QString, chapter_id: QString, text_revision: u64, text: QString) -> QString),
    apply_annotation_edits_json: qt_method!(fn(&self, project_id: QString, chapter_id: QString, baseline_id: QString, base_revision: u64, text_revision: u64, cause: QString, edits_json: QString, text: QString) -> QString),
    find_auto_name_links_json: qt_method!(fn(&self, project_id: QString, chapter_id: QString, text_revision: u64, text: QString) -> QString),
    app: AppRef,
}

impl AnnotationBackend {
    pub fn new(app: AppRef) -> Self {
        Self { app, ..Default::default() }
    }

    fn execute<T: Serialize>(&self, operation: impl FnOnce(&WriterCoreApi) -> Result<T, WriterError>) -> QString {
        let result = match self.app.with_app(|app| app.core_api()) {
            Ok(Some(api)) => operation(&api),
            Ok(None) => Err(WriterError::Other("请先打开工作区".to_string())),
            Err(_) => Err(WriterError::Other("工作区状态暂不可用".to_string())),
        };
        ResultEnvelope::from_api_result(result)
            .to_json_string()
            .into()
    }

    fn decode<T: DeserializeOwned>(raw: &QString, label: &str) -> Result<T, WriterError> {
        serde_json::from_str(&raw.to_string())
            .map_err(|error| WriterError::Other(format!("{label} JSON 无效: {error}")))
    }

    fn decode_string_list(raw: &QString, label: &str) -> Result<Vec<String>, WriterError> {
        Self::decode(raw, label)
    }

    fn list_tags_json(&self, project_id: QString) -> QString {
        let project_id = project_id.to_string();
        self.execute(|api| api.list_tags(project_id.clone()))
    }

    fn list_named_entries_json(&self, project_id: QString) -> QString {
        let project_id = project_id.to_string();
        self.execute(|api| api.list_named_entries(project_id.clone()))
    }

    fn create_tag_json(&self, project_id: QString, name: QString, color_json: QString) -> QString {
        let project_id = project_id.to_string();
        let name = name.to_string();
        let color = Self::decode::<Option<String>>(&color_json, "color");
        match color {
            Ok(color) => self.execute(|api| api.create_tag(project_id.clone(), name.clone(), color)),
            Err(error) => ResultEnvelope::<serde_json::Value>::error(error).to_json_string().into(),
        }
    }

    fn rename_tag_json(&self, project_id: QString, tag_id: QString, name: QString) -> QString {
        let (project_id, tag_id, name) = (project_id.to_string(), tag_id.to_string(), name.to_string());
        self.execute(|api| api.rename_tag(project_id.clone(), tag_id.clone(), name.clone()))
    }

    fn delete_tag_json(&self, project_id: QString, tag_id: QString) -> QString {
        let (project_id, tag_id) = (project_id.to_string(), tag_id.to_string());
        self.execute(|api| api.delete_tag(project_id.clone(), tag_id.clone()))
    }

    fn merge_tags_json(&self, project_id: QString, source_tag_id: QString, target_tag_id: QString) -> QString {
        let (project_id, source_tag_id, target_tag_id) = (project_id.to_string(), source_tag_id.to_string(), target_tag_id.to_string());
        self.execute(|api| api.merge_tags(project_id.clone(), source_tag_id.clone(), target_tag_id.clone()))
    }

    fn set_tag_bindings_json(&self, project_id: QString, target_json: QString, tag_ids_json: QString) -> QString {
        let (project_id, target, tag_ids) = match (
            Self::decode::<AnnotationObjectRefDto>(&target_json, "target"),
            Self::decode_string_list(&tag_ids_json, "tagIds"),
        ) {
            (Ok(target), Ok(tag_ids)) => (project_id.to_string(), target, tag_ids),
            (Err(error), _) | (_, Err(error)) => return ResultEnvelope::<serde_json::Value>::error(error).to_json_string().into(),
        };
        self.execute(|api| api.set_tag_bindings(project_id.clone(), target, tag_ids))
    }

    fn list_bindings_for_target_json(&self, project_id: QString, target_json: QString) -> QString {
        let target = match Self::decode::<AnnotationObjectRefDto>(&target_json, "target") {
            Ok(target) => target,
            Err(error) => return ResultEnvelope::<serde_json::Value>::error(error).to_json_string().into(),
        };
        let project_id = project_id.to_string();
        self.execute(|api| api.list_bindings_for_target(project_id.clone(), target))
    }

    fn create_text_anchor_json(&self, project_id: QString, chapter_id: QString, start_byte: u32, end_byte: u32, text_revision: u64, text: QString) -> QString {
        let (project_id, chapter_id, text) = (project_id.to_string(), chapter_id.to_string(), text.to_string());
        self.execute(|api| api.create_text_anchor(project_id.clone(), chapter_id.clone(), start_byte, end_byte, text_revision, text.clone()))
    }

    fn create_named_entry_json(&self, project_id: QString, canonical_name: QString, aliases_json: QString, misspellings_json: QString, auto_link_enabled: bool) -> QString {
        let (aliases, misspellings) = match (Self::decode_string_list(&aliases_json, "aliases"), Self::decode_string_list(&misspellings_json, "misspellings")) {
            (Ok(aliases), Ok(misspellings)) => (aliases, misspellings),
            (Err(error), _) | (_, Err(error)) => return ResultEnvelope::<serde_json::Value>::error(error).to_json_string().into(),
        };
        let (project_id, canonical_name) = (project_id.to_string(), canonical_name.to_string());
        self.execute(|api| api.create_named_entry(project_id.clone(), canonical_name.clone(), aliases, misspellings, auto_link_enabled))
    }

    fn update_named_entry_json(&self, project_id: QString, entry_id: QString, canonical_name: QString, aliases_json: QString, misspellings_json: QString, auto_link_enabled: bool) -> QString {
        let (aliases, misspellings) = match (Self::decode_string_list(&aliases_json, "aliases"), Self::decode_string_list(&misspellings_json, "misspellings")) {
            (Ok(aliases), Ok(misspellings)) => (aliases, misspellings),
            (Err(error), _) | (_, Err(error)) => return ResultEnvelope::<serde_json::Value>::error(error).to_json_string().into(),
        };
        let (project_id, entry_id, canonical_name) = (project_id.to_string(), entry_id.to_string(), canonical_name.to_string());
        self.execute(|api| api.update_named_entry(project_id.clone(), entry_id.clone(), canonical_name.clone(), aliases, misspellings, auto_link_enabled))
    }

    fn delete_named_entry_json(&self, project_id: QString, entry_id: QString) -> QString {
        let (project_id, entry_id) = (project_id.to_string(), entry_id.to_string());
        self.execute(|api| api.delete_named_entry(project_id.clone(), entry_id.clone()))
    }

    fn create_link_json(&self, source_json: QString, target_json: QString) -> QString {
        let (source, target) = match (Self::decode::<AnnotationObjectRefDto>(&source_json, "source"), Self::decode::<AnnotationTargetDto>(&target_json, "target")) {
            (Ok(source), Ok(target)) => (source, target),
            (Err(error), _) | (_, Err(error)) => return ResultEnvelope::<serde_json::Value>::error(error).to_json_string().into(),
        };
        self.execute(|api| api.create_link(source, target))
    }

    fn update_link_json(&self, project_id: QString, link_id: QString, source_json: QString, target_json: QString) -> QString {
        let (source, target) = match (Self::decode::<AnnotationObjectRefDto>(&source_json, "source"), Self::decode::<AnnotationTargetDto>(&target_json, "target")) {
            (Ok(source), Ok(target)) => (source, target),
            (Err(error), _) | (_, Err(error)) => return ResultEnvelope::<serde_json::Value>::error(error).to_json_string().into(),
        };
        let (project_id, link_id) = (project_id.to_string(), link_id.to_string());
        self.execute(|api| api.update_link(project_id.clone(), link_id.clone(), source, target))
    }

    fn delete_link_json(&self, project_id: QString, link_id: QString) -> QString {
        let (project_id, link_id) = (project_id.to_string(), link_id.to_string());
        self.execute(|api| api.delete_link(project_id.clone(), link_id.clone()))
    }

    fn resolve_link_json(&self, target_json: QString) -> QString {
        let target = match Self::decode::<AnnotationTargetDto>(&target_json, "target") {
            Ok(target) => target,
            Err(error) => return ResultEnvelope::<serde_json::Value>::error(error).to_json_string().into(),
        };
        self.execute(|api| api.resolve_link(target))
    }

    fn find_backlinks_json(&self, target_json: QString) -> QString {
        let target = match Self::decode::<AnnotationTargetDto>(&target_json, "target") {
            Ok(target) => target,
            Err(error) => return ResultEnvelope::<serde_json::Value>::error(error).to_json_string().into(),
        };
        self.execute(|api| api.find_backlinks(target))
    }

    fn list_links_for_source_json(&self, source_json: QString) -> QString {
        let source = match Self::decode::<AnnotationObjectRefDto>(&source_json, "source") {
            Ok(source) => source,
            Err(error) => return ResultEnvelope::<serde_json::Value>::error(error).to_json_string().into(),
        };
        self.execute(|api| api.list_links_for_source(source))
    }

    fn get_text_anchor_json(&self, project_id: QString, anchor_id: QString) -> QString {
        let (project_id, anchor_id) = (project_id.to_string(), anchor_id.to_string());
        self.execute(|api| api.get_text_anchor(project_id.clone(), anchor_id.clone()))
    }

    fn get_chapter_annotations_json(&self, project_id: QString, chapter_id: QString, text_revision: u64, text: QString) -> QString {
        let (project_id, chapter_id, text) = (project_id.to_string(), chapter_id.to_string(), text.to_string());
        self.execute(|api| api.get_chapter_annotations(project_id.clone(), chapter_id.clone(), text_revision, text.clone()))
    }

    fn apply_annotation_edits_json(&self, project_id: QString, chapter_id: QString, baseline_id: QString, base_revision: u64, text_revision: u64, cause: QString, edits_json: QString, text: QString) -> QString {
        let edits = match Self::decode::<Vec<AnnotationEditDto>>(&edits_json, "edits") {
            Ok(edits) => edits,
            Err(error) => return ResultEnvelope::<serde_json::Value>::error(error).to_json_string().into(),
        };
        let cause = match cause.to_string().as_str() {
            "Typing" => EditorTransactionCauseDto::Typing,
            "Delete" => EditorTransactionCauseDto::Delete,
            "ImeComposition" => EditorTransactionCauseDto::ImeComposition,
            "TypingCommit" => EditorTransactionCauseDto::TypingCommit,
            "Paste" => EditorTransactionCauseDto::Paste,
            "Undo" => EditorTransactionCauseDto::Undo,
            "Redo" => EditorTransactionCauseDto::Redo,
            "Load" => EditorTransactionCauseDto::Load,
            "Format" => EditorTransactionCauseDto::Format,
            "Programmatic" => EditorTransactionCauseDto::Programmatic,
            value => return ResultEnvelope::<serde_json::Value>::error(
                WriterError::Other(format!("未知编辑事务来源: {value}"))
            ).to_json_string().into(),
        };
        let (project_id, chapter_id, baseline_id, text) = (
            project_id.to_string(), chapter_id.to_string(), baseline_id.to_string(), text.to_string()
        );
        self.execute(|api| api.apply_annotation_edits(
            project_id.clone(), chapter_id.clone(), baseline_id.clone(),
            base_revision, text_revision, cause, edits, text.clone()
        ))
    }

    fn find_auto_name_links_json(&self, project_id: QString, chapter_id: QString, text_revision: u64, text: QString) -> QString {
        let (project_id, chapter_id, text) = (project_id.to_string(), chapter_id.to_string(), text.to_string());
        self.execute(|api| api.find_auto_name_links(project_id.clone(), chapter_id.clone(), text_revision, text.clone()))
    }
}
