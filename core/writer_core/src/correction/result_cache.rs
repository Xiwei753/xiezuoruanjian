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
/// 一次暂停扫描前最多保留的待重建窗口数。键入事务只平移旧窗口并追加本次真实
/// 改动窗口（第二阶段的 [`apply_pending_windows`] 会统一合并、扩段）；只有
/// 长连击超过该上限时才整体合并，避免列表无界增长。
const MAX_PENDING_WINDOWS: usize = 32;

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
    /// 当前 checked 采用的同步记录键；None 表示尚未持久化/旧缓存。
    #[serde(skip)]
    pub(crate) synced_record_key: Option<String>,
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
    synced_record_key: Option<String>,
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
    /// 这些 checked 结果实际来自哪种扫描模式；旧缓存缺省时不能冒充模型覆盖。
    #[serde(default)]
    checked_mode: String,
    /// 已有 checked 结果尚未成功发布到同步 mutation/outbox。
    #[serde(default)]
    publication_pending: bool,
    #[serde(default)]
    segments: Vec<PersistedSegment>,
}

/// In-memory cache state plus the file location for one chapter.
pub(crate) struct ChapterCorrectionCacheState {
    pub(crate) scanner_fingerprint: String,
    /// 当前 checked 结果实际来自的扫描模式（model/rules）；空表示未知旧缓存。
    pub(crate) checked_mode: String,
    /// 本地已检查结果尚未成功发布；持久化后由下一次恢复重试，不重跑模型。
    pub(crate) publication_pending: bool,
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
            checked_mode: String::new(),
            publication_pending: false,
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
        state.checked_mode = cache.checked_mode;
        state.publication_pending = cache.publication_pending;
        // 旧缓存没有记录实际扫描模式：不能把规则结果当成模型覆盖继承下来；
        // 清空 checked 后由下一次真实扫描重新建立覆盖（正文/建议不写回）。
        if state.checked_mode.is_empty() {
            state.needs_reconcile = true;
            return state;
        }
        state.segments = cache
            .segments
            .into_iter()
            .map(|segment| CachedSegment {
                start_byte: 0,
                end_byte: 0,
                text_fingerprint: segment.text_fingerprint,
                context_fingerprint: segment.context_fingerprint,
                synced_record_key: segment.synced_record_key,
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
                        synced_record_key: segment.synced_record_key.clone(),
                        suggestions: suggestions.clone(),
                    })
            })
            .collect();
        let cache = ChapterCorrectionCache {
            schema_version: SCHEMA_VERSION,
            scanner_fingerprint: self.scanner_fingerprint.clone(),
            checked_mode: self.checked_mode.clone(),
            publication_pending: self.publication_pending,
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
///
/// Issue #1063：钳位规则由 `editor::text_coordinates` 唯一维护。
fn floor_char_boundary(text: &str, position: usize) -> usize {
    crate::editor::text_coordinates::clamp_to_char_boundary(text, position)
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

pub(crate) fn suggestions_match(
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
            synced_record_key: None,
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
                // The replaced segment is re-segmented from this window and
                // rescanned; its stale checked results are recovered by that
                // scan, so replacing them alone does not dirty the file.
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
    // Shifted windows keep their exact range (the stage-two scan merges and
    // expands them anyway). Only a long burst that exceeds the cap collapses
    // the list, so pending windows cannot grow without bound between scans.
    if state.pending_windows.len() > MAX_PENDING_WINDOWS {
        merge_windows(&mut state.pending_windows);
    }
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
                synced_record_key: None,
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
                // The window is re-segmented into dirty segments below; the
                // replaced segment's checked results are superseded by the
                // rescan, so dropping them does not require rewriting the file.
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
                synced_record_key: None,
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
mod tests;
