//! UTF-16 ↔ UTF-8 偏移转换
//!
//! Qt 侧使用 UTF-16 offset（QTextCursor、QInputMethodEvent），
//! Rust 内部使用 UTF-8 byte offset。转换只在此模块做一次。

/// UTF-16 offset → UTF-8 byte offset
pub fn utf16_to_utf8_offset(text: &str, utf16_offset: usize) -> usize {
    let mut utf8_pos = 0;
    let mut utf16_count = 0;
    for ch in text.chars() {
        if utf16_count >= utf16_offset {
            break;
        }
        utf16_count += ch.len_utf16() as usize;
        utf8_pos += ch.len_utf8();
    }
    utf8_pos
}

/// UTF-8 byte offset → UTF-16 offset
pub fn utf8_to_utf16_offset(text: &str, utf8_offset: usize) -> usize {
    let mut remaining = utf8_offset;
    let mut utf16_count = 0;
    for ch in text.chars() {
        if remaining == 0 {
            break;
        }
        let ch_len = ch.len_utf8();
        if ch_len > remaining {
            break;
        }
        remaining -= ch_len;
        utf16_count += ch.len_utf16() as usize;
    }
    utf16_count
}

#[cfg(test)]
mod tests {
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
        assert_eq!(text.len_utf8(), 4);
        assert_eq!(text.len_utf16(), 2);
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
}
