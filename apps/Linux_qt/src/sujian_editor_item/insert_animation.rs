use super::visual_payload::{VisualRunSnapshot, ReflowRunSnapshot};
use super::transaction_key::VisualTransactionKey;
use super::animation_mode::AnimationMode;
use std::time::Instant;

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

pub(crate) fn compute_glyph_insert_animation_frame(
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

pub(crate) fn compute_cluster_insert_animation_frame(
    insert_runs: &[VisualRunSnapshot],
    reflow_runs: &[ReflowRunSnapshot],
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

        let dx = run.x - old_cx;
        let dy = run.y - old_cy;
        let gx = old_cx + dx * local_eased;
        let gy = old_cy + dy * local_eased;
        let opacity = local_eased;
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
