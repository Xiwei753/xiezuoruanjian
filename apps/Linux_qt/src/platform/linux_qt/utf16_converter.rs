//! UTF-16 ↔ UTF-8 偏移转换
//!
//! Qt 侧使用 UTF-16 code unit offset（QTextCursor、QInputMethodEvent），
//! Rust 内部使用 UTF-8 byte offset。转换只在此模块做一次。
//!
//! Issue #701 评论 5699569220: IME replacement 的 UTF-16→UTF-8 坐标换算
//! 只在此模块和 `platform_ime.rs` 做一次。`editing.rs` 不再二次换算。
//!
//! ## 边界对齐（clamping）行为
//!
//! 当输入 offset 落在多字节字符内部时，函数会自动对齐到最近的字符边界：
//! - `utf16_to_utf8_offset`：UTF-16 offset 落在代理对中间时，对齐到该字符的起始位置
//! - `utf8_to_utf16_offset`：UTF-8 byte offset 落在多字节序列中间时，对齐到该字符的起始位置
//!
//! 超出文本长度的 offset 会被 clamp 到文本末尾。
//!
//! Issue #701 评论 5702214893: `utf16_forward_from_byte` 在代理对中间时停在
//! 字符起点而非跨过整个字符。当 `remaining < ch.len_utf16()`（例如 emoji 占
//! 2 个 UTF-16 code unit，但只走 1 个）时，不推进 `pos`，`break` 停在当前
//! 字符起点。只有 `remaining >= ch.len_utf16()` 才跨过整个字符。这保证
//! forward 在代理对中间的 offset 仍对齐到字符边界，与 backward 行为一致。

/// 从 `byte_start` 出发，在 `text` 上向前走 `utf16_count` 个 UTF-16 code unit，
/// 返回到达的 UTF-8 byte offset。
///
/// `byte_start` 会被 clamp 到 `[0, text.len()]` 且对齐到字符边界。
/// 超出文本末尾时返回 `text.len()`。`utf16_count == 0` 时返回对齐后的
/// `byte_start`。
///
/// 用于把 Qt `QInputMethodEvent::replacementStart`（相对 preedit 起点的
/// UTF-16 QChar 偏移）在 base_text 上换算成 UTF-8 byte offset。
pub fn utf16_forward_from_byte(text: &str, byte_start: usize, utf16_count: usize) -> usize {
    let start = align_to_char_boundary(text, byte_start);
    if utf16_count == 0 {
        return start;
    }
    let mut remaining = utf16_count;
    let mut pos = start;
    for ch in text[start..].chars() {
        if remaining == 0 {
            break;
        }
        let utf16_len = ch.len_utf16();
        if remaining < utf16_len {
            // 走不到整个字符（如 emoji 代理对只走 1 个 code unit），
            // 停在当前字符起点，不推进 pos。Issue #701 评论 5702214893。
            break;
        }
        remaining -= utf16_len;
        pos += ch.len_utf8();
    }
    pos
}

/// 从 `byte_start` 出发，在 `text` 上向后走 `utf16_count` 个 UTF-16 code unit，
/// 返回到达的 UTF-8 byte offset。
///
/// `byte_start` 会被 clamp 到 `[0, text.len()]` 且对齐到字符边界。
/// `utf16_count == 0` 时返回对齐后的 `byte_start`。
///
/// 用于把 Qt `QInputMethodEvent::replacementStart`（负值，相对 preedit 起点
/// 向前的 UTF-16 QChar 偏移）在 base_text 上换算成 UTF-8 byte offset。
pub fn utf16_backward_from_byte(text: &str, byte_start: usize, utf16_count: usize) -> usize {
    let start = align_to_char_boundary(text, byte_start);
    if utf16_count == 0 {
        return start;
    }
    let mut remaining = utf16_count;
    let mut pos = start;
    for ch in text[..start].chars().rev() {
        if remaining == 0 {
            break;
        }
        let utf16_len = ch.len_utf16();
        remaining = remaining.saturating_sub(utf16_len);
        pos -= ch.len_utf8();
    }
    pos
}

/// 把 `byte_offset` clamp 到 `[0, text.len()]` 并对齐到最近的字符起始边界。
fn align_to_char_boundary(text: &str, byte_offset: usize) -> usize {
    if byte_offset >= text.len() {
        text.len()
    } else if text.is_char_boundary(byte_offset) {
        byte_offset
    } else {
        let mut clamped = byte_offset;
        while clamped > 0 && !text.is_char_boundary(clamped) {
            clamped -= 1;
        }
        clamped
    }
}

/// UTF-16 offset → UTF-8 byte offset
#[cfg(test)]
pub fn utf16_to_utf8_offset(text: &str, utf16_offset: usize) -> usize {
    let mut utf8_pos = 0;
    let mut utf16_count = 0;
    for ch in text.chars() {
        if utf16_count >= utf16_offset {
            break;
        }
        utf16_count += ch.len_utf16();
        if utf16_count > utf16_offset {
            break;
        }
        utf8_pos += ch.len_utf8();
    }
    utf8_pos
}

/// UTF-8 byte offset → UTF-16 offset
#[cfg(test)]
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
        utf16_count += ch.len_utf16();
    }
    utf16_count
}

#[cfg(test)]
mod tests;
