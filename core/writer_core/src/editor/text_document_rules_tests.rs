//! `text_document_rules` 的单元测试（从生产文件拆出，避免内嵌测试模块超限）。

use super::*;

#[test]
fn normalizes_newlines_and_control_chars() {
    assert_eq!(normalize_inserted_text("a\u{2029}b"), "a\nb");
    assert_eq!(normalize_inserted_text("a\r\nb"), "a\nb");
    assert_eq!(normalize_inserted_text("a\rb"), "a\nb");
    assert_eq!(normalize_inserted_text("a\n\tb\u{0007}"), "a\n\tb");
    // 全角空格/NBSP 不是控制字符，保留。
    assert_eq!(
        normalize_inserted_text("a\u{3000}\u{00A0}b"),
        "a\u{3000}\u{00A0}b"
    );
}

#[test]
fn normalization_is_idempotent() {
    let once = normalize_inserted_text("a\r\nb\u{2029}c\u{0001}");
    assert_eq!(normalize_inserted_text(&once), once);
}

#[test]
fn crlf_is_one_paragraph_boundary() {
    assert_eq!(paragraph_byte_ranges("ab\r\ncd"), vec![(0, 2), (4, 6)]);
    assert_eq!(paragraph_start_offsets("ab\r\ncd"), vec![0, 4]);
}

#[test]
fn lone_cr_is_not_a_boundary() {
    assert_eq!(paragraph_byte_ranges("ab\rcd"), vec![(0, 5)]);
    assert_eq!(paragraph_separator_len_at("ab\rcd", 2), None);
}

#[test]
fn u2029_is_a_boundary() {
    assert_eq!(paragraph_byte_ranges("a\u{2029}b"), vec![(0, 1), (4, 5)]);
}

#[test]
fn empty_paragraphs_and_trailing_newline() {
    assert_eq!(
        paragraph_byte_ranges("a\n\nb\n"),
        vec![(0, 1), (2, 2), (3, 4), (5, 5)]
    );
    assert!(paragraph_byte_ranges("").is_empty());
}

#[test]
fn spans_match_ranges() {
    let text = "第一段\r\n第二段\n\n\u{2029}尾段";
    let spans: Vec<&str> = paragraph_spans(text).collect();
    for (span, (start, end)) in spans.iter().zip(paragraph_byte_ranges(text)) {
        assert_eq!(*span, &text[start..end]);
    }
    assert_eq!(spans, vec!["第一段", "第二段", "", "", "尾段"]);
}

#[test]
fn spans_keep_real_separator_ranges() {
    // CRLF 两个 byte 都是分隔符，正文结束于 \r 之前。
    let text = "a\r\nb";
    let spans = paragraph_byte_spans(text);
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].content, (0, 1));
    assert_eq!(spans[0].separator, (1, 3));
    assert_eq!(spans[0].separator_len(), 2);
    assert_eq!(spans[0].byte_len(), 3);
    assert_eq!(spans[1].content, (3, 4));
    assert_eq!(spans[1].separator, (4, 4));
    assert_eq!(spans[1].byte_len(), 1);

    // U+2029 是 3 byte 分隔符。
    let text = "a\u{2029}b";
    let spans = paragraph_byte_spans(text);
    assert_eq!(spans[0].content, (0, 1));
    assert_eq!(spans[0].separator, (1, 4));
    assert_eq!(spans[0].separator_len(), 3);
    assert_eq!(spans[1].content, (4, 5));

    // 末尾分隔符产生尾空段，尾空段没有分隔符。
    let spans = paragraph_byte_spans("a\r\n");
    assert_eq!(spans.len(), 2);
    assert_eq!(spans[0].separator, (1, 3));
    assert_eq!(spans[1].content, (3, 3));
    assert!(!spans[1].has_separator());
}

#[test]
fn separator_len_at_reports_utf8_lengths() {
    assert_eq!(paragraph_separator_len_at("a\nb", 1), Some(1));
    assert_eq!(paragraph_separator_len_at("a\r\nb", 1), Some(2));
    assert_eq!(paragraph_separator_len_at("a\u{2029}b", 1), Some(3));
    assert_eq!(paragraph_separator_len_at("a\nb", 0), None);
    assert_eq!(paragraph_separator_len_at("a\nb", 3), None);
    assert_eq!(paragraph_separator_len_at("", 0), None);
}

#[test]
fn start_before_and_end_after_expand_to_paragraph_boundaries() {
    let text = "aa\r\nbb\u{2029}cc\n";
    // 第二段正文起点 4。
    assert_eq!(paragraph_start_before(text, 5), 4);
    assert_eq!(paragraph_start_before(text, 4), 4);
    // 落在 CRLF 内部：属于第一段，段尾到 CRLF 之后。
    assert_eq!(paragraph_start_before(text, 2), 0);
    assert_eq!(paragraph_end_after(text, 2), 4);
    // 普通段尾：U+2029 结束处。
    assert_eq!(paragraph_end_after(text, 5), 9);
    // U+2029 之后：第三段从 9 开始。
    assert_eq!(paragraph_start_before(text, 10), 9);
    assert_eq!(paragraph_end_after(text, 10), 12);
    // 文末与越界。
    assert_eq!(paragraph_start_before(text, text.len()), 12);
    assert_eq!(paragraph_end_after(text, text.len()), text.len());
    assert_eq!(paragraph_start_before(text, 999), 12);
    assert_eq!(paragraph_end_after(text, 999), text.len());
    // 非 char boundary 左对齐，不 panic。
    assert_eq!(paragraph_start_before("甲\n乙", 1), 0);
    assert_eq!(paragraph_end_after("甲\n乙", 1), 4);
}
