//! 正文文本的规范化与段落规则（Issue #1063）。
//!
//! 这里只定义两个**互相独立**的操作，避免把「写入前规范化」和「读磁盘原文分段」
//! 混成一种规则：
//!
//! 1. [`normalize_inserted_text`]：导入/粘贴/富文本带入正文前的换行与控制字符
//!    规范化（U+2029、CRLF、孤立 CR 统一成 `\n`，丢弃除 `\n`/`\t` 外的控制字符）。
//! 2. [`paragraph_byte_ranges`] / [`paragraph_start_offsets`]：把磁盘原文当作段落
//!    布局输入时的分隔符解释（`CRLF` 是一个段落边界，`\n`、`U+2029` 也是；
//!    孤立 `CR` 不是段落边界）。
//!
//! 二者都不能直接复用 [`crate::editor::formatting::format_paragraphs`]：那还会
//! 修剪段首/段尾空白，只适合显式「格式化正文」命令，不能用于开章或粘贴。
//! 本模块不依赖平台类型，只处理纯文本。

/// 导入/粘贴写入正文前的换行与控制字符规范化。
///
/// - `U+2029`（段落分隔符）、`CRLF`、孤立 `CR` 一律转成 `\n`；
/// - 保留 `\n` 与 `\t`；
/// - 其余 Unicode 控制字符（C0/C1 与 DEL）全部丢弃。
///
/// 规则与平台无关，各端不得再各写一份；幂等：对已规范化文本再调用结果不变。
pub fn normalize_inserted_text(text: &str) -> String {
    let replaced = text
        .replace('\u{2029}', "\n")
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    replaced
        .chars()
        .filter(|&c| c == '\n' || c == '\t' || !c.is_control())
        .collect()
}

/// 段落分隔符在 `bytes[index]` 处的 byte 长度；不是分隔符时返回 `None`。
///
/// 分隔符集合：`\n`、`CRLF`、`U+2029`。孤立 `CR` 不是段落边界。
fn separator_len_at(bytes: &[u8], index: usize) -> Option<usize> {
    match bytes[index] {
        b'\n' => Some(1),
        b'\r' if bytes.get(index + 1) == Some(&b'\n') => Some(2),
        0xE2 if bytes.get(index + 1) == Some(&0x80) && bytes.get(index + 2) == Some(&0xA9) => {
            Some(3)
        }
        _ => None,
    }
}

/// 按段落规则切分正文，返回每个段落的半开 byte range（不含分隔符）。
///
/// 空正文返回空；连续分隔符产生空段落（例如 `"a\n\nb"` 有三个段落 range：
/// `0..1`、`2..2`、`3..4`）；末尾分隔符产生一个末尾空段落（`"a\n"` →
/// `0..1`、`2..2`）。
pub fn paragraph_byte_ranges(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    if bytes.is_empty() {
        return ranges;
    }
    let mut start = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        if let Some(separator_len) = separator_len_at(bytes, index) {
            ranges.push((start, index));
            index += separator_len;
            start = index;
        } else {
            index += 1;
        }
    }
    ranges.push((start, bytes.len()));
    ranges
}

/// 按段落规则切分正文，按顺序给出每个段落的 `&str`。
pub fn paragraph_spans(text: &str) -> impl Iterator<Item = &str> {
    paragraph_byte_ranges(text)
        .into_iter()
        .map(move |(start, end)| &text[start..end])
}

/// 每个段落的起始 byte offset（含首段 0；空正文为空）。
///
/// 供平台插入文本后重算段落样式起点使用，规则与 [`paragraph_byte_ranges`] 相同。
pub fn paragraph_start_offsets(text: &str) -> Vec<usize> {
    paragraph_byte_ranges(text)
        .into_iter()
        .map(|(start, _)| start)
        .collect()
}

#[cfg(test)]
mod tests {
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
}
