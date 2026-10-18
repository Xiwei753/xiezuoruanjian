// SPDX-License-Identifier: GPL-3.0-only
//! Stable diagnostics returned by the offline proofreading engine.

use serde::{Deserialize, Serialize};

/// A suggested correction in the original UTF-8 text.
///
/// `start_byte..end_byte` is a half-open UTF-8 byte range. A diagnostic never
/// mutates the input. `confidence` is absent unless the engine has a calibrated
/// probability; a KenLM log score is only a ranking signal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionDiagnostic {
    pub start_byte: usize,
    pub end_byte: usize,
    pub original: String,
    pub replacement: String,
    pub source: String,
    pub rule_id: String,
    pub severity: String,
    pub confidence: Option<f32>,
}
