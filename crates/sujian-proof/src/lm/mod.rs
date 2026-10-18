// SPDX-License-Identifier: GPL-3.0-only
//! Read-only pure-Rust KenLM probing-model queries.
//!
//! The reader currently supports KenLM mmap binary format version 5 with the
//! unquantized `PROBING` backend and an embedded vocabulary. Model bytes stay
//! in a read-only file mapping; the small vocabulary hash index is built once
//! so clones share both it and the mapping.

mod binary;
mod mapping;
mod probing;
mod score;
mod vocab;

use std::{path::Path, sync::Arc};

use thiserror::Error;

use crate::CorrectionError;

/// Scoring operations needed by candidate ranking.
///
/// Scores are base-10 log probabilities and are ranking values, not calibrated
/// confidence percentages.
pub trait LanguageScorer: Send + Sync {
    fn score_tokens(&self, tokens: &[&str]) -> Result<f64, CorrectionError>;
    /// Score the supplied sequence without adding `<s>` or `</s>`. The default
    /// keeps third-party scorer implementations source-compatible; scorers
    /// whose boundary behavior differs should override it.
    fn score_tokens_without_boundaries(&self, tokens: &[&str]) -> Result<f64, CorrectionError> {
        self.score_tokens(tokens)
    }
    fn perplexity(&self, tokens: &[&str]) -> Result<f64, CorrectionError>;
}

/// Errors returned while opening or querying a KenLM model.
#[derive(Debug, Error)]
pub enum KenlmError {
    #[error("could not open KenLM model: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid KenLM model: {0}")]
    InvalidFormat(String),
    #[error("unsupported KenLM model type {0}; this reader supports PROBING (0)")]
    UnsupportedModelType(u32),
    #[error("unsupported KenLM search version {0}; this reader supports version 0")]
    UnsupportedSearchVersion(u32),
    #[error("unsupported KenLM vocabulary version {0}; this reader supports version 0")]
    UnsupportedVocabularyVersion(u32),
    #[error("KenLM model order {0} is outside the supported range 2..=10")]
    UnsupportedOrder(usize),
}

struct Model {
    mapping: mapping::MappedModel,
    layout: binary::ModelLayout,
    vocabulary: vocab::Vocabulary,
}

/// An immutable KenLM model instance backed by a read-only file mapping.
///
/// The model file must not be modified or truncated while this value or one
/// of its clones is alive. The mapping is shared when the model is cloned.
#[derive(Clone)]
pub struct KenlmModel {
    inner: Arc<Model>,
}

impl KenlmModel {
    /// Open a KenLM binary model from a caller-supplied path.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, KenlmError> {
        let mapping = mapping::MappedModel::open(path)?;
        let layout = binary::parse(mapping.as_bytes())?;
        let vocabulary = vocab::parse(
            mapping.as_bytes(),
            layout.vocabulary_strings.clone(),
            layout.counts[0],
        )?;

        Ok(Self {
            inner: Arc::new(Model {
                mapping,
                layout,
                vocabulary,
            }),
        })
    }

    /// Return the KenLM sentence score in base-10 log probability.
    ///
    /// `<s>` is added as context and `</s>` is scored at the end, matching
    /// KenLM's default `Model.score(..., bos=true, eos=true)` behavior.
    pub fn score_tokens(&self, tokens: &[&str]) -> Result<f64, KenlmError> {
        score::score_tokens(&self.inner, tokens, true, true)
    }

    /// Return the score for tokens without KenLM's BOS and EOS events.
    pub fn score_tokens_without_boundaries(&self, tokens: &[&str]) -> Result<f64, KenlmError> {
        score::score_tokens(&self.inner, tokens, false, false)
    }

    /// Return perplexity as `10 ^ (-score / scored_events)`.
    ///
    /// The event count includes the scored end-of-sentence marker and excludes
    /// the beginning-of-sentence context marker. OOV tokens use KenLM's
    /// `<unk>` vocabulary id and normal unigram/backoff scores.
    pub fn perplexity(&self, tokens: &[&str]) -> Result<f64, KenlmError> {
        let score = self.score_tokens(tokens)?;
        let scored_events = tokens.len().saturating_add(1);
        Ok(10.0_f64.powf(-score / scored_events as f64))
    }
}

impl LanguageScorer for KenlmModel {
    fn score_tokens(&self, tokens: &[&str]) -> Result<f64, CorrectionError> {
        KenlmModel::score_tokens(self, tokens)
            .map_err(|error| CorrectionError::Model(error.to_string()))
    }

    fn score_tokens_without_boundaries(&self, tokens: &[&str]) -> Result<f64, CorrectionError> {
        KenlmModel::score_tokens_without_boundaries(self, tokens)
            .map_err(|error| CorrectionError::Model(error.to_string()))
    }

    fn perplexity(&self, tokens: &[&str]) -> Result<f64, CorrectionError> {
        KenlmModel::perplexity(self, tokens)
            .map_err(|error| CorrectionError::Model(error.to_string()))
    }
}
