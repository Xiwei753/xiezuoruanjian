//! Issue #1063: 段落边界扩展必须识别 `CRLF` / `U+2029`，不能只找 `'\n'`。

use super::compute_affected_paragraph_ranges;

/// 编辑第一段正文：new 侧范围要包含整段以及其后的 `U+2029`（3 byte）。
#[test]
fn u2029_edit_in_first_paragraph_includes_separator() {
    // old "a\u{2029}b"：a=0..1，U+2029=1..4，b=4..5。
    let (old_s, old_e, new_s, new_e) =
        compute_affected_paragraph_ranges("a\u{2029}b", "A\u{2029}b", (0, 1), (0, 1));
    assert_eq!((old_s, old_e), (0, 4));
    assert_eq!((new_s, new_e), (0, 4));
}

/// 编辑第二段正文：起点必须是 `U+2029` 之后的真实 byte offset（4），不是 1。
#[test]
fn u2029_edit_in_second_paragraph_uses_real_start() {
    let (old_s, old_e, new_s, new_e) =
        compute_affected_paragraph_ranges("a\u{2029}b", "a\u{2029}bX", (4, 4), (4, 5));
    assert_eq!((old_s, old_e), (4, 5));
    assert_eq!((new_s, new_e), (4, 6));
}

/// offset 落在 `CRLF` 内部（`'\n'` 字节上）不 panic，且段落范围包含整个分隔符。
#[test]
fn crlf_offset_inside_separator_no_panic() {
    // old "a\r\nb"：a=0，CRLF=1..3，b=3。
    let (old_s, old_e, new_s, new_e) =
        compute_affected_paragraph_ranges("a\r\nb", "a\r\nbX", (2, 2), (3, 4));
    assert_eq!((old_s, old_e), (0, 3));
    assert_eq!((new_s, new_e), (3, 5));
}

/// offset 落在 `U+2029` 内部（非 char boundary）时由 Core 左对齐，不 panic。
#[test]
fn u2029_offset_inside_separator_no_panic() {
    let (old_s, old_e, new_s, new_e) =
        compute_affected_paragraph_ranges("a\u{2029}b", "a\u{2029}b", (2, 2), (2, 2));
    assert_eq!((old_s, old_e), (0, 4));
    assert_eq!((new_s, new_e), (0, 4));
}
