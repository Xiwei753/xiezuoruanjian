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
    match bytes.get(index)? {
        b'\n' => Some(1),
        b'\r' if bytes.get(index + 1) == Some(&b'\n') => Some(2),
        0xE2 if bytes.get(index + 1) == Some(&0x80) && bytes.get(index + 2) == Some(&0xA9) => {
            Some(3)
        }
        _ => None,
    }
}

/// `byte_index` 处段落分隔符的 byte 长度；不是分隔符或越界时返回 `None`。
///
/// 平台定位分隔符/计算坐标时使用本函数，不要再假设分隔符固定为 1 byte：
/// `\n` 是 1、`CRLF` 是 2、`U+2029` 是 3，孤立 `CR` 不是分隔符。
pub fn paragraph_separator_len_at(text: &str, byte_index: usize) -> Option<usize> {
    separator_len_at(text.as_bytes(), byte_index)
}

/// 一个段落在原文中的 byte 描述：正文范围 + 紧随其后的分隔符范围。
///
/// `separator` 为半开区间 `(start, end)`；文末最后一段没有分隔符时
/// `separator.start == separator.end == content.1`。CRLF 的两个字节都属于
/// 分隔符，不属于前一段正文。排版/索引/命中测试必须用 [`Self::byte_len`]
/// 推进坐标，不能把「有分隔符」当成「分隔符 1 byte」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParagraphByteSpan {
    /// 段落正文（不含分隔符）的半开 byte range。
    pub content: (usize, usize),
    /// 紧随正文的段落分隔符（`\n`/`CRLF`/`U+2029`）半开 byte range。
    pub separator: (usize, usize),
}

impl ParagraphByteSpan {
    /// 段落正文范围（不含分隔符）。
    pub fn content_range(&self) -> (usize, usize) {
        self.content
    }

    /// 段落分隔符范围；没有分隔符时为空的 `(len, len)`。
    pub fn separator_range(&self) -> (usize, usize) {
        self.separator
    }

    /// 段落正文的 byte 长度。
    pub fn content_len(&self) -> usize {
        self.content.1 - self.content.0
    }

    /// 分隔符的 byte 长度（`\n`=1、`CRLF`=2、`U+2029`=3、无分隔符=0）。
    pub fn separator_len(&self) -> usize {
        self.separator.1 - self.separator.0
    }

    /// 段落占用的总 byte 长度（正文 + 真实分隔符）。
    pub fn byte_len(&self) -> usize {
        self.content_len() + self.separator_len()
    }

    /// 段落后面是否有分隔符。
    pub fn has_separator(&self) -> bool {
        self.separator_len() > 0
    }

    /// 段落（正文 + 分隔符）的完整 byte 范围。
    pub fn span(&self) -> (usize, usize) {
        (self.content.0, self.separator.1)
    }
}

/// 按段落规则切分正文，返回每个段落的正文范围与真实分隔符范围。
///
/// 空正文返回空；连续分隔符产生空段落（`"a\n\nb"` → 三个 span：`a`+`\n`、
/// 空段+`\n`、`b`+无分隔符）；末尾分隔符产生一个末尾空段落（`"a\n"` →
/// `a`+`\n`、空段+无分隔符）。
pub fn paragraph_byte_spans(text: &str) -> Vec<ParagraphByteSpan> {
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    if bytes.is_empty() {
        return spans;
    }
    let mut start = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        if let Some(separator_len) = separator_len_at(bytes, index) {
            spans.push(ParagraphByteSpan {
                content: (start, index),
                separator: (index, index + separator_len),
            });
            index += separator_len;
            start = index;
        } else {
            index += 1;
        }
    }
    spans.push(ParagraphByteSpan {
        content: (start, bytes.len()),
        separator: (bytes.len(), bytes.len()),
    });
    spans
}

/// 按段落规则切分正文，返回每个段落的半开 byte range（不含分隔符）。
///
/// 需要真实分隔符长度时用 [`paragraph_byte_spans`]；本函数只保留正文范围，
/// 不能用于推进 byte/qchar 坐标（CRLF 与 U+2029 不是 1 byte）。
pub fn paragraph_byte_ranges(text: &str) -> Vec<(usize, usize)> {
    paragraph_byte_spans(text)
        .into_iter()
        .map(|span| span.content)
        .collect()
}

/// 按段落规则切分正文，按顺序给出每个段落的 `&str`。
pub fn paragraph_spans(text: &str) -> impl Iterator<Item = &str> {
    paragraph_byte_ranges(text)
        .into_iter()
        .map(move |(start, end)| &text[start..end])
}

/// 每个段落的起始 byte offset（含首段 0；空正文为空）。
///
/// 供平台插入文本后重算段落样式起点使用，规则与 [`paragraph_byte_spans`] 相同。
pub fn paragraph_start_offsets(text: &str) -> Vec<usize> {
    paragraph_byte_spans(text)
        .into_iter()
        .map(|span| span.content.0)
        .collect()
}

/// `byte_offset` 所在段落的起始 byte offset（用于把编辑范围扩展到完整段落）。
///
/// 落在某个分隔符内部时，该分隔符属于前一段，因此返回其后一段的起点；
/// 越界或非 char boundary 时先按 [`crate::editor::text_coordinates::clamp_to_char_boundary`]
/// 左对齐。文首无前段时返回 0。
pub fn paragraph_start_before(text: &str, byte_offset: usize) -> usize {
    let limit = crate::editor::text_coordinates::clamp_to_char_boundary(text, byte_offset);
    text[..limit]
        .char_indices()
        .rev()
        .find_map(|(index, ch)| match ch {
            '\n' => Some(index + 1),
            '\u{2029}' => Some(index + ch.len_utf8()),
            _ => None,
        })
        .unwrap_or(0)
}

/// `byte_offset` 所在段落的结束 byte offset（含分隔符，用于扩展编辑范围）。
///
/// 与 [`paragraph_start_before`] 对称：落在分隔符内部时返回该分隔符结束处；
/// 找不到后续分隔符时返回文末。
pub fn paragraph_end_after(text: &str, byte_offset: usize) -> usize {
    let start = crate::editor::text_coordinates::clamp_to_char_boundary(text, byte_offset);
    text[start..]
        .char_indices()
        .find_map(|(index, ch)| match ch {
            '\n' => Some(start + index + 1),
            '\u{2029}' => Some(start + index + ch.len_utf8()),
            _ => None,
        })
        .unwrap_or(text.len())
}

#[cfg(test)]
#[path = "text_document_rules_tests.rs"]
mod tests;
