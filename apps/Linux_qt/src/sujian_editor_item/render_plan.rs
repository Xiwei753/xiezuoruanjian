use super::transaction_key::VisualTransactionKey;
use super::visual_payload::{VisualRunSnapshot, ReflowRunSnapshot};
use super::cursor_animation::{CursorAnimationPlan, CursorBlinkMode, CursorTransition};
use super::animation_mode::AnimationMode;
use writer_core::editor::CursorRect;

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
    pub byte_start: usize,
    pub byte_end: usize,
    pub animation_mode: AnimationMode,
    pub is_delete: bool,
    pub old_paragraph_text: Option<String>,
    pub font_id: String,
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

#[derive(Clone, Debug)]
pub(crate) struct RenderPlan {
    pub static_text: StaticTextPlan,
    pub text_animation: TextAnimationPlan,
    pub selection_preedit: SelectionPreeditPlan,
    pub cursor: CursorAnimationPlan,
    pub ime: ImeUpdatePlan,
}
