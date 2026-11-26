//! 正文格式化（Core 层）— 平台无关的纯函数。
//!
//! Issue #920：正文格式化规则原属于 Qt `EditorController.qml::formatText()`，
//! 是各端共享的正文业务规则，移到 Core 后由各平台通过编辑会话提交流程调用，
//! 避免 Qt 拥有与其他端不同的格式化规则。
//!
//! 保留原 Qt 实现的事实规则：
//! - 以 `CRLF` / `LF` / `U+2029` 分段（单个 `CR` 不分段）；
//! - 去掉每段开头的 ASCII 空格 / Tab / 全角空格（U+3000）/ NBSP（U+00A0）；
//! - 去掉每段行尾的 ASCII 空格 / Tab（保留空行语义）；
//! - 统一用 `LF` 连接。
//!
//! 本模块不依赖任何平台 UI 类型，只接收、返回纯文本。

use crate::editor::text_document_rules::paragraph_byte_ranges;

/// 段落开头需要去除的空白字符：ASCII 空格、Tab、全角空格、NBSP。
fn is_leading_strippable(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\u{3000}' | '\u{00A0}')
}

/// 段落行尾需要去除的水平空白字符：ASCII 空格、Tab。
fn is_trailing_strippable(c: char) -> bool {
    matches!(c, ' ' | '\t')
}

/// 把正文按段落格式化：分段、去段首/段尾空白，再用 `LF` 连接。
///
/// 空段落被保留（连接后表现为连续换行），与 Qt 原实现一致。
pub fn format_paragraphs(text: &str) -> String {
    // 段落分隔符规则（`\n` / `CRLF` / `U+2029`，孤立 `CR` 不分段）由
    // `text_document_rules` 唯一维护，这里只做段落修剪与连接。
    let mut formatted: Vec<&str> = Vec::with_capacity(paragraph_byte_ranges(text).len());
    for (start, end) in paragraph_byte_ranges(text) {
        let paragraph = &text[start..end];
        let trimmed_start = paragraph.trim_start_matches(is_leading_strippable);
        // 空行不去尾部（与 Qt 的 `if (p.length > 0)` 一致），保留空段落。
        let trimmed = if trimmed_start.is_empty() {
            trimmed_start
        } else {
            trimmed_start.trim_end_matches(is_trailing_strippable)
        };
        formatted.push(trimmed);
    }

    formatted.join("\n")
}

#[cfg(test)]
mod tests {
    use super::format_paragraphs;

    #[test]
    fn strips_leading_and_trailing_whitespace_per_paragraph() {
        let input = "  hello  \n\t world \t\n";
        assert_eq!(format_paragraphs(input), "hello\nworld\n");
    }

    #[test]
    fn strips_full_width_space_and_nbsp_at_start() {
        let input = "\u{3000}\u{00A0}内容\t\n";
        // 行尾换行是空段落，连接后保留（与 Qt 一致）。
        assert_eq!(format_paragraphs(input), "内容\n");
    }

    #[test]
    fn normalizes_crlf_and_paragraph_separator_to_lf() {
        let input = "a\r\nb\u{2029}c";
        assert_eq!(format_paragraphs(input), "a\nb\nc");
    }

    #[test]
    fn preserves_empty_paragraphs_and_trailing_newline() {
        assert_eq!(format_paragraphs("a\n\nb\n"), "a\n\nb\n");
    }

    #[test]
    fn lone_carriage_return_is_not_a_separator() {
        assert_eq!(format_paragraphs("a\rb"), "a\rb");
    }

    #[test]
    fn does_not_strip_nbsp_or_full_width_at_line_end() {
        // 行尾只去 ASCII 空格/Tab；全角空格与 NBSP 保留。
        assert_eq!(format_paragraphs("x\u{3000}\u{00A0}"), "x\u{3000}\u{00A0}");
    }

    #[test]
    fn empty_input_stays_empty() {
        assert_eq!(format_paragraphs(""), "");
    }

    #[test]
    fn multibyte_text_with_separators_does_not_panic() {
        assert_eq!(
            format_paragraphs("  中文\u{2029}  另一段  "),
            "中文\n另一段"
        );
    }
}
