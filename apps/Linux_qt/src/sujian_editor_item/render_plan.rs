use super::transaction_key::VisualTransactionKey;
use super::cursor_animation::CursorAnimationPlan;
use super::animation_mode::AnimationMode;
use super::texture_cache::TexturePhase;
use super::line_snapshot::LineSnapshotId;

#[derive(Clone, Debug)]
pub(crate) struct HiddenRangeInfo {
    pub key: VisualTransactionKey,
    pub byte_range: (usize, usize),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct StaticTextPlan {
    pub hidden_ranges: Vec<HiddenRangeInfo>,
}

impl StaticTextPlan {
    pub fn merged_byte_ranges(&self) -> Vec<(usize, usize)> {
        let mut all: Vec<(usize, usize)> = self
            .hidden_ranges
            .iter()
            .map(|r| r.byte_range)
            .collect();
        all.sort_by_key(|r| r.0);
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for (rs, re) in all {
            if let Some(last) = merged.last_mut() {
                if rs <= last.1 {
                    last.1 = last.1.max(re);
                    continue;
                }
            }
            merged.push((rs, re));
        }
        merged
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TextAnimationGlyphInfo {
    pub key: VisualTransactionKey,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub opacity: f64,
    pub baseline_in_quad: f64,
    pub animation_mode: AnimationMode,
    pub is_delete: bool,
    pub texture_phase: TexturePhase,
    pub run_identity: i32,
    pub line_snapshot_id: Option<LineSnapshotId>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImeUpdateKind {
    None,
    QueryInput,
}

impl Default for ImeUpdateKind {
    fn default() -> Self {
        ImeUpdateKind::None
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ImeUpdatePlan {
    pub kind: ImeUpdateKind,
    pub cursor_changed: bool,
    pub anchor_changed: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FrameContext {
    pub viewport_height: f64,
    pub scroll_offset_y: f64,
    pub dpr: f64,
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
    pub static_text: StaticTextPlan,
    pub text_animation: TextAnimationPlan,
    pub selection_preedit: SelectionPreeditPlan,
    pub cursor: CursorAnimationPlan,
    pub ime: ImeUpdatePlan,
    pub frame_context: FrameContext,
    pub cursor_style: CursorStyle,
}
