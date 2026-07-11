use super::visual_payload::{VisualRunSnapshot, ReflowRunSnapshot};
use super::insert_animation::GlyphFrameData;

pub(crate) fn compute_run_reflow_animation_frame(
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

pub(crate) fn compute_line_reflow_animation_frame(
    insert_runs: &[VisualRunSnapshot],
    reflow_runs: &[ReflowRunSnapshot],
    old_cursor_rect: Option<&writer_core::editor::CursorRect>,
    progress: f64,
) -> Vec<GlyphFrameData> {
    let eased = 1.0 - (1.0 - progress).powi(2);
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

    let mut reflow_by_line: std::collections::HashMap<i64, Vec<&ReflowRunSnapshot>> = std::collections::HashMap::new();
    for run in reflow_runs {
        let line_key = (run.new_y / run.h.max(1.0)).round() as i64;
        reflow_by_line.entry(line_key).or_default().push(run);
    }

    let line_count = reflow_by_line.len().max(1);
    let stagger = 0.2 / line_count as f64;

    let mut sorted_lines: Vec<_> = reflow_by_line.into_iter().collect();
    sorted_lines.sort_by_key(|(k, _)| *k);

    for (line_idx, (_, line_runs)) in sorted_lines.into_iter().enumerate() {
        let line_progress = ((progress - stagger * line_idx as f64) / (1.0 - stagger * (line_count - 1) as f64)).clamp(0.0, 1.0);
        let line_eased = 1.0 - (1.0 - line_progress).powi(2);

        for run in line_runs {
            let gx = run.old_x + (run.new_x - run.old_x) * line_eased;
            let gy = run.old_y + (run.new_y - run.old_y) * line_eased;
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
    }

    frames
}
