//! Issue #853: 每帧唯一视觉计划的构建入口。
//!
//! 正文动画只有两层输出：
//!
//! - `clip_rects`：整段过渡期间由动画层接管的完整 target cluster。
//! - `text_animation.glyphs`：吐字切片、吞字旧字、shaping 交接与 Reflow 位置。
//!
//! 光标完全独立：`CursorRenderState` 由 GUI 线程算好传进来，本模块不参与。
//! IME preedit 由 `SelectionPreeditPlan` 独立承载，不与正文动画互相携带。

use super::coordinator::LinuxEditorAnimationCoordinator;
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
        // Issue #853: 同帧只采样一次前沿。吐字 glyph、吞字 overlay 与静态层所有权
        // 都从这份样本生成，避免同一帧使用不同 progress。
        let frontier_sample = self.sample_edit_frontier(frame_now);

        // 静态正文层让出动画层接管的完整 target cluster：吐字、已露前缀、Reflow
        // 目标位置与 shaping new side。exclusion 在整个过渡期间保持完整不变。
        // renderer 在静态层让位前检查纹理；资源缺失时 canonical 同帧恢复。
        let mut clip_rects: Vec<AnimationClipRect> = self.frontier_target_clip_rects();
        clip_rects.extend(self.reflow_target_clip_rects());
        clip_rects.extend(self.shaping_transition_target_clip_rects());

        let mut glyphs: Vec<TextAnimationGlyphInfo> = Vec::new();

        // 吐字：动画层从 0 宽切片逐帧画到完整 target glyph。静态 exclusion 覆盖
        // 整个 cluster，动画结束时 glyph 与 exclusion 在同一个 RenderPlan 中移除，
        // canonical 静态文字同帧接回。
        if let Some(sample) = frontier_sample.as_ref() {
            for reveal in self.reveal_visuals_for(sample) {
                if reveal.visible_width <= 1e-6 || reveal.rect.h <= 1e-6 {
                    continue;
                }
                let source_rect = super::shaping_transition::visible_source_slice(
                    &reveal.source_rect,
                    reveal.full_width,
                    reveal.visible_width,
                );
                glyphs.push(TextAnimationGlyphInfo {
                    x: reveal.rect.x,
                    y: reveal.rect.y,
                    w: reveal.visible_width,
                    h: reveal.rect.h,
                    opacity: 1.0,
                    snapshot_id: reveal.snapshot_id,
                    source_rect,
                });
            }
        }

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
                // Issue #826 评论 31：吞字 overlay 的不透明度是**起点常量**，
                // 时间由 clip（keep rect 的缩放）表达。被整块删除的 cluster 若
                // 上一帧正由 ShapingTransition 淡到 0.58，这里就必须画 0.58 ——
                // 绝不能固定 1.0 让屏幕先跳亮再开始吞，也不能随时间淡到 0
                // 变成第二条时间轴。
                opacity: glyph.opacity,
                snapshot_id: glyph.snapshot_id,
                source_rect: glyph.source_rect,
            });
        }

        // Issue #826 评论 24：不可拆 shaping cluster 的 old/new 原子交接。
        //
        // 两侧都整块画、带各自的 opacity：旧 cluster 淡出、新 cluster 淡入。
        // 绝不能按 byte 比例裁 source_rect —— 混合 cluster 的新旧 source_rect
        // 本身就是两套不同形状的资源。
        for frame in self.shaping_transition_glyphs(frame_now) {
            for side in frame.old.into_iter().chain(frame.new) {
                if side.opacity <= 1e-6 || side.rect.w <= 1e-6 {
                    continue;
                }
                glyphs.push(TextAnimationGlyphInfo {
                    x: side.rect.x,
                    y: side.rect.y,
                    w: side.rect.w,
                    h: side.rect.h,
                    opacity: side.opacity,
                    snapshot_id: side.snapshot_id,
                    source_rect: side.source_rect,
                });
            }
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

        // 光标由 cursor controller 唯一拥有；本帧只把它的显示状态放进计划。
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

#[cfg(test)]
mod tests;
