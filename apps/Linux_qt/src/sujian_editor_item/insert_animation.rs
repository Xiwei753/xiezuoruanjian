use super::shaped_visual_run::ShapedVisualRun;

#[allow(dead_code)]
pub(crate) struct GlyphFrameData {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub opacity: f64,
    pub baseline_in_quad: f64,
    pub byte_start: usize,
    pub byte_end: usize,
}

#[allow(dead_code)]
pub(crate) fn compute_glyph_insert_animation_frame(
    insert_runs: &[ShapedVisualRun],
    reflow_runs: &[ShapedVisualRun],
    old_cursor_rect: Option<&writer_core::editor::CursorRect>,
    progress: f64,
) -> Vec<GlyphFrameData> {
    let eased = 1.0 - (1.0 - progress).powi(3);
    let mut frames = Vec::with_capacity(insert_runs.len() + reflow_runs.len());

    let old_cx = old_cursor_rect.map(|c| c.x).unwrap_or(0.0);
    let old_cy = old_cursor_rect.map(|c| c.top).unwrap_or(0.0);

    for run in insert_runs {
        let dx = run.visual_x - old_cx;
        let dy = run.visual_y - old_cy;
        let gx = old_cx + dx * eased;
        let gy = old_cy + dy * eased;
        let opacity = eased;
        let baseline_in_quad = (run.baseline_y - run.visual_y) + (run.visual_y - gy);

        frames.push(GlyphFrameData {
            x: gx,
            y: gy,
            w: run.visual_w,
            h: run.visual_h,
            opacity,
            baseline_in_quad,
            byte_start: run.source_string_start,
            byte_end: run.source_string_end,
        });
    }

    for run in reflow_runs {
        frames.push(GlyphFrameData {
            x: run.visual_x,
            y: run.visual_y,
            w: run.visual_w,
            h: run.visual_h,
            opacity: 1.0,
            baseline_in_quad: run.baseline_y - run.visual_y,
            byte_start: run.source_string_start,
            byte_end: run.source_string_end,
        });
    }

    frames
}

#[allow(dead_code)]
pub(crate) fn compute_cluster_insert_animation_frame(
    insert_runs: &[ShapedVisualRun],
    reflow_runs: &[ShapedVisualRun],
    old_cursor_rect: Option<&writer_core::editor::CursorRect>,
    progress: f64,
) -> Vec<GlyphFrameData> {
    let eased = 1.0 - (1.0 - progress).powi(2);
    let mut frames = Vec::with_capacity(insert_runs.len() + reflow_runs.len());

    let old_cx = old_cursor_rect.map(|c| c.x).unwrap_or(0.0);
    let old_cy = old_cursor_rect.map(|c| c.top).unwrap_or(0.0);

    if insert_runs.is_empty() {
        return frames;
    }

    let cluster_count = insert_runs.len().max(1);
    let stagger = 0.3 / cluster_count as f64;

    for (i, run) in insert_runs.iter().enumerate() {
        let local_progress = ((progress - stagger * i as f64) / (1.0 - stagger * (cluster_count - 1) as f64)).clamp(0.0, 1.0);
        let local_eased = 1.0 - (1.0 - local_progress).powi(2);

        let dx = run.visual_x - old_cx;
        let dy = run.visual_y - old_cy;
        let gx = old_cx + dx * local_eased;
        let gy = old_cy + dy * local_eased;
        let opacity = local_eased;
        let baseline_in_quad = (run.baseline_y - run.visual_y) + (run.visual_y - gy);

        frames.push(GlyphFrameData {
            x: gx,
            y: gy,
            w: run.visual_w,
            h: run.visual_h,
            opacity,
            baseline_in_quad,
            byte_start: run.source_string_start,
            byte_end: run.source_string_end,
        });
    }

    for run in reflow_runs {
        frames.push(GlyphFrameData {
            x: run.visual_x,
            y: run.visual_y,
            w: run.visual_w,
            h: run.visual_h,
            opacity: 1.0,
            baseline_in_quad: run.baseline_y - run.visual_y,
            byte_start: run.source_string_start,
            byte_end: run.source_string_end,
        });
    }

    frames
}
