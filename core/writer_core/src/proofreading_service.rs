use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProofreadingMode {
    Off,
    MarkOnly,
    ConfirmReplace,
    AutoReplace,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ProofreadingStrength {
    Low,
    Normal,
    Strict,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CorrectionRule {
    pub wrong_word: String,
    pub correct_word: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IgnoreRule {
    pub word: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofreadingDictionary {
    pub corrections: Vec<CorrectionRule>,
    pub ignores: Vec<IgnoreRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProofreadingSuggestion {
    pub start_index: usize,
    pub end_index: usize,
    pub original_text: String,
    pub suggested_text: String,
    pub description: Option<String>,
}

pub struct ProofreadingService {
    pub mode: ProofreadingMode,
    pub strength: ProofreadingStrength,
    pub dictionary: ProofreadingDictionary,
}

impl ProofreadingService {
    pub fn new() -> Self {
        Self {
            mode: ProofreadingMode::MarkOnly,
            strength: ProofreadingStrength::Normal,
            dictionary: ProofreadingDictionary {
                corrections: Vec::new(),
                ignores: Vec::new(),
            },
        }
    }

    pub fn proofread(&self, _text: &str) -> crate::Result<Vec<ProofreadingSuggestion>> {
        if self.mode == ProofreadingMode::Off {
            return Ok(Vec::new());
        }

        let suggestions = Vec::new();
        Ok(suggestions)
    }
}

impl Default for ProofreadingService {
    fn default() -> Self {
        Self::new()
    }
}
