//! Content-addressed, persistent correction result cache.
//!
//! Entries are keyed by the exact segment text and its neighboring context, so
//! an edit that only shifts byte offsets keeps hitting the same entries without
//! any explicit edit-transaction bookkeeping. The cache file holds derived data
//! only: no chapter text backup, no editor state, and nothing that participates
//! in text persistence or sync.

use std::path::PathBuf;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::api::WriterError;

/// On-disk layout version. A mismatch discards the file instead of guessing.
pub(crate) const SCHEMA_VERSION: u32 = 1;
/// Context bytes scanned around one segment, matching the live scan path.
pub(crate) const SEGMENT_CONTEXT_BYTES: usize = 256;
/// Very long unpunctuated paragraphs are checked in chunks of this size.
pub(crate) const SEGMENT_CHUNK_BYTES: usize = 16 * 1024;

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

/// One checked segment. An empty `suggestions` vector means "checked, no error".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CachedSegment {
    pub(crate) start_byte: u32,
    pub(crate) end_byte: u32,
    pub(crate) text_fingerprint: String,
    pub(crate) context_fingerprint: String,
    #[serde(default)]
    pub(crate) suggestions: Vec<CachedSuggestion>,
}

/// Persistent chapter cache file content.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChapterCorrectionCache {
    schema_version: u32,
    #[serde(default)]
    scanner_fingerprint: String,
    #[serde(default)]
    segments: Vec<CachedSegment>,
}

/// In-memory cache state plus the file location for one chapter.
pub(crate) struct ChapterCorrectionCacheState {
    pub(crate) scanner_fingerprint: String,
    pub(crate) segments: Vec<CachedSegment>,
    pub(crate) dirty: bool,
    pub(crate) path: PathBuf,
}

impl ChapterCorrectionCacheState {
    /// Loads the persisted cache, or starts empty for a missing/corrupt file.
    pub(crate) fn load(path: PathBuf) -> Self {
        let mut state = Self {
            scanner_fingerprint: String::new(),
            segments: Vec::new(),
            dirty: false,
            path,
        };
        let content = match std::fs::read_to_string(&state.path) {
            Ok(content) => content,
            Err(error) => {
                if error.kind() != std::io::ErrorKind::NotFound {
                    log::warn!(
                        "[correction] failed to read result cache {}: {error}",
                        state.path.display()
                    );
                }
                return state;
            }
        };
        let cache = match serde_json::from_str::<ChapterCorrectionCache>(&content) {
            Ok(cache) => cache,
            Err(error) => {
                log::warn!(
                    "[correction] failed to parse result cache {}: {error}",
                    state.path.display()
                );
                return state;
            }
        };
        if cache.schema_version != SCHEMA_VERSION {
            return state;
        }
        state.scanner_fingerprint = cache.scanner_fingerprint;
        state.segments = cache.segments;
        state
    }

    /// Persists the cache only when something changed since the last write.
    pub(crate) fn save(&mut self) -> Result<(), WriterError> {
        if !self.dirty {
            return Ok(());
        }
        let cache = ChapterCorrectionCache {
            schema_version: SCHEMA_VERSION,
            scanner_fingerprint: self.scanner_fingerprint.clone(),
            segments: self.segments.clone(),
        };
        let json =
            serde_json::to_string(&cache).map_err(|error| WriterError::Json(error.to_string()))?;
        crate::storage::atomic_write_string(&self.path, &json)
            .map_err(|error| WriterError::Io(error.to_string()))?;
        self.dirty = false;
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

/// Splits `text` into the canonical check segments every caller shares.
///
/// A segment ends after a sentence terminator (plus any trailing closing
/// marks), so boundaries depend only on the text itself and stay reproducible
/// between single-sentence checks, chapter checks, and restarts. Segments
/// longer than [`SEGMENT_CHUNK_BYTES`] are chunked on character boundaries.
pub(crate) fn canonical_segments(text: &str) -> Vec<(usize, usize)> {
    let mut segments = Vec::new();
    if text.is_empty() {
        return segments;
    }
    let mut segment_start = 0usize;
    let mut index = 0usize;
    while index < text.len() {
        let Some(character) = text[index..].chars().next() else {
            break;
        };
        let next = index + character.len_utf8();
        if !is_sentence_terminator(character) {
            index = next;
            continue;
        }
        let mut segment_end = next;
        let mut cursor = next;
        while cursor < text.len() {
            let Some(trailing) = text[cursor..].chars().next() else {
                break;
            };
            if !is_sentence_terminator(trailing) && !is_closing_mark(trailing) {
                break;
            }
            cursor += trailing.len_utf8();
            segment_end = cursor;
        }
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
        let mut piece_end = piece_start.saturating_add(SEGMENT_CHUNK_BYTES).min(end);
        while piece_end > piece_start && !text.is_char_boundary(piece_end) {
            piece_end -= 1;
        }
        if piece_end == piece_start {
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let text = "好".repeat(SEGMENT_CHUNK_BYTES);
        let segments = canonical_segments(&text);
        assert!(segments.len() >= 2);
        for (start, end) in &segments {
            assert!(text.is_char_boundary(*start));
            assert!(text.is_char_boundary(*end));
            assert!(end - start <= SEGMENT_CHUNK_BYTES);
        }
        assert_eq!(segments.first().map(|segment| segment.0), Some(0));
        assert_eq!(segments.last().map(|segment| segment.1), Some(text.len()));
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
}
