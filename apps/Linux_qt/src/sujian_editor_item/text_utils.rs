//! 纯文本转换工具 — UTF-8 byte offset / char boundary 归一化与对齐。
//!
//! 所有函数都是纯计算，不触碰任何编辑器状态。
//! 正文真相由 EditorKernel 持有，CommittedTextMirror 作为 Qt 只读平台投影。

/// 将富文本/平台文本归一化为编辑器纯文本。
///
/// 规则：段落分隔符 U+2029 → `\n`，CRLF/CR → `\n`，
/// 保留 `\n` 和 `\t`，过滤其余控制字符。
/// 归一化后的文本可直接写入 `chapter.md`（正文永远是纯文本）。
pub fn normalize_plain_text(text: &str) -> String {
    let replaced = text
        .replace('\u{2029}', "\n")
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    replaced
        .chars()
        .filter(|&c| c == '\n' || c == '\t' || !c.is_control())
        .collect()
}

/// 返回 `index` 之前最近的 char boundary（UTF-8 byte offset）。
///
/// 用于退格删除：定位前一个字符的起始字节位置。
/// 如果 `index` 为 0 或文本为空则返回 `None`。
pub fn prev_char_boundary(text: &str, index: usize) -> Option<usize> {
    if index == 0 || text.is_empty() {
        return None;
    }
    text.char_indices()
        .map(|(idx, _)| idx)
        .take_while(|idx| *idx < index)
        .last()
}

/// 返回 `index` 之后最近的 char boundary（UTF-8 byte offset）。
///
/// 用于 Delete 键：定位后一个字符的起始字节位置。
/// 如果 `index` 已在文本末尾则返回 `None`；末尾字符之后返回 `text.len()`。
pub fn next_char_boundary(text: &str, index: usize) -> Option<usize> {
    if index >= text.len() {
        return None;
    }
    text.char_indices()
        .map(|(idx, _)| idx)
        .find(|idx| *idx > index)
        .or(Some(text.len()))
}

/// 将 UTF-8 byte offset 对齐到最近的 char boundary。
///
/// 超出文本长度时返回 `text.len()`；落在多字节字符中间时向左回退。
/// 所有外部输入的 offset（来自 IME、光标移动等）都应经过此函数校验。
pub fn clamp_to_char_boundary(text: &str, index: usize) -> usize {
    if index > text.len() {
        return text.len();
    }
    if text.is_char_boundary(index) {
        return index;
    }
    let mut clamped = index;
    while clamped > 0 && !text.is_char_boundary(clamped) {
        clamped -= 1;
    }
    clamped
}

/// UTF-8 byte offset → 字符索引（char count）。
///
/// 先将 byte offset 对齐到 char boundary，再计算 `[0..offset)` 范围内的字符数。
/// 用于需要字符级计数的场景（如 Android StaticLayout）。
pub fn byte_to_char_index(text: &str, byte_index: usize) -> usize {
    text[..clamp_to_char_boundary(text, byte_index)]
        .chars()
        .count()
}

#[cfg(test)]
pub fn byte_index_at_char_offset_in_range(
    text: &str,
    start: usize,
    end: usize,
    char_offset: usize,
) -> usize {
    if char_offset == 0 {
        return start;
    }
    for (offset, (byte, _)) in text[start..end].char_indices().enumerate() {
        if offset == char_offset {
            return start + byte;
        }
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_char_offset_does_not_jump_to_document_end() {
        let text = "第一行\n第二行";
        let line_end = "第一行".len();

        assert_eq!(
            byte_index_at_char_offset_in_range(text, 0, line_end, 3),
            line_end
        );
    }

    #[test]
    fn test_next_char_boundary_empty() {
        assert_eq!(next_char_boundary("", 0), None);
        assert_eq!(next_char_boundary("", 1), None);
    }

    #[test]
    fn test_next_char_boundary_ascii() {
        let text = "abc";
        assert_eq!(next_char_boundary(text, 0), Some(1));
        assert_eq!(next_char_boundary(text, 1), Some(2));
        assert_eq!(next_char_boundary(text, 2), Some(3));
        assert_eq!(next_char_boundary(text, 3), None);
        assert_eq!(next_char_boundary(text, 10), None);
    }

    #[test]
    fn test_next_char_boundary_utf8() {
        let text = "你好"; // 3 bytes each
        assert_eq!(next_char_boundary(text, 0), Some(3));
        assert_eq!(next_char_boundary(text, 1), Some(3));
        assert_eq!(next_char_boundary(text, 2), Some(3));
        assert_eq!(next_char_boundary(text, 3), Some(6));
        assert_eq!(next_char_boundary(text, 4), Some(6));
        assert_eq!(next_char_boundary(text, 6), None);
        assert_eq!(next_char_boundary(text, 10), None);
    }
}
