//! Content-addressed, persistent correction result cache.
//!
//! Entries are keyed by the exact segment text and its neighboring context.
//! Resident entries also carry the byte offsets of the current text, but those
//! offsets are deliberately **not** persisted: inserting one character shifts
//! every later segment, and that pure shift must not rewrite the cache file.
//! The file holds derived data only: no chapter text backup, no editor state,
//! and nothing that participates in text persistence or sync.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Condvar, Mutex};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::api::WriterError;

/// On-disk layout version. A mismatch discards the file instead of guessing.
pub(crate) const SCHEMA_VERSION: u32 = 2;
/// Context bytes scanned around one segment, matching the live scan path.
pub(crate) const SEGMENT_CONTEXT_BYTES: usize = 256;
/// Hard ceiling for one segment. Long paragraphs are chunked below this size.
pub(crate) const SEGMENT_CHUNK_MAX_BYTES: usize = 16 * 1024;
/// Content-defined chunks never split below this size, so ordinary paragraphs
/// keep exactly one segment.
const SEGMENT_CHUNK_MIN_BYTES: usize = 4 * 1024;
/// Rolling window used for content-defined chunk boundaries.
const CHUNK_WINDOW_BYTES: usize = 64;
/// Chooses roughly one anchor every eight KiB, then caps the chunk at
/// [`SEGMENT_CHUNK_MAX_BYTES`].
const CHUNK_ANCHOR_MASK: u64 = (1 << 13) - 1;
const CHUNK_HASH_BASE: u64 = 0x0000_0100_0000_01b3;
/// `CHUNK_HASH_BASE^(CHUNK_WINDOW_BYTES - 1)`; used to evict the oldest byte.
const CHUNK_HASH_OLDEST: u64 = {
    let mut power = 1u64;
    let mut index = 1;
    while index < CHUNK_WINDOW_BYTES {
        power = power.wrapping_mul(CHUNK_HASH_BASE);
        index += 1;
    }
    power
};

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
/// Separator mixed into [`parts_fingerprint`] so part boundaries cannot alias.
const FNV_PART_SEPARATOR: u8 = 0x1f;

fn fnv1a(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(FNV_PRIME);
    }
}

/// Deterministic content digest. Cache identity must never depend on
/// `DefaultHasher`, whose seed changes per process.
pub(crate) fn content_fingerprint(text: &str) -> String {
    let mut hash = FNV_OFFSET_BASIS;
    fnv1a(&mut hash, text.as_bytes());
    format!("{hash:016x}")
}

/// Fingerprint a sequence of already-formatted parts.
pub(crate) fn parts_fingerprint(parts: &[String]) -> String {
    let mut hash = FNV_OFFSET_BASIS;
    for part in parts {
        fnv1a(&mut hash, &[FNV_PART_SEPARATOR]);
        fnv1a(&mut hash, part.as_bytes());
    }
    format!("{hash:016x}")
}

/// Stable file name for one chapter cache; raw ids never appear on disk.
pub(crate) fn cache_file_name(project_id: &str, chapter_id: &str) -> String {
    format!(
        "{}{}.json",
        content_fingerprint(project_id),
        content_fingerprint(chapter_id)
    )
}

/// Stable stamp for file metadata used in scanner fingerprints.
pub(crate) fn modified_stamp(modified: Option<SystemTime>) -> String {
    match modified {
        Some(modified) => match modified.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(duration) => duration.as_nanos().to_string(),
            Err(error) => format!("-{}", error.duration().as_nanos()),
        },
        None => "unknown".to_string(),
    }
}

/// One suggestion stored relative to its segment start.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CachedSuggestion {
    pub(crate) start_byte: u32,
    pub(crate) end_byte: u32,
    pub(crate) original: String,
    pub(crate) replacement: String,
    pub(crate) source: String,
    pub(crate) rule_id: String,
    pub(crate) severity: String,
}

/// One canonical segment in the current text, checked or not.
///
/// `start_byte`/`end_byte` are volatile. Persisted entries only keep the two
/// fingerprints and the relative suggestions, so shifting the text later never
/// dirties the file by itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CachedSegment {
    #[serde(skip)]
    pub(crate) start_byte: u32,
    #[serde(skip)]
    pub(crate) end_byte: u32,
    pub(crate) text_fingerprint: String,
    pub(crate) context_fingerprint: String,
    /// `Some` means checked. An empty vector means "checked, no suggestion".
    #[serde(skip)]
    pub(crate) checked: Option<Vec<CachedSuggestion>>,
    /// True when this request must scan the segment. Never persisted.
    #[serde(skip)]
    pub(crate) dirty: bool,
}

/// On-disk representation of one checked segment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedSegment {
    text_fingerprint: String,
    context_fingerprint: String,
    #[serde(default)]
    suggestions: Vec<CachedSuggestion>,
}

/// Persistent chapter cache file content.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChapterCorrectionCache {
    schema_version: u32,
    #[serde(default)]
    scanner_fingerprint: String,
    #[serde(default)]
    segments: Vec<PersistedSegment>,
}

/// In-memory cache state plus the file location for one chapter.
pub(crate) struct ChapterCorrectionCacheState {
    pub(crate) scanner_fingerprint: String,
    /// Offset-ordered segments covering the current text in memory.
    pub(crate) segments: Vec<CachedSegment>,
    /// True when the on-disk representation no longer matches memory.
    /// Pure offset shifts leave this false.
    pub(crate) semantic_dirty: bool,
    /// True while a model scan owns this chapter's single-flight slot.
    pub(crate) scanning: bool,
    /// Session/revision identity for the in-memory full text. Revisions from
    /// another session are never matched against each other.
    pub(crate) last_session_id: u64,
    pub(crate) last_revision: u64,
    /// Loaded entries have no offsets until the first full reconciliation.
    pub(crate) needs_reconcile: bool,
    /// Byte windows made dirty by lightweight edit transactions. They are
    /// re-segmented against the newest text by the next scan request, so a
    /// keystroke never has to copy or fingerprint the whole chapter.
    pub(crate) pending_windows: Vec<(usize, usize)>,
    pub(crate) path: PathBuf,
}

/// One chapter's cache plus the single-flight guard. Model scans run without
/// holding `state`; competing requests wait on `condvar`.
pub(crate) struct ChapterCorrectionSlot {
    pub(crate) state: Mutex<ChapterCorrectionCacheState>,
    pub(crate) condvar: Condvar,
}

impl ChapterCorrectionSlot {
    pub(crate) fn new(state: ChapterCorrectionCacheState) -> Self {
        Self {
            state: Mutex::new(state),
            condvar: Condvar::new(),
        }
    }
}

/// A half-open UTF-8 edit mapping. `old` addresses the previous text and
/// `new` addresses the text supplied with the edit request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EditRange {
    pub(crate) old_start: usize,
    pub(crate) old_end: usize,
    pub(crate) new_start: usize,
    pub(crate) new_end: usize,
}

impl ChapterCorrectionCacheState {
    /// Loads the persisted cache, or starts empty for a missing/corrupt file.
    pub(crate) fn load(path: PathBuf) -> Self {
        let mut state = Self {
            scanner_fingerprint: String::new(),
            segments: Vec::new(),
            semantic_dirty: false,
            scanning: false,
            last_session_id: 0,
            last_revision: 0,
            needs_reconcile: false,
            pending_windows: Vec::new(),
            path,
        };
        let content = match std::fs::read_to_string(&state.path) {
            Ok(content) => content,
            Err(error) => {
                if error.kind() != std::io::ErrorKind::NotFound {
                    log::warn!(
                        "[correction] result cache read failed error_kind={:?}",
                        error.kind()
                    );
                }
                return state;
            }
        };
        let cache = match serde_json::from_str::<ChapterCorrectionCache>(&content) {
            Ok(cache) => cache,
            Err(_error) => {
                log::warn!("[correction] result cache parse failed");
                return state;
            }
        };
        if cache.schema_version != SCHEMA_VERSION {
            return state;
        }
        state.scanner_fingerprint = cache.scanner_fingerprint;
        state.segments = cache
            .segments
            .into_iter()
            .map(|segment| CachedSegment {
                start_byte: 0,
                end_byte: 0,
                text_fingerprint: segment.text_fingerprint,
                context_fingerprint: segment.context_fingerprint,
                checked: Some(segment.suggestions),
                dirty: false,
            })
            .collect();
        state.needs_reconcile = !state.segments.is_empty();
        state
    }

    /// Persists only checked segments, in offset order, and only when the
    /// semantic content changed. Offset-only adjustments stay in memory.
    pub(crate) fn save(&mut self) -> Result<(), WriterError> {
        if !self.semantic_dirty {
            return Ok(());
        }
        let segments = self
            .segments
            .iter()
            .filter_map(|segment| {
                segment
                    .checked
                    .as_ref()
                    .map(|suggestions| PersistedSegment {
                        text_fingerprint: segment.text_fingerprint.clone(),
                        context_fingerprint: segment.context_fingerprint.clone(),
                        suggestions: suggestions.clone(),
                    })
            })
            .collect();
        let cache = ChapterCorrectionCache {
            schema_version: SCHEMA_VERSION,
            scanner_fingerprint: self.scanner_fingerprint.clone(),
            segments,
        };
        let json =
            serde_json::to_string(&cache).map_err(|error| WriterError::Json(error.to_string()))?;
        crate::storage::atomic_write_string(&self.path, &json)
            .map_err(|error| WriterError::Io(error.to_string()))?;
        self.semantic_dirty = false;
        Ok(())
    }
}

fn is_sentence_terminator(character: char) -> bool {
    matches!(
        character,
        '。' | '！'
            | '？'
            | '!'
            | '?'
            | '.'
            | ';'
            | '；'
            | '…'
            | '\n'
            | '\r'
            | '\u{2028}'
            | '\u{2029}'
    )
}

fn is_closing_mark(character: char) -> bool {
    matches!(
        character,
        '”' | '’' | '」' | '』' | '》' | '】' | ')' | '）' | '"' | '\''
    )
}

fn char_at(text: &str, index: usize) -> Option<char> {
    text.get(index..)?.chars().next()
}

/// Clamps an arbitrary byte position to the start of its character. Edit
/// boundaries are normally on character boundaries, but neighbor lookups can
/// deliberately probe one byte before a boundary.
fn floor_char_boundary(text: &str, position: usize) -> usize {
    let mut position = position.min(text.len());
    while position > 0 && !text.is_char_boundary(position) {
        position -= 1;
    }
    position
}

/// Byte offset just after a terminator run (and any closing marks) starting at
/// `index`.
fn consume_boundary_run(text: &str, index: usize) -> usize {
    let mut cursor = index;
    while let Some(character) = char_at(text, cursor) {
        if !is_sentence_terminator(character) && !is_closing_mark(character) {
            break;
        }
        cursor += character.len_utf8();
    }
    cursor
}

/// Start of the canonical segment containing (or ending before) `position`.
fn sentence_start_before(text: &str, position: usize) -> usize {
    if position == 0 {
        return 0;
    }
    let limit = floor_char_boundary(text, position);
    for (index, character) in text[..limit].char_indices().rev() {
        if !is_sentence_terminator(character) {
            continue;
        }
        let boundary = consume_boundary_run(text, index);
        if boundary <= position {
            return boundary;
        }
    }
    0
}

/// End of the canonical segment containing `position`. When `position` already
/// sits on a segment boundary, that boundary is returned unchanged.
fn sentence_end_from(text: &str, position: usize) -> usize {
    let position = floor_char_boundary(text, position);
    if position >= text.len() {
        return text.len();
    }
    if position > 0 && sentence_start_before(text, position) == position {
        return position;
    }
    for (offset, character) in text[position..].char_indices() {
        if is_sentence_terminator(character) {
            return consume_boundary_run(text, position + offset);
        }
    }
    text.len()
}

/// End of the segment after the one ending at `position`; used to include one
/// neighbor sentence when an edit can merge or split paragraphs.
fn sentence_end_after(text: &str, position: usize) -> usize {
    let position = floor_char_boundary(text, position);
    if position >= text.len() {
        return text.len();
    }
    for (offset, character) in text[position..].char_indices() {
        if is_sentence_terminator(character) {
            return consume_boundary_run(text, position + offset);
        }
    }
    text.len()
}

/// Smallest canonical window that can change when `[change_start, change_end)`
/// changes: the touched sentences plus one neighbor on each side.
fn affected_window(text: &str, change_start: usize, change_end: usize) -> (usize, usize) {
    let start = sentence_start_before(text, change_start);
    let previous_start = if start == 0 {
        0
    } else {
        sentence_start_before(text, start.saturating_sub(1))
    };
    let touched_end = sentence_end_from(text, change_end);
    let next_end = sentence_end_after(text, touched_end);
    (previous_start, next_end)
}

/// Splits `text` into the canonical check segments every caller shares.
///
/// A segment ends after a sentence terminator (plus any trailing closing
/// marks), so boundaries depend only on the text itself and stay reproducible
/// between single-sentence checks, chapter checks, and restarts. Long
/// unpunctuated paragraphs are split on content-defined anchors rather than a
/// fixed length, so inserting text near the start does not shift every later
/// boundary.
pub(crate) fn canonical_segments(text: &str) -> Vec<(usize, usize)> {
    let mut segments = Vec::new();
    if text.is_empty() {
        return segments;
    }
    let mut segment_start = 0usize;
    let mut index = 0usize;
    while index < text.len() {
        let Some(character) = char_at(text, index) else {
            break;
        };
        let next = index + character.len_utf8();
        if !is_sentence_terminator(character) {
            index = next;
            continue;
        }
        let segment_end = consume_boundary_run(text, index);
        push_segment(&mut segments, text, segment_start, segment_end);
        segment_start = segment_end;
        index = segment_end;
    }
    if segment_start < text.len() {
        push_segment(&mut segments, text, segment_start, text.len());
    }
    segments
}

fn push_segment(segments: &mut Vec<(usize, usize)>, text: &str, start: usize, end: usize) {
    if start >= end {
        return;
    }
    let mut piece_start = start;
    while piece_start < end {
        let mut piece_end = end;
        let mut cursor = piece_start;
        let mut window: VecDeque<u8> = VecDeque::with_capacity(CHUNK_WINDOW_BYTES + 4);
        let mut hash = 0u64;
        while cursor < end {
            let Some(character) = char_at(text, cursor) else {
                break;
            };
            let next = cursor + character.len_utf8();
            let mut utf8 = [0u8; 4];
            for byte in character.encode_utf8(&mut utf8).as_bytes() {
                if window.len() == CHUNK_WINDOW_BYTES {
                    if let Some(oldest) = window.pop_front() {
                        hash = hash.wrapping_sub(u64::from(oldest).wrapping_mul(CHUNK_HASH_OLDEST));
                    }
                }
                hash = hash
                    .wrapping_mul(CHUNK_HASH_BASE)
                    .wrapping_add(u64::from(*byte));
                window.push_back(*byte);
            }
            cursor = next;
            let piece_len = cursor - piece_start;
            if piece_len >= SEGMENT_CHUNK_MIN_BYTES && hash & CHUNK_ANCHOR_MASK == 0 {
                piece_end = cursor;
                break;
            }
            if piece_len >= SEGMENT_CHUNK_MAX_BYTES {
                let mut cut = piece_start + SEGMENT_CHUNK_MAX_BYTES;
                while cut > piece_start && !text.is_char_boundary(cut) {
                    cut -= 1;
                }
                if cut > piece_start {
                    piece_end = cut;
                }
                break;
            }
        }
        if piece_end <= piece_start {
            piece_end = end;
        }
        segments.push((piece_start, piece_end));
        piece_start = piece_end;
    }
}

/// Expands one segment to the context that a cache-miss scan reads.
pub(crate) fn context_range(text: &str, start: usize, end: usize) -> (usize, usize) {
    let mut context_start = start.saturating_sub(SEGMENT_CONTEXT_BYTES);
    while !text.is_char_boundary(context_start) {
        context_start -= 1;
    }
    let mut context_end = end.saturating_add(SEGMENT_CONTEXT_BYTES).min(text.len());
    while !text.is_char_boundary(context_end) {
        context_end -= 1;
    }
    (context_start, context_end)
}

fn segment_key(text_fingerprint: &str, context_fingerprint: &str) -> String {
    format!("{text_fingerprint}:{context_fingerprint}")
}

fn suggestions_match(
    text: &str,
    start: usize,
    end: usize,
    suggestions: &[CachedSuggestion],
) -> bool {
    let segment_len = end - start;
    suggestions.iter().all(|suggestion| {
        let (Ok(relative_start), Ok(relative_end)) = (
            usize::try_from(suggestion.start_byte),
            usize::try_from(suggestion.end_byte),
        ) else {
            return false;
        };
        relative_start < relative_end
            && relative_end <= segment_len
            && text.get(start + relative_start..start + relative_end)
                == Some(suggestion.original.as_str())
    })
}

fn to_u32(value: usize) -> Result<u32, WriterError> {
    u32::try_from(value).map_err(|error| WriterError::Other(error.to_string()))
}

/// Rebuilds the full offset-ordered index and revalidates persisted entries.
/// Only chapter open, full replacement, or a mismatched cache takes this path.
pub(crate) fn reconcile_full(text: &str, state: &mut ChapterCorrectionCacheState) -> bool {
    let mut by_key: HashMap<String, VecDeque<CachedSegment>> = HashMap::new();
    for segment in std::mem::take(&mut state.segments) {
        if segment.checked.is_some() {
            by_key
                .entry(segment_key(
                    &segment.text_fingerprint,
                    &segment.context_fingerprint,
                ))
                .or_default()
                .push_back(segment);
        }
    }

    let mut segments = Vec::new();
    let mut dropped = false;
    for (start, end) in canonical_segments(text) {
        let text_fingerprint = content_fingerprint(&text[start..end]);
        let (context_start, context_end) = context_range(text, start, end);
        let context_fingerprint = content_fingerprint(&text[context_start..context_end]);
        let key = segment_key(&text_fingerprint, &context_fingerprint);
        let cached = by_key.get_mut(&key).and_then(VecDeque::pop_front);
        let checked = match cached {
            Some(cached) => {
                let suggestions = cached.checked.unwrap_or_default();
                if suggestions_match(text, start, end, &suggestions) {
                    Some(suggestions)
                } else {
                    dropped = true;
                    None
                }
            }
            None => None,
        };
        segments.push(CachedSegment {
            start_byte: start as u32,
            end_byte: end as u32,
            text_fingerprint,
            context_fingerprint,
            checked,
            dirty: false,
        });
    }
    dropped |= by_key.values().any(|entries| !entries.is_empty());
    state.segments = segments;
    state.needs_reconcile = false;
    state.pending_windows.clear();
    state.semantic_dirty |= dropped;
    dropped
}

/// Marks unseen segments touched by an edit as work for the next local scan.
/// Segments with verified results are content-identical to the new text and do
/// not need to run the model again just because a transaction replay rebuilt
/// the offset index.
pub(crate) fn mark_unseen_segments_in_edit_windows(
    state: &mut ChapterCorrectionCacheState,
    edits: &[EditRange],
) {
    for segment in &mut state.segments {
        if segment.checked.is_some() {
            continue;
        }
        let start = segment.start_byte as usize;
        let end = segment.end_byte as usize;
        if edits.iter().any(|edit| {
            start < edit.new_end.saturating_add(SEGMENT_CONTEXT_BYTES)
                && edit.new_start.saturating_sub(SEGMENT_CONTEXT_BYTES) < end
        }) {
            segment.dirty = true;
        }
    }
}

fn validate_edit_order(edits: &[EditRange]) -> Result<(), WriterError> {
    let mut previous_old_end = 0usize;
    let mut previous_new_end = 0usize;
    let mut delta: i128 = 0;
    for edit in edits {
        if edit.old_start > edit.old_end
            || edit.new_start > edit.new_end
            || edit.old_start < previous_old_end
            || edit.new_start < previous_new_end
        {
            return Err(WriterError::Other(
                "invalid correction edit range ordering".into(),
            ));
        }
        let expected_new_start = edit.old_start as i128 + delta;
        if expected_new_start != edit.new_start as i128 {
            return Err(WriterError::Other(
                "correction edit ranges do not describe consecutive texts".into(),
            ));
        }
        delta += (edit.new_end - edit.new_start) as i128 - (edit.old_end - edit.old_start) as i128;
        previous_old_end = edit.old_end;
        previous_new_end = edit.new_end;
    }
    Ok(())
}

fn validate_edits(text: &str, edits: &[EditRange]) -> Result<(), WriterError> {
    validate_edit_order(edits)?;
    let delta: i128 = edits
        .iter()
        .map(|edit| {
            (edit.new_end - edit.new_start) as i128 - (edit.old_end - edit.old_start) as i128
        })
        .sum();
    let old_len = text.len() as i128 - delta;
    if edits.iter().any(|edit| edit.new_end > text.len()) || old_len < 0 {
        return Err(WriterError::Other(
            "correction edit ranges exceed the previous text length".into(),
        ));
    }
    Ok(())
}

fn merge_windows(windows: &mut Vec<(usize, usize)>) {
    windows.retain(|(start, end)| start < end);
    windows.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(windows.len());
    for (start, end) in windows.drain(..) {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    *windows = merged;
}

/// First phase of an edit check: move untouched segments and remember the
/// windows that must be re-segmented. No chapter text, fingerprinting, or model
/// scan is needed here, so every editor transaction can be applied cheaply and
/// in order. The second phase ([`apply_pending_windows`]) runs on the latest
/// text before scanning.
pub(crate) fn apply_edits_light(
    state: &mut ChapterCorrectionCacheState,
    edits: &[EditRange],
) -> Result<(), WriterError> {
    let mut ordered = edits.to_vec();
    ordered.sort_by_key(|edit| (edit.old_start, edit.new_start));
    let edits = ordered.as_slice();
    validate_edit_order(edits)?;
    if edits.is_empty() {
        return Ok(());
    }
    if state.needs_reconcile {
        // Existing windows describe an older text; the full reconcile below
        // rebuilds the index from the newest text, so only the current edits
        // need to be carried forward as scan hints.
        state.pending_windows.clear();
        state.pending_windows.extend(edits.iter().map(|edit| {
            (
                edit.new_start.saturating_sub(SEGMENT_CONTEXT_BYTES),
                edit.new_end.saturating_add(SEGMENT_CONTEXT_BYTES),
            )
        }));
        merge_windows(&mut state.pending_windows);
        return Ok(());
    }

    // A stage-one call does not receive text, so pending windows recorded by
    // earlier keystrokes must be shifted through this transaction before the
    // second stage re-segments them.
    let mut shifted_windows = Vec::with_capacity(state.pending_windows.len());
    for (start, end) in std::mem::take(&mut state.pending_windows) {
        let shifted_start = map_offset(start, edits, false);
        let shifted_end = map_offset(end, edits, true);
        if shifted_start < shifted_end {
            shifted_windows.push((shifted_start, shifted_end));
        }
    }
    state.pending_windows = shifted_windows;

    let mut translated = Vec::with_capacity(state.segments.len());
    let mut semantic_dropped = false;
    for mut segment in std::mem::take(&mut state.segments) {
        let was_checked = segment.checked.is_some();
        let start = map_offset(segment.start_byte as usize, edits, false);
        let end = map_offset(segment.end_byte as usize, edits, true);
        if start < end {
            segment.start_byte = start as u32;
            segment.end_byte = end as u32;
            translated.push(segment);
        } else if was_checked {
            semantic_dropped = true;
        }
    }

    let affected = translated
        .iter()
        .enumerate()
        .filter(|(_, segment)| {
            let start = segment.start_byte as usize;
            let end = segment.end_byte as usize;
            edits.iter().any(|edit| {
                start < edit.new_end.saturating_add(SEGMENT_CONTEXT_BYTES)
                    && edit.new_start.saturating_sub(SEGMENT_CONTEXT_BYTES) < end
            })
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let mut windows = Vec::new();
    let mut kept = Vec::with_capacity(translated.len());
    if affected.is_empty() {
        kept = translated;
        windows.extend(edits.iter().map(|edit| {
            (
                edit.new_start.saturating_sub(SEGMENT_CONTEXT_BYTES),
                edit.new_end.saturating_add(SEGMENT_CONTEXT_BYTES),
            )
        }));
    } else {
        let mut groups: Vec<(usize, usize)> = Vec::new();
        for index in affected {
            let group = (
                index.saturating_sub(1),
                (index + 1).min(translated.len() - 1),
            );
            match groups.last_mut() {
                Some(last) if group.0 <= last.1 + 1 => last.1 = last.1.max(group.1),
                _ => groups.push(group),
            }
        }
        let mut replaced = vec![false; translated.len()];
        for (group_start, group_end) in groups {
            let mut window_start = translated[group_start].start_byte as usize;
            let mut window_end = translated[group_end].end_byte as usize;
            for edit in edits {
                if edit.new_start <= window_end && edit.new_end >= window_start {
                    window_start = window_start.min(edit.new_start);
                    window_end = window_end.max(edit.new_end);
                }
            }
            windows.push((window_start, window_end));
            for index in group_start..=group_end {
                if translated[index].checked.is_some() {
                    semantic_dropped = true;
                }
                replaced[index] = true;
            }
        }
        kept.extend(
            translated
                .into_iter()
                .zip(replaced)
                .filter(|(_, replaced)| !replaced)
                .map(|(segment, _)| segment),
        );
    }

    merge_windows(&mut windows);
    state.pending_windows.extend(windows);
    merge_windows(&mut state.pending_windows);
    state.segments = kept;
    state.needs_reconcile = false;
    state.semantic_dirty |= semantic_dropped;
    Ok(())
}

/// Second phase: segment the dirty windows against the newest chapter text.
/// Existing checked segments outside those windows keep their cached results.
pub(crate) fn apply_pending_windows(
    text: &str,
    state: &mut ChapterCorrectionCacheState,
    force_reconcile: bool,
) -> Result<(), WriterError> {
    let pending = std::mem::take(&mut state.pending_windows);
    if force_reconcile || state.needs_reconcile {
        reconcile_full(text, state);
    }
    if pending.is_empty() {
        return Ok(());
    }

    let mut windows = pending
        .into_iter()
        .map(|(start, end)| {
            (
                floor_char_boundary(text, start.min(text.len())),
                floor_char_boundary(text, end.min(text.len())),
            )
        })
        .collect::<Vec<_>>();
    // A dirty window may start inside an existing segment. Expand it to that
    // segment's full extent before replacing segments, otherwise the prefix
    // outside the window would become an unscanned hole.
    for window in &mut windows {
        for segment in &state.segments {
            let start = segment.start_byte as usize;
            let end = segment.end_byte as usize;
            if window.0 < end && start < window.1 {
                window.0 = window.0.min(start);
                window.1 = window.1.max(end);
            }
        }
    }
    merge_windows(&mut windows);

    let mut added = Vec::new();
    for (window_start, window_end) in windows {
        for (local_start, local_end) in canonical_segments(&text[window_start..window_end]) {
            let start = window_start + local_start;
            let end = window_start + local_end;
            let text_fingerprint = content_fingerprint(&text[start..end]);
            let (context_start, context_end) = context_range(text, start, end);
            let context_fingerprint = content_fingerprint(&text[context_start..context_end]);
            added.push(CachedSegment {
                start_byte: to_u32(start)?,
                end_byte: to_u32(end)?,
                text_fingerprint,
                context_fingerprint,
                checked: None,
                dirty: true,
            });
        }
    }

    let mut semantic_dropped = false;
    let mut kept = std::mem::take(&mut state.segments);
    kept.retain(|segment| {
        let start = segment.start_byte as usize;
        let end = segment.end_byte as usize;
        let overlaps_added = added
            .iter()
            .any(|added| start < added.end_byte as usize && (added.start_byte as usize) < end);
        if overlaps_added && segment.checked.is_some() {
            semantic_dropped = true;
        }
        !overlaps_added
    });
    kept.extend(added);
    kept.sort_by_key(|segment| (segment.start_byte, segment.end_byte));
    state.segments = kept;
    state.semantic_dirty |= semantic_dropped;
    Ok(())
}

fn map_offset(position: usize, edits: &[EditRange], is_end: bool) -> usize {
    let mut shift: i128 = 0;
    for edit in edits {
        if position < edit.old_start {
            break;
        }
        if position == edit.old_start {
            // A segment ending exactly where an edit starts must not absorb
            // the replacement text; the following segment owns it.
            return edit.new_start;
        }
        if position >= edit.old_end {
            // Subsequent edits already carry the cumulative shift in their
            // `new_start`; only this edit's own length delta may be added.
            shift +=
                (edit.new_end - edit.new_start) as i128 - (edit.old_end - edit.old_start) as i128;
            continue;
        }
        return if is_end { edit.new_end } else { edit.new_start };
    }
    (position as i128 + shift).max(0) as usize
}

/// Applies the edit transaction's real change ranges to the committed index:
/// retained segments only shift (without dirtying the file), while segments
/// whose content or context changed are re-segmented and marked for scanning.
pub(crate) fn reconcile_edits(
    text: &str,
    state: &mut ChapterCorrectionCacheState,
    edits: &[EditRange],
) -> Result<(), WriterError> {
    // Display patches may be emitted in application order (descending old
    // offsets). The mapping and consistency checks below need document order.
    let mut ordered = edits.to_vec();
    ordered.sort_by_key(|edit| (edit.old_start, edit.new_start));
    let edits = ordered.as_slice();
    validate_edits(text, edits)?;
    if state.needs_reconcile || edits.is_empty() {
        reconcile_full(text, state);
        return Ok(());
    }

    let mut translated = Vec::with_capacity(state.segments.len());
    let mut semantic_dropped = false;
    for mut segment in std::mem::take(&mut state.segments) {
        let was_checked = segment.checked.is_some();
        let start = map_offset(segment.start_byte as usize, edits, false);
        let end = map_offset(segment.end_byte as usize, edits, true);
        if start < end
            && end <= text.len()
            && text.is_char_boundary(start)
            && text.is_char_boundary(end)
        {
            segment.start_byte = start as u32;
            segment.end_byte = end as u32;
            translated.push(segment);
        } else if was_checked {
            // The segment no longer exists in the edited text; its persisted
            // suggestions must be removed even when no replacement segment is
            // scannable (for example, deleting the whole chapter).
            semantic_dropped = true;
        }
    }

    // Dirty only the segments whose text or scan context the edit touched,
    // plus one neighbor on each side so punctuation/line-break edits can
    // split or merge the adjacent canonical segment. Using existing segment
    // boundaries as the local window also keeps content-defined chunk anchors
    // reproducible inside long unpunctuated paragraphs.
    let affected = translated
        .iter()
        .enumerate()
        .filter(|(_, segment)| {
            let start = segment.start_byte as usize;
            let end = segment.end_byte as usize;
            edits.iter().any(|edit| {
                start < edit.new_end.saturating_add(SEGMENT_CONTEXT_BYTES)
                    && edit.new_start.saturating_sub(SEGMENT_CONTEXT_BYTES) < end
            })
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let mut windows = Vec::new();
    let mut kept = Vec::with_capacity(translated.len());
    if affected.is_empty() {
        kept = translated;
        windows.extend(
            edits
                .iter()
                .map(|edit| affected_window(text, edit.new_start, edit.new_end)),
        );
    } else {
        let mut groups: Vec<(usize, usize)> = Vec::new();
        for index in affected {
            let group = (
                index.saturating_sub(1),
                (index + 1).min(translated.len() - 1),
            );
            match groups.last_mut() {
                Some(last) if group.0 <= last.1 + 1 => last.1 = last.1.max(group.1),
                _ => groups.push(group),
            }
        }
        let mut replaced = vec![false; translated.len()];
        for (group_start, group_end) in groups {
            let mut window_start = translated[group_start].start_byte as usize;
            let mut window_end = translated[group_end].end_byte as usize;
            // An insertion before the first retained segment maps that
            // segment's start forward, leaving the inserted text outside the
            // original segment boundaries. Expand the window back over the
            // actual change so the new text is segmented too.
            for edit in edits {
                if edit.new_start <= window_end && edit.new_end >= window_start {
                    window_start = window_start.min(edit.new_start);
                    window_end = window_end.max(edit.new_end);
                }
            }
            windows.push((window_start, window_end));
            for index in group_start..=group_end {
                if translated[index].checked.is_some() {
                    semantic_dropped = true;
                }
                replaced[index] = true;
            }
        }
        kept.extend(
            translated
                .into_iter()
                .zip(replaced)
                .filter(|(_, replaced)| !replaced)
                .map(|(segment, _)| segment),
        );
    }

    windows.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in windows {
        if start >= end {
            continue;
        }
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }

    let mut added = Vec::new();
    for (window_start, window_end) in merged {
        let window_start = window_start.min(text.len());
        let window_end = window_end.min(text.len());
        for (local_start, local_end) in canonical_segments(&text[window_start..window_end]) {
            let start = window_start + local_start;
            let end = window_start + local_end;
            let text_fingerprint = content_fingerprint(&text[start..end]);
            let (context_start, context_end) = context_range(text, start, end);
            let context_fingerprint = content_fingerprint(&text[context_start..context_end]);
            added.push(CachedSegment {
                start_byte: to_u32(start)?,
                end_byte: to_u32(end)?,
                text_fingerprint,
                context_fingerprint,
                checked: None,
                dirty: true,
            });
        }
    }

    kept.retain(|segment| {
        let start = segment.start_byte as usize;
        let end = segment.end_byte as usize;
        let overlaps_added = added
            .iter()
            .any(|added| start < added.end_byte as usize && (added.start_byte as usize) < end);
        if overlaps_added && segment.checked.is_some() {
            semantic_dropped = true;
        }
        !overlaps_added
    });
    kept.extend(added);
    kept.sort_by_key(|segment| (segment.start_byte, segment.end_byte));
    state.segments = kept;
    state.needs_reconcile = false;
    // Pure offset shifts do not rewrite the chapter cache. Removing or
    // replacing checked segments does: their persisted suggestions must not
    // survive an edit that made them invalid.
    state.semantic_dirty |= semantic_dropped;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked_segment(start: usize, end: usize, text: &str) -> CachedSegment {
        let (context_start, context_end) = context_range(text, start, end);
        CachedSegment {
            start_byte: start as u32,
            end_byte: end as u32,
            text_fingerprint: content_fingerprint(&text[start..end]),
            context_fingerprint: content_fingerprint(&text[context_start..context_end]),
            checked: Some(Vec::new()),
            dirty: false,
        }
    }

    fn empty_state(text: &str) -> ChapterCorrectionCacheState {
        ChapterCorrectionCacheState {
            scanner_fingerprint: "scanner".to_string(),
            segments: canonical_segments(text)
                .into_iter()
                .map(|(start, end)| checked_segment(start, end, text))
                .collect(),
            semantic_dirty: false,
            scanning: false,
            last_session_id: 1,
            last_revision: 1,
            needs_reconcile: false,
            path: PathBuf::new(),
        }
    }

    #[test]
    fn stage_one_edits_shift_existing_pending_windows() {
        let text = "甲".repeat(400);
        let mut state = empty_state(&text);
        state.pending_windows = vec![(300, 320)];
        apply_edits_light(
            &mut state,
            &[EditRange {
                old_start: 0,
                old_end: 0,
                new_start: 0,
                new_end: 1,
            }],
        )
        .expect("stage one edit should apply");
        assert!(
            state.pending_windows.contains(&(301, 321)),
            "pending windows from an earlier keystroke must follow later insertions"
        );
    }

    #[test]
    fn fingerprint_is_stable_and_content_addressed() {
        assert_eq!(content_fingerprint("素笺"), content_fingerprint("素笺"));
        assert_ne!(content_fingerprint("素笺"), content_fingerprint("素简"));
        assert_eq!(content_fingerprint("").len(), 16);
    }

    #[test]
    fn parts_fingerprint_separates_parts() {
        let left = vec!["ab".to_string(), "c".to_string()];
        let right = vec!["a".to_string(), "bc".to_string()];
        assert_ne!(parts_fingerprint(&left), parts_fingerprint(&right));
    }

    #[test]
    fn cache_file_name_is_digest_only() {
        let name = cache_file_name("project-a", "chapter-b");
        assert!(name.ends_with(".json"));
        assert!(!name.contains("project"));
        assert_eq!(name, cache_file_name("project-a", "chapter-b"));
        assert_ne!(name, cache_file_name("project-a", "chapter-c"));
    }

    #[test]
    fn segments_cover_text_and_end_after_terminators() {
        let text = "第一句。第二句！带引号。”第三句";
        let segments = canonical_segments(text);
        assert_eq!(segments, vec![(0, 12), (12, 24), (24, 39), (39, 48)]);
        for (start, end) in &segments {
            assert!(text.is_char_boundary(*start));
            assert!(text.is_char_boundary(*end));
            assert!(start < end);
        }
        assert_eq!(segments.first().map(|segment| segment.0), Some(0));
        assert_eq!(segments.last().map(|segment| segment.1), Some(text.len()));
    }

    #[test]
    fn segments_consume_terminator_runs_and_newlines() {
        let text = "甲……乙\n\n丙\r\n丁！？」戊";
        let segments = canonical_segments(text);
        assert_eq!(segments.last().map(|segment| segment.1), Some(text.len()));
        for (start, end) in &segments {
            assert!(start < end);
        }
    }

    #[test]
    fn long_paragraphs_are_chunked_on_char_boundaries() {
        let text = "好".repeat(SEGMENT_CHUNK_MAX_BYTES);
        let segments = canonical_segments(&text);
        assert!(segments.len() >= 2);
        for (start, end) in &segments {
            assert!(text.is_char_boundary(*start));
            assert!(text.is_char_boundary(*end));
            assert!(end - start <= SEGMENT_CHUNK_MAX_BYTES);
        }
        assert_eq!(segments.first().map(|segment| segment.0), Some(0));
        assert_eq!(segments.last().map(|segment| segment.1), Some(text.len()));
    }

    #[test]
    fn chunk_boundaries_are_content_anchored() {
        let paragraph =
            "这是一个很长的段落，用来验证内容锚点分块不会因为段首插入而整体平移。".repeat(800);
        let before = canonical_segments(&paragraph);
        let mut inserted = String::from("新增文字");
        inserted.push_str(&paragraph);
        let after = canonical_segments(&inserted);
        // At least one boundary other than the end should share content position.
        let before_set = before[..before.len().saturating_sub(1)]
            .iter()
            .map(|(start, _)| *start)
            .collect::<Vec<_>>();
        let shared = after
            .iter()
            .take(after.len().saturating_sub(1))
            .filter(|(start, _)| before_set.contains(start))
            .count();
        assert!(
            shared > 0,
            "content-defined chunking should preserve anchored boundaries"
        );
    }

    #[test]
    fn empty_text_has_no_segments() {
        assert!(canonical_segments("").is_empty());
    }

    #[test]
    fn context_range_clamps_to_text() {
        let text = "甲乙丙丁";
        assert_eq!(context_range(text, 0, text.len()), (0, text.len()));
        let long = "甲".repeat(1024);
        let (start, end) = context_range(&long, 400, 500);
        assert!(start < 400 && end > 500);
        assert!(long.is_char_boundary(start) && long.is_char_boundary(end));
    }

    #[test]
    fn inserting_one_character_keeps_unaffected_results_and_only_dirties_local_segments() {
        let text = (0..20)
            .map(|index| format!("第{index}句话{}。", "甲".repeat(100)))
            .collect::<String>();
        let mut state = empty_state(&text);
        let third = text.find("第10句话").expect("tenth sentence");
        let boundary = third + "第10句".len();
        let inserted = format!("{}新{}", &text[..boundary], &text[boundary..]);
        let edits = [EditRange {
            old_start: boundary,
            old_end: boundary,
            new_start: boundary,
            new_end: boundary + "新".len(),
        }];
        let last_start_before = state.segments.last().expect("segment").start_byte as usize;
        reconcile_edits(&inserted, &mut state, &edits).expect("edit should apply");
        let dirty = state
            .segments
            .iter()
            .filter(|segment| segment.dirty)
            .count();
        assert!(dirty >= 1);
        assert!(dirty < state.segments.len());
        assert!(
            !state.semantic_dirty,
            "pure shifts must not rewrite the cache"
        );
        // Later, untouched segments moved by exactly the inserted length.
        let last = state.segments.last().expect("segment");
        assert_eq!(last.start_byte as usize, last_start_before + "新".len());
        assert!(last.checked.is_some());
    }

    #[test]
    fn deleting_all_text_marks_the_persisted_result_set_dirty() {
        let text = "甲。乙。丙。";
        let mut state = empty_state(text);
        let edits = [EditRange {
            old_start: 0,
            old_end: text.len(),
            new_start: 0,
            new_end: 0,
        }];
        reconcile_edits("", &mut state, &edits).expect("edit should apply");
        assert!(state.segments.is_empty());
        assert!(
            state.semantic_dirty,
            "removing checked segments must persist the deletion"
        );
    }

    #[test]
    fn multiple_disjoint_edits_translate_segments_in_document_order() {
        let text = (0..10)
            .map(|index| format!("第{index}句{}。", "甲".repeat(300)))
            .collect::<String>();
        let mut state = empty_state(&text);
        let first = text.find("第0句").expect("first sentence token");
        let sixth = text.find("第6句").expect("sixth sentence token");
        let old_first = "第0句";
        let new_first = "开头句";
        let old_sixth = "第6句";
        let new_sixth = "第六句话";
        let delta_first = new_first.len() as i64 - old_first.len() as i64;
        let second_new_start = (sixth as i64 + delta_first) as usize;
        let new_text = format!(
            "{}开头句{}第六句话{}",
            &text[..first],
            &text[first + old_first.len()..sixth],
            &text[sixth + old_sixth.len()..]
        );
        let edits = [
            EditRange {
                old_start: first,
                old_end: first + old_first.len(),
                new_start: first,
                new_end: first + new_first.len(),
            },
            EditRange {
                old_start: sixth,
                old_end: sixth + old_sixth.len(),
                new_start: second_new_start,
                new_end: second_new_start + new_sixth.len(),
            },
        ];
        // Display patches may arrive in application order (largest old offset
        // first); the cache still has to interpret them in document order.
        reconcile_edits(&new_text, &mut state, &[edits[1], edits[0]]).expect("edits should apply");
        assert_eq!(state.segments.len(), 10);
        assert_eq!(
            state
                .segments
                .last()
                .map(|segment| segment.end_byte as usize),
            Some(new_text.len())
        );
        let dirty = state
            .segments
            .iter()
            .filter(|segment| segment.dirty)
            .count();
        assert!(dirty > 0);
        assert!(dirty < state.segments.len());
        assert!(
            state.segments.iter().any(|segment| {
                segment.checked.is_some()
                    && (segment.start_byte as usize) >= first + new_first.len()
                    && (segment.end_byte as usize) <= second_new_start
            }),
            "segments between the two edits keep their checked state"
        );
    }

    #[test]
    fn insertion_at_chapter_start_creates_a_segment_for_the_new_text() {
        let text = "第一句。第二句。第三句。";
        let mut state = empty_state(text);
        let inserted = "新增。第一句。第二句。第三句。";
        let edits = [EditRange {
            old_start: 0,
            old_end: 0,
            new_start: 0,
            new_end: "新增。".len(),
        }];
        reconcile_edits(inserted, &mut state, &edits).expect("edit should apply");
        assert_eq!(
            state
                .segments
                .first()
                .map(|segment| segment.start_byte as usize),
            Some(0)
        );
        assert_eq!(
            state
                .segments
                .last()
                .map(|segment| segment.end_byte as usize),
            Some(inserted.len())
        );
        for pair in state.segments.windows(2) {
            assert_eq!(pair[0].end_byte, pair[1].start_byte);
        }
    }

    #[test]
    fn long_unpunctuated_paragraph_dirties_only_local_chunks() {
        let text =
            "这是一个没有句末标点的超长段落，用来验证插入文字只会影响附近的缓存分块而不是整段"
                .repeat(700);
        let mut state = empty_state(&text);
        let total = state.segments.len();
        assert!(total >= 4, "test text should produce several chunks");
        let boundary = text.len() / 2;
        let boundary = (0..=boundary)
            .rev()
            .find(|position| text.is_char_boundary(*position))
            .expect("char boundary");
        let inserted = format!("{}新{}", &text[..boundary], &text[boundary..]);
        let edits = [EditRange {
            old_start: boundary,
            old_end: boundary,
            new_start: boundary,
            new_end: boundary + "新".len(),
        }];
        reconcile_edits(&inserted, &mut state, &edits).expect("edit should apply");
        let dirty = state
            .segments
            .iter()
            .filter(|segment| segment.dirty)
            .count();
        assert!(dirty >= 1);
        assert!(
            dirty <= 6,
            "long paragraph edit should rescan only local chunks, dirtied {dirty} of {total}"
        );
        assert!(!state.semantic_dirty);
    }

    #[test]
    fn full_reconcile_reuses_matching_content() {
        let text = "甲。乙。丙。";
        let mut state = empty_state(text);
        let before = state.segments.clone();
        let dropped = reconcile_full(text, &mut state);
        assert!(!dropped);
        assert_eq!(state.segments.len(), before.len());
        assert!(state
            .segments
            .iter()
            .all(|segment| segment.checked.is_some() && !segment.dirty));
    }

    #[test]
    fn full_reconcile_leaves_new_segments_unseen() {
        let text = "旧句。";
        let changed = "旧句。新句。";
        let mut state = empty_state(text);
        state.segments.clear();
        let dropped = reconcile_full(changed, &mut state);
        assert!(!dropped);
        assert_eq!(state.segments.len(), 2);
        assert!(state
            .segments
            .iter()
            .all(|segment| segment.checked.is_none() && !segment.dirty));
    }

    #[test]
    fn edit_windows_mark_only_nearby_unseen_segments() {
        let sentence = "这".repeat(300);
        let text = format!("{sentence}。{sentence}。{sentence}。");
        let second = text
            .match_indices(sentence.as_str())
            .nth(1)
            .map(|(start, _)| start)
            .expect("second sentence");
        let mut state = empty_state("");
        state.segments.clear();
        reconcile_full(&text, &mut state);
        assert!(state.segments.iter().all(|segment| !segment.dirty));

        mark_unseen_segments_in_edit_windows(
            &mut state,
            &[EditRange {
                old_start: second + 3,
                old_end: second + 3,
                new_start: second + 3,
                new_end: second + 4,
            }],
        );
        let dirty = state
            .segments
            .iter()
            .filter(|segment| segment.dirty)
            .count();
        assert!(dirty >= 1);
        assert!(dirty < state.segments.len());
    }
}
