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
/// Issue #826 评论 5: 标准区间合并。
///
/// 三条硬规则，任何一条破坏都会让静态正文出现空洞或误裁：
/// 1. 只在**同一视觉行**（`y` / `h` 相同）且**同一 `snapshot_id`** 的组内处理。
///    跨 `snapshot_id` 合并会破坏 renderer 的纹理缺失回退规则：snapshot A 纹理存在、
///    snapshot B 纹理缺失时，合并并挂到 A 上会让 B 那块静态正文被裁掉，而 B 的动画
///    glyph 又画不出来。
/// 2. 只合并**相交或相邻**的区间（`next_left <= current_right + EPS`）。
///    有 gap 就另起一条，否则中间的正常正文会被整段挖掉。
/// 3. **每一组**都执行 sweep，不只处理第一条。
fn merge_clip_rects(
    rects: Vec<(f64, f64, f64, f64, LineSnapshotId)>,
) -> Vec<(f64, f64, f64, f64, LineSnapshotId)> {
    const EPS: f64 = 1e-6;
    let mut kept: Vec<(f64, f64, f64, f64, LineSnapshotId)> = rects
        .into_iter()
        .filter(|&(_, _, w, h, _)| w > 0.0 && h > 0.0)
        .collect();
    if kept.len() <= 1 {
        return kept;
    }
    // 1. 按 (y, h, x) 排序：同一视觉行连续，行内 x 递增。
    //    `LineSnapshotId` 没有 `Ord`，所以不把它排进 key；分组时改成扫「连续段」，
    //    遇到 (snapshot_id, y, h) 任一不同就 flush 另起一条，效果等价且不会跨组合并。
    kept.sort_by(|a, b| {
        a.1.partial_cmp(&b.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal))
    });

    let mut merged: Vec<(f64, f64, f64, f64, LineSnapshotId)> = Vec::with_capacity(kept.len());
    let mut slot: Option<(f64, f64, f64, f64, LineSnapshotId)> = None;
    for (x, y, w, h, id) in kept {
        let Some((cur_x, cur_y, cur_w, cur_h, cur_id)) = slot else {
            slot = Some((x, y, w, h, id));
            continue;
        };
        // 换组（不同 snapshot_id 或不同视觉行）：先把当前区间落盘，另起一条。
        if cur_id != id || cur_y != y || cur_h != h {
            merged.push((cur_x, cur_y, cur_w, cur_h, cur_id));
            slot = Some((x, y, w, h, id));
            continue;
        }
        let cur_right = cur_x + cur_w;
        // 相交或相邻才合并；有 gap 就保留两条。
        if x <= cur_right + EPS {
            let left = cur_x.min(x);
            let right = cur_right.max(x + w);
            slot = Some((left, cur_y, right - left, cur_h, cur_id));
        } else {
            merged.push((cur_x, cur_y, cur_w, cur_h, cur_id));
            slot = Some((x, y, w, h, id));
        }
    }
    if let Some(rect) = slot {
        merged.push(rect);
    }
    merged
}

#[cfg(test)]
mod tests;
