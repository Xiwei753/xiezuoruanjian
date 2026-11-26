//! 纯文本转换工具 — Core 文本坐标规则的 Qt 薄适配层（Issue #1063）。
//!
//! UTF-8 byte ↔ UTF-16/char 换算、char boundary 钳位与导入文本规范化只维护
//! Core `writer_core::editor::text_coordinates` / `text_document_rules` 一套规则；
//! 这里只做调用转发，不再写第二套边界扫描。
//!
//! 所有函数都是纯计算，不触碰任何编辑器状态。
//! 正文真相由 EditorKernel 持有，CommittedTextMirror 作为 Qt 只读平台投影。

/// 将富文本/平台文本归一化为编辑器纯文本。
///
/// Issue #1063：规则由 Core [`writer_core::editor::text_document_rules::normalize_inserted_text`]
/// 唯一定义：段落分隔符 U+2029 / CRLF / 孤立 CR → `\n`，保留 `\n` 与 `\t`，
/// 过滤其余控制字符。归一化后的文本可直接写入 `chapter.md`（正文永远是纯文本）。
pub fn normalize_plain_text(text: &str) -> String {
    writer_core::editor::text_document_rules::normalize_inserted_text(text)
}

/// 返回 `index` 之前最近的 char boundary（UTF-8 byte offset）。
///
/// Issue #1063：光标/删除已统一改用 Core grapheme 边界
/// （`next_grapheme_boundary` / `previous_grapheme_boundary`），生产路径不再
/// 按 Unicode scalar 步进。此函数只保留给本模块既有单元测试，语义不变。
#[cfg(test)]
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
/// Issue #1063：生产路径已统一改用 Core grapheme 边界；此函数只保留给本模块
/// 既有单元测试，语义不变。
#[cfg(test)]
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
/// 直接委托 Core [`writer_core::editor::text_coordinates::clamp_to_char_boundary`]：
/// 越界返回 `text.len()`；落在多字节字符中间时向左回退到字符起始。
/// 所有外部输入的 offset（来自 IME、光标移动等）都应经过此函数校验。
pub use writer_core::editor::text_coordinates::clamp_to_char_boundary;

/// UTF-8 byte offset → 字符索引（char count）。
///
/// 先用 Core `clamp_to_char_boundary` 对齐 offset，再计算 `[0..offset)` 范围内的
/// 字符数。用于需要字符级计数的场景（如 Android StaticLayout）。
pub fn byte_to_char_index(text: &str, byte_index: usize) -> usize {
    text[..clamp_to_char_boundary(text, byte_index)]
        .chars()
        .count()
}

/// 字符索引（char count）→ UTF-8 byte offset。
///
/// Issue #1047：QML 的 `cursor_position` 是 Unicode 字符索引；恢复章节阅读
/// 位置时必须先转成 Rust 正文的 byte offset，不能把 Qt 的 UTF-16 偏移直接
/// 当 byte offset。超界时钳到文本末尾，返回值一定是合法 char boundary。
pub fn char_to_byte_index(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map(|(byte_index, _)| byte_index)
        .unwrap_or(text.len())
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
