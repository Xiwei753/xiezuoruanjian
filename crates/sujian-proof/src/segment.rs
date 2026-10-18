// SPDX-License-Identifier: GPL-3.0-only

use jieba_rs::Jieba;
use std::sync::OnceLock;

static SHARED_JIEBA: OnceLock<Jieba> = OnceLock::new();

/// A token and its UTF-8 byte range in the original input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TokenSpan {
    pub(crate) text: String,
    pub(crate) start_byte: usize,
    pub(crate) end_byte: usize,
}

/// A contiguous run handled independently by the detector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TextSegment {
    pub(crate) start_byte: usize,
    pub(crate) end_byte: usize,
}

/// Jieba word segmentation plus character segmentation that preserves exact
/// UTF-8 positions. KenLM character models must receive one Unicode scalar per
/// token; Jieba's word output is only used for OOV detection.
#[derive(Clone, Debug)]
pub(crate) struct Segmenter {
    jieba: &'static Jieba,
}

impl Default for Segmenter {
    fn default() -> Self {
        Self::new()
    }
}

impl Segmenter {
    pub(crate) fn new() -> Self {
        Self {
            jieba: SHARED_JIEBA.get_or_init(Jieba::new),
        }
    }

    /// Mirrors pycorrector's split around runs of non-Han/ASCII word symbols:
    /// Chinese punctuation and whitespace never join adjacent character
    /// n-grams. Separator runs remain independent segments so callers can keep
    /// their original bytes without rewriting text.
    pub(crate) fn sentence_segments(&self, text: &str) -> Vec<TextSegment> {
        let mut segments = Vec::new();
        let mut run_start = None;
        let mut previous_is_word = None;

        for (byte, ch) in text.char_indices() {
            let is_word = is_detector_word_char(ch);
            match (run_start, previous_is_word) {
                (None, _) => {
                    run_start = Some(byte);
                    previous_is_word = Some(is_word);
                }
                (Some(start), Some(previous)) if previous != is_word => {
                    segments.push(TextSegment {
                        start_byte: start,
                        end_byte: byte,
                    });
                    run_start = Some(byte);
                    previous_is_word = Some(is_word);
                }
                _ => {}
            }
        }

        if let Some(start) = run_start {
            segments.push(TextSegment {
                start_byte: start,
                end_byte: text.len(),
            });
        }
        segments
    }

    pub(crate) fn characters(&self, text: &str) -> Vec<TokenSpan> {
        text.char_indices()
            .map(|(start_byte, ch)| TokenSpan {
                text: ch.to_string(),
                start_byte,
                end_byte: start_byte + ch.len_utf8(),
            })
            .collect()
    }

    pub(crate) fn words(&self, text: &str) -> Vec<TokenSpan> {
        self.jieba
            .cut(text, true)
            .into_iter()
            .map(|token| TokenSpan {
                text: token.word.to_owned(),
                start_byte: token.byte_start,
                end_byte: token.byte_end,
            })
            .collect()
    }

    /// Whether a token belongs to Jieba's bundled general-purpose dictionary.
    /// Its segmentation weights remain separate from THUOCL candidate scores.
    pub(crate) fn has_word(&self, word: &str) -> bool {
        self.jieba.has_word(word)
    }

    pub(crate) fn char_tokens<'a>(&self, text: &'a str) -> Vec<&'a str> {
        text.char_indices()
            .map(|(start, ch)| &text[start..start + ch.len_utf8()])
            .collect()
    }
}

pub(crate) fn is_chinese_string(text: &str) -> bool {
    !text.is_empty() && text.chars().all(is_han)
}

pub(crate) fn is_han(ch: char) -> bool {
    matches!(ch as u32,
        0x3400..=0x4DBF |
        0x4E00..=0x9FFF |
        0xF900..=0xFAFF |
        0x20000..=0x2FA1F
    )
}

fn is_detector_word_char(ch: char) -> bool {
    is_han(ch) || ch.is_ascii_alphanumeric() || matches!(ch, '+' | '#' | '&')
}
