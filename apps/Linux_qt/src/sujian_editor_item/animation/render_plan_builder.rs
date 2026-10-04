//! Issue #826: 每帧渲染计划构建。
//!
//! 正文动画只有两层输出：
//!
//! - `clip_rects`：吐字遮罩前沿之后的 canonical 新字，从静态正文层裁掉。
//! - `text_animation.glyphs`：吞字/替换的旧正文 overlay（来自 `base_snapshot`
//!   行纹理）+ Reflow 层未改文字的移动位置（来自 `target_snapshot` 行纹理）。
//!
//! 光标完全独立：`CursorRenderState` 由 GUI 线程算好传进来，本模块不参与。
//! IME preedit 由 `SelectionPreeditPlan` 独立承载，不与正文动画互相携带。

use super::coordinator::LinuxEditorAnimationCoordinator;
use crate::sujian_editor_item::layout_snapshot::LineSnapshotId;
use crate::sujian_editor_item::qt_text_node::AnimationClipRect;
use crate::sujian_editor_item::render_plan::{
    CursorRenderState, CursorStyle, RenderPlan, SelectionPreeditPlan, SelectionPreeditStyle,
    TextAnimationGlyphInfo, TextAnimationPlan,
};

impl LinuxEditorAnimationCoordinator {
    /// Issue #826: 构建本帧渲染计划。
    ///
    /// `frame_now` 是 Scene Graph 当前帧的统一时间点，前沿与 Reflow 都在这一个
    /// 时间上采样，不各自 `Instant::now()`。
    pub(crate) fn build_render_plan_full(
        &self,
        cursor_render_state: CursorRenderState,
        selection_preedit: SelectionPreeditPlan,
        cursor_style: CursorStyle,
        selection_preedit_style: SelectionPreeditStyle,
        frame_now: std::time::Instant,
    ) -> RenderPlan {
        // Issue #826: 同帧只采样一次前沿。吐字遮罩和吞字 overlay 必须看到同一个
        // progress，否则会出现"新字已经露出来、旧字还没收掉"的重叠帧。
        let frontier_sample = self.sample_edit_frontier(frame_now);

        // Issue #826 评论 4 问题 3：静态正文层要同时避开两处：
        //
        // 1. EditFrontier 的吐字遮罩 —— 本轮新增、还没露出的 canonical 新字；
        // 2. Reflow 的 canonical 目标位置 —— 动画层正在画"正在移动的那一份"，
        //    静态层如果同时画最终位置就会重影（段中 Enter 的 `FGHIJ` 典型）。
        //
        // Reflow 完成后 `active_reflow` 清掉，下一帧静态层自动恢复 canonical。
        let mut raw_clips: Vec<(f64, f64, f64, f64, LineSnapshotId)> = frontier_sample
            .as_ref()
            .map(|sample| self.hidden_canonical_rects_for(sample))
            .unwrap_or_default();
        raw_clips.extend(self.reflow_target_clip_rects());

        let clip_rects: Vec<AnimationClipRect> = merge_clip_rects(raw_clips)
            .into_iter()
            .map(|(x, y, w, h, snapshot_id)| AnimationClipRect {
                x,
                y,
                w,
                h,
                snapshot_id,
            })
            .collect();

        let mut glyphs: Vec<TextAnimationGlyphInfo> = Vec::new();

        // 吞字 / 替换：本轮删除开始前的旧正文 overlay。
        let overlay_glyphs = frontier_sample
            .as_ref()
            .map(|sample| self.old_overlay_glyphs_for(sample))
            .unwrap_or_default();
        for glyph in overlay_glyphs {
            glyphs.push(TextAnimationGlyphInfo {
                x: glyph.dest_rect.x,
                y: glyph.dest_rect.y,
                w: glyph.dest_rect.w,
                h: glyph.dest_rect.h,
                opacity: 1.0,
                snapshot_id: glyph.snapshot_id,
                source_rect: glyph.source_rect,
            });
        }

        // Reflow 层：没改的字从旧位置插值到新位置。
        for span in self.reflow_glyphs(frame_now) {
            glyphs.push(TextAnimationGlyphInfo {
                x: span.dest_rect.x,
                y: span.dest_rect.y,
                w: span.dest_rect.w,
                h: span.dest_rect.h,
                opacity: 1.0,
                snapshot_id: span.snapshot_id,
                source_rect: span.source_rect,
            });
        }

        let text_animation = TextAnimationPlan { glyphs };

        // Issue #826: 光标与文字动画解耦，这里只是把 cursor_controller 当前的
        // visual 位置原样带给 renderer，不再由正文事务驱动。
        let caret = (
            cursor_render_state.x,
            cursor_render_state.y,
            cursor_render_state.h,
        );
        RenderPlan {
            text_animation,
            selection_preedit,
            cursor: cursor_render_state,
            cursor_style,
            selection_preedit_style,
            clip_rects,
            drawn_caret_rect: Some(caret),
        }
    }
}

/// Issue #826 评论 4 问题 3：合并静态层 exclusion clip。
///
/// 吐字遮罩与 Reflow 目标位置可能落在同一行、并且互相重叠（例如段中插入时
/// 新字刚被遮罩裁掉、同时它后面被挤动的字又要把新位置让出来）。
/// 重叠区间会让 `qt_text_node` 的 complement 计算出负宽度，所以按 y 带分组
/// 后逐段合并 x 区间。
fn merge_clip_rects(
    rects: Vec<(f64, f64, f64, f64, LineSnapshotId)>,
) -> Vec<(f64, f64, f64, f64, LineSnapshotId)> {
    let mut kept: Vec<(f64, f64, f64, f64, LineSnapshotId)> = rects
        .into_iter()
        .filter(|&(_, _, w, h, _)| w > 0.0 && h > 0.0)
        .collect();
    if kept.len() <= 1 {
        return kept;
    }
    // 以第一个 rect 的 y 带为基准做合并。行高在同一 snapshot 内一致，
    // 不同行的 rect 不会被错误合并 —— 保守起见只在 y 完全相同的组内合并。
    let band_y = kept[0].1;
    let band_h = kept[0].3;
    let snapshot_id = kept[0].4;
    let mut merged: Vec<(f64, f64, f64, f64, LineSnapshotId)> = Vec::new();
    for (x, y, w, h, id) in kept {
        if y != band_y || h != band_h {
            merged.push((x, y, w, h, id));
            continue;
        }
        match merged.iter_mut().find(|slot| slot.1 == y && slot.3 == h) {
            Some(slot) => {
                let right = (slot.0 + slot.2).max(x + w);
                let left = slot.0.min(x);
                slot.0 = left;
                slot.2 = right - left;
            }
            None => merged.push((x, y, w, h, snapshot_id)),
        }
    }
    merged
}

#[cfg(test)]
mod tests;
