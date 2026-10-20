use std::path::Path;

use crate::api::service::{ApiResult, WriterCoreApi};
use crate::api::types::{
    CorrectionChapterSnapshotDto, CorrectionIgnoreScopeDto, CorrectionModelPathStatusDto,
    CorrectionResourceStatusDto,
};
use crate::correction::{CorrectionScanner, CorrectionStore};

impl WriterCoreApi {
    fn correction_store(&self) -> ApiResult<CorrectionStore> {
        CorrectionStore::open(&self.app_data_root)
    }

    pub(crate) fn correction_scanner(
        &self,
        project_id: String,
        chapter_id: String,
        session_id: u64,
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
            session_id,
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
        session_id: u64,
        text_revision: u64,
        text: String,
        start_byte: u32,
        end_byte: u32,
    ) -> ApiResult<CorrectionChapterSnapshotDto> {
        self.correction_scanner(project_id, chapter_id, session_id, text_revision)?
            .ensure_checked(&text, start_byte, end_byte)
    }

    /// Cached chapter snapshot for instant restore; never scans.
    pub fn correction_chapter_snapshot(
        &self,
        project_id: String,
        chapter_id: String,
        session_id: u64,
        text_revision: u64,
        text: String,
    ) -> ApiResult<CorrectionChapterSnapshotDto> {
        self.correction_scanner(project_id, chapter_id, session_id, text_revision)?
            .chapter_snapshot(&text)
    }

    /// Checks every missing or invalid segment and returns the merged snapshot.
    pub fn check_correction_chapter(
        &self,
        project_id: String,
        chapter_id: String,
        session_id: u64,
        text_revision: u64,
        text: String,
    ) -> ApiResult<CorrectionChapterSnapshotDto> {
        self.correction_scanner(project_id, chapter_id, session_id, text_revision)?
            .check_chapter(&text)
    }

    /// Applies the editor transaction's real change ranges and scans only the
    /// dirty canonical segments, returning the merged chapter snapshot.
    #[allow(clippy::too_many_arguments)]
    pub fn check_correction_edits(
        &self,
        project_id: String,
        chapter_id: String,
        session_id: u64,
        text_revision: u64,
        base_revision: u64,
        text: String,
        edits: Vec<crate::api::types::CorrectionEditRangeDto>,
    ) -> ApiResult<CorrectionChapterSnapshotDto> {
        let edits = edits
            .into_iter()
            .map(|edit| {
                Ok(crate::correction::EditRange {
                    old_start: usize::try_from(edit.old_start_byte)
                        .map_err(|error| crate::api::WriterError::Other(error.to_string()))?,
                    old_end: usize::try_from(edit.old_end_byte)
                        .map_err(|error| crate::api::WriterError::Other(error.to_string()))?,
                    new_start: usize::try_from(edit.new_start_byte)
                        .map_err(|error| crate::api::WriterError::Other(error.to_string()))?,
                    new_end: usize::try_from(edit.new_end_byte)
                        .map_err(|error| crate::api::WriterError::Other(error.to_string()))?,
                })
            })
            .collect::<Result<Vec<_>, crate::api::WriterError>>()?;
        self.correction_scanner(project_id, chapter_id, session_id, text_revision)?
            .apply_edits(&text, base_revision, edits)
    }
}
