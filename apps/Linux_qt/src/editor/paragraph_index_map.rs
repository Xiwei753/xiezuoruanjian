/// 段落级 UTF-8 byte ↔ QChar index 双向映射表。
///
/// Qt 的 QTextDocument 使用 QChar index（UTF-16 code unit offset），
/// Rust 内部使用 UTF-8 byte offset。每个段落独立构建映射表，
/// 避免全文映射的内存开销。
///
/// `qchar_to_document_byte`：每个 QChar 位置 → 文档级 UTF-8 byte offset。
///   代理对中的两个 QChar 位置映射到同一个 byte offset（字符起始）。
#[derive(Clone)]
pub struct ParagraphIndexMap {
    qchar_to_document_byte: Vec<usize>,
    paragraph_text_len_byte: usize,
    paragraph_text_len_qchar: usize,
}

impl ParagraphIndexMap {
    pub fn build(paragraph_text: &str, paragraph_document_byte_start: usize) -> Self {
        let text_len = paragraph_text.len();
        let mut qchar_to_byte = Vec::new();

        let mut qchar_offset: usize = 0;

        for (byte_pos, ch) in paragraph_text.char_indices() {
            let abs_byte = paragraph_document_byte_start + byte_pos;
            let utf16_len = ch.len_utf16();
            for _ in 0..utf16_len {
                qchar_to_byte.push(abs_byte);
            }
            qchar_offset += utf16_len;
        }

        Self {
            qchar_to_document_byte: qchar_to_byte,
            paragraph_text_len_byte: text_len,
            paragraph_text_len_qchar: qchar_offset,
        }
    }

    /// QChar index → 文档级 UTF-8 byte offset。
    ///
    /// 越界回退：QChar 超出段落长度时返回段落末尾的 byte offset；
    /// 映射表为空时返回 0。
    pub fn qchar_to_document_byte(&self, qchar_index: usize) -> usize {
        self.qchar_to_document_byte
            .get(qchar_index)
            .copied()
            .unwrap_or_else(|| {
                self.qchar_to_document_byte
                    .last()
                    .map(|b| {
                        let first_byte = *self.qchar_to_document_byte.first().unwrap_or(&0);
                        if qchar_index >= self.paragraph_text_len_qchar {
                            first_byte + self.paragraph_text_len_byte
                        } else {
                            *b
                        }
                    })
                    .unwrap_or(0)
            })
    }

    pub fn qchar_range_to_document_byte_range(
        &self,
        qchar_start: usize,
        qchar_end: usize,
    ) -> (usize, usize) {
        let byte_start = self.qchar_to_document_byte(qchar_start);
        let byte_end = self.qchar_to_document_byte(qchar_end);
        (byte_start, byte_end)
    }
}

/// 全文级 UTF-16 code unit (QChar) offset → UTF-8 byte offset 转换。
///
/// 代理对中的低代理项（trailing surrogate）映射到该字符的 UTF-8 起始 byte offset，
/// 与 ParagraphIndexMap 的行为一致。超出文本末尾的 offset 返回 `text.len()`。
pub fn utf16_code_unit_to_utf8_byte(text: &str, qchar_offset: usize) -> usize {
    let mut utf16_offset: usize = 0;
    for (byte_pos, ch) in text.char_indices() {
        if utf16_offset == qchar_offset {
            return byte_pos;
        }
        let utf16_len = ch.len_utf16();
        if utf16_offset + utf16_len > qchar_offset && utf16_len > 1 {
            return byte_pos;
        }
        utf16_offset += utf16_len;
    }
    text.len()
}

/// 全文级 UTF-16 code unit range → UTF-8 byte range 转换。
///
/// `qchar_length` 为 UTF-16 code unit 数量（非字符数），返回半开区间
/// (byte_start, byte_end)。代理对中的低代理项按字符起始处理。
pub fn utf16_code_unit_range_to_utf8_byte_range(
    text: &str,
    qchar_start: usize,
    qchar_length: usize,
) -> (usize, usize) {
    let byte_start = utf16_code_unit_to_utf8_byte(text, qchar_start);
    let byte_end = utf16_code_unit_to_utf8_byte(text, qchar_start + qchar_length);
    (byte_start, byte_end)
}

/// Issue #668: 全文级 UTF-8 byte offset → UTF-16 code unit offset 反向转换。
///
/// Qt IME 协议（ImCursorPosition/ImAnchorPosition/ImAbsolutePosition）期望
/// UTF-16 code unit（QChar 位置）。Rust 内部用 UTF-8 byte offset 表示光标/
/// 选区位置，传给 Qt 前必须转成 UTF-16 code unit。
///
/// 先把 byte offset 对齐到 UTF-8 char boundary（防止落在多字节字符中间），
/// 再对前缀 `[0..aligned_byte)` 用 `encode_utf16().count()` 得到 Qt QChar 位置。
/// 超出文本末尾的 byte offset 返回全文 UTF-16 code unit 长度。
///
/// 与 `utf16_code_unit_to_utf8_byte` 互为反向：对任意 char boundary byte
/// offset `b`，有 `utf8_byte_to_utf16_code_unit(text, b)` 等于该位置对应的
/// QChar 位置；对任意 QChar 位置 `q`，有
/// `utf8_byte_to_utf16_code_unit(text, utf16_code_unit_to_utf8_byte(text, q)) == q`
/// 当 `q` 落在字符起始时成立。
pub fn utf8_byte_to_utf16_code_unit(text: &str, byte_offset: usize) -> usize {
    let aligned = if byte_offset >= text.len() {
        text.len()
    } else if text.is_char_boundary(byte_offset) {
        byte_offset
    } else {
        // 向左回退到最近的 char boundary，与 clamp_to_char_boundary 行为一致。
        let mut clamped = byte_offset;
        while clamped > 0 && !text.is_char_boundary(clamped) {
            clamped -= 1;
        }
        clamped
    };
    text[..aligned].encode_utf16().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ascii_mapping() {
        let map = ParagraphIndexMap::build("hello", 100);
        assert_eq!(map.qchar_to_document_byte(0), 100);
        assert_eq!(map.qchar_to_document_byte(4), 104);
    }

    #[test]
    fn test_cjk_mapping() {
        let map = ParagraphIndexMap::build("你好世界", 0);
        assert_eq!(map.qchar_to_document_byte(0), 0);
        assert_eq!(map.qchar_to_document_byte(1), 3);
        assert_eq!(map.qchar_to_document_byte(2), 6);
    }

    #[test]
    fn test_mixed_mapping() {
        let map = ParagraphIndexMap::build("a你b好", 0);
        assert_eq!(map.qchar_to_document_byte(0), 0);
        assert_eq!(map.qchar_to_document_byte(1), 1);
        assert_eq!(map.qchar_to_document_byte(2), 4);
        assert_eq!(map.qchar_to_document_byte(3), 5);
    }

    #[test]
    fn test_range_conversion() {
        let map = ParagraphIndexMap::build("你好世界", 10);
        let (bs, be) = map.qchar_range_to_document_byte_range(0, 2);
        assert_eq!(bs, 10);
        assert_eq!(be, 16);
    }

    #[test]
    fn test_with_document_offset() {
        let map = ParagraphIndexMap::build("abc", 50);
        assert_eq!(map.qchar_to_document_byte(0), 50);
        assert_eq!(map.qchar_to_document_byte(2), 52);
    }

    #[test]
    fn test_out_of_bounds() {
        let map = ParagraphIndexMap::build("abc", 0);
        assert!(map.qchar_to_document_byte(100) >= 3);
    }

    #[test]
    fn test_emoji_surrogate_pair() {
        let map = ParagraphIndexMap::build("a😀b", 0);
        assert_eq!(map.qchar_to_document_byte(0), 0);
        assert_eq!(map.qchar_to_document_byte(1), 1);
        assert_eq!(map.qchar_to_document_byte(2), 1);
        assert_eq!(map.qchar_to_document_byte(3), 5);
    }

    #[test]
    fn test_utf16_to_utf8_ascii() {
        assert_eq!(utf16_code_unit_to_utf8_byte("hello", 0), 0);
        assert_eq!(utf16_code_unit_to_utf8_byte("hello", 3), 3);
        assert_eq!(utf16_code_unit_to_utf8_byte("hello", 5), 5);
        assert_eq!(utf16_code_unit_to_utf8_byte("hello", 10), 5);
    }

    #[test]
    fn test_utf16_to_utf8_cjk() {
        assert_eq!(utf16_code_unit_to_utf8_byte("你好世界", 0), 0);
        assert_eq!(utf16_code_unit_to_utf8_byte("你好世界", 1), 3);
        assert_eq!(utf16_code_unit_to_utf8_byte("你好世界", 2), 6);
    }

    #[test]
    fn test_utf16_to_utf8_emoji() {
        assert_eq!(utf16_code_unit_to_utf8_byte("a😀b", 0), 0);
        assert_eq!(utf16_code_unit_to_utf8_byte("a😀b", 1), 1);
        assert_eq!(utf16_code_unit_to_utf8_byte("a😀b", 2), 1);
        assert_eq!(utf16_code_unit_to_utf8_byte("a😀b", 3), 5);
    }

    #[test]
    fn test_utf16_to_utf8_mixed() {
        assert_eq!(utf16_code_unit_to_utf8_byte("a你b好", 0), 0);
        assert_eq!(utf16_code_unit_to_utf8_byte("a你b好", 1), 1);
        assert_eq!(utf16_code_unit_to_utf8_byte("a你b好", 2), 4);
        assert_eq!(utf16_code_unit_to_utf8_byte("a你b好", 3), 5);
    }

    #[test]
    fn test_utf16_range_to_utf8_range() {
        assert_eq!(
            utf16_code_unit_range_to_utf8_byte_range("a😀b", 1, 2),
            (1, 5)
        );
        assert_eq!(
            utf16_code_unit_range_to_utf8_byte_range("你好", 0, 2),
            (0, 6)
        );
        assert_eq!(
            utf16_code_unit_range_to_utf8_byte_range("abc", 0, 3),
            (0, 3)
        );
    }

    #[test]
    fn test_utf16_to_utf8_empty() {
        assert_eq!(utf16_code_unit_to_utf8_byte("", 0), 0);
    }

    #[test]
    fn test_utf16_to_utf8_multiple_emoji() {
        assert_eq!(utf16_code_unit_to_utf8_byte("😀😁", 0), 0);
        assert_eq!(utf16_code_unit_to_utf8_byte("😀😁", 1), 0);
        assert_eq!(utf16_code_unit_to_utf8_byte("😀😁", 2), 4);
        assert_eq!(utf16_code_unit_to_utf8_byte("😀😁", 3), 4);
        assert_eq!(utf16_code_unit_to_utf8_byte("😀😁", 4), 8);
    }

    // ── Issue #668: UTF-8 byte offset → UTF-16 code unit 反向转换测试 ──

    #[test]
    fn test_utf8_to_utf16_ascii() {
        assert_eq!(utf8_byte_to_utf16_code_unit("hello", 0), 0);
        assert_eq!(utf8_byte_to_utf16_code_unit("hello", 3), 3);
        assert_eq!(utf8_byte_to_utf16_code_unit("hello", 5), 5);
        // 超出末尾返回全文 UTF-16 长度
        assert_eq!(utf8_byte_to_utf16_code_unit("hello", 10), 5);
    }

    #[test]
    fn test_utf8_to_utf16_cjk() {
        // 你好世界：每个 CJK 字符 UTF-8 占 3 byte、UTF-16 占 1 code unit
        assert_eq!(utf8_byte_to_utf16_code_unit("你好世界", 0), 0);
        assert_eq!(utf8_byte_to_utf16_code_unit("你好世界", 3), 1);
        assert_eq!(utf8_byte_to_utf16_code_unit("你好世界", 6), 2);
        assert_eq!(utf8_byte_to_utf16_code_unit("你好世界", 12), 4);
    }

    #[test]
    fn test_utf8_to_utf16_emoji() {
        // a😀b：a=1byte/1unit，😀=4byte/2unit，b=1byte/1unit
        assert_eq!(utf8_byte_to_utf16_code_unit("a😀b", 0), 0);
        assert_eq!(utf8_byte_to_utf16_code_unit("a😀b", 1), 1);
        // 😀 起始 byte=1，对应 UTF-16 code unit=1
        // 😀 结束 byte=5，对应 UTF-16 code unit=3（1+2）
        assert_eq!(utf8_byte_to_utf16_code_unit("a😀b", 5), 3);
        assert_eq!(utf8_byte_to_utf16_code_unit("a😀b", 6), 4);
    }

    #[test]
    fn test_utf8_to_utf16_mid_byte_clamps_to_boundary() {
        // 落在多字节字符中间时向左回退到 char boundary
        // a😀b：byte=2/3/4 都在 😀 中间，应回退到 byte=1（UTF-16 unit=1）
        assert_eq!(utf8_byte_to_utf16_code_unit("a😀b", 2), 1);
        assert_eq!(utf8_byte_to_utf16_code_unit("a😀b", 3), 1);
        assert_eq!(utf8_byte_to_utf16_code_unit("a😀b", 4), 1);
    }

    #[test]
    fn test_utf8_to_utf16_empty() {
        assert_eq!(utf8_byte_to_utf16_code_unit("", 0), 0);
    }

    #[test]
    fn test_utf8_to_utf16_multiple_emoji() {
        // 😀😁：每个 emoji 4 byte / 2 unit
        assert_eq!(utf8_byte_to_utf16_code_unit("😀😁", 0), 0);
        assert_eq!(utf8_byte_to_utf16_code_unit("😀😁", 4), 2);
        assert_eq!(utf8_byte_to_utf16_code_unit("😀😁", 8), 4);
    }

    #[test]
    fn test_utf8_to_utf16_round_trip_with_utf16_to_utf8() {
        // 对字符起始的 QChar 位置，反向转换应保持一致。
        // "a😀b你好" 的 UTF-16 长度是 6（a=1, 😀=2, b=1, 你=1, 好=1），
        // 只测试字符起始位置 0,1,3,4,5,6（不含代理对中间的 2 和超出末尾的 7+）。
        let text = "a😀b你好";
        // QChar 位置 0,1,3,4,5,6 对应 byte 0,1,5,6,9,12
        for qchar_pos in [0usize, 1, 3, 4, 5, 6] {
            let byte = utf16_code_unit_to_utf8_byte(text, qchar_pos);
            let back = utf8_byte_to_utf16_code_unit(text, byte);
            assert_eq!(back, qchar_pos, "qchar_pos={} byte={}", qchar_pos, byte);
        }
    }
}
