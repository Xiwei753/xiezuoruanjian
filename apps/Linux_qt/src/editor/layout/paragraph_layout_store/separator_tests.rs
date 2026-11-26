//! Issue #1063: 段落坐标必须按真实分隔符长度推进（`\n`=1、`CRLF`=2、`U+2029`=3、无=0）。

use super::*;
use writer_core::editor::text_document_rules;

fn rebuild(store: &mut ParagraphLayoutStore, text: &str) {
    store.rebuild_from_text(text, 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
}

#[test]
fn split_paragraphs_reports_real_separator_len() {
    assert_eq!(
        split_paragraphs_with_separator("a\r\nb"),
        vec![("a", 2), ("b", 0)]
    );
    assert_eq!(
        split_paragraphs_with_separator("a\u{2029}b"),
        vec![("a", 3), ("b", 0)]
    );
    assert_eq!(
        split_paragraphs_with_separator("a\r\n"),
        vec![("a", 2), ("", 0)]
    );
    assert_eq!(split_paragraphs_with_separator(""), vec![("", 0)]);
}

#[test]
fn rebuild_uses_real_separator_bytes_for_offsets() {
    let mut store = ParagraphLayoutStore::new();
    rebuild(&mut store, "a\r\nb");
    assert_eq!(store.entry(0).unwrap().separator_len, 2);
    assert_eq!(store.entry(0).unwrap().byte_len(), 3);
    assert_eq!(store.compute_document_byte_start(1), 3);

    let mut store = ParagraphLayoutStore::new();
    rebuild(&mut store, "a\u{2029}b");
    assert_eq!(store.entry(0).unwrap().separator_len, 3);
    assert_eq!(store.entry(0).unwrap().byte_len(), 4);
    assert_eq!(store.compute_document_byte_start(1), 4);

    // 尾空段起点 = 正文 + 分隔符 = 3，尾空段本身无分隔符。
    let mut store = ParagraphLayoutStore::new();
    rebuild(&mut store, "a\r\n");
    assert_eq!(store.paragraph_count(), 2);
    assert_eq!(store.compute_document_byte_start(1), 3);
    assert_eq!(store.entry(1).unwrap().separator_len, 0);
    assert!(!store.entry(1).unwrap().hard_break);
}

#[test]
fn edit_in_second_paragraph_keeps_prefix_sums() {
    for (text, expected_start) in [("a\r\nb", 3usize), ("a\u{2029}b", 4)] {
        let mut store = ParagraphLayoutStore::new();
        rebuild(&mut store, text);
        // 在第二段中插入 "X"：分隔符长度不变。
        store.update_paragraph_text(1, "bX", 0, 16.0, "Sans", 1.2, 0.0, 0.0, 600.0);
        assert_eq!(store.compute_document_byte_start(1), expected_start);
        assert_eq!(store.entry(1).unwrap().separator_len, 0);
        // 前缀和（段落 byte 总长）等于新全文长度。
        let total: usize = (0..store.paragraph_count())
            .map(|i| store.entry(i).unwrap().byte_len())
            .sum();
        assert_eq!(total, text.len() + 1);
        assert_eq!(
            store.compute_document_byte_start(1),
            store.entry(0).unwrap().byte_len()
        );
    }
}

#[test]
fn offsets_match_core_spans_on_mixed_separators() {
    let text = "a\r\nb\u{2029}c\n";
    let mut store = ParagraphLayoutStore::new();
    rebuild(&mut store, text);
    let spans = text_document_rules::paragraph_byte_spans(text);
    assert_eq!(store.paragraph_count(), spans.len());
    for (i, span) in spans.iter().enumerate() {
        assert_eq!(store.compute_document_byte_start(i), span.content.0);
        assert_eq!(store.entry(i).unwrap().separator_len, span.separator_len());
        assert_eq!(store.entry(i).unwrap().byte_len(), span.byte_len());
    }
}

#[test]
fn affected_range_and_locator_use_real_start() {
    for (text, edit_start, edit_end, expected_start) in
        [("a\r\nb", 3usize, 4usize, 3usize), ("a\u{2029}b", 4, 5, 4)]
    {
        let mut store = ParagraphLayoutStore::new();
        rebuild(&mut store, text);
        let ranges = store.compute_affected_paragraphs(edit_start, edit_end);
        assert!(!ranges.full_relayout);
        assert_eq!(ranges.relayout_start, 1);
        assert_eq!(ranges.relayout_end, 2);
        assert_eq!(store.compute_document_byte_start(1), expected_start);
    }
}

#[test]
fn split_and_merge_track_real_separator_len() {
    // 拆段：在 "abc" 的第 1 byte 后插入 U+2029（3 byte）。
    let mut store = ParagraphLayoutStore::new();
    rebuild(&mut store, "abc");
    store
        .split_paragraph(0, 1, 3, 16.0, "Sans", 1.2, 0.0, 0.0, 600.0, "a", "bc")
        .unwrap();
    assert_eq!(store.entry(0).unwrap().separator_len, 3);
    assert_eq!(store.entry(1).unwrap().separator_len, 0);
    assert_eq!(store.compute_document_byte_start(1), 4);

    // 合段：合并 "a\r\n" 与 "bc"，合并后尾部分隔符取被合并段（无）。
    let mut store = ParagraphLayoutStore::new();
    rebuild(&mut store, "a\r\nbc");
    store
        .merge_paragraphs(0, 16.0, "Sans", 1.2, 0.0, 0.0, 600.0, "abc")
        .unwrap();
    assert_eq!(store.paragraph_count(), 1);
    assert_eq!(store.entry(0).unwrap().separator_len, 0);
    assert_eq!(store.entry(0).unwrap().byte_len(), 3);
}
