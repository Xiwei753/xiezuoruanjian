//! Correction diagnostics and user actions exposed across platform boundaries.

/// A correction suggestion tied to the exact Core text revision that produced it.
/// Byte ranges are UTF-8 half-open intervals; UI indices are computed by the platform.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionSuggestionDto {
    pub start_byte: u32,
    pub end_byte: u32,
    pub original: String,
    pub replacement: String,
    pub source: String,
    pub rule_id: String,
    pub severity: String,
    pub text_revision: u64,
}

/// Current correction resource availability under the Core app-data root.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionResourceStatusDto {
    pub model_configured: bool,
    pub model_file_available: bool,
    pub model_path: Option<String>,
    pub bundled_lexicon_available: bool,
    pub installed_dictionary_files: Vec<String>,
    pub missing_dictionary_files: Vec<String>,
}

/// Lifecycle status for a caller-supplied local correction model path.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CorrectionModelPathStatusDto {
    pub configured: bool,
    pub in_use: bool,
    pub deletion_eligible: bool,
}

impl TryFrom<(sujian_proof::CorrectionDiagnostic, u64)> for CorrectionSuggestionDto {
    type Error = crate::api::WriterError;

    fn try_from(
        (diagnostic, text_revision): (sujian_proof::CorrectionDiagnostic, u64),
    ) -> Result<Self, Self::Error> {
        let start_byte = u32::try_from(diagnostic.start_byte)
            .map_err(|error| crate::api::WriterError::Other(error.to_string()))?;
        let end_byte = u32::try_from(diagnostic.end_byte)
            .map_err(|error| crate::api::WriterError::Other(error.to_string()))?;
        Ok(Self {
            start_byte,
            end_byte,
            original: diagnostic.original,
            replacement: diagnostic.replacement,
            source: diagnostic.source,
            rule_id: diagnostic.rule_id,
            severity: diagnostic.severity,
            text_revision,
        })
    }
}

/// Scope of an explicit user ignore action.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum CorrectionIgnoreScopeDto {
    Once,
    Chapter,
    Permanent,
    Rule,
}
