// SPDX-License-Identifier: GPL-3.0-only
//! Offline Chinese proofreading engine used by Sujian's platform independent Core.
//!
//! The primary correction path is a local MacBERT4CSC ONNX model. Model files
//! are supplied by the application bundle or an explicit local path; this
//! crate never downloads them. The engine returns byte-addressed suggestions
//! and never mutates the supplied text.

pub mod confusion;
pub mod diagnostic;
pub mod neural;
pub mod postprocess;

mod corrector;
mod lexicon;
mod proper_names;
mod rules;
mod segment;
mod tokenizer;

pub use diagnostic::CorrectionDiagnostic;
pub use lexicon::{Lexicon, LexiconPaths};
pub use neural::{NeuralPredictor, PositionPrediction, ProofreadingModel};
pub use postprocess::Calibrator;
pub use proper_names::{is_valid_work_entity, ProperNameKind};
pub use rules::RuleSet;

use std::sync::Arc;
use thiserror::Error;

/// Configuration shared by correction requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorrectionConfig {
    /// Maximum number of ranked replacements returned for each suspicious span.
    /// The neural pipeline emits at most one equal-length replacement per
    /// character position.
    pub max_candidates_per_error: usize,
    /// Include warning-level suggestions. Confirmed author relations and
    /// explicit rules remain visible when this is false.
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

/// Stateless-in-request correction engine with a shared model and immutable
/// lexicon/rule data. `scan` returns suggestions and never edits the caller's
/// text.
pub struct CorrectionEngine {
    corrector: corrector::Corrector,
}

impl CorrectionEngine {
    pub fn new(
        predictor: Arc<dyn NeuralPredictor>,
        lexicon: Lexicon,
        rules: RuleSet,
        config: CorrectionConfig,
    ) -> Result<Self, CorrectionError> {
        Ok(Self {
            corrector: corrector::Corrector::new(predictor, lexicon, rules, config)?,
        })
    }

    /// Create an engine that shares immutable lexicon data across requests.
    pub fn new_shared(
        predictor: Arc<dyn NeuralPredictor>,
        lexicon: Arc<Lexicon>,
        rules: RuleSet,
        config: CorrectionConfig,
    ) -> Result<Self, CorrectionError> {
        Ok(Self {
            corrector: corrector::Corrector::new_shared(predictor, lexicon, rules, config)?,
        })
    }

    /// Create an engine that reports deterministic relations and rules without
    /// requiring a neural model.
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
            corrector: corrector::Corrector::rules_only(lexicon, rules, config)?,
        })
    }

    pub fn scan(&self, text: &str) -> Result<Vec<CorrectionDiagnostic>, CorrectionError> {
        self.corrector.suggest(text)
    }
}
