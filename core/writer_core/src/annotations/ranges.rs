use crate::editor::{EditorEditResult, EditorOperationKind};

use super::model::{AnnotationKind, AnnotationMutation, TextAnchor};

/// One committed edit expressed in the old and resulting UTF-8 byte spaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextEditRange {
    pub old_start_byte: usize,
    pub old_end_byte: usize,
    pub new_start_byte: usize,
    pub new_end_byte: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextAnchorEdit {
    Updated(TextAnchor),
    Deleted(TextAnchor),
}

/// Read byte-range edits from the exact editor transaction returned by Core.
/// `DisplayPatch` carries old and final coordinates for every delta, including
/// replace-all and undo/redo batches; no second text diff is performed here.
pub fn edit_ranges_from_result(result: &EditorEditResult) -> Vec<TextEditRange> {
    if !matches!(
        result.operation_kind,
        EditorOperationKind::Insert
            | EditorOperationKind::Delete
            | EditorOperationKind::Replace
            | EditorOperationKind::CompositionCommit
            | EditorOperationKind::Format
    ) {
        return Vec::new();
    }

    let mut edits: Vec<_> = result
        .display_patches
        .iter()
        .map(|patch| TextEditRange {
            old_start_byte: patch.replace_byte_range.start().value(),
            old_end_byte: patch.replace_byte_range.end().value(),
            new_start_byte: patch.inserted_byte_range.start().value(),
            new_end_byte: patch.inserted_byte_range.end().value(),
        })
        .collect();
    edits.sort_by_key(|edit| (edit.old_start_byte, edit.old_end_byte, edit.new_start_byte));
    edits
}

/// Map one anchor through a committed edit batch.
///
/// Surviving text keeps its range identity. Insertions at the start of an
/// anchor are treated as preceding text, insertions at its end as following
/// text, and insertions inside it expand the range. If every original byte in
/// the anchor was replaced or deleted, the anchor is tombstoned even if new
/// text was inserted at the same location.
pub fn map_text_anchor_after_edits(
    anchor: &TextAnchor,
    edits: &[TextEditRange],
    new_revision: u64,
    updated_at_ms: i64,
) -> crate::Result<TextAnchorEdit> {
    validate_edits(edits)?;
    if anchor.start_byte > anchor.end_byte {
        return Err(invalid_range("text anchor start exceeds end"));
    }

    let original_len = anchor.end_byte - anchor.start_byte;
    let removed_len = edits
        .iter()
        .filter(|edit| edit.old_start_byte < edit.old_end_byte)
        .map(|edit| {
            edit.old_end_byte
                .min(anchor.end_byte)
                .saturating_sub(edit.old_start_byte.max(anchor.start_byte))
        })
        .sum::<usize>();

    let mut updated = anchor.clone();
    updated.updated_at_ms = updated_at_ms;
    updated.text_revision = new_revision;
    if original_len == 0 || removed_len >= original_len {
        updated.deleted_at_ms = Some(updated_at_ms);
        return Ok(TextAnchorEdit::Deleted(updated));
    }

    updated.start_byte = map_boundary(anchor.start_byte, edits, Boundary::Start)?;
    updated.end_byte = map_boundary(anchor.end_byte, edits, Boundary::End)?;
    if updated.start_byte > updated.end_byte {
        return Err(invalid_range("mapped text anchor start exceeds end"));
    }
    updated.deleted_at_ms = None;
    Ok(TextAnchorEdit::Updated(updated))
}

/// Build store mutations from the editor's committed transaction. Callers can
/// apply the returned mutations and record the returned store paths in history.
pub fn map_anchors_after_editor_result(
    anchors: &[TextAnchor],
    result: &EditorEditResult,
    updated_at_ms: i64,
) -> crate::Result<Vec<AnnotationMutation>> {
    let edits = edit_ranges_from_result(result);
    if edits.is_empty() || result.base_revision == result.new_revision {
        return Ok(Vec::new());
    }
    let base_revision = result.base_revision.value();
    let new_revision = result.new_revision.value();
    let mut mutations = Vec::with_capacity(anchors.len());
    for anchor in anchors {
        if anchor.text_revision != base_revision {
            return Err(invalid_range(
                "text anchor revision does not match edit base",
            ));
        }
        match map_text_anchor_after_edits(anchor, &edits, new_revision, updated_at_ms)? {
            TextAnchorEdit::Updated(anchor) => {
                mutations.push(AnnotationMutation::UpsertTextAnchor(anchor));
            }
            TextAnchorEdit::Deleted(anchor) => mutations.push(AnnotationMutation::Delete {
                kind: AnnotationKind::TextAnchor,
                project_id: anchor.project_id,
                object_id: anchor.anchor_id,
                deleted_at_ms: updated_at_ms,
            }),
        }
    }
    Ok(mutations)
}

#[derive(Debug, Clone, Copy)]
enum Boundary {
    Start,
    End,
}

fn map_boundary(
    offset: usize,
    edits: &[TextEditRange],
    boundary: Boundary,
) -> crate::Result<usize> {
    let mut shift = 0i128;
    let mut index = 0;
    while index < edits.len() {
        let edit = edits[index];
        let start = edit.old_start_byte;
        let end = edit.old_end_byte;

        if start == end {
            let mut group_end = index + 1;
            let mut new_start = edit.new_start_byte;
            let mut new_end = edit.new_end_byte;
            while group_end < edits.len()
                && edits[group_end].old_start_byte == start
                && edits[group_end].old_end_byte == end
            {
                new_start = new_start.min(edits[group_end].new_start_byte);
                new_end = new_end.max(edits[group_end].new_end_byte);
                group_end += 1;
            }
            if offset > start {
                shift += (new_end - new_start) as i128;
            } else if offset == start {
                return match boundary {
                    Boundary::Start => Ok(new_end),
                    Boundary::End => Ok(new_start),
                };
            } else {
                break;
            }
            index = group_end;
            continue;
        }

        if offset < start {
            break;
        }
        if offset >= end {
            shift += (edit.new_end_byte as i128)
                - (edit.new_start_byte as i128)
                - ((end - start) as i128);
            index += 1;
            continue;
        }

        let mapped = match boundary {
            Boundary::Start => edit.new_start_byte,
            Boundary::End if offset == start => edit.new_start_byte,
            Boundary::End => edit.new_end_byte,
        };
        return Ok(mapped);
    }

    let mapped = (offset as i128) + shift;
    usize::try_from(mapped).map_err(|_| invalid_range("mapped text anchor offset overflowed"))
}

fn validate_edits(edits: &[TextEditRange]) -> crate::Result<()> {
    let mut previous_end = 0usize;
    let mut first = true;
    for edit in edits {
        if edit.old_start_byte > edit.old_end_byte || edit.new_start_byte > edit.new_end_byte {
            return Err(invalid_range("edit range start exceeds end"));
        }
        if !first && edit.old_start_byte < previous_end {
            return Err(invalid_range("edit ranges overlap"));
        }
        if edit.old_start_byte == edit.old_end_byte && edit.new_start_byte == edit.new_end_byte {
            // A zero-to-zero patch carries no text change and does not affect a range.
        }
        previous_end = previous_end.max(edit.old_end_byte);
        first = false;
    }
    Ok(())
}

fn invalid_range(message: &str) -> crate::error::Error {
    crate::error::Error::Other(message.to_string())
}
