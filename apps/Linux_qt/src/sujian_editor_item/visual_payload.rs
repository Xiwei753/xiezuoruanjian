use writer_core::editor::{CursorRect, GlyphRect, ReflowGlyphRect};

#[derive(Clone, Debug)]
pub(crate) struct VisualRunSnapshot {
    pub char_: String,
    pub byte_start: usize,
    pub byte_end: usize,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub baseline_y: f64,
}

impl VisualRunSnapshot {
    pub fn from_glyph_rect(g: &GlyphRect) -> Self {
        Self {
            char_: g.char_.clone(),
            byte_start: g.byte_start,
            byte_end: g.byte_end,
            x: g.x,
            y: g.y,
            w: g.w,
            h: g.h,
            baseline_y: g.baseline_y,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ReflowRunSnapshot {
    pub char_: String,
    pub byte_start: usize,
    pub byte_end: usize,
    pub old_x: f64,
    pub old_y: f64,
    pub old_baseline_y: f64,
    pub new_x: f64,
    pub new_y: f64,
    pub new_baseline_y: f64,
    pub w: f64,
    pub h: f64,
}

impl ReflowRunSnapshot {
    pub fn from_reflow_glyph_rect(r: &ReflowGlyphRect) -> Self {
        Self {
            char_: r.char_.clone(),
            byte_start: r.byte_start,
            byte_end: r.byte_end,
            old_x: r.old_x,
            old_y: r.old_y,
            old_baseline_y: r.old_baseline_y,
            new_x: r.new_x,
            new_y: r.new_y,
            new_baseline_y: r.new_baseline_y,
            w: r.w,
            h: r.h,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum VisualPayload {
    InsertRuns {
        insert_runs: Vec<VisualRunSnapshot>,
        reflow_runs: Vec<ReflowRunSnapshot>,
        inserted_range: Option<(usize, usize)>,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
    },
    DeleteRuns {
        delete_runs: Vec<VisualRunSnapshot>,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
    },
    ReflowRuns {
        insert_runs: Vec<VisualRunSnapshot>,
        reflow_runs: Vec<ReflowRunSnapshot>,
        inserted_range: Option<(usize, usize)>,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
    },
    CursorTransition {
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
    },
}

impl VisualPayload {
    pub fn from_insert_transaction(
        insert_glyph_rects: &[GlyphRect],
        reflow_glyph_rects: &[ReflowGlyphRect],
        inserted_range: Option<(usize, usize)>,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        has_reflow: bool,
    ) -> Self {
        let insert_runs: Vec<VisualRunSnapshot> = insert_glyph_rects.iter().map(VisualRunSnapshot::from_glyph_rect).collect();
        let reflow_runs: Vec<ReflowRunSnapshot> = reflow_glyph_rects.iter().map(ReflowRunSnapshot::from_reflow_glyph_rect).collect();

        if has_reflow && !reflow_runs.is_empty() {
            VisualPayload::ReflowRuns {
                insert_runs,
                reflow_runs,
                inserted_range,
                old_cursor_rect,
                new_cursor_rect,
            }
        } else {
            VisualPayload::InsertRuns {
                insert_runs,
                reflow_runs,
                inserted_range,
                old_cursor_rect,
                new_cursor_rect,
            }
        }
    }

    pub fn from_delete_transaction(
        deleted_glyph_rects: &[GlyphRect],
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
    ) -> Self {
        let delete_runs: Vec<VisualRunSnapshot> = deleted_glyph_rects.iter().map(VisualRunSnapshot::from_glyph_rect).collect();
        VisualPayload::DeleteRuns {
            delete_runs,
            old_cursor_rect,
            new_cursor_rect,
        }
    }

    pub fn cursor_rects(&self) -> (Option<&CursorRect>, Option<&CursorRect>) {
        match self {
            VisualPayload::InsertRuns { old_cursor_rect, new_cursor_rect, .. } => (old_cursor_rect.as_ref(), new_cursor_rect.as_ref()),
            VisualPayload::DeleteRuns { old_cursor_rect, new_cursor_rect, .. } => (old_cursor_rect.as_ref(), new_cursor_rect.as_ref()),
            VisualPayload::ReflowRuns { old_cursor_rect, new_cursor_rect, .. } => (old_cursor_rect.as_ref(), new_cursor_rect.as_ref()),
            VisualPayload::CursorTransition { old_cursor_rect, new_cursor_rect } => (old_cursor_rect.as_ref(), new_cursor_rect.as_ref()),
        }
    }

    pub fn total_glyph_count(&self) -> usize {
        match self {
            VisualPayload::InsertRuns { insert_runs, reflow_runs, .. } => insert_runs.len() + reflow_runs.len(),
            VisualPayload::DeleteRuns { delete_runs, .. } => delete_runs.len(),
            VisualPayload::ReflowRuns { insert_runs, reflow_runs, .. } => insert_runs.len() + reflow_runs.len(),
            VisualPayload::CursorTransition { .. } => 0,
        }
    }
}
