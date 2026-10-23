// SPDX-License-Identifier: GPL-3.0-only

use jieba_rs::Jieba;
use std::collections::BTreeSet;
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
/// single characters. Ambiguity is carried forward as evidence; it must not be
/// used as an unconditional veto because real OOV typos often live in exactly
/// those spans.
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
    /// A general-dictionary token in either path strictly contains one of the
    /// span edges. Cutting a confirmed stable word is a hard protection; an
    /// HMM-invented OOV token is only ambiguity.
    pub(crate) crosses_stable_word: bool,
    /// Whether the HMM path has an exact token for the span.
    pub(crate) hmm_covers_span: bool,
    /// Whether the dictionary-only path has an exact token for the span.
    pub(crate) dictionary_covers_span: bool,
    /// HMM token immediately before the span, kept for explanation context.
    pub(crate) left_word: Option<String>,
    /// HMM token immediately after the span, kept for explanation context.
    pub(crate) right_word: Option<String>,
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
        self.hmm_covers_span |= other.hmm_covers_span;
        self.dictionary_covers_span |= other.dictionary_covers_span;
        if self.left_word.is_none() {
            self.left_word = other.left_word;
        }
        if self.right_word.is_none() {
            self.right_word = other.right_word;
        }
        for alternative in other.alternatives {
            if !self.alternatives.contains(&alternative) {
                self.alternatives.push(alternative);
            }
        }
        self.alternatives.truncate(8);
    }
}

/// A constrained candidate range inside a run of single-character tokens.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CandidateSpan {
    pub(crate) text: String,
    pub(crate) start_byte: usize,
    pub(crate) end_byte: usize,
    pub(crate) evidence: SegmentationEvidence,
}

/// Immutable per-sentence segmentation and protection analysis. Building it
/// once keeps every candidate span query free of repeated `jieba.cut` calls.
#[derive(Clone, Debug)]
pub(crate) struct SentenceAnalysis {
    sentence: String,
    chars: Vec<TokenSpan>,
    hmm_tokens: Vec<TokenSpan>,
    dictionary_tokens: Vec<TokenSpan>,
    protected_ranges: Vec<(usize, usize)>,
    protected_entries: Vec<(usize, usize)>,
}

impl SentenceAnalysis {
    pub(crate) fn sentence(&self) -> &str {
        &self.sentence
    }

    pub(crate) fn chars(&self) -> &[TokenSpan] {
        &self.chars
    }

    pub(crate) fn hmm_tokens(&self) -> &[TokenSpan] {
        &self.hmm_tokens
    }

    pub(crate) fn protected_ranges(&self) -> &[(usize, usize)] {
        &self.protected_ranges
    }

    pub(crate) fn protected_entries(&self) -> &[(usize, usize)] {
        &self.protected_entries
    }

    /// Collect boundary, protection, and ambiguity evidence for one UTF-8
    /// byte span. This only queries the immutable analysis; it never reruns
    /// Jieba or sentence-level protection matching.
    pub(crate) fn span_evidence(&self, start_byte: usize, end_byte: usize) -> SegmentationEvidence {
        if start_byte >= end_byte || end_byte > self.sentence.len() {
            return SegmentationEvidence::default();
        }
        let Some(span) = self.sentence.get(start_byte..end_byte) else {
            return SegmentationEvidence::default();
        };

        let boundaries_agree = has_boundary(&self.hmm_tokens, start_byte)
            && has_boundary(&self.hmm_tokens, end_byte)
            && has_boundary(&self.dictionary_tokens, start_byte)
            && has_boundary(&self.dictionary_tokens, end_byte);
        let hmm_exact = self
            .hmm_tokens
            .iter()
            .any(|token| token.start_byte == start_byte && token.end_byte == end_byte);
        let dictionary_exact = self
            .dictionary_tokens
            .iter()
            .any(|token| token.start_byte == start_byte && token.end_byte == end_byte);
        let ambiguous_word = hmm_exact != dictionary_exact && !self.has_dictionary_word(span);
        // Only cutting a confirmed general-dictionary word protects the span.
        // An HMM-only unknown token is ambiguity, not a stable word.
        let crosses_stable_word = self
            .hmm_tokens
            .iter()
            .chain(self.dictionary_tokens.iter())
            .any(|token| {
                let straddles = (token.start_byte < start_byte && start_byte < token.end_byte)
                    || (token.start_byte < end_byte && end_byte < token.end_byte);
                straddles && self.has_dictionary_word(&token.text)
            });

        let overlaps_protected =
            self.protected_ranges
                .iter()
                .any(|(protected_start, protected_end)| {
                    start_byte < *protected_end && *protected_start < end_byte
                });

        let left_word = self
            .hmm_tokens
            .iter()
            .find(|token| token.end_byte == start_byte)
            .map(|token| token.text.clone())
            .or_else(|| {
                self.dictionary_tokens
                    .iter()
                    .find(|token| token.end_byte == start_byte)
                    .map(|token| token.text.clone())
            });
        let right_word = self
            .hmm_tokens
            .iter()
            .find(|token| token.start_byte == end_byte)
            .map(|token| token.text.clone())
            .or_else(|| {
                self.dictionary_tokens
                    .iter()
                    .find(|token| token.start_byte == end_byte)
                    .map(|token| token.text.clone())
            });

        let mut alternatives = Vec::new();
        for token in self.hmm_tokens.iter().chain(self.dictionary_tokens.iter()) {
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
            whole_word_known: (hmm_exact || dictionary_exact) && self.has_dictionary_word(span),
            boundaries_agree,
            overlaps_protected,
            ambiguous_word,
            crosses_stable_word,
            hmm_covers_span: hmm_exact,
            dictionary_covers_span: dictionary_exact,
            left_word,
            right_word,
            alternatives,
        }
    }

    /// Candidate ranges for split-OOV recovery. A Jieba HMM token may either
    /// be the complete unknown word (按装) or absorb one preceding character
    /// (他|头资 -> 他头资). Only the whole token and suffixes after its first
    /// character are proposed; prefixes and arbitrary internal windows would
    /// turn ordinary single-character syntax into fake words (叫大|介).
    ///
    /// The dictionary-only path supplies the stable edges, so a range can
    /// never begin or end in the middle of a known word; protected spans and
    /// stable-word crossings are rejected as well.
    pub(crate) fn candidate_spans(&self, max_chars: usize) -> Vec<CandidateSpan> {
        let max_chars = max_chars.max(2);
        let mut spans = Vec::new();
        let mut seen = BTreeSet::new();
        for token in &self.hmm_tokens {
            let char_count = token.text.chars().count();
            if char_count < 2
                || !is_chinese_string(&token.text)
                || self.has_dictionary_word(&token.text)
            {
                continue;
            }
            let chars = self
                .chars
                .iter()
                .filter(|ch| token.start_byte <= ch.start_byte && ch.end_byte <= token.end_byte)
                .collect::<Vec<_>>();
            let push = |start: usize,
                        end: usize,
                        seen: &mut BTreeSet<(usize, usize)>,
                        spans: &mut Vec<CandidateSpan>| {
                self.push_candidate_span(
                    chars[start].start_byte,
                    chars[end - 1].end_byte,
                    false,
                    seen,
                    spans,
                );
            };
            if char_count <= max_chars {
                push(0, chars.len(), &mut seen, &mut spans);
            }
            for end in 3..=chars.len().min(1 + max_chars) {
                push(1, end, &mut seen, &mut spans);
            }
        }

        // The two views overlap; emit in a stable byte order regardless of the
        // token path that discovered a range.
        spans.sort_by(|left, right| {
            left.start_byte
                .cmp(&right.start_byte)
                .then_with(|| left.end_byte.cmp(&right.end_byte))
                .then_with(|| left.text.cmp(&right.text))
        });
        spans
    }

    fn push_candidate_span(
        &self,
        start_byte: usize,
        end_byte: usize,
        require_agreeing_boundaries: bool,
        seen: &mut BTreeSet<(usize, usize)>,
        spans: &mut Vec<CandidateSpan>,
    ) {
        if !seen.insert((start_byte, end_byte)) {
            return;
        }
        let evidence = self.span_evidence(start_byte, end_byte);
        if (require_agreeing_boundaries && !evidence.boundaries_agree)
            || evidence.overlaps_protected
            || evidence.crosses_stable_word
        {
            return;
        }
        if !require_agreeing_boundaries
            && (!has_boundary(&self.dictionary_tokens, start_byte)
                || !has_boundary(&self.dictionary_tokens, end_byte))
        {
            return;
        }
        spans.push(CandidateSpan {
            text: self.sentence[start_byte..end_byte].to_owned(),
            start_byte,
            end_byte,
            evidence,
        });
    }

    fn has_dictionary_word(&self, word: &str) -> bool {
        SHARED_JIEBA.get().is_some_and(|jieba| jieba.has_word(word))
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

    /// Build the immutable segmentation/protection snapshot for one sentence.
    pub(crate) fn analyze_sentence(&self, sentence: &str, lexicon: &Lexicon) -> SentenceAnalysis {
        SentenceAnalysis {
            sentence: sentence.to_owned(),
            chars: self.characters(sentence),
            hmm_tokens: self.words(sentence),
            dictionary_tokens: self.dictionary_words(sentence),
            protected_ranges: lexicon.protected_word_ranges(sentence),
            protected_entries: lexicon.protected_word_ranges_unmerged(sentence),
        }
    }

    /// Collect boundary, protection, and ambiguity evidence for one UTF-8
    /// byte span using an already-built sentence analysis.
    pub(crate) fn candidate_span_evidence(
        &self,
        analysis: &SentenceAnalysis,
        start_byte: usize,
        end_byte: usize,
    ) -> SegmentationEvidence {
        analysis.span_evidence(start_byte, end_byte)
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
