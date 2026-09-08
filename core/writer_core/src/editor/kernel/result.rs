use serde::{Deserialize, Serialize};

use super::types::{DisplayPatch, EditorVisualIntent};
use crate::editor::strong_types::{EditorRevision, Utf8ByteRange};

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

/// #624 评论8 — 内容增量：本次编辑实际插入/删除的字符统计。
///
/// 直接对本次 inserted_text / deleted_text 局部计算，不依赖 old/new 两份全文。
/// Cursor/selection/composition-update 没有 committed 正文变化时为全 0；
/// composition commit、Undo/Redo 按实际 delta 统计。
/// `_chars` 按 Unicode scalar（`char`）计数，非 UTF-8 byte、非 UTF-16 code unit。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorContentDelta {
    pub inserted_chars: u32,
    pub deleted_chars: u32,
    pub inserted_non_whitespace_chars: u32,
    pub deleted_non_whitespace_chars: u32,
}

impl EditorContentDelta {
    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn from_inserted_text(inserted: &str) -> Self {
        Self {
            inserted_chars: inserted.chars().count() as u32,
            deleted_chars: 0,
            inserted_non_whitespace_chars: inserted.chars().filter(|c| !c.is_whitespace()).count()
                as u32,
            deleted_non_whitespace_chars: 0,
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn from_deleted_text(deleted: &str) -> Self {
        Self {
            inserted_chars: 0,
            deleted_chars: deleted.chars().count() as u32,
            inserted_non_whitespace_chars: 0,
            deleted_non_whitespace_chars: deleted.chars().filter(|c| !c.is_whitespace()).count()
                as u32,
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    pub(crate) fn from_texts(inserted: &str, deleted: &str) -> Self {
        Self {
            inserted_chars: inserted.chars().count() as u32,
            deleted_chars: deleted.chars().count() as u32,
            inserted_non_whitespace_chars: inserted.chars().filter(|c| !c.is_whitespace()).count()
                as u32,
            deleted_non_whitespace_chars: deleted.chars().filter(|c| !c.is_whitespace()).count()
                as u32,
        }
    }

    #[cfg(test)]
    pub(crate) fn from_parts(
        inserted_chars: u32,
        deleted_chars: u32,
        inserted_non_whitespace_chars: u32,
        deleted_non_whitespace_chars: u32,
    ) -> Self {
        Self {
            inserted_chars,
            deleted_chars,
            inserted_non_whitespace_chars,
            deleted_non_whitespace_chars,
        }
    }

    /// 累加多个 delta（replace-all / deleteSurrounding / undo 多 patch）。
    pub(crate) fn accumulate(&mut self, other: &Self) {
        self.inserted_chars = self.inserted_chars.saturating_add(other.inserted_chars);
        self.deleted_chars = self.deleted_chars.saturating_add(other.deleted_chars);
        self.inserted_non_whitespace_chars = self
            .inserted_non_whitespace_chars
            .saturating_add(other.inserted_non_whitespace_chars);
        self.deleted_non_whitespace_chars = self
            .deleted_non_whitespace_chars
            .saturating_add(other.deleted_non_whitespace_chars);
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
    /// #624 评论8：本次编辑的字符增量（正文无变化时为全 0）。
    pub content_delta: EditorContentDelta,
}

#[derive(Debug, Clone)]
pub enum EditorInputError {
    InvalidCursorOffset { offset: usize, text_len: usize },
}

impl std::fmt::Display for EditorInputError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidCursorOffset { offset, text_len } => {
                write!(
                    f,
                    "cursor offset {} is not a valid UTF-8 char boundary (text len {})",
                    offset, text_len
                )
            }
        }
    }
}

impl std::error::Error for EditorInputError {}
