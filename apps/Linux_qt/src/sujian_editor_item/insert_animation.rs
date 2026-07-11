use super::visual_payload::{VisualRunSnapshot, ReflowRunSnapshot};
use super::transaction_key::VisualTransactionKey;
use super::animation_mode::AnimationMode;
use std::time::Instant;

pub(crate) fn compute_insert_animation_frame(
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
