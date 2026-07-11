use super::visual_payload::{VisualRunSnapshot, ReflowRunSnapshot};
use super::insert_animation::GlyphFrameData;

pub(crate) fn compute_reflow_animation_frame(
    insert_runs: &[VisualRunSnapshot],
    reflow_runs: &[ReflowRunSnapshot],
    old_cursor_rect: Option<&writer_core::editor::CursorRect>,
    progress: f64,
) -> Vec<GlyphFrameData> {
    let eased = 1.0 - (1.0 - progress).powi(3);
    let mut frames = Vec::with_capacity(insert_runs.len() + reflow_runs.len());

    let old_cx = old_cursor_rect.map(|c| c.x).unwrap_or(0.0);
    let old_cy = old_cursor_rect.map(|c| c.top).unwrap_or(0.0);

    for run in insert_runs {
        let dx = run.x - old_cx;
        let dy = run.y - old_cy;
        let gx = old_cx + dx * eased;
        let gy = old_cy + dy * eased;
        let opacity = eased;
        let baseline_in_quad = (run.baseline_y - run.y) + (run.y - gy);

        frames.push(GlyphFrameData {
            x: gx,
            y: gy,
            w: run.w,
            h: run.h,
            opacity,
            baseline_in_quad,
            byte_start: run.byte_start,
            byte_end: run.byte_end,
        });
    }

    for run in reflow_runs {
        let gx = run.old_x + (run.new_x - run.old_x) * eased;
        let gy = run.old_y + (run.new_y - run.old_y) * eased;
        let opacity = 1.0;
        let baseline_in_quad = (run.old_baseline_y - run.old_y) + (run.old_y - gy);

        frames.push(GlyphFrameData {
            x: gx,
            y: gy,
            w: run.w,
            h: run.h,
            opacity,
            baseline_in_quad,
            byte_start: run.byte_start,
            byte_end: run.byte_end,
        });
    }

    frames
}
