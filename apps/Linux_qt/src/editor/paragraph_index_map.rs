/// 段落级 UTF-8 byte ↔ QChar index 双向映射表。
///
/// Qt 的 QTextDocument 使用 QChar index（UTF-16 code unit offset），
/// Rust 内部使用 UTF-8 byte offset。每个段落独立构建映射表，
/// 避免全文映射的内存开销。
///
/// `qchar_to_document_byte`：每个 QChar 位置 → 文档级 UTF-8 byte offset。
///   代理对中的两个 QChar 位置映射到同一个 byte offset（字符起始）。
/// `document_byte_to_qchar`：每个 UTF-8 byte 位置 → QChar offset。
///   多字节字符的每个 byte 位置映射到同一个 QChar offset。
#[derive(Clone)]
pub struct ParagraphIndexMap {
    qchar_to_document_byte: Vec<usize>,
    document_byte_to_qchar: Vec<usize>,
    paragraph_text_len_byte: usize,
    paragraph_text_len_qchar: usize,
}

impl ParagraphIndexMap {
    pub fn build(paragraph_text: &str, paragraph_document_byte_start: usize) -> Self {
        let text_len = paragraph_text.len();
        let mut qchar_to_byte = Vec::new();
        let mut byte_to_qchar = Vec::with_capacity(text_len + 1);

        let mut qchar_offset: usize = 0;

        for (byte_pos, ch) in paragraph_text.char_indices() {
            let abs_byte = paragraph_document_byte_start + byte_pos;
            let utf16_len = ch.len_utf16();
            for _ in 0..utf16_len {
                qchar_to_byte.push(abs_byte);
            }
            let char_len = ch.len_utf8();
            for _ in 0..char_len {
                byte_to_qchar.push(qchar_offset);
            }
            qchar_offset += utf16_len;
        }
        byte_to_qchar.push(qchar_offset);

        Self {
            qchar_to_document_byte: qchar_to_byte,
            document_byte_to_qchar: byte_to_qchar,
            paragraph_text_len_byte: text_len,
            paragraph_text_len_qchar: qchar_offset,
        }
    }

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

    pub fn document_byte_to_qchar(&self, document_byte: usize, paragraph_document_byte_start: usize) -> usize {
        let local_byte = document_byte.saturating_sub(paragraph_document_byte_start);
        self.document_byte_to_qchar
            .get(local_byte)
            .copied()
            .unwrap_or(self.paragraph_text_len_qchar)
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

    pub fn document_byte_range_to_qchar_range(
        &self,
        byte_start: usize,
        byte_end: usize,
        paragraph_document_byte_start: usize,
    ) -> (usize, usize) {
        let qchar_start = self.document_byte_to_qchar(byte_start, paragraph_document_byte_start);
        let qchar_end = self.document_byte_to_qchar(byte_end, paragraph_document_byte_start);
        (qchar_start, qchar_end)
    }

    pub fn paragraph_qchar_len(&self) -> usize {
        self.paragraph_text_len_qchar
    }

    pub fn paragraph_byte_len(&self) -> usize {
        self.paragraph_text_len_byte
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ascii_mapping() {
        let map = ParagraphIndexMap::build("hello", 100);
        assert_eq!(map.qchar_to_document_byte(0), 100);
        assert_eq!(map.qchar_to_document_byte(4), 104);
        assert_eq!(map.document_byte_to_qchar(100, 100), 0);
        assert_eq!(map.document_byte_to_qchar(104, 100), 4);
    }

    #[test]
    fn test_cjk_mapping() {
        let map = ParagraphIndexMap::build("你好世界", 0);
        assert_eq!(map.qchar_to_document_byte(0), 0);
        assert_eq!(map.qchar_to_document_byte(1), 3);
        assert_eq!(map.qchar_to_document_byte(2), 6);
        assert_eq!(map.document_byte_to_qchar(0, 0), 0);
        assert_eq!(map.document_byte_to_qchar(3, 0), 1);
        assert_eq!(map.document_byte_to_qchar(6, 0), 2);
    }

    #[test]
    fn test_mixed_mapping() {
        let map = ParagraphIndexMap::build("a你b好", 0);
        assert_eq!(map.qchar_to_document_byte(0), 0);
        assert_eq!(map.qchar_to_document_byte(1), 1);
        assert_eq!(map.qchar_to_document_byte(2), 4);
        assert_eq!(map.qchar_to_document_byte(3), 5);

        assert_eq!(map.document_byte_to_qchar(0, 0), 0);
        assert_eq!(map.document_byte_to_qchar(1, 0), 1);
        assert_eq!(map.document_byte_to_qchar(4, 0), 2);
        assert_eq!(map.document_byte_to_qchar(5, 0), 3);
    }

    #[test]
    fn test_range_conversion() {
        let map = ParagraphIndexMap::build("你好世界", 10);
        let (bs, be) = map.qchar_range_to_document_byte_range(0, 2);
        assert_eq!(bs, 10);
        assert_eq!(be, 16);

        let (qs, qe) = map.document_byte_range_to_qchar_range(10, 16, 10);
        assert_eq!(qs, 0);
        assert_eq!(qe, 2);
    }

    #[test]
    fn test_with_document_offset() {
        let map = ParagraphIndexMap::build("abc", 50);
        assert_eq!(map.qchar_to_document_byte(0), 50);
        assert_eq!(map.qchar_to_document_byte(2), 52);
        assert_eq!(map.document_byte_to_qchar(50, 50), 0);
        assert_eq!(map.document_byte_to_qchar(52, 50), 2);
    }

    #[test]
    fn test_out_of_bounds() {
        let map = ParagraphIndexMap::build("abc", 0);
        assert!(map.qchar_to_document_byte(100) >= 3);
        assert_eq!(map.document_byte_to_qchar(100, 0), 3);
    }

    #[test]
    fn test_empty_text() {
        let map = ParagraphIndexMap::build("", 0);
        assert_eq!(map.paragraph_qchar_len(), 0);
        assert_eq!(map.paragraph_byte_len(), 0);
    }

    #[test]
    fn test_emoji_surrogate_pair() {
        let map = ParagraphIndexMap::build("a😀b", 0);
        assert_eq!(map.qchar_to_document_byte(0), 0);
        assert_eq!(map.qchar_to_document_byte(1), 1);
        assert_eq!(map.qchar_to_document_byte(2), 1);
        assert_eq!(map.qchar_to_document_byte(3), 5);

        assert_eq!(map.document_byte_to_qchar(0, 0), 0);
        assert_eq!(map.document_byte_to_qchar(1, 0), 1);
        assert_eq!(map.document_byte_to_qchar(5, 0), 3);
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
        assert_eq!(utf16_code_unit_range_to_utf8_byte_range("a😀b", 1, 2), (1, 5));
        assert_eq!(utf16_code_unit_range_to_utf8_byte_range("你好", 0, 2), (0, 6));
        assert_eq!(utf16_code_unit_range_to_utf8_byte_range("abc", 0, 3), (0, 3));
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
}
