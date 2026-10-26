use crate::editor::{EditorEditResult, EditorOperationKind};

use super::model::{AnnotationKind, AnnotationMutation, TextAnchor};

const ANCHOR_CONTEXT_CHARS: usize = 32;

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
    if original_len == 0 || removed_len >= original_len {
        updated.updated_at_ms = updated_at_ms;
        updated.deleted_at_ms = Some(updated_at_ms);
        return Ok(TextAnchorEdit::Deleted(updated));
    }

    updated.start_byte = map_boundary(anchor.start_byte, edits, Boundary::Start)?;
    updated.end_byte = map_boundary(anchor.end_byte, edits, Boundary::End)?;
    if updated.start_byte > updated.end_byte {
        return Err(invalid_range("mapped text anchor start exceeds end"));
    }
    if updated.start_byte != anchor.start_byte
        || updated.end_byte != anchor.end_byte
        || edits.iter().any(|edit| {
            if edit.old_start_byte == edit.old_end_byte {
                edit.old_start_byte > anchor.start_byte && edit.old_start_byte < anchor.end_byte
            } else {
                edit.old_start_byte < anchor.end_byte && edit.old_end_byte > anchor.start_byte
            }
        })
    {
        updated.updated_at_ms = updated_at_ms;
    }
    updated.deleted_at_ms = None;
    Ok(TextAnchorEdit::Updated(updated))
}

/// Validate a batch emitted by the editor transaction and compute the resulting
/// UTF-8 byte length without materializing either chapter string.
pub fn resulting_text_len(current_len: usize, edits: &[TextEditRange]) -> crate::Result<usize> {
    validate_edits(edits)?;
    let mut shift = 0i128;
    let mut previous_new_end = 0usize;
    for (index, edit) in edits.iter().enumerate() {
        if edit.old_end_byte > current_len {
            return Err(invalid_range(
                "edit range is outside the current chapter text",
            ));
        }
        if index > 0 && edit.new_start_byte < previous_new_end {
            return Err(invalid_range("resulting edit ranges overlap"));
        }
        let expected_new_start = (edit.old_start_byte as i128) + shift;
        let expected_new_start = usize::try_from(expected_new_start)
            .map_err(|_| invalid_range("resulting edit offset overflowed"))?;
        if expected_new_start != edit.new_start_byte {
            return Err(invalid_range(
                "resulting edit range does not match the preceding text edits",
            ));
        }
        let old_len = edit.old_end_byte - edit.old_start_byte;
        let new_len = edit.new_end_byte - edit.new_start_byte;
        shift += new_len as i128 - old_len as i128;
        previous_new_end = edit.new_end_byte;
    }
    let resulting_len = (current_len as i128) + shift;
    let resulting_len = usize::try_from(resulting_len)
        .map_err(|_| invalid_range("resulting chapter length overflowed"))?;
    if previous_new_end > resulting_len {
        return Err(invalid_range(
            "resulting edit range exceeds the resulting chapter text",
        ));
    }
    Ok(resulting_len)
}

/// Build store mutations from the editor's committed transaction. Callers can
/// apply the returned mutations and record the returned store paths in history.
pub fn map_anchors_after_editor_result(
    anchors: &[TextAnchor],
    result: &EditorEditResult,
    base_text: &str,
    new_text: &str,
    updated_at_ms: i64,
) -> crate::Result<Vec<AnnotationMutation>> {
    let edits = edit_ranges_from_result(result);
    if edits.is_empty() || base_text == new_text {
        return Ok(Vec::new());
    }
    let mut mutations = Vec::with_capacity(anchors.len());
    for anchor in anchors {
        let Some(anchor) = rebase_text_anchor(anchor, base_text)? else {
            continue;
        };
        match map_text_anchor_after_edits(&anchor, &edits, updated_at_ms)? {
            TextAnchorEdit::Updated(mut anchor) => {
                capture_text_anchor(&mut anchor, new_text)?;
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

/// Refresh the persisted identity for a verified range in the supplied text.
/// The digest covers the whole chapter; the quote and small context window are
/// retained only so a changed chapter can relocate the range safely.
pub fn capture_text_anchor(anchor: &mut TextAnchor, text: &str) -> crate::Result<()> {
    let text_hash = format!("{:x}", md5::compute(text.as_bytes()));
    capture_text_anchor_with_hash(anchor, text, &text_hash)
}

/// Capture an anchor using a chapter digest computed once by the caller.
pub fn capture_text_anchor_with_hash(
    anchor: &mut TextAnchor,
    text: &str,
    text_hash: &str,
) -> crate::Result<()> {
    if anchor.start_byte >= anchor.end_byte
        || anchor.end_byte > text.len()
        || !text.is_char_boundary(anchor.start_byte)
        || !text.is_char_boundary(anchor.end_byte)
    {
        return Err(invalid_range("text anchor is outside the chapter text"));
    }
    anchor.text_hash = text_hash.to_string();
    anchor.anchored_text = text[anchor.start_byte..anchor.end_byte].to_string();

    let before_start = text[..anchor.start_byte]
        .char_indices()
        .rev()
        .take(ANCHOR_CONTEXT_CHARS)
        .last()
        .map(|(index, _)| index)
        .unwrap_or(0);
    let after_end = text[anchor.end_byte..]
        .char_indices()
        .nth(ANCHOR_CONTEXT_CHARS)
        .map(|(index, _)| anchor.end_byte + index)
        .unwrap_or(text.len());
    anchor.context_before = text[before_start..anchor.start_byte].to_string();
    anchor.context_after = text[anchor.end_byte..after_end].to_string();
    Ok(())
}

/// Verify an anchor against the supplied chapter text or relocate it by its
/// saved quote and context. Ambiguous and legacy anchors are left unresolved;
/// callers must not display or mutate their historical offsets.
pub fn rebase_text_anchor(anchor: &TextAnchor, text: &str) -> crate::Result<Option<TextAnchor>> {
    let text_hash = format!("{:x}", md5::compute(text.as_bytes()));
    rebase_text_anchor_with_hash(anchor, text, &text_hash)
}

/// Rebase an anchor while reusing a chapter digest shared by all anchors in
/// the same snapshot.
pub fn rebase_text_anchor_with_hash(
    anchor: &TextAnchor,
    text: &str,
    text_hash: &str,
) -> crate::Result<Option<TextAnchor>> {
    if anchor.deleted_at_ms.is_some() || anchor.anchored_text.is_empty() {
        return Ok(None);
    }
    let range_is_valid = anchor.start_byte < anchor.end_byte
        && anchor.end_byte <= text.len()
        && text.is_char_boundary(anchor.start_byte)
        && text.is_char_boundary(anchor.end_byte);
    if range_is_valid
        && anchor.text_hash == text_hash
        && text[anchor.start_byte..anchor.end_byte] == anchor.anchored_text
    {
        return Ok(Some(anchor.clone()));
    }

    let mut candidates = Vec::new();
    let mut from = 0usize;
    while from < text.len() {
        let Some(relative_start) = text[from..].find(&anchor.anchored_text) else {
            break;
        };
        let start = from + relative_start;
        let end = start + anchor.anchored_text.len();
        if text.is_char_boundary(start) && text.is_char_boundary(end) {
            let before_score = anchor
                .context_before
                .chars()
                .rev()
                .zip(text[..start].chars().rev())
                .take_while(|(left, right)| left == right)
                .count();
            let after_score = anchor
                .context_after
                .chars()
                .zip(text[end..].chars())
                .take_while(|(left, right)| left == right)
                .count();
            candidates.push((start, end, before_score + after_score));
        }
        let next = text[start..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(1);
        from = start.saturating_add(next);
    }
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.2));
    let Some(&(start_byte, end_byte, best_score)) = candidates.first() else {
        return Ok(None);
    };
    if candidates.len() > 1 && best_score == candidates[1].2 {
        return Ok(None);
    }

    let mut rebased = anchor.clone();
    rebased.start_byte = start_byte;
    rebased.end_byte = end_byte;
    capture_text_anchor_with_hash(&mut rebased, text, text_hash)?;
    Ok(Some(rebased))
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
