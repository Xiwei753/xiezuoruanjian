// SPDX-License-Identifier: Apache-2.0
// Modified behavioral port of pycorrector/pycorrector/detector.py by XuMing
// (xuming624); this Rust implementation does not bundle upstream data files.

use crate::confusion::ConfusionMatcher;
use crate::lexicon::Lexicon;
use crate::lm::LanguageScorer;
use crate::segment::{
    is_chinese_string, is_han, SegmentationEvidence, Segmenter, SentenceAnalysis,
};
use crate::CorrectionError;

// Behavioral port of pycorrector Detector._detect and _get_maybe_error_index
// (Apache-2.0): confusion terms, dictionary OOV tokens,
// then character-level 2/3-gram MAD outliers. All returned offsets are UTF-8
// byte ranges; pycorrector's character indices are converted at the boundary.
//
// In this port every statistical signal is only a nomination with evidence.
// Neither an OOV token nor a low character score is an error by itself.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DetectionKind {
    ConfirmedEntitySpelling,
    ConfusionWord,
    OutOfVocabularyWord,
    Character,
}

/// Evidence gathered for one suspect span. Several detectors may nominate the
/// same bytes; their evidence is merged and handed to the decision layer
/// instead of being discarded by a detection-stage priority rule.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct DetectionEvidence {
    /// An author-registered exact spelling or whole-word confusion relation.
    pub(crate) exact_relation: bool,
    /// The span matched an explicit confusion dictionary entry.
    pub(crate) confusion_entry: bool,
    /// The general Jieba dictionary does not cover the source span. This is a
    /// dictionary fact, not proof that the span is wrong.
    pub(crate) oov_general_dictionary_miss: bool,
    /// The source span is itself a general-dictionary word. Valid words must
    /// not be overturned by a higher local language-model score alone.
    pub(crate) source_is_general_word: bool,
    /// Character positions (zero-based scalars relative to the span) that the
    /// 2/3-gram MAD check considered anomalous, with the outlier strength.
    pub(crate) char_outliers: Vec<(usize, f64)>,
    pub(crate) segmentation: SegmentationEvidence,
}

impl DetectionEvidence {
    fn merge(&mut self, other: DetectionEvidence) {
        self.exact_relation |= other.exact_relation;
        self.confusion_entry |= other.confusion_entry;
        self.oov_general_dictionary_miss |= other.oov_general_dictionary_miss;
        self.source_is_general_word |= other.source_is_general_word;
        self.char_outliers.extend(other.char_outliers);
        self.char_outliers.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| right.1.total_cmp(&left.1))
        });
        self.char_outliers.dedup_by(|left, right| left.0 == right.0);
        self.segmentation.merge(other.segmentation);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Suspect {
    pub(crate) start_byte: usize,
    pub(crate) end_byte: usize,
    pub(crate) original: String,
    pub(crate) kind: DetectionKind,
    pub(crate) exact_replacement: Option<String>,
    pub(crate) evidence: DetectionEvidence,
}

impl Suspect {
    fn merge(&mut self, other: Suspect) {
        if detection_priority(other.kind) < detection_priority(self.kind) {
            self.kind = other.kind;
        }
        if self.exact_replacement.is_none() {
            self.exact_replacement = other.exact_replacement;
        }
        self.evidence.merge(other.evidence);
    }
}

/// Nomination threshold for character-level 2/3-gram outliers. This only
/// decides which positions are worth analyzing; it is not an error probability
/// or a substitute for the decision gate's external spelling evidence.
const CHAR_OUTLIER_THRESHOLD: f64 = 1.7;

#[derive(Clone, Debug)]
pub(crate) struct Detector {
    detect_words: bool,
    detect_characters: bool,
}

impl Default for Detector {
    fn default() -> Self {
        Self {
            detect_words: true,
            detect_characters: true,
        }
    }
}

impl Detector {
    pub(crate) fn detect(
        &self,
        text: &str,
        lexicon: &Lexicon,
        segmenter: &Segmenter,
        confusion_matcher: &ConfusionMatcher,
        scorer: &dyn LanguageScorer,
    ) -> Result<Vec<Suspect>, CorrectionError> {
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }

        let mut candidates = Vec::new();
        let mut protected_ranges = Vec::new();
        let mut protected_entries = Vec::new();
        for segment in segmenter.sentence_segments(text) {
            let sentence = &text[segment.start_byte..segment.end_byte];
            let analysis = segmenter.analyze_sentence(sentence, lexicon);
            protected_ranges.extend(
                analysis
                    .protected_ranges()
                    .iter()
                    .map(|(start, end)| (segment.start_byte + start, segment.start_byte + end)),
            );
            protected_entries.extend(
                analysis
                    .protected_entries()
                    .iter()
                    .map(|(start, end)| (segment.start_byte + start, segment.start_byte + end)),
            );

            let mut sentence_candidates = Vec::new();
            for matched in confusion_matcher.scan_text(sentence) {
                if lexicon.is_ignored(&matched.original_text) {
                    continue;
                }
                let start_byte = segment.start_byte + matched.start_byte;
                let end_byte = segment.start_byte + matched.end_byte;
                sentence_candidates.push(Suspect {
                    start_byte,
                    end_byte,
                    original: matched.original_text,
                    kind: DetectionKind::ConfusionWord,
                    exact_replacement: Some(matched.suggestion),
                    evidence: DetectionEvidence {
                        exact_relation: true,
                        confusion_entry: true,
                        segmentation: segmenter.candidate_span_evidence(
                            &analysis,
                            matched.start_byte,
                            matched.end_byte,
                        ),
                        ..DetectionEvidence::default()
                    },
                });
            }

            // Exact, author-confirmed wrong spellings only. There is no
            // similarity scan over registered names.
            for relation in lexicon.work_entity_lexicon().misspellings() {
                for (start, end) in exact_name_occurrences(sentence, &relation.original) {
                    sentence_candidates.push(Suspect {
                        start_byte: segment.start_byte + start,
                        end_byte: segment.start_byte + end,
                        original: relation.original.clone(),
                        kind: DetectionKind::ConfirmedEntitySpelling,
                        exact_replacement: Some(relation.replacement.clone()),
                        evidence: DetectionEvidence {
                            exact_relation: true,
                            segmentation: segmenter.candidate_span_evidence(&analysis, start, end),
                            ..DetectionEvidence::default()
                        },
                    });
                }
            }

            if !sentence.chars().any(is_han) {
                continue;
            }

            if self.detect_words {
                for token in analysis.hmm_tokens() {
                    if !is_chinese_string(&token.text)
                        || lexicon.is_ignored(&token.text)
                        || lexicon.is_oov_exception(&token.text)
                        || segmenter.has_word(&token.text)
                    {
                        continue;
                    }
                    sentence_candidates.push(Suspect {
                        start_byte: segment.start_byte + token.start_byte,
                        end_byte: segment.start_byte + token.end_byte,
                        original: token.text.clone(),
                        kind: DetectionKind::OutOfVocabularyWord,
                        exact_replacement: None,
                        evidence: DetectionEvidence {
                            oov_general_dictionary_miss: true,
                            source_is_general_word: lexicon.is_general_known_word(&token.text)
                                || segmenter.has_word(&token.text),
                            segmentation: segmenter.candidate_span_evidence(
                                &analysis,
                                token.start_byte,
                                token.end_byte,
                            ),
                            ..DetectionEvidence::default()
                        },
                    });
                }
            }

            let character_suspects = if self.detect_characters {
                detect_character_outliers(
                    &analysis,
                    segment.start_byte,
                    segmenter,
                    lexicon,
                    scorer,
                )?
            } else {
                Vec::new()
            };
            // A character anomaly inside an OOV token is independent evidence
            // for that token. Keep it attached to the token's scalar position
            // so the decision layer can require that the changed character is
            // the one with evidence.
            for suspect in sentence_candidates
                .iter_mut()
                .filter(|suspect| suspect.kind == DetectionKind::OutOfVocabularyWord)
            {
                for outlier in &character_suspects {
                    if suspect.start_byte <= outlier.start_byte
                        && outlier.end_byte <= suspect.end_byte
                    {
                        let relative_start = suspect.start_byte.saturating_sub(segment.start_byte);
                        let outlier_start = outlier.start_byte.saturating_sub(segment.start_byte);
                        let position = sentence[relative_start..outlier_start].chars().count();
                        let strength = outlier
                            .evidence
                            .char_outliers
                            .first()
                            .map_or(0.0, |(_, strength)| *strength);
                        suspect.evidence.char_outliers.push((position, strength));
                    }
                }
            }
            sentence_candidates.extend(character_suspects);
            candidates.extend(sentence_candidates);
        }

        candidates.retain(|candidate| {
            let overlaps_protected = protected_ranges
                .iter()
                .any(|(start, end)| candidate.start_byte < *end && *start < candidate.end_byte);
            if !overlaps_protected {
                return true;
            }
            // An explicitly registered misspelling may contain a protected
            // canonical name as a prefix. Only drop it when a single complete
            // protected entry actually wraps the whole confirmed spelling.
            if candidate.kind != DetectionKind::ConfirmedEntitySpelling {
                return false;
            }
            !protected_entries
                .iter()
                .any(|(start, end)| *start <= candidate.start_byte && candidate.end_byte <= *end)
        });
        Ok(merge_evidence(candidates))
    }
}

/// Exact byte occurrences of an author-confirmed wrong spelling. ASCII edges
/// do not match inside a longer Latin identifier (`X-42` in `ZX-42B`).
pub(crate) fn exact_name_occurrences(text: &str, name: &str) -> Vec<(usize, usize)> {
    if name.is_empty() {
        return Vec::new();
    }
    let mut occurrences = Vec::new();
    let mut search_from = 0usize;
    while let Some(relative) = text[search_from..].find(name) {
        let start = search_from + relative;
        let end = start + name.len();
        let left_ok = !name.chars().next().is_some_and(is_ascii_name_char)
            || text[..start]
                .chars()
                .next_back()
                .is_none_or(|previous| !is_ascii_name_char(previous));
        let right_ok = !name.chars().next_back().is_some_and(is_ascii_name_char)
            || text[end..]
                .chars()
                .next()
                .is_none_or(|next| !is_ascii_name_char(next));
        // For mixed-script names, only treat the edge as an ASCII identifier
        // when that edge character itself participates in an ASCII token.
        let left_ok = left_ok || !name.chars().next().is_some_and(|first| first.is_ascii());
        let right_ok = right_ok || !name.chars().next_back().is_some_and(|last| last.is_ascii());
        if left_ok && right_ok {
            occurrences.push((start, end));
        }
        search_from = end.max(start + 1);
    }
    occurrences
}

fn is_ascii_name_char(value: char) -> bool {
    value.is_ascii_alphanumeric() || value == '_'
}

fn detect_character_outliers(
    analysis: &SentenceAnalysis,
    base_byte: usize,
    segmenter: &Segmenter,
    lexicon: &Lexicon,
    scorer: &dyn LanguageScorer,
) -> Result<Vec<Suspect>, CorrectionError> {
    let sentence = analysis.sentence();
    let chars = analysis.chars();
    if chars.len() < 2 {
        return Ok(Vec::new());
    }

    let mut combined = vec![0.0; chars.len()];
    let mut score_sets = 0usize;
    for n in [2, 3] {
        if chars.len() < n {
            continue;
        }
        let mut ngram_scores = Vec::with_capacity(chars.len() - n + 1);
        for start in 0..=chars.len() - n {
            let tokens = chars[start..start + n]
                .iter()
                .map(|token| token.text.as_str())
                .collect::<Vec<_>>();
            ngram_scores.push(scorer.score_tokens_without_boundaries(&tokens)?);
        }
        let aligned = align_window_scores(&ngram_scores, n, chars.len());
        for (position, score) in aligned.into_iter().enumerate() {
            combined[position] += score;
        }
        score_sets += 1;
    }
    if score_sets == 0 {
        return Ok(Vec::new());
    }
    for score in &mut combined {
        *score /= score_sets as f64;
    }

    let outliers = mad_outlier_indices(&combined, 0.6745, CHAR_OUTLIER_THRESHOLD);
    let word_tokens = analysis.hmm_tokens();
    let mut matches = Vec::new();
    for (index, strength) in outliers {
        let Some(token) = chars.get(index) else {
            continue;
        };
        if !is_chinese_string(&token.text) || lexicon.is_ignored(&token.text) {
            continue;
        }
        matches.push(Suspect {
            start_byte: base_byte + token.start_byte,
            end_byte: base_byte + token.end_byte,
            original: token.text.clone(),
            kind: DetectionKind::Character,
            exact_replacement: None,
            evidence: DetectionEvidence {
                source_is_general_word: lexicon.is_general_known_word(&token.text)
                    || segmenter.has_word(&token.text),
                char_outliers: vec![(0, strength)],
                segmentation: segmenter.candidate_span_evidence(
                    analysis,
                    token.start_byte,
                    token.end_byte,
                ),
                ..DetectionEvidence::default()
            },
        });

        // Keep the enclosing word as a second nomination. A single character
        // cannot be compared as a complete word, while 高梁 → 高粱 needs the
        // whole span's local score. The character position stays attached so
        // the decision layer can require that the changed character is the one
        // with anomaly evidence.
        let Some(word) = word_tokens.iter().find(|word| {
            word.start_byte <= token.start_byte
                && token.end_byte <= word.end_byte
                && word.text.chars().count() > 1
                && is_chinese_string(&word.text)
                && !lexicon.is_ignored(&word.text)
        }) else {
            continue;
        };
        let relative_position = sentence[word.start_byte..token.start_byte].chars().count();
        matches.push(Suspect {
            start_byte: base_byte + word.start_byte,
            end_byte: base_byte + word.end_byte,
            original: word.text.clone(),
            kind: DetectionKind::Character,
            exact_replacement: None,
            evidence: DetectionEvidence {
                source_is_general_word: lexicon.is_general_known_word(&word.text)
                    || segmenter.has_word(&word.text),
                char_outliers: vec![(relative_position, strength)],
                segmentation: segmenter.candidate_span_evidence(
                    analysis,
                    word.start_byte,
                    word.end_byte,
                ),
                ..DetectionEvidence::default()
            },
        });
    }
    Ok(matches)
}

fn align_window_scores(scores: &[f64], n: usize, char_len: usize) -> Vec<f64> {
    if scores.is_empty() {
        return vec![0.0; char_len];
    }
    let mut padded = Vec::with_capacity(scores.len() + (2 * (n - 1)));
    padded.extend(std::iter::repeat(scores[0]).take(n - 1));
    padded.extend_from_slice(scores);
    padded.extend(std::iter::repeat(*scores.last().unwrap_or(&scores[0])).take(n - 1));
    (0..char_len)
        .map(|start| padded[start..start + n].iter().sum::<f64>() / n as f64)
        .collect()
}

fn mad_outlier_indices(scores: &[f64], ratio: f64, threshold: f64) -> Vec<(usize, f64)> {
    if scores.is_empty() || scores.iter().any(|score| !score.is_finite()) {
        return Vec::new();
    }
    let score_median = median(scores);
    let deviations = scores
        .iter()
        .map(|score| (score - score_median).abs())
        .collect::<Vec<_>>();
    let mad = median(&deviations);
    if mad == 0.0 {
        return Vec::new();
    }
    scores
        .iter()
        .zip(deviations)
        .enumerate()
        .filter_map(|(index, (score, deviation))| {
            let strength = ratio * deviation / mad;
            (strength > threshold && *score < score_median).then_some((index, strength))
        })
        .collect()
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    }
}

/// Merge suspects that cover exactly the same bytes. Their evidence is kept
/// together; conflicts between different spans are resolved only after the
/// decision layer has judged each candidate.
fn merge_evidence(mut candidates: Vec<Suspect>) -> Vec<Suspect> {
    candidates.sort_by(|left, right| {
        left.start_byte
            .cmp(&right.start_byte)
            .then_with(|| left.end_byte.cmp(&right.end_byte))
            .then_with(|| detection_priority(left.kind).cmp(&detection_priority(right.kind)))
            .then_with(|| left.original.cmp(&right.original))
    });
    let mut merged = Vec::<Suspect>::new();
    for candidate in candidates {
        if let Some(previous) = merged.last_mut() {
            if previous.start_byte == candidate.start_byte
                && previous.end_byte == candidate.end_byte
            {
                previous.merge(candidate);
                continue;
            }
        }
        merged.push(candidate);
    }
    merged
}

fn detection_priority(kind: DetectionKind) -> u8 {
    match kind {
        DetectionKind::ConfirmedEntitySpelling => 0,
        DetectionKind::ConfusionWord => 1,
        DetectionKind::OutOfVocabularyWord => 2,
        DetectionKind::Character => 3,
    }
}
