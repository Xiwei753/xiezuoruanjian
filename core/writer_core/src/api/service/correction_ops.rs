use std::path::Path;

use crate::api::service::{ApiResult, WriterCoreApi};
use crate::api::types::{
    CorrectionIgnoreScopeDto, CorrectionModelPathStatusDto, CorrectionResourceStatusDto,
    CorrectionSuggestionDto,
};
use crate::correction::{scan_text, CorrectionScanner, CorrectionStore};

impl WriterCoreApi {
    fn correction_store(&self) -> ApiResult<CorrectionStore> {
        CorrectionStore::open(&self.app_data_root)
    }

    pub(crate) fn correction_scanner(
        &self,
        project_id: String,
        chapter_id: String,
        text_revision: u64,
    ) -> ApiResult<CorrectionScanner> {
        if project_id.trim().is_empty() || chapter_id.trim().is_empty() {
            return Err(crate::api::WriterError::Other(
                "project id and chapter id must be non-empty".into(),
            ));
        }
        CorrectionScanner::new(
            self.correction_store()?,
            project_id,
            chapter_id,
            text_revision,
        )
    }

    /// Validate and save the user's local KenLM model path. Scans share the
    /// process-cached model mapping; WriterCore never packages the model.
    pub fn set_correction_model_path(&self, path: String) -> ApiResult<()> {
        self.correction_store()?.set_model_path(Path::new(&path))
    }

    pub fn get_correction_resource_status(&self) -> ApiResult<CorrectionResourceStatusDto> {
        self.correction_store()?.resource_status()
    }

    pub fn get_correction_model_path_status(
        &self,
        path: String,
    ) -> ApiResult<CorrectionModelPathStatusDto> {
        self.correction_store()?.model_path_status(Path::new(&path))
    }

    pub fn add_correction_dictionary_entry(
        &self,
        project_id: String,
        entry: String,
    ) -> ApiResult<bool> {
        if project_id.trim().is_empty() {
            return Err(crate::api::WriterError::Other(
                "project id must be non-empty".into(),
            ));
        }
        self.correction_store()?
            .add_dictionary_entry(&project_id, &entry)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn ignore_correction(
        &self,
        project_id: String,
        chapter_id: String,
        rule_id: String,
        original: String,
        scope: CorrectionIgnoreScopeDto,
        text_revision: u64,
        start_byte: u32,
        end_byte: u32,
    ) -> ApiResult<bool> {
        if project_id.trim().is_empty() || chapter_id.trim().is_empty() || rule_id.trim().is_empty()
        {
            return Err(crate::api::WriterError::Other(
                "project id, chapter id, and correction rule id must be non-empty".into(),
            ));
        }
        self.correction_store()?.ignore(
            &project_id,
            &chapter_id,
            &rule_id,
            &original,
            scope,
            text_revision,
            start_byte,
            end_byte,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn check_correction_segment(
        &self,
        project_id: String,
        chapter_id: String,
        text_revision: u64,
        text: String,
        start_byte: u32,
        end_byte: u32,
    ) -> ApiResult<Vec<CorrectionSuggestionDto>> {
        self.check_correction_segment_with_base(
            project_id,
            chapter_id,
            text_revision,
            text,
            start_byte,
            end_byte,
            0,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn check_correction_segment_with_base(
        &self,
        project_id: String,
        chapter_id: String,
        text_revision: u64,
        text: String,
        start_byte: u32,
        end_byte: u32,
        offset_base: u32,
    ) -> ApiResult<Vec<CorrectionSuggestionDto>> {
        if project_id.trim().is_empty() || chapter_id.trim().is_empty() {
            return Err(crate::api::WriterError::Other(
                "project id and chapter id must be non-empty".into(),
            ));
        }
        scan_text(
            &self.correction_store()?,
            &project_id,
            &chapter_id,
            text_revision,
            &text,
            (start_byte, end_byte),
            offset_base,
        )
    }

    pub fn check_correction_chapter(
        &self,
        project_id: String,
        chapter_id: String,
        text_revision: u64,
        text: String,
    ) -> ApiResult<Vec<CorrectionSuggestionDto>> {
        if project_id.trim().is_empty() || chapter_id.trim().is_empty() {
            return Err(crate::api::WriterError::Other(
                "project id and chapter id must be non-empty".into(),
            ));
        }
        self.correction_scanner(project_id, chapter_id, text_revision)?
            .scan_chapter(&text)
    }
}
