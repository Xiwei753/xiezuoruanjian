use serde::{Deserialize, Serialize};

use crate::editor::strong_types::{EditorRevision, Utf8ByteRange};
use super::types::{DisplayPatch, EditorVisualIntent};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EditorEditOutcome {
    Applied(EditorEditResult),
    AppliedWithAdjustedSelection(EditorEditResult),
    NoChange(EditorEditResult),
    StaleRevision(EditorEditResult),
    InvalidOffset(EditorEditResult),
    InvalidRange(EditorEditResult),
}

impl EditorEditOutcome {
    pub fn into_result(self) -> EditorEditResult {
        match self {
            EditorEditOutcome::Applied(r)
            | EditorEditOutcome::AppliedWithAdjustedSelection(r)
            | EditorEditOutcome::NoChange(r)
            | EditorEditOutcome::StaleRevision(r)
            | EditorEditOutcome::InvalidOffset(r)
            | EditorEditOutcome::InvalidRange(r) => r,
        }
    }

    pub fn is_applied(&self) -> bool {
        matches!(
            self,
            EditorEditOutcome::Applied(_) | EditorEditOutcome::AppliedWithAdjustedSelection(_)
        )
    }

    pub fn is_stale(&self) -> bool {
        matches!(self, EditorEditOutcome::StaleRevision(_))
    }

    pub fn is_invalid(&self) -> bool {
        matches!(
            self,
            EditorEditOutcome::InvalidOffset(_) | EditorEditOutcome::InvalidRange(_)
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorEditResult {
    pub transaction_id: u64,
    pub base_revision: EditorRevision,
    pub new_revision: EditorRevision,
    pub display_patches: Vec<DisplayPatch>,
    pub old_selection_byte_range: Utf8ByteRange,
    pub new_selection_byte_range: Utf8ByteRange,
    pub visual_intent: EditorVisualIntent,
}

#[derive(Debug, Clone)]
pub enum EditorInputError {
    InvalidCursorOffset { offset: usize, text_len: usize },
}

impl std::fmt::Display for EditorInputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCursorOffset { offset, text_len } => {
                write!(f, "cursor offset {} is not a valid UTF-8 char boundary (text len {})", offset, text_len)
            }
        }
    }
}

impl std::error::Error for EditorInputError {}
