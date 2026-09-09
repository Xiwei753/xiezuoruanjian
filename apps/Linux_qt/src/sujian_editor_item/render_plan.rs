use super::cursor_animation::CursorAnimationPlan;
use super::layout_snapshot::{LineSnapshotId, SourceRect};
use super::transaction_key::VisualTransactionKey;

#[derive(Clone, Debug)]
pub(crate) struct HiddenClipRect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct StaticTextPlan {
    pub hidden_clip_rects: Vec<HiddenClipRect>,
}

impl StaticTextPlan {
    pub fn merged_clip_rects(&self) -> Vec<(f64, f64, f64, f64)> {
        let mut all: Vec<(f64, f64, f64, f64)> = self
            .hidden_clip_rects
            .iter()
            .map(|r| (r.x, r.y, r.x + r.w, r.y + r.h))
            .collect();
        all.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let mut merged: Vec<(f64, f64, f64, f64)> = Vec::new();
        for (left, top, right, bottom) in all {
            if let Some(last) = merged.last_mut() {
                if left <= last.2 + 0.01 && top <= last.3 + 0.01 {
                    last.2 = last.2.max(right);
                    last.3 = last.3.max(bottom);
                    continue;
                }
            }
            merged.push((left, top, right, bottom));
        }
        merged
    }
}

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
