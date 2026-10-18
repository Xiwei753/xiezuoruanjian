use crate::api::{
    CorrectionIgnoreScopeDto, CorrectionSuggestionDto, EditorEditResultDto,
    EditorTransactionCauseDto, WriterError,
};

impl super::WriterAppService {
    pub fn set_correction_model_path(&self, path: String) -> Result<(), WriterError> {
        self.api.set_correction_model_path(path)
    }

    fn correction_session_kernel(
        &self,
        session_id: u64,
        project_id: &str,
        chapter_id: &str,
        expected_revision: u64,
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
        let actual_revision = session.kernel.revision();
        if actual_revision != expected_revision {
            return Err(WriterError::Other(format!(
                "stale correction request: expected revision {expected_revision}, current revision {actual_revision}"
            )));
        }
        Ok(session.kernel.clone())
    }

    fn correction_session_context(
        &self,
        session_id: u64,
        project_id: &str,
        chapter_id: &str,
        expected_revision: u64,
        start_byte: u32,
        end_byte: u32,
    ) -> Result<(String, u32), WriterError> {
        let kernel =
            self.correction_session_kernel(session_id, project_id, chapter_id, expected_revision)?;
        let start =
            usize::try_from(start_byte).map_err(|error| WriterError::Other(error.to_string()))?;
        let end =
            usize::try_from(end_byte).map_err(|error| WriterError::Other(error.to_string()))?;
        let byte_len = kernel.byte_len();
        if start >= end
            || end > byte_len
            || !kernel.is_char_boundary(start)
            || !kernel.is_char_boundary(end)
        {
            return Err(WriterError::Other(format!(
                "invalid correction byte range {start_byte}..{end_byte} for text of {byte_len} bytes"
            )));
        }

        // Copy just the active sentence and a small amount of neighboring context.
        // The editor rope lets this stay independent of the full chapter size.
        const CONTEXT_BYTES: usize = 256;
        let mut context_start = start.saturating_sub(CONTEXT_BYTES);
        while !kernel.is_char_boundary(context_start) {
            context_start -= 1;
        }
        let mut context_end = end.saturating_add(CONTEXT_BYTES).min(byte_len);
        while !kernel.is_char_boundary(context_end) {
            context_end -= 1;
        }
        let context = kernel.byte_slice(context_start, context_end).to_string();
        let context_start =
            u32::try_from(context_start).map_err(|error| WriterError::Other(error.to_string()))?;
        Ok((context, context_start))
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
    ) -> Result<Vec<CorrectionSuggestionDto>, WriterError> {
        let (text, context_start) = self.correction_session_context(
            session_id,
            &project_id,
            &chapter_id,
            text_revision,
            start_byte,
            end_byte,
        )?;
        self.api.check_correction_segment_with_base(
            project_id,
            chapter_id,
            text_revision,
            text,
            start_byte - context_start,
            end_byte - context_start,
            context_start,
        )
    }

    pub fn check_correction_chapter(
        &self,
        session_id: u64,
        project_id: String,
        chapter_id: String,
        text_revision: u64,
    ) -> Result<Vec<CorrectionSuggestionDto>, WriterError> {
        let kernel =
            self.correction_session_kernel(session_id, &project_id, &chapter_id, text_revision)?;
        let byte_len = kernel.byte_len();
        u32::try_from(byte_len).map_err(|error| {
            WriterError::Other(format!("chapter exceeds correction byte limit: {error}"))
        })?;
        if byte_len == 0 {
            return Ok(Vec::new());
        }

        let scanner = self
            .api
            .correction_scanner(project_id, chapter_id, text_revision)?;
        const CORE_CHUNK_BYTES: usize = 16 * 1024;
        const CONTEXT_BYTES: usize = 256;
        let mut suggestions = Vec::new();
        let mut core_start = 0usize;
        while core_start < byte_len {
            let mut core_end = core_start.saturating_add(CORE_CHUNK_BYTES).min(byte_len);
            while !kernel.is_char_boundary(core_end) {
                core_end -= 1;
            }

            let mut context_start = core_start.saturating_sub(CONTEXT_BYTES);
            while !kernel.is_char_boundary(context_start) {
                context_start -= 1;
            }
            let mut context_end = core_end.saturating_add(CONTEXT_BYTES).min(byte_len);
            while !kernel.is_char_boundary(context_end) {
                context_end -= 1;
            }
            let context = kernel.byte_slice(context_start, context_end).to_string();
            let context_len = u32::try_from(context.len())
                .map_err(|error| WriterError::Other(error.to_string()))?;
            let base = u32::try_from(context_start)
                .map_err(|error| WriterError::Other(error.to_string()))?;
            let chunk_suggestions = scanner.scan_range(&context, (0, context_len), base)?;
            let core_start_byte =
                u32::try_from(core_start).map_err(|error| WriterError::Other(error.to_string()))?;
            let core_end_byte =
                u32::try_from(core_end).map_err(|error| WriterError::Other(error.to_string()))?;
            suggestions.extend(chunk_suggestions.into_iter().filter(|suggestion| {
                suggestion.start_byte >= core_start_byte && suggestion.start_byte < core_end_byte
            }));
            core_start = usize::try_from(core_end_byte)
                .map_err(|error| WriterError::Other(error.to_string()))?;
        }
        Ok(suggestions)
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
