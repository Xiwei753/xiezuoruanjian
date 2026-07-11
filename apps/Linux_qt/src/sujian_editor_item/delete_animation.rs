use super::visual_payload::VisualRunSnapshot;
use super::insert_animation::GlyphFrameData;

pub(crate) fn compute_glyph_delete_animation_frame(
    delete_runs: &[VisualRunSnapshot],
    progress: f64,
) -> Vec<GlyphFrameData> {
    let opacity = 1.0 - progress;
    let mut frames = Vec::with_capacity(delete_runs.len());

    for run in delete_runs {
        let baseline_in_quad = run.baseline_y - run.y;
        let scale = 1.0 - progress * 0.15;
        let dw = run.w * scale;
        let dh = run.h * scale;
        let dx = run.x + (run.w - dw) * 0.5;
        let dy = run.y + (run.h - dh) * 0.5;
        let bl_offset = (run.baseline_y - run.y) * scale;

        frames.push(GlyphFrameData {
            x: dx,
            y: dy,
            w: dw,
            h: dh,
            opacity,
            baseline_in_quad: bl_offset,
            byte_start: run.byte_start,
            byte_end: run.byte_end,
        });
    }

    frames
}

pub(crate) fn compute_cluster_delete_animation_frame(
    delete_runs: &[VisualRunSnapshot],
    progress: f64,
) -> Vec<GlyphFrameData> {
    let eased = 1.0 - (1.0 - progress).powi(2);
    let opacity = 1.0 - eased;
    let mut frames = Vec::with_capacity(delete_runs.len());

    for run in delete_runs {
        let baseline_in_quad = run.baseline_y - run.y;
        frames.push(GlyphFrameData {
            x: run.x,
            y: run.y,
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

pub(crate) fn compute_run_delete_animation_frame(
    delete_runs: &[VisualRunSnapshot],
    progress: f64,
) -> Vec<GlyphFrameData> {
    let eased = 1.0 - (1.0 - progress).powi(3);
    let opacity = 1.0 - eased;
    let mut frames = Vec::with_capacity(delete_runs.len());

    for run in delete_runs {
        let baseline_in_quad = run.baseline_y - run.y;
        let shift_x = run.x - eased * run.w * 0.3;
        frames.push(GlyphFrameData {
            x: shift_x,
            y: run.y,
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

pub(crate) fn compute_line_reflow_delete_animation_frame(
    delete_runs: &[VisualRunSnapshot],
    progress: f64,
) -> Vec<GlyphFrameData> {
    let opacity = 1.0 - progress;
    let mut frames = Vec::with_capacity(delete_runs.len());

    for run in delete_runs {
        let baseline_in_quad = run.baseline_y - run.y;
        let collapse_y = run.y + progress * run.h * 0.5;
        frames.push(GlyphFrameData {
            x: run.x,
            y: collapse_y,
            w: run.w,
            h: run.h * (1.0 - progress * 0.5),
            opacity,
            baseline_in_quad,
            byte_start: run.byte_start,
            byte_end: run.byte_end,
        });
    }

    frames
}
