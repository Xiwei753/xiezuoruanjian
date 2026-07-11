use super::visual_payload::VisualRunSnapshot;
use super::insert_animation::GlyphFrameData;

pub(crate) fn compute_delete_animation_frame(
    delete_runs: &[VisualRunSnapshot],
    progress: f64,
) -> Vec<GlyphFrameData> {
    let opacity = 1.0 - progress;
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
