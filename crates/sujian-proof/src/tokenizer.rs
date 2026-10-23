// SPDX-License-Identifier: GPL-3.0-only
//! Deterministic BERT tokenizer for the bundled MacBERT4CSC vocabulary.
//!
//! The proofreading model predicts one character per original Han token. This
//! tokenizer therefore keeps an explicit mapping from every Chinese character
//! to its Unicode scalar, UTF-8 byte range, and position in the model input.
//! Tokens that do not map to exactly one original Han character are retained
//! as context but can never become a correction target.

use std::collections::HashMap;
use std::path::Path;

use crate::CorrectionError;

/// Maximum number of model positions supported by the bundled model.
pub(crate) const MAX_MODEL_POSITIONS: usize = 512;
/// Number of tokens reserved for `[CLS]` and `[SEP]`.
pub(crate) const SPECIAL_TOKEN_SLOTS: usize = 2;
/// Overlap used when a sentence is longer than one model window.
const WINDOW_STRIDE: usize = 64;

/// Why a token exists in the model input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TokenOrigin {
    /// A Chinese character with an exact one-to-one source position.
    Han,
    /// Any other token: ASCII word piece, punctuation, special token, or an
    /// unmapped Han character. It participates in context only.
    Context,
}

/// One model token together with the original source range it came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EncodedToken {
    pub(crate) id: i64,
    pub(crate) origin: TokenOrigin,
    pub(crate) start_byte: usize,
    pub(crate) end_byte: usize,
    pub(crate) original: Option<char>,
}

/// A model-ready encoding of one complete sentence.
#[derive(Clone, Debug)]
pub(crate) struct EncodedSentence {
    pub(crate) tokens: Vec<EncodedToken>,
    cls_id: i64,
    sep_id: i64,
}

/// One non-special model window.
#[derive(Clone, Debug)]
pub(crate) struct TokenWindow {
    pub(crate) input_ids: Vec<i64>,
    pub(crate) attention_mask: Vec<i64>,
    pub(crate) token_type_ids: Vec<i64>,
    /// Maps a position inside this window (including `[CLS]`) to the index in
    /// [`EncodedSentence::tokens`], or `None` for special tokens.
    pub(crate) source_indices: Vec<Option<usize>>,
    /// Byte distance from the window edge for each source token; used to
    /// resolve overlap in favour of the window with more context.
    pub(crate) edge_distance: Vec<usize>,
}

impl EncodedSentence {
    /// Split the encoding into model-sized windows. A short sentence produces
    /// one window; long sentences overlap by `WINDOW_STRIDE` tokens so a
    /// correction at a boundary still sees context on both sides.
    pub(crate) fn windows(&self) -> Vec<TokenWindow> {
        let capacity = MAX_MODEL_POSITIONS.saturating_sub(SPECIAL_TOKEN_SLOTS);
        if self.tokens.is_empty() {
            return vec![self.window(0, 0)];
        }
        let mut windows = Vec::new();
        let mut start = 0;
        while start < self.tokens.len() {
            let end = (start + capacity).min(self.tokens.len());
            windows.push(self.window(start, end));
            if end == self.tokens.len() {
                break;
            }
            start = end.saturating_sub(WINDOW_STRIDE);
        }
        windows
    }

    fn window(&self, start: usize, end: usize) -> TokenWindow {
        let mut input_ids = Vec::with_capacity(end - start + SPECIAL_TOKEN_SLOTS);
        let mut source_indices = Vec::with_capacity(end - start + SPECIAL_TOKEN_SLOTS);
        let mut edge_distance = Vec::with_capacity(end - start + SPECIAL_TOKEN_SLOTS);
        input_ids.push(self.cls_id);
        source_indices.push(None);
        edge_distance.push(usize::MAX);
        for (offset, token) in self.tokens[start..end].iter().enumerate() {
            input_ids.push(token.id);
            source_indices.push(Some(start + offset));
            edge_distance.push((offset + 1).min(end - start - offset));
        }
        input_ids.push(self.sep_id);
        source_indices.push(None);
        edge_distance.push(usize::MAX);
        let len = input_ids.len();
        TokenWindow {
            input_ids,
            attention_mask: vec![1; len],
            token_type_ids: vec![0; len],
            source_indices,
            edge_distance,
        }
    }
}

/// BERT basic + WordPiece tokenizer backed by the bundled `tokenizer.json`.
pub(crate) struct BertTokenizer {
    vocab: HashMap<String, i64>,
    han_ids: Vec<(i64, char)>,
    cls_id: i64,
    sep_id: i64,
    unk_id: i64,
    lowercase: bool,
    continuing_subword_prefix: String,
    max_input_chars_per_word: usize,
}

impl BertTokenizer {
    /// Load the exact tokenizer configuration shipped with the model. The
    /// bundled `tokenizer.json` is authoritative for vocabulary, normalizer,
    /// and WordPiece settings; `vocab.txt` is only a compatibility fallback
    /// for old resource directories.
    pub(crate) fn open_bundle(bundle_dir: &Path) -> Result<Self, CorrectionError> {
        let tokenizer_path = bundle_dir.join("tokenizer.json");
        if tokenizer_path.is_file() {
            let content = std::fs::read_to_string(&tokenizer_path).map_err(|error| {
                CorrectionError::Model(format!(
                    "failed to read tokenizer configuration {}: {error}",
                    tokenizer_path.display()
                ))
            })?;
            return Self::from_tokenizer_json(&content);
        }
        Self::open(&bundle_dir.join("vocab.txt"))
    }

    pub(crate) fn open(path: &Path) -> Result<Self, CorrectionError> {
        let content = std::fs::read_to_string(path).map_err(|error| {
            CorrectionError::Model(format!(
                "failed to read tokenizer vocabulary {}: {error}",
                path.display()
            ))
        })?;
        Self::from_vocab(&content)
    }

    pub(crate) fn from_tokenizer_json(content: &str) -> Result<Self, CorrectionError> {
        let value: serde_json::Value = serde_json::from_str(content)
            .map_err(|error| CorrectionError::Model(format!("invalid tokenizer.json: {error}")))?;
        let model = value
            .get("model")
            .ok_or_else(|| CorrectionError::Model("tokenizer.json lacks model".into()))?;
        if model.get("type").and_then(|value| value.as_str()) != Some("WordPiece") {
            return Err(CorrectionError::Model(
                "only the bundled BERT WordPiece tokenizer is supported".into(),
            ));
        }
        let vocab_object = model
            .get("vocab")
            .and_then(|value| value.as_object())
            .ok_or_else(|| CorrectionError::Model("tokenizer.json lacks model.vocab".into()))?;
        let mut vocab = HashMap::with_capacity(vocab_object.len());
        let mut han_ids = Vec::new();
        for (token, id) in vocab_object {
            let id = id
                .as_i64()
                .ok_or_else(|| CorrectionError::Model("tokenizer id is not an integer".into()))?;
            if let Some(ch) = single_han(token) {
                han_ids.push((id, ch));
            }
            vocab.insert(token.clone(), id);
        }
        let normalizer = value.get("normalizer");
        let lowercase = normalizer
            .and_then(|normalizer| normalizer.get("lowercase"))
            .and_then(|value| value.as_bool())
            .unwrap_or(true);
        let continuing_subword_prefix = model
            .get("continuing_subword_prefix")
            .and_then(|value| value.as_str())
            .unwrap_or("##")
            .to_string();
        let max_input_chars_per_word = model
            .get("max_input_chars_per_word")
            .and_then(|value| value.as_u64())
            .and_then(|value| usize::try_from(value).ok())
            .filter(|value| *value > 0)
            .unwrap_or(100);
        Self::from_parts(
            vocab,
            han_ids,
            lowercase,
            continuing_subword_prefix,
            max_input_chars_per_word,
        )
    }

    pub(crate) fn from_vocab(content: &str) -> Result<Self, CorrectionError> {
        let mut vocab = HashMap::new();
        let mut han_ids = Vec::new();
        for (index, token) in content.lines().enumerate() {
            let token = token.trim_end_matches('\r');
            let id = index as i64;
            if let Some(ch) = single_han(token) {
                han_ids.push((id, ch));
            }
            vocab.insert(token.to_string(), id);
        }
        Self::from_parts(vocab, han_ids, true, "##".to_string(), 100)
    }

    fn from_parts(
        vocab: HashMap<String, i64>,
        han_ids: Vec<(i64, char)>,
        lowercase: bool,
        continuing_subword_prefix: String,
        max_input_chars_per_word: usize,
    ) -> Result<Self, CorrectionError> {
        let required = |token: &str| {
            vocab.get(token).copied().ok_or_else(|| {
                CorrectionError::Model(format!("tokenizer vocabulary is missing {token}"))
            })
        };
        Ok(Self {
            cls_id: required("[CLS]")?,
            sep_id: required("[SEP]")?,
            unk_id: required("[UNK]")?,
            vocab,
            han_ids,
            lowercase,
            continuing_subword_prefix,
            max_input_chars_per_word,
        })
    }

    /// Candidate token ids that represent exactly one Chinese character. The
    /// model can only replace one original character with another; multi-char
    /// word pieces and special tokens are never replacement targets.
    pub(crate) fn han_ids(&self) -> &[(i64, char)] {
        &self.han_ids
    }

    pub(crate) fn encode(&self, text: &str) -> EncodedSentence {
        let mut tokens = Vec::new();
        for piece in basic_tokens(text, self.lowercase) {
            match piece {
                BasicPiece::Han {
                    ch,
                    start_byte,
                    end_byte,
                } => {
                    let mut encoded = [0; 4];
                    let id = self.vocab.get(ch.encode_utf8(&mut encoded)).copied();
                    tokens.push(match id {
                        Some(id) => EncodedToken {
                            id,
                            origin: TokenOrigin::Han,
                            start_byte,
                            end_byte,
                            original: Some(ch),
                        },
                        None => EncodedToken {
                            id: self.unk_id,
                            origin: TokenOrigin::Context,
                            start_byte,
                            end_byte,
                            original: None,
                        },
                    });
                }
                BasicPiece::Text {
                    text,
                    start_byte,
                    end_byte,
                } => {
                    for (piece_start, piece) in wordpiece(
                        &text,
                        self.unk_id,
                        &self.vocab,
                        &self.continuing_subword_prefix,
                        self.max_input_chars_per_word,
                    ) {
                        tokens.push(EncodedToken {
                            id: piece,
                            origin: TokenOrigin::Context,
                            start_byte: start_byte + piece_start,
                            end_byte,
                            original: None,
                        });
                    }
                }
            }
        }
        EncodedSentence {
            tokens,
            cls_id: self.cls_id,
            sep_id: self.sep_id,
        }
    }
}

enum BasicPiece {
    Han {
        ch: char,
        start_byte: usize,
        end_byte: usize,
    },
    Text {
        text: String,
        start_byte: usize,
        end_byte: usize,
    },
}

/// BERT `BasicTokenizer` for the scripts the editor can contain. Chinese
/// characters become individual tokens; Latin runs are lowercased and split
/// on whitespace/punctuation before WordPiece.
fn basic_tokens(text: &str, lowercase: bool) -> Vec<BasicPiece> {
    let mut pieces = Vec::new();
    let mut word_start: Option<usize> = None;
    let mut word = String::new();

    fn flush(
        pieces: &mut Vec<BasicPiece>,
        word_start: &mut Option<usize>,
        word: &mut String,
        end_byte: usize,
    ) {
        if let Some(start_byte) = word_start.take() {
            if !word.is_empty() {
                pieces.push(BasicPiece::Text {
                    text: std::mem::take(word),
                    start_byte,
                    end_byte,
                });
            }
        }
    }

    for (byte, ch) in text.char_indices() {
        if is_han(ch) {
            flush(&mut pieces, &mut word_start, &mut word, byte);
            pieces.push(BasicPiece::Han {
                ch,
                start_byte: byte,
                end_byte: byte + ch.len_utf8(),
            });
            continue;
        }
        if ch.is_whitespace() || is_bert_punctuation(ch) {
            flush(&mut pieces, &mut word_start, &mut word, byte);
            let mut encoded = [0; 4];
            let token = ch.encode_utf8(&mut encoded);
            pieces.push(BasicPiece::Text {
                text: token.to_string(),
                start_byte: byte,
                end_byte: byte + ch.len_utf8(),
            });
            continue;
        }
        if ch.is_control() {
            continue;
        }
        if word_start.is_none() {
            word_start = Some(byte);
        }
        if lowercase {
            for lower in ch.to_lowercase() {
                word.push(lower);
            }
        } else {
            word.push(ch);
        }
    }
    flush(&mut pieces, &mut word_start, &mut word, text.len());
    pieces
}

fn wordpiece(
    word: &str,
    unk_id: i64,
    vocab: &HashMap<String, i64>,
    continuing_subword_prefix: &str,
    max_input_chars_per_word: usize,
) -> Vec<(usize, i64)> {
    if word.is_empty() {
        return Vec::new();
    }
    let chars = word.char_indices().collect::<Vec<_>>();
    // BERT emits one `[UNK]` for the whole overlong/unsegmentable word, not
    // one `[UNK]` per character. Repeating UNK changes the model's context.
    if chars.len() > max_input_chars_per_word {
        return vec![(0, unk_id)];
    }
    let mut result = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let mut end = chars.len();
        let mut found = None;
        while start < end {
            let text = &word[chars[start].0..chars.get(end).map_or(word.len(), |(b, _)| *b)];
            let candidate = if start == 0 {
                text.to_string()
            } else {
                format!("{continuing_subword_prefix}{text}")
            };
            if let Some(id) = vocab.get(&candidate) {
                found = Some((id, end));
                break;
            }
            end -= 1;
        }
        match found {
            Some((id, end)) => {
                result.push((chars[start].0, *id));
                start = end;
            }
            None => return vec![(0, unk_id)],
        }
    }
    result
}

fn single_han(token: &str) -> Option<char> {
    let mut chars = token.chars();
    match (chars.next(), chars.next()) {
        (Some(ch), None) if is_han(ch) => Some(ch),
        _ => None,
    }
}

fn is_han(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2FA1F
            | 0x30000..=0x323AF
    )
}

fn is_bert_punctuation(ch: char) -> bool {
    if ch.is_ascii_punctuation() {
        return true;
    }
    matches!(
        ch,
        '。' | '，'
            | '、'
            | '；'
            | '：'
            | '？'
            | '！'
            | '…'
            | '—'
            | '「'
            | '」'
            | '『'
            | '』'
            | '（'
            | '）'
            | '《'
            | '》'
            | '〈'
            | '〉'
            | '【'
            | '】'
            | '“'
            | '”'
            | '‘'
            | '’'
            | '·'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokenizer() -> BertTokenizer {
        let mut vocab = String::new();
        vocab.push_str("[PAD]\n[UNK]\n[CLS]\n[SEP]\n[MASK]\n");
        vocab.push_str("甲\n乙\n丙\n他\n她\n说\n。\n");
        BertTokenizer::from_vocab(&vocab).expect("vocab")
    }

    #[test]
    fn han_characters_map_to_exact_utf8_ranges() {
        let encoded = tokenizer().encode("甲说。");
        assert_eq!(encoded.tokens.len(), 3);
        assert_eq!(encoded.tokens[0].original, Some('甲'));
        assert_eq!(encoded.tokens[0].start_byte, 0);
        assert_eq!(encoded.tokens[0].end_byte, 3);
        assert_eq!(encoded.tokens[1].start_byte, 3);
        assert_eq!(encoded.tokens[2].end_byte, "甲说。".len());
    }

    #[test]
    fn context_tokens_never_get_a_han_mapping() {
        let encoded = tokenizer().encode("hello甲");
        assert!(encoded.tokens.len() >= 2);
        assert_eq!(encoded.tokens.last().and_then(|t| t.original), Some('甲'));
        assert!(encoded.tokens[..encoded.tokens.len() - 1]
            .iter()
            .all(|token| token.original.is_none()));
    }

    #[test]
    fn long_sentences_split_into_overlapping_windows() {
        let mut vocab = String::new();
        vocab.push_str("[PAD]\n[UNK]\n[CLS]\n[SEP]\n[MASK]\n");
        for ch in '甲'..='龟' {
            vocab.push(ch);
            vocab.push('\n');
        }
        let tokenizer = BertTokenizer::from_vocab(&vocab).expect("vocab");
        let text = "甲".repeat(600);
        let windows = tokenizer.encode(&text).windows();
        assert!(windows.len() >= 2);
        assert!(windows
            .iter()
            .all(|window| window.input_ids.len() <= MAX_MODEL_POSITIONS));
    }
}
