//! result_cache.rs 的单元测试（从生产文件拆出，保持原有断言）。

use super::*;

fn checked_segment(start: usize, end: usize, text: &str) -> CachedSegment {
    let (context_start, context_end) = context_range(text, start, end);
    CachedSegment {
        start_byte: start as u32,
        end_byte: end as u32,
        text_fingerprint: content_fingerprint(&text[start..end]),
        context_fingerprint: content_fingerprint(&text[context_start..context_end]),
        synced_record_key: None,
        checked: Some(Vec::new()),
        dirty: false,
    }
}

fn empty_state(text: &str) -> ChapterCorrectionCacheState {
    ChapterCorrectionCacheState {
        scanner_fingerprint: "scanner".to_string(),
        checked_mode: "model".to_string(),
        publication_pending: false,
        segments: canonical_segments(text)
            .into_iter()
            .map(|(start, end)| checked_segment(start, end, text))
            .collect(),
        semantic_dirty: false,
        scanning: false,
        last_session_id: 1,
        last_revision: 1,
        needs_reconcile: false,
        pending_windows: Vec::new(),
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
    let text = "这是一个没有句末标点的超长段落，用来验证插入文字只会影响附近的缓存分块而不是整段"
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
