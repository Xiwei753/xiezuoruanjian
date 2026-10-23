// SPDX-License-Identifier: GPL-3.0-only

use jieba_rs::Jieba;
use std::sync::OnceLock;

use crate::lexicon::Lexicon;

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

/// What the two Jieba paths say about one candidate span.
///
/// A single segmentation is not proof of a word: the HMM path may join an
/// unknown verb-entity pair, while the dictionary path may split it into
/// single characters. A candidate that cuts through a stable token is
/// ambiguous and must not be treated as a spelling error.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct SegmentationEvidence {
    /// The complete span is a Jieba dictionary word and an exact token in at
    /// least one segmentation path.
    pub(crate) whole_word_known: bool,
    /// Both the HMM and dictionary-only paths place token boundaries at the
    /// start and end of the span.
    pub(crate) boundaries_agree: bool,
    /// The span intersects a complete author-protected work-dictionary entry.
    pub(crate) overlaps_protected: bool,
    /// The span is an exact token in only one segmentation path and is not a
    /// general-dictionary word. This is the 看|宿禾 / 裹|臀布 ambiguity state.
    pub(crate) ambiguous_word: bool,
    /// A token in either path strictly contains one of the span edges.
    pub(crate) crosses_stable_word: bool,
    /// Other tokens overlapping the span, kept as explanation context.
    pub(crate) alternatives: Vec<String>,
}

impl SegmentationEvidence {
    pub(crate) fn merge(&mut self, other: SegmentationEvidence) {
        self.whole_word_known |= other.whole_word_known;
        self.boundaries_agree |= other.boundaries_agree;
        self.overlaps_protected |= other.overlaps_protected;
        self.ambiguous_word |= other.ambiguous_word;
        self.crosses_stable_word |= other.crosses_stable_word;
        for alternative in other.alternatives {
            if !self.alternatives.contains(&alternative) {
                self.alternatives.push(alternative);
            }
        }
        self.alternatives.truncate(8);
    }
}

/// Jieba word segmentation plus character segmentation that preserves exact
/// UTF-8 positions. KenLM character models must receive one Unicode scalar per
/// token; Jieba's word output is used for OOV and segmentation evidence.
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
        self.tokens(text, true)
    }

    /// Dictionary-only segmentation. The HMM path can invent an unknown word
    /// from an entity boundary; comparing both paths keeps that guess from
    /// becoming an error verdict.
    pub(crate) fn dictionary_words(&self, text: &str) -> Vec<TokenSpan> {
        self.tokens(text, false)
    }

    fn tokens(&self, text: &str, hmm: bool) -> Vec<TokenSpan> {
        self.jieba
            .cut(text, hmm)
            .into_iter()
            .map(|token| TokenSpan {
                text: token.word.to_owned(),
                start_byte: token.byte_start,
                end_byte: token.byte_end,
            })
            .collect()
    }

    /// Collect boundary, protection, and ambiguity evidence for one UTF-8
    /// byte span in `sentence`.
    pub(crate) fn candidate_span_evidence(
        &self,
        sentence: &str,
        start_byte: usize,
        end_byte: usize,
        lexicon: &Lexicon,
    ) -> SegmentationEvidence {
        if start_byte >= end_byte || end_byte > sentence.len() {
            return SegmentationEvidence::default();
        }
        let Some(span) = sentence.get(start_byte..end_byte) else {
            return SegmentationEvidence::default();
        };

        let hmm_tokens = self.words(sentence);
        let dictionary_tokens = self.dictionary_words(sentence);
        let boundaries_agree = has_boundary(&hmm_tokens, start_byte)
            && has_boundary(&hmm_tokens, end_byte)
            && has_boundary(&dictionary_tokens, start_byte)
            && has_boundary(&dictionary_tokens, end_byte);
        let hmm_exact = hmm_tokens
            .iter()
            .any(|token| token.start_byte == start_byte && token.end_byte == end_byte);
        let dictionary_exact = dictionary_tokens
            .iter()
            .any(|token| token.start_byte == start_byte && token.end_byte == end_byte);
        let ambiguous_word = hmm_exact != dictionary_exact && !self.has_word(span);
        let crosses_stable_word = [&hmm_tokens, &dictionary_tokens]
            .into_iter()
            .flat_map(|tokens| tokens.iter())
            .any(|token| {
                (token.start_byte < start_byte && start_byte < token.end_byte)
                    || (token.start_byte < end_byte && end_byte < token.end_byte)
            });

        let overlaps_protected = lexicon
            .protected_word_ranges_unmerged(sentence)
            .into_iter()
            .any(|(protected_start, protected_end)| {
                start_byte < protected_end && protected_start < end_byte
            });

        let mut alternatives = Vec::new();
        for token in hmm_tokens.iter().chain(dictionary_tokens.iter()) {
            if token.start_byte < end_byte
                && start_byte < token.end_byte
                && token.text != span
                && is_chinese_string(&token.text)
                && !alternatives.contains(&token.text)
            {
                alternatives.push(token.text.clone());
            }
        }
        alternatives.truncate(8);

        SegmentationEvidence {
            whole_word_known: (hmm_exact || dictionary_exact) && self.has_word(span),
            boundaries_agree,
            overlaps_protected,
            ambiguous_word,
            crosses_stable_word,
            alternatives,
        }
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

fn has_boundary(tokens: &[TokenSpan], byte: usize) -> bool {
    tokens
        .iter()
        .any(|token| token.start_byte == byte || token.end_byte == byte)
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
