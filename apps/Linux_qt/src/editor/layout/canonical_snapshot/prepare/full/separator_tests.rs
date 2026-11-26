//! Issue #1063: canonical 段落 offset 索引必须按真实分隔符长度维护（byte + qchar）。

use super::super::super::super::paragraph_layout_store::ParagraphLayoutStore;
use super::*;
use std::sync::Arc;

fn rebuild_store(store: &mut ParagraphLayoutStore, text: &str) {
    store.rebuild_from_text(text, 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
}

#[test]
fn full_rebuild_offset_index_uses_real_separators() {
    let idx = build_offset_index_from_text("a\r\nb");
    assert_eq!(idx.len(), 2);
    // CRLF = 2 byte / 2 UTF-16 单元。
    assert_eq!(idx.byte_length_at(0), 3);
    assert_eq!(idx.qchar_length_at(0), 3);
    assert_eq!(idx.paragraph_byte_start(1), 3);
    assert_eq!(idx.paragraph_qchar_start(1), 3);

    let idx = build_offset_index_from_text("a\u{2029}b");
    assert_eq!(idx.len(), 2);
    // U+2029 = 3 byte / 1 UTF-16 单元。
    assert_eq!(idx.byte_length_at(0), 4);
    assert_eq!(idx.qchar_length_at(0), 2);
    assert_eq!(idx.paragraph_byte_start(1), 4);
    assert_eq!(idx.paragraph_qchar_start(1), 2);
}

#[test]
fn incremental_offset_index_matches_rebuild_for_crlf_edit() {
    // 旧 "a\r\nb" → 在第二段插入 "X" → 新 "a\r\nbX"（段落数不变）。
    let old_text = "a\r\nb";
    let new_text = "a\r\nbX";
    let mut store = ParagraphLayoutStore::new();
    rebuild_store(&mut store, old_text);
    store.update_paragraph_text(1, "bX", 0, 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);

    let affected = vec![paragraph_span_at(new_text, 3, 2, 0)];
    let source = ParasSource::Incremental {
        prefix_count: 1,
        suffix_count: 0,
        affected: affected.clone(),
    };
    let idx = update_offset_index_incremental(
        Arc::new(build_offset_index_from_text(old_text)),
        2,
        2,
        0,
        0,
        1,
        2,
        &affected,
        &source,
    );
    assert!(offset_index_matches_rebuild(&idx, new_text));
    assert_eq!(idx.byte_length_at(1), 2);
    assert_eq!(idx.qchar_length_at(1), 2);
}

#[test]
fn incremental_offset_index_matches_rebuild_for_u2029_edit() {
    // 旧 "a\u{2029}b" → 第二段插入 "X" → 新 "a\u{2029}bX"。
    let old_text = "a\u{2029}b";
    let new_text = "a\u{2029}bX";
    let affected = vec![paragraph_span_at(new_text, 4, 2, 0)];
    let source = ParasSource::Incremental {
        prefix_count: 1,
        suffix_count: 0,
        affected: affected.clone(),
    };
    let idx = update_offset_index_incremental(
        Arc::new(build_offset_index_from_text(old_text)),
        2,
        2,
        0,
        0,
        1,
        2,
        &affected,
        &source,
    );
    assert!(offset_index_matches_rebuild(&idx, new_text));
    // 第二段正文 2 个 UTF-16 单元，分隔符 0。
    assert_eq!(idx.byte_length_at(1), 2);
    assert_eq!(idx.qchar_length_at(1), 2);
    // 第一段：正文 "a" + U+2029（1 单元）。
    assert_eq!(idx.qchar_length_at(0), 2);
}

#[test]
fn incremental_offset_index_matches_rebuild_on_split() {
    // 旧 "a\r\nb" → 末尾回车新增 "c" → 新 "a\r\nb\r\nc"（段落数 2→3）。
    let old_text = "a\r\nb";
    let new_text = "a\r\nb\r\nc";
    let affected = vec![
        paragraph_span_at(new_text, 3, 1, 2),
        paragraph_span_at(new_text, 6, 1, 0),
    ];
    let source = ParasSource::Incremental {
        prefix_count: 1,
        suffix_count: 0,
        affected: affected.clone(),
    };
    let idx = update_offset_index_incremental(
        Arc::new(build_offset_index_from_text(old_text)),
        3,
        2,
        1,
        2,
        1,
        3,
        &affected,
        &source,
    );
    assert!(offset_index_matches_rebuild(&idx, new_text));
    assert_eq!(idx.len(), 3);
    assert_eq!(idx.byte_length_at(1), 3);
    assert_eq!(idx.qchar_length_at(1), 3);
    assert_eq!(idx.byte_length_at(2), 1);
}
