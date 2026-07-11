use super::shaped_visual_run::ShapedVisualRun;
use super::insert_animation::GlyphFrameData;

#[allow(dead_code)]
pub(crate) fn compute_glyph_delete_animation_frame(
    delete_runs: &[ShapedVisualRun],
    progress: f64,
) -> Vec<GlyphFrameData> {
    let opacity = 1.0 - progress;
    let mut frames = Vec::with_capacity(delete_runs.len());

    for run in delete_runs {
        let _baseline_in_quad = run.baseline_y - run.visual_y;
        let scale = 1.0 - progress * 0.15;
        let dw = run.visual_w * scale;
        let dh = run.visual_h * scale;
        let dx = run.visual_x + (run.visual_w - dw) * 0.5;
        let dy = run.visual_y + (run.visual_h - dh) * 0.5;
        let bl_offset = (run.baseline_y - run.visual_y) * scale;

        frames.push(GlyphFrameData {
            x: dx,
            y: dy,
            w: dw,
            h: dh,
            opacity,
            baseline_in_quad: bl_offset,
            byte_start: run.source_string_start,
            byte_end: run.source_string_end,
        });
    }

    frames
}

#[allow(dead_code)]
pub(crate) fn compute_cluster_delete_animation_frame(
    delete_runs: &[ShapedVisualRun],
    progress: f64,
) -> Vec<GlyphFrameData> {
    let eased = 1.0 - (1.0 - progress).powi(2);
    let opacity = 1.0 - eased;
    let mut frames = Vec::with_capacity(delete_runs.len());

    for run in delete_runs {
        let baseline_in_quad = run.baseline_y - run.visual_y;
        frames.push(GlyphFrameData {
            x: run.visual_x,
            y: run.visual_y,
            w: run.visual_w,
            h: run.visual_h,
            opacity,
            baseline_in_quad,
            byte_start: run.source_string_start,
            byte_end: run.source_string_end,
        });
    }

    frames
}

#[allow(dead_code)]
pub(crate) fn compute_run_delete_animation_frame(
    delete_runs: &[ShapedVisualRun],
    progress: f64,
) -> Vec<GlyphFrameData> {
    let eased = 1.0 - (1.0 - progress).powi(3);
    let opacity = 1.0 - eased;
    let mut frames = Vec::with_capacity(delete_runs.len());

    for run in delete_runs {
        let baseline_in_quad = run.baseline_y - run.visual_y;
        let shift_x = run.visual_x - eased * run.visual_w * 0.3;
        frames.push(GlyphFrameData {
            x: shift_x,
            y: run.visual_y,
            w: run.visual_w,
            h: run.visual_h,
            opacity,
            baseline_in_quad,
            byte_start: run.source_string_start,
            byte_end: run.source_string_end,
        });
    }

    frames
}

#[allow(dead_code)]
pub(crate) fn compute_line_reflow_delete_animation_frame(
    delete_runs: &[ShapedVisualRun],
    progress: f64,
) -> Vec<GlyphFrameData> {
    let opacity = 1.0 - progress;
    let mut frames = Vec::with_capacity(delete_runs.len());

    for run in delete_runs {
        let baseline_in_quad = run.baseline_y - run.visual_y;
        let collapse_y = run.visual_y + progress * run.visual_h * 0.5;
        frames.push(GlyphFrameData {
            x: run.visual_x,
            y: collapse_y,
            w: run.visual_w,
            h: run.visual_h * (1.0 - progress * 0.5),
            opacity,
            baseline_in_quad,
            byte_start: run.source_string_start,
            byte_end: run.source_string_end,
        });
    }

    frames
}
