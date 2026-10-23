// SPDX-License-Identifier: GPL-3.0-only
//! Sentence boundaries and UTF-8 offset helpers.
//!
//! Segmentation here is only a context boundary: it never decides that a span
//! is an error. The neural model receives complete sentences (with a small
//! amount of neighbouring context in the corrector) and returns character
//! positions in the original text.

/// A half-open UTF-8 byte range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextSegment {
    pub(crate) start_byte: usize,
    pub(crate) end_byte: usize,
}

/// Punctuation-aware sentence segmenter.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Segmenter;

impl Segmenter {
    pub(crate) fn new() -> Self {
        Self
    }

    /// Split into sentences at sentence-final punctuation and paragraph
    /// breaks. The punctuation remains part of the preceding segment so the
    /// model sees a complete syntactic unit.
    pub(crate) fn sentence_segments(&self, text: &str) -> Vec<TextSegment> {
        let mut segments = Vec::new();
        let mut start = 0;
        let mut chars = text.char_indices().peekable();
        while let Some((byte, ch)) = chars.next() {
            if !is_sentence_end(ch) {
                continue;
            }
            let mut end = byte + ch.len_utf8();
            // Consume a run of closing punctuation/quotes and repeated marks.
            while let Some((next_byte, next)) = chars.peek().copied() {
                if is_sentence_tail(next) {
                    chars.next();
                    end = next_byte + next.len_utf8();
                } else {
                    break;
                }
            }
            push_segment(text, start, end, &mut segments);
            start = end;
        }
        push_segment(text, start, text.len(), &mut segments);
        segments
    }
}

fn push_segment(text: &str, start: usize, end: usize, segments: &mut Vec<TextSegment>) {
    if start >= end {
        return;
    }
    if text[start..end].trim().is_empty() {
        return;
    }
    segments.push(TextSegment {
        start_byte: start,
        end_byte: end,
    });
}

fn is_sentence_end(ch: char) -> bool {
    matches!(ch, '。' | '！' | '？' | '!' | '?' | '；' | ';' | '…' | '\n')
}

fn is_sentence_tail(ch: char) -> bool {
    matches!(
        ch,
        '。' | '！' | '？' | '!' | '?' | '；' | ';' | '…' | '”' | '’' | '」' | '』' | '）' | ')'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn punctuation_keeps_complete_sentences() {
        let text = "甲说。乙回答！";
        let segments = Segmenter::new().sentence_segments(text);
        assert_eq!(segments.len(), 2);
        assert_eq!(
            &text[segments[0].start_byte..segments[0].end_byte],
            "甲说。"
        );
        assert_eq!(
            &text[segments[1].start_byte..segments[1].end_byte],
            "乙回答！"
        );
    }

    #[test]
    fn closing_quote_stays_with_the_sentence() {
        let text = "他说：“好。”然后离开。";
        let segments = Segmenter::new().sentence_segments(text);
        assert_eq!(
            &text[segments[0].start_byte..segments[0].end_byte],
            "他说：“好。”"
        );
    }

    #[test]
    fn empty_segments_are_skipped() {
        let segments = Segmenter::new().sentence_segments("甲。\n\n乙。");
        assert_eq!(segments.len(), 2);
    }
}
