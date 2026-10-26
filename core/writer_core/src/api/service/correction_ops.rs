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
        self.ensure_named_entries_migrated(&project_id)?;
        CorrectionScanner::new(
            self.correction_store()?,
            project_id,
            chapter_id,
            session_id,
            text_revision,
        )
    }

    /// Validate and save the user's local ONNX model resource directory. Scans
    /// share the process-cached model; WriterCore never packages the weights.
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
        self.add_correction_dictionary_entry_with_kind(
            project_id,
            entry,
            crate::api::CorrectionDictionaryEntryKindDto::Term,
        )
    }

    pub fn add_correction_dictionary_entry_with_kind(
        &self,
        project_id: String,
        entry: String,
        kind: crate::api::CorrectionDictionaryEntryKindDto,
    ) -> ApiResult<bool> {
        if project_id.trim().is_empty() {
            return Err(crate::api::WriterError::Other(
                "project id must be non-empty".into(),
            ));
        }
        if kind == crate::api::CorrectionDictionaryEntryKindDto::Term {
            return self.correction_store()?.add_dictionary_entry_with_kind(
                &project_id,
                &entry,
                kind,
            );
        }
        let entry = entry.trim();
        if !sujian_proof::is_valid_work_entity(entry) {
            return Err(crate::api::WriterError::Other(
                "人物/地点词条必须是完整的作品名称（不超过 64 个字符）".into(),
            ));
        }
        self.ensure_named_entries_migrated(&project_id)?;
        if self
            .annotation_store(&project_id)?
            .list_named_entries()
            .map_err(crate::api::WriterError::from)?
            .iter()
            .any(|named| named.canonical_name == entry)
        {
            return Ok(false);
        }
        self.create_named_entry(project_id, entry.to_string(), Vec::new(), Vec::new(), false)
            .map(|_| true)
    }

    pub fn get_correction_dictionary_entries(
        &self,
        project_id: String,
    ) -> ApiResult<Vec<crate::api::CorrectionDictionaryEntryDto>> {
        if project_id.trim().is_empty() {
            return Err(crate::api::WriterError::Other(
                "project id must be non-empty".into(),
            ));
        }
        self.ensure_named_entries_migrated(&project_id)?;
        let mut records = self
            .correction_store()?
            .dictionary_entry_records(&project_id)?
            .into_iter()
            .filter(|entry| entry.kind == crate::api::CorrectionDictionaryEntryKindDto::Term)
            .collect::<Vec<_>>();
        records.extend(
            self.annotation_store(&project_id)?
                .list_named_entries()
                .map_err(crate::api::WriterError::from)?
                .into_iter()
                .map(|entry| crate::api::CorrectionDictionaryEntryDto {
                    text: entry.canonical_name,
                    kind: crate::api::CorrectionDictionaryEntryKindDto::Person,
                }),
        );
        records.sort_by(|left, right| left.text.cmp(&right.text));
        Ok(records)
    }

    pub fn add_correction_entity_name(
        &self,
        project_id: String,
        canonical: String,
        name: String,
        kind: crate::api::CorrectionEntityNameKindDto,
    ) -> ApiResult<bool> {
        if project_id.trim().is_empty() {
            return Err(crate::api::WriterError::Other(
                "project id must be non-empty".into(),
            ));
        }
        self.ensure_named_entries_migrated(&project_id)?;
        let canonical = canonical.trim();
        let name = name.trim();
        if !sujian_proof::is_valid_work_entity(canonical)
            || !sujian_proof::is_valid_work_entity(name)
            || canonical == name
        {
            return Err(crate::api::WriterError::Other(
                "entity written form must be a complete name (at most 64 characters)".into(),
            ));
        }
        let entries = self
            .annotation_store(&project_id)?
            .list_named_entries()
            .map_err(crate::api::WriterError::from)?;
        let Some(entry) = entries
            .into_iter()
            .find(|entry| entry.canonical_name == canonical)
        else {
            let mut aliases = Vec::new();
            let mut misspellings = Vec::new();
            match kind {
                crate::api::CorrectionEntityNameKindDto::Alias => aliases.push(name.to_string()),
                crate::api::CorrectionEntityNameKindDto::Misspelling => {
                    misspellings.push(name.to_string())
                }
            }
            self.create_named_entry(
                project_id,
                canonical.to_string(),
                aliases,
                misspellings,
                false,
            )?;
            return Ok(true);
        };
        let duplicate = match kind {
            crate::api::CorrectionEntityNameKindDto::Alias => {
                entry.aliases.iter().any(|value| value == name)
                    || entry
                        .confirmed_misspellings
                        .iter()
                        .any(|value| value == name)
            }
            crate::api::CorrectionEntityNameKindDto::Misspelling => {
                entry
                    .confirmed_misspellings
                    .iter()
                    .any(|value| value == name)
                    || entry.aliases.iter().any(|value| value == name)
            }
        };
        if duplicate {
            return Ok(false);
        }
        let mut aliases = entry.aliases;
        let mut misspellings = entry.confirmed_misspellings;
        match kind {
            crate::api::CorrectionEntityNameKindDto::Alias => aliases.push(name.to_string()),
            crate::api::CorrectionEntityNameKindDto::Misspelling => {
                misspellings.push(name.to_string())
            }
        }
        self.update_named_entry(
            project_id,
            entry.entry_id,
            entry.canonical_name,
            aliases,
            misspellings,
            entry.auto_link_enabled,
        )?;
        Ok(true)
    }

    pub fn get_correction_entity_names(
        &self,
        project_id: String,
    ) -> ApiResult<Vec<crate::api::CorrectionEntityNameDto>> {
        if project_id.trim().is_empty() {
            return Err(crate::api::WriterError::Other(
                "project id must be non-empty".into(),
            ));
        }
        self.ensure_named_entries_migrated(&project_id)?;
        self.correction_store()?.entity_name_records(&project_id)
    }

    pub fn remove_correction_entity_name(
        &self,
        project_id: String,
        canonical: String,
        name: String,
    ) -> ApiResult<bool> {
        if project_id.trim().is_empty() {
            return Err(crate::api::WriterError::Other(
                "project id must be non-empty".into(),
            ));
        }
        self.ensure_named_entries_migrated(&project_id)?;
        let canonical = canonical.trim();
        let name = name.trim();
        let entries = self
            .annotation_store(&project_id)?
            .list_named_entries()
            .map_err(crate::api::WriterError::from)?;
        let Some(entry) = entries
            .into_iter()
            .find(|entry| entry.canonical_name == canonical)
        else {
            return Ok(false);
        };
        let original_alias_count = entry.aliases.len();
        let original_misspelling_count = entry.confirmed_misspellings.len();
        let aliases = entry
            .aliases
            .into_iter()
            .filter(|value| value != name)
            .collect::<Vec<_>>();
        let misspellings = entry
            .confirmed_misspellings
            .into_iter()
            .filter(|value| value != name)
            .collect::<Vec<_>>();
        if aliases.len() == original_alias_count && misspellings.len() == original_misspelling_count
        {
            return Ok(false);
        }
        self.update_named_entry(
            project_id,
            entry.entry_id,
            entry.canonical_name,
            aliases,
            misspellings,
            entry.auto_link_enabled,
        )?;
        Ok(true)
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

    /// Lightweight edit phase: update the chapter index without running the
    /// model or copying/fingerprinting the full text.
    #[allow(clippy::too_many_arguments)]
    pub fn apply_correction_edits(
        &self,
        project_id: String,
        chapter_id: String,
        session_id: u64,
        text_revision: u64,
        base_revision: u64,
        edits: Vec<crate::api::types::CorrectionEditRangeDto>,
    ) -> ApiResult<()> {
        if project_id.trim().is_empty() || chapter_id.trim().is_empty() {
            return Err(crate::api::WriterError::Other(
                "project id and chapter id must be non-empty".into(),
            ));
        }
        let edits = map_correction_edits(edits)?;
        self.correction_store()?.apply_correction_edits(
            &project_id,
            &chapter_id,
            session_id,
            text_revision,
            base_revision,
            &edits,
        )
    }

    /// Model phase for the two-stage edit flow: segment and scan only the
    /// windows recorded by [Self::apply_correction_edits].
    pub fn scan_correction_pending(
        &self,
        project_id: String,
        chapter_id: String,
        session_id: u64,
        text_revision: u64,
        text: String,
    ) -> ApiResult<CorrectionChapterSnapshotDto> {
        self.correction_scanner(project_id, chapter_id, session_id, text_revision)?
            .scan_pending(&text)
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
        let edits = map_correction_edits(edits)?;
        self.correction_scanner(project_id, chapter_id, session_id, text_revision)?
            .apply_edits(&text, base_revision, edits)
    }
}

fn map_correction_edits(
    edits: Vec<crate::api::types::CorrectionEditRangeDto>,
) -> ApiResult<Vec<crate::correction::EditRange>> {
    edits
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
        .collect()
}
