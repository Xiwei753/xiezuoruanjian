use serde::{Deserialize, Serialize};

use super::types::{DisplayPatch, EditorVisualIntent};

/// 编辑结果分类 — 平台必须区分不同结果走不同恢复路径。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EditorEditOutcome {
    /// 编辑成功应用，正文已变更
    Applied(EditorEditResult),
    /// 编辑成功应用，但平台传入的选区 offset 不在 char boundary 上，内核已自动对齐
    AppliedWithAdjustedSelection(EditorEditResult),
    /// 命令无实际效果（如空替换、空选区变更），正文未变
    NoChange(EditorEditResult),
    /// expected_revision 与当前 revision 不匹配，平台需用结果中的最新 revision 重试
    StaleRevision(EditorEditResult),
    /// offset 不在 UTF-8 char boundary 上或超出文本范围
    InvalidOffset(EditorEditResult),
    /// range 语义非法（如 start ≥ end 对于 delete）
    InvalidRange(EditorEditResult),
}

impl EditorEditOutcome {
    /// 将所有变体统一为 `EditorEditResult`。
    ///
    /// 所有变体均携带 `EditorEditResult`，即使编辑未成功应用也包含
    /// 最新 revision、选区等信息，供平台端读取并更新显示状态。
    /// 消费 `self`，调用后原 outcome 不可再使用。
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

/// 编辑结果 — EditorKernel.apply() 的返回值。
///
/// 包含正文变化（display_patches）、选区变化和视觉意图。
/// 平台端按此结果增量更新显示镜像、布局和动画。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorEditResult {
    /// 事务 ID
    pub transaction_id: u64,
    /// 基础 revision ID
    pub base_revision: u64,
    /// 新 revision ID
    pub new_revision: u64,
    /// 显示补丁列表
    pub display_patches: Vec<DisplayPatch>,
    /// 旧选区（UTF-8 byte offset）
    pub old_selection_byte_range: (usize, usize),
    /// 新选区（UTF-8 byte offset）
    pub new_selection_byte_range: (usize, usize),
    /// 视觉意图
    pub visual_intent: EditorVisualIntent,
}

/// 编辑器输入校验错误
#[derive(Debug, Clone)]
pub enum EditorInputError {
    /// 光标 offset 超出文本长度或不在 UTF-8 char boundary 上
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