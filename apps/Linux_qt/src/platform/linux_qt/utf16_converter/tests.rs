//! utf16_converter.rs 的单元测试。
//!
//! 按仓库结构门禁（`tools/check_source_structure.py` 的
//! production-test-bloat 规则）从生产文件拆出，测试逻辑保持不变。
use super::*;

#[test]
fn ascii_roundtrip() {
    let text = "hello";
    assert_eq!(utf16_to_utf8_offset(text, 3), 3);
    assert_eq!(utf8_to_utf16_offset(text, 3), 3);
}

#[test]
fn chinese_roundtrip() {
    let text = "你好世界";
    assert_eq!(utf16_to_utf8_offset(text, 2), 6);
    assert_eq!(utf8_to_utf16_offset(text, 6), 2);
}

#[test]
fn mixed_roundtrip() {
    let text = "hi你好";
    assert_eq!(utf16_to_utf8_offset(text, 3), 5);
    assert_eq!(utf8_to_utf16_offset(text, 5), 3);
}

#[test]
fn emoji_surrogate_pair() {
    let text = "😀";
    assert_eq!(text.len(), 4);
    assert_eq!(text.encode_utf16().count(), 2);
    assert_eq!(utf16_to_utf8_offset(text, 0), 0);
    assert_eq!(utf16_to_utf8_offset(text, 1), 0);
    assert_eq!(utf16_to_utf8_offset(text, 2), 4);
    assert_eq!(utf8_to_utf16_offset(text, 0), 0);
    assert_eq!(utf8_to_utf16_offset(text, 4), 2);
}

#[test]
fn utf8_mid_character_clamps_to_boundary() {
    let text = "a😀b";
    assert_eq!(utf8_to_utf16_offset(text, 2), 1);
    assert_eq!(utf8_to_utf16_offset(text, 3), 1);
    assert_eq!(utf8_to_utf16_offset(text, 4), 1);
    assert_eq!(utf8_to_utf16_offset(text, 5), 3);
}

#[test]
fn utf16_mid_surrogate_clamps_to_boundary() {
    let text = "😀";
    assert_eq!(utf16_to_utf8_offset(text, 1), 0);
    assert_eq!(utf16_to_utf8_offset(text, 2), 4);
}

#[test]
fn boundary_beyond_text() {
    let text = "abc";
    assert_eq!(utf16_to_utf8_offset(text, 100), 3);
    assert_eq!(utf8_to_utf16_offset(text, 100), 3);
}

#[test]
fn forward_zero_returns_aligned_start() {
    let text = "abc";
    assert_eq!(utf16_forward_from_byte(text, 1, 0), 1);
    assert_eq!(utf16_forward_from_byte(text, 0, 0), 0);
}

#[test]
fn forward_ascii() {
    let text = "hello";
    assert_eq!(utf16_forward_from_byte(text, 1, 2), 3);
    assert_eq!(utf16_forward_from_byte(text, 0, 5), 5);
}

#[test]
fn forward_chinese() {
    let text = "你好世界";
    // 每个 CJK 字符 1 UTF-16 code unit, 3 UTF-8 bytes
    assert_eq!(utf16_forward_from_byte(text, 0, 2), 6);
    assert_eq!(utf16_forward_from_byte(text, 3, 1), 6);
}

#[test]
fn forward_emoji_surrogate_pair() {
    let text = "a😀b";
    // 😀 是 2 UTF-16 code unit, 4 UTF-8 bytes（byte 1..5）
    // 从 byte 1（😀 起始）走 1 个 UTF-16 code unit：😀 占 2 个 code unit，
    // 走 1 个不够跨过整个字符，停在 😀 起点 byte 1（不推进 pos）。
    // Issue #701 评论 5702214893: forward 在代理对中间停在字符起点。
    assert_eq!(utf16_forward_from_byte(text, 1, 1), 1);
    // 走 2 个 UTF-16 code unit：正好跨过 😀，到达 byte 5。
    assert_eq!(utf16_forward_from_byte(text, 1, 2), 5);
    // 走 3 个 UTF-16 code unit：跨过 😀（2）+ 'b'（1），到达 byte 6。
    assert_eq!(utf16_forward_from_byte(text, 1, 3), 6);
}

#[test]
fn forward_beyond_end_clamps() {
    let text = "abc";
    assert_eq!(utf16_forward_from_byte(text, 1, 100), 3);
}

#[test]
fn forward_mid_byte_aligns_to_boundary() {
    let text = "a😀b";
    // byte 2 落在 😀 中间，对齐到 byte 1
    assert_eq!(utf16_forward_from_byte(text, 2, 0), 1);
    assert_eq!(utf16_forward_from_byte(text, 3, 0), 1);
}

#[test]
fn backward_zero_returns_aligned_start() {
    let text = "abc";
    assert_eq!(utf16_backward_from_byte(text, 2, 0), 2);
}

#[test]
fn backward_ascii() {
    let text = "hello";
    assert_eq!(utf16_backward_from_byte(text, 3, 2), 1);
    assert_eq!(utf16_backward_from_byte(text, 5, 5), 0);
}

#[test]
fn backward_chinese() {
    let text = "你好世界";
    assert_eq!(utf16_backward_from_byte(text, 6, 2), 0);
    assert_eq!(utf16_backward_from_byte(text, 6, 1), 3);
}

#[test]
fn backward_emoji_surrogate_pair() {
    let text = "a😀b";
    // 从 byte 5（'b' 起始 = 😀 结束）向后走 1 个 UTF-16 code unit：
    // 😀 占 2 个 code unit，一次跨过整个字符，到达 byte 1（😀 起始）。
    assert_eq!(utf16_backward_from_byte(text, 5, 1), 1);
    // 向后走 2 个 UTF-16 code unit：正好跨过 😀，到达 byte 1。
    assert_eq!(utf16_backward_from_byte(text, 5, 2), 1);
    // 从 byte 6（末尾）向后走 3 个 UTF-16 code unit：
    // 'b'（1）+ 😀（2）= 3，跨过后到达 byte 1（😀 起始 = 'a' 结束）。
    assert_eq!(utf16_backward_from_byte(text, 6, 3), 1);
}

#[test]
fn backward_mid_byte_aligns_to_boundary() {
    let text = "a😀b";
    // byte 3 落在 😀 中间，对齐到 byte 1
    assert_eq!(utf16_backward_from_byte(text, 3, 0), 1);
}

#[test]
fn forward_then_backward_roundtrip() {
    let text = "hi你好😀";
    let pos = utf16_forward_from_byte(text, 0, 4);
    assert_eq!(pos, 8);
    let back = utf16_backward_from_byte(text, pos, 4);
    assert_eq!(back, 0);
}
