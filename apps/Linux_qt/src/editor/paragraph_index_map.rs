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
}
