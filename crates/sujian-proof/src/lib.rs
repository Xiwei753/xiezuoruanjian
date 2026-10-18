// SPDX-License-Identifier: GPL-3.0-only
//! Offline Chinese proofreading engine used by Sujian's platform independent Core.
//!
//! Suggestions carry UTF-8 byte ranges and never mutate the supplied text. The
//! KenLM model is opened from a caller supplied local path and is not embedded
//! in this crate or its binaries.

pub mod confusion;
pub mod diagnostic;
pub mod lm;

mod candidates;
mod corrector;
mod detector;
mod lexicon;
mod rules;
mod segment;

pub use diagnostic::CorrectionDiagnostic;
pub use lexicon::{Lexicon, LexiconPaths};
pub use lm::LanguageScorer;
pub use rules::RuleSet;

use std::sync::Arc;
use thiserror::Error;

/// Configuration shared by correction requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorrectionConfig {
    /// Maximum number of ranked replacements returned for each suspicious span.
    pub max_candidates_per_error: usize,
    /// Include context-dependent hints that do not meet the explicit-error threshold.
    pub include_weak_hints: bool,
}

impl Default for CorrectionConfig {
    fn default() -> Self {
        Self {
            max_candidates_per_error: 5,
            include_weak_hints: true,
        }
    }
}

/// Errors from model loading, configuration, or invalid request ranges.
#[derive(Debug, Error)]
pub enum CorrectionError {
    #[error("correction model error: {0}")]
    Model(String),
    #[error("invalid UTF-8 byte range {start}..{end} for text of {text_len} bytes")]
    InvalidRange {
        start: usize,
        end: usize,
        text_len: usize,
    },
    #[error("invalid correction configuration: {0}")]
    InvalidConfiguration(String),
    #[error("correction model is not configured")]
    ModelUnavailable,
}

/// Stateless-in-request traditional correction engine with a shared model and
/// immutable lexicon/rule data. `scan` returns suggestions and never edits the
/// caller's text.
pub struct CorrectionEngine {
    corrector: corrector::TraditionalCorrector,
}

impl CorrectionEngine {
    pub fn new(
        scorer: Arc<dyn LanguageScorer>,
        lexicon: Lexicon,
        rules: RuleSet,
        config: CorrectionConfig,
    ) -> Result<Self, CorrectionError> {
        Ok(Self {
            corrector: corrector::TraditionalCorrector::new(scorer, lexicon, rules, config)?,
        })
    }

    /// Create an engine that shares immutable lexicon data across requests.
    pub fn new_shared(
        scorer: Arc<dyn LanguageScorer>,
        lexicon: Arc<Lexicon>,
        rules: RuleSet,
        config: CorrectionConfig,
    ) -> Result<Self, CorrectionError> {
        Ok(Self {
            corrector: corrector::TraditionalCorrector::new_shared(scorer, lexicon, rules, config)?,
        })
    }

    /// Create an engine that reports fixed rules without requiring a KenLM model.
    pub fn rules_only(
        lexicon: Lexicon,
        rules: RuleSet,
        config: CorrectionConfig,
    ) -> Result<Self, CorrectionError> {
        Self::rules_only_shared(Arc::new(lexicon), rules, config)
    }

    /// Create a rules-only engine that shares immutable lexicon data.
    pub fn rules_only_shared(
        lexicon: Arc<Lexicon>,
        rules: RuleSet,
        config: CorrectionConfig,
    ) -> Result<Self, CorrectionError> {
        Ok(Self {
            corrector: corrector::TraditionalCorrector::rules_only(lexicon, rules, config)?,
        })
    }

    pub fn scan(&self, text: &str) -> Result<Vec<CorrectionDiagnostic>, CorrectionError> {
        self.corrector.suggest(text)
    }
}
