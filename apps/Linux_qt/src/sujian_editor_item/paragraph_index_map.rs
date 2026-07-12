#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ParagraphIndexMap {
    entries: Vec<ParagraphEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ParagraphEntry {
    para_index: usize,
    qchar_start: usize,
    qchar_end: usize,
    byte_start: usize,
    byte_end: usize,
}

impl ParagraphIndexMap {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    pub fn from_paragraphs(paragraphs: &[(usize, usize, usize, usize)]) -> Self {
        let entries = paragraphs
            .iter()
            .enumerate()
            .map(|(i, &(qchar_start, qchar_end, byte_start, byte_end))| ParagraphEntry {
                para_index: i,
                qchar_start,
                qchar_end,
                byte_start,
                byte_end,
            })
            .collect();
        Self { entries }
    }

    pub fn byte_to_paragraph(&self, byte_offset: usize) -> Option<usize> {
        self.entries
            .iter()
            .find(|e| byte_offset >= e.byte_start && byte_offset < e.byte_end)
            .map(|e| e.para_index)
    }

    pub fn qchar_to_byte(&self, para_index: usize, qchar_offset: usize) -> Option<usize> {
        self.entries
            .iter()
            .find(|e| e.para_index == para_index)
            .and_then(|e| {
                if qchar_offset > e.qchar_end - e.qchar_start {
                    return None;
                }
                let ratio = qchar_offset as f64 / (e.qchar_end - e.qchar_start) as f64;
                let byte_range = e.byte_end - e.byte_start;
                Some(e.byte_start + (ratio * byte_range as f64).round() as usize)
            })
    }

    pub fn paragraph_count(&self) -> usize {
        self.entries.len()
    }
}

impl Default for ParagraphIndexMap {
    fn default() -> Self {
        Self::new()
    }
}
