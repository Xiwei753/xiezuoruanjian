use super::cursor_animation::CursorAnimationPlan;
use super::layout_snapshot::{LineSnapshotId, SourceRect};
use super::transaction_key::VisualTransactionKey;

#[derive(Clone, Debug)]
pub(crate) struct TextAnimationGlyphInfo {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub opacity: f64,
    pub snapshot_id: LineSnapshotId,
    pub source_rect: SourceRect,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TextAnimationPlan {
    pub glyphs: Vec<TextAnimationGlyphInfo>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct SelectionPreeditPlan {
    pub has_selection: bool,
    pub selection_ranges: Vec<SelectionRange>,
    pub has_preedit: bool,
    pub preedit_ranges: Vec<PreeditRange>,
}

#[derive(Clone, Debug)]
pub(crate) struct SelectionRange {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub color: String,
}

#[derive(Clone, Debug)]
pub(crate) struct PreeditRange {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub color: String,
    pub underline: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FrameContext {
    pub active_transaction_keys: Vec<VisualTransactionKey>,
    pub keys_to_complete: Vec<VisualTransactionKey>,
    pub keys_to_cancel: Vec<VisualTransactionKey>,
}

#[derive(Clone, Debug)]
pub(crate) struct CursorStyle {
    pub color: String,
    pub width: f64,
}

impl Default for CursorStyle {
    fn default() -> Self {
        Self {
            color: "#006497".to_string(),
            width: 2.0,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RenderPlan {
    pub text_animation: TextAnimationPlan,
    pub selection_preedit: SelectionPreeditPlan,
    pub cursor: CursorAnimationPlan,
    pub frame_context: FrameContext,
    pub cursor_style: CursorStyle,
}
