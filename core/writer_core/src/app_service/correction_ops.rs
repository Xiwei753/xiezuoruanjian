use crate::api::{
    CorrectionChapterSnapshotDto, CorrectionEditRangeDto, CorrectionIgnoreScopeDto,
    CorrectionModelPathStatusDto, CorrectionResourceStatusDto, EditorEditResultDto,
    EditorTransactionCauseDto, WriterError,
};

impl super::WriterAppService {
    pub fn set_correction_model_path(&self, path: String) -> Result<(), WriterError> {
        self.api.set_correction_model_path(path)
    }

    pub fn get_correction_resource_status(
        &self,
    ) -> Result<CorrectionResourceStatusDto, WriterError> {
        self.api.get_correction_resource_status()
    }

    pub fn get_correction_model_path_status(
        &self,
        path: String,
    ) -> Result<CorrectionModelPathStatusDto, WriterError> {
        self.api.get_correction_model_path_status(path)
    }

    fn correction_session_identity(
        &self,
        session_id: u64,
        project_id: &str,
        chapter_id: &str,
    ) -> Result<crate::editor::EditorKernel, WriterError> {
        let mut registry = self
            .session_registry
            .lock()
            .map_err(|_| WriterError::Other("editor session registry lock poisoned".into()))?;
        let session = registry
            .get_session_mut(crate::editor::TextEditSessionId::new(session_id))
            .ok_or_else(|| WriterError::Other("correction editor session is not active".into()))?;
        let target = session.target_id.split(':').collect::<Vec<_>>();
        if target.len() != 4
            || target[0] != "chapter-body"
            || target[1] != project_id
            || target[3] != chapter_id
        {
            return Err(WriterError::Other(
                "correction request does not match the active chapter session".into(),
            ));
        }
        Ok(session.kernel.clone())
    }

    fn correction_session_kernel(
        &self,
        session_id: u64,
        project_id: &str,
        chapter_id: &str,
        expected_revision: u64,
    ) -> Result<crate::editor::EditorKernel, WriterError> {
        let kernel = self.correction_session_identity(session_id, project_id, chapter_id)?;
        let actual_revision = kernel.revision();
        if actual_revision != expected_revision {
            return Err(WriterError::Other(format!(
                "stale correction request: expected revision {expected_revision}, current revision {actual_revision}"
            )));
        }
        Ok(kernel)
    }

    fn correction_session_target_revision(
        &self,
        session_id: u64,
        project_id: &str,
        chapter_id: &str,
        expected_revision: u64,
    ) -> Result<(), WriterError> {
        self.correction_session_kernel(session_id, project_id, chapter_id, expected_revision)?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn check_correction_segment(
        &self,
        session_id: u64,
        project_id: String,
        chapter_id: String,
        text_revision: u64,
        start_byte: u32,
        end_byte: u32,
    ) -> Result<CorrectionChapterSnapshotDto, WriterError> {
        let kernel =
            self.correction_session_kernel(session_id, &project_id, &chapter_id, text_revision)?;
        let start =
            usize::try_from(start_byte).map_err(|error| WriterError::Other(error.to_string()))?;
        let end =
            usize::try_from(end_byte).map_err(|error| WriterError::Other(error.to_string()))?;
        let byte_len = kernel.byte_len();
        if start > end
            || end > byte_len
            || !kernel.is_char_boundary(start)
            || !kernel.is_char_boundary(end)
        {
            return Err(WriterError::Other(format!(
                "invalid correction byte range {start_byte}..{end_byte} for text of {byte_len} bytes"
            )));
        }
        let text = kernel.snapshot_text();
        self.api.check_correction_segment(
            project_id,
            chapter_id,
            session_id,
            text_revision,
            text,
            start_byte,
            end_byte,
        )
    }

    /// Cached chapter snapshot for instant restore; never scans and never
    /// reruns the model for segments that are still valid.
    pub fn get_correction_chapter_snapshot(
        &self,
        session_id: u64,
        project_id: String,
        chapter_id: String,
        text_revision: u64,
    ) -> Result<CorrectionChapterSnapshotDto, WriterError> {
        let kernel =
            self.correction_session_kernel(session_id, &project_id, &chapter_id, text_revision)?;
        let text = kernel.snapshot_text();
        self.api.correction_chapter_snapshot(
            project_id,
            chapter_id,
            session_id,
            text_revision,
            text,
        )
    }

    pub fn check_correction_chapter(
        &self,
        session_id: u64,
        project_id: String,
        chapter_id: String,
        text_revision: u64,
    ) -> Result<CorrectionChapterSnapshotDto, WriterError> {
        let kernel =
            self.correction_session_kernel(session_id, &project_id, &chapter_id, text_revision)?;
        let text = kernel.snapshot_text();
        self.api
            .check_correction_chapter(project_id, chapter_id, session_id, text_revision, text)
    }

    /// Lightweight edit phase used by editors: records the transaction ranges
    /// without reading or fingerprinting the chapter. No model runs here.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_correction_edits(
        &self,
        session_id: u64,
        project_id: String,
        chapter_id: String,
        text_revision: u64,
        base_revision: u64,
        edits: Vec<CorrectionEditRangeDto>,
    ) -> Result<(), WriterError> {
        self.correction_session_identity(session_id, &project_id, &chapter_id)?;
        self.api.apply_correction_edits(
            project_id,
            chapter_id,
            session_id,
            text_revision,
            base_revision,
            edits,
        )
    }

    /// Model phase for the two-stage edit flow. The caller supplies the text
    /// captured with the newest transaction; the live kernel may be ahead.
    pub fn scan_correction_pending(
        &self,
        session_id: u64,
        project_id: String,
        chapter_id: String,
        text_revision: u64,
        text: String,
    ) -> Result<CorrectionChapterSnapshotDto, WriterError> {
        self.correction_session_identity(session_id, &project_id, &chapter_id)?;
        self.api
            .scan_correction_pending(project_id, chapter_id, session_id, text_revision, text)
    }

    /// Applies the actual edit transaction ranges and returns the merged
    /// chapter snapshot. Only segments touched by those edits run the model.
    ///
    /// `text` is captured together with the transaction by the platform. The
    /// kernel may already be at a newer revision when this queued request is
    /// processed; rejecting it would leave a hole that forces the next request
    /// to rebuild the whole index. The correction engine validates revisions
    /// and edits against this text, so only the session identity is checked
    /// against the live kernel here.
    #[allow(clippy::too_many_arguments)]
    pub fn check_correction_edits(
        &self,
        session_id: u64,
        project_id: String,
        chapter_id: String,
        text_revision: u64,
        base_revision: u64,
        text: String,
        edits: Vec<CorrectionEditRangeDto>,
    ) -> Result<CorrectionChapterSnapshotDto, WriterError> {
        self.correction_session_identity(session_id, &project_id, &chapter_id)?;
        if base_revision > text_revision {
            return Err(WriterError::Other(
                "correction edit base revision is newer than the current revision".into(),
            ));
        }
        self.api.check_correction_edits(
            project_id,
            chapter_id,
            session_id,
            text_revision,
            base_revision,
            text,
            edits,
        )
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
    ) -> Result<bool, WriterError> {
        self.api.ignore_correction(
            project_id,
            chapter_id,
            rule_id,
            original,
            scope,
            text_revision,
            start_byte,
            end_byte,
        )
    }

    pub fn add_correction_dictionary_entry(
        &self,
        project_id: String,
        entry: String,
    ) -> Result<bool, WriterError> {
        self.api.add_correction_dictionary_entry(project_id, entry)
    }

    pub fn add_correction_dictionary_entry_with_kind(
        &self,
        project_id: String,
        entry: String,
        kind: crate::api::CorrectionDictionaryEntryKindDto,
    ) -> Result<bool, WriterError> {
        self.api
            .add_correction_dictionary_entry_with_kind(project_id, entry, kind)
    }

    pub fn get_correction_dictionary_entries(
        &self,
        project_id: String,
    ) -> Result<Vec<crate::api::CorrectionDictionaryEntryDto>, WriterError> {
        self.api.get_correction_dictionary_entries(project_id)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn apply_correction(
        &self,
        session_id: u64,
        project_id: String,
        chapter_id: String,
        text_revision: u64,
        start_byte: u32,
        end_byte: u32,
        original: String,
        replacement: String,
    ) -> EditorEditResultDto {
        if self
            .correction_session_target_revision(session_id, &project_id, &chapter_id, text_revision)
            .is_err()
        {
            return EditorEditResultDto::stale_fallback();
        }
        self.text_edit_session_replace(
            session_id,
            start_byte,
            end_byte,
            replacement,
            original,
            EditorTransactionCauseDto::Programmatic,
            text_revision,
        )
    }
}
