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
        // Issue #826 评论 6：clip 保留 `StaticClipKind`，**这里不合并**。
        //
        // 合并必须放到 renderer 里、纹理可用性过滤**之后**：
        // FrontierMask 不依赖动画纹理、必须永远保留；ReflowTarget 纹理 miss 时
        // 必须撤掉让 canonical 恢复。先合并再过滤的话，两类 clip 混成一块后
        // renderer 就无法判断哪部分仍必须裁、哪部分该放弃。
        let mut clip_rects: Vec<AnimationClipRect> = frontier_sample
            .as_ref()
            .map(|sample| self.hidden_canonical_rects_for(sample))
            .unwrap_or_default();
        clip_rects.extend(self.reflow_target_clip_rects());
        // Issue #826 评论 20：吐字 carry 的 canonical 目标位置同样要让位。
        clip_rects.extend(
            frontier_sample
                .as_ref()
                .map(|sample| self.reveal_carried_target_clip_rects(sample))
                .unwrap_or_default(),
        );
        // Issue #826 评论 24：mixed cluster 交接层的新侧目标位置同样要让位。
        clip_rects.extend(self.shaping_transition_target_clip_rects());

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

        // Issue #826 评论 20：吐字「已可见前缀」——上一帧真正看见的那几个像素，
        // 从旧屏幕位置补间到最新 canonical 位置。
        let carried_glyphs = frontier_sample
            .as_ref()
            .map(|sample| self.reveal_carried_glyphs_for(sample))
            .unwrap_or_default();
        for glyph in carried_glyphs {
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

        // Issue #826 评论 24：不可拆 shaping cluster 的 old/new 原子交接。
        //
        // 两侧都整块画、带各自的 opacity：旧 cluster 淡出、新 cluster 淡入。
        // 绝不能按 byte 比例裁 source_rect —— 混合 cluster 的新旧 source_rect
        // 本身就是两套不同形状的资源。
        for frame in self.shaping_transition_glyphs(frame_now) {
            for side in [frame.old, frame.new].into_iter().flatten() {
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

#[cfg(test)]
mod tests;
