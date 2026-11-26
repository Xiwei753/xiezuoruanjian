//! `text_coordinates` 的单元测试（从生产文件拆出，避免内嵌测试模块超限）。

use super::*;

#[test]
fn clamps_utf8_offset_to_char_start() {
    let text = "a😀b";
    assert_eq!(clamp_to_char_boundary(text, 0), 0);
    assert_eq!(clamp_to_char_boundary(text, 1), 1);
    // 😀 占 byte 1..5，中间位置左对齐到 1。
    assert_eq!(clamp_to_char_boundary(text, 2), 1);
    assert_eq!(clamp_to_char_boundary(text, 4), 1);
    assert_eq!(clamp_to_char_boundary(text, 5), 5);
    assert_eq!(clamp_to_char_boundary(text, 99), text.len());
}

#[test]
fn strict_utf8_to_utf16_rejects_mid_char() {
    let text = "a😀b";
    assert_eq!(utf8_to_utf16(text, 0), Ok(0));
    assert_eq!(utf8_to_utf16(text, 1), Ok(1));
    assert_eq!(utf8_to_utf16(text, 5), Ok(3));
    assert_eq!(
        utf8_to_utf16(text, 2),
        Err(TextOffsetError::NotCharBoundary { offset: 2 })
    );
    assert_eq!(
        utf8_to_utf16(text, 99),
        Err(TextOffsetError::Utf8OutOfBounds {
            offset: 99,
            text_len: 6
        })
    );
}

#[test]
fn clamped_utf8_to_utf16_left_aligns() {
    let text = "a😀b";
    assert_eq!(utf8_to_utf16_clamped(text, 2), 1);
    assert_eq!(utf8_to_utf16_clamped(text, 4), 1);
    assert_eq!(utf8_to_utf16_clamped(text, 99), 4);
}

#[test]
fn strict_utf16_to_utf8_rejects_surrogate_middle() {
    let text = "a😀b";
    assert_eq!(utf16_to_utf8(text, 0), Ok(0));
    assert_eq!(utf16_to_utf8(text, 1), Ok(1));
    // 😀 是 UTF-16 位置 1..3，位置 2 在代理对中间。
    assert_eq!(
        utf16_to_utf8(text, 2),
        Err(TextOffsetError::NotUtf16Boundary { offset: 2 })
    );
    assert_eq!(utf16_to_utf8(text, 3), Ok(5));
    assert_eq!(utf16_to_utf8(text, 4), Ok(6));
    assert_eq!(
        utf16_to_utf8(text, 99),
        Err(TextOffsetError::Utf16OutOfBounds {
            offset: 99,
            text_utf16_len: 4
        })
    );
}

#[test]
fn clamped_utf16_to_utf8_maps_surrogate_middle_to_code_point_start() {
    let text = "a😀b";
    assert_eq!(utf16_to_utf8_clamped(text, 2), 1);
    assert_eq!(utf16_to_utf8_clamped(text, 99), text.len());
}

#[test]
fn range_validation_distinguishes_errors() {
    let text = "a😀b";
    assert!(validate_utf8_range(text, 0, text.len()).is_ok());
    assert_eq!(
        validate_utf8_range(text, 5, 1),
        Err(TextOffsetError::InvertedRange { start: 5, end: 1 })
    );
    assert_eq!(
        validate_utf8_range(text, 0, 2),
        Err(TextOffsetError::NotCharBoundary { offset: 2 })
    );
    assert_eq!(
        validate_utf8_range(text, 0, 99),
        Err(TextOffsetError::Utf8OutOfBounds {
            offset: 99,
            text_len: 6
        })
    );
}

#[test]
fn rope_clamp_matches_str_clamp() {
    for text in ["", "abc", "a😀b", "中文😀"] {
        let rope = Rope::from(text);
        for offset in 0..text.len() + 3 {
            assert_eq!(
                rope_clamp_to_char_boundary(&rope, offset),
                clamp_to_char_boundary(text, offset),
                "text={text:?} offset={offset}"
            );
        }
    }
}

#[test]
fn clamp_utf8_range_keeps_order() {
    assert_eq!(clamp_utf8_range("a😀b", 4, 2), (1, 1));
    assert_eq!(clamp_utf8_range("abc", 0, 3), (0, 3));
}
