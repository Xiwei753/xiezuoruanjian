use writer_core::editor::{CursorRect, GlyphRect, ReflowGlyphRect};
use super::animation_mode::AnimationMode;
use super::shaped_visual_run::{ShapedVisualRun, ReflowVisualSnapshot};

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
    pub font_id: String,
    pub glyph_index: u32,
    pub glyph_position_x: f64,
    pub glyph_position_y: f64,
    pub string_index: usize,
    pub old_paragraph_text: Option<String>,
    pub texture_atlas_x: f64,
    pub texture_atlas_y: f64,
    pub texture_atlas_w: f64,
    pub texture_atlas_h: f64,
    pub shaped_run: Option<ShapedVisualRun>,
}

#[derive(Clone, Debug)]
pub(crate) struct ShapedGlyphInfo {
    pub font_id: String,
    pub glyph_index: u32,
    pub glyph_position_x: f64,
    pub glyph_position_y: f64,
    pub string_index: usize,
    pub raw_font_family: String,
}

impl VisualRunSnapshot {
    pub fn from_glyph_rect_with_shaping(g: &GlyphRect, shaping: Option<&ShapedGlyphInfo>) -> Self {
        Self {
            char_: g.char_.clone(),
            byte_start: g.byte_start,
            byte_end: g.byte_end,
            x: g.x,
            y: g.y,
            w: g.w,
            h: g.h,
            baseline_y: g.baseline_y,
            font_id: shaping.map(|s| s.font_id.clone()).unwrap_or_default(),
            glyph_index: shaping.map(|s| s.glyph_index).unwrap_or(0),
            glyph_position_x: shaping.map(|s| s.glyph_position_x).unwrap_or(g.x),
            glyph_position_y: shaping.map(|s| s.glyph_position_y).unwrap_or(g.baseline_y),
            string_index: shaping.map(|s| s.string_index).unwrap_or(0),
            old_paragraph_text: None,
            texture_atlas_x: 0.0,
            texture_atlas_y: 0.0,
            texture_atlas_w: g.w,
            texture_atlas_h: g.h,
            shaped_run: None,
        }
    }

    pub fn with_old_paragraph(mut self, text: String) -> Self {
        self.old_paragraph_text = Some(text);
        self
    }

    pub fn with_font_id(mut self, id: &str) -> Self {
        self.font_id = id.to_string();
        self
    }

    pub fn with_glyph_index(mut self, idx: u32) -> Self {
        self.glyph_index = idx;
        self
    }

    pub fn with_string_index(mut self, idx: usize) -> Self {
        self.string_index = idx;
        self
    }

    pub fn with_shaped_run(mut self, run: ShapedVisualRun) -> Self {
        self.shaped_run = Some(run);
        self
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
    pub font_id: String,
    pub glyph_index: u32,
    pub string_index: usize,
    pub old_shaped_run: Option<ShapedVisualRun>,
    pub new_shaped_run: Option<ShapedVisualRun>,
}

impl ReflowRunSnapshot {
    pub fn from_reflow_glyph_rect_with_shaping(r: &ReflowGlyphRect, shaping: Option<&ShapedGlyphInfo>) -> Self {
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
            font_id: shaping.map(|s| s.font_id.clone()).unwrap_or_default(),
            glyph_index: shaping.map(|s| s.glyph_index).unwrap_or(0),
            string_index: shaping.map(|s| s.string_index).unwrap_or(0),
            old_shaped_run: None,
            new_shaped_run: None,
        }
    }

    pub fn with_font_id(mut self, id: &str) -> Self {
        self.font_id = id.to_string();
        self
    }

    pub fn with_old_shaped_run(mut self, run: ShapedVisualRun) -> Self {
        self.old_shaped_run = Some(run);
        self
    }

    pub fn with_new_shaped_run(mut self, run: ShapedVisualRun) -> Self {
        self.new_shaped_run = Some(run);
        self
    }
}

#[derive(Clone, Debug)]
pub(crate) struct GlyphAnimationPayload {
    pub snapshot: VisualRunSnapshot,
    pub old_cursor_rect: Option<CursorRect>,
    pub new_cursor_rect: Option<CursorRect>,
}

#[derive(Clone, Debug)]
pub(crate) struct ClusterAnimationPayload {
    pub snapshots: Vec<VisualRunSnapshot>,
    pub old_cursor_rect: Option<CursorRect>,
    pub new_cursor_rect: Option<CursorRect>,
}

#[derive(Clone, Debug)]
pub(crate) struct RunAnimationPayload {
    pub shaped_run: ShapedVisualRun,
    pub reflow_snapshot: Option<ReflowVisualSnapshot>,
    pub old_cursor_rect: Option<CursorRect>,
    pub new_cursor_rect: Option<CursorRect>,
}

#[derive(Clone, Debug)]
pub(crate) struct LineReflowAnimationPayload {
    pub insert_shaped_runs: Vec<ShapedVisualRun>,
    pub reflow_snapshot: ReflowVisualSnapshot,
    pub inserted_range: Option<(usize, usize)>,
    pub old_cursor_rect: Option<CursorRect>,
    pub new_cursor_rect: Option<CursorRect>,
}

#[derive(Clone, Debug)]
pub(crate) enum VisualPayload {
    InsertRuns {
        insert_runs: Vec<VisualRunSnapshot>,
        reflow_runs: Vec<ReflowRunSnapshot>,
        inserted_range: Option<(usize, usize)>,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        animation_mode: AnimationMode,
    },
    DeleteRuns {
        delete_runs: Vec<VisualRunSnapshot>,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        animation_mode: AnimationMode,
    },
    ReflowRuns {
        insert_runs: Vec<VisualRunSnapshot>,
        reflow_runs: Vec<ReflowRunSnapshot>,
        inserted_range: Option<(usize, usize)>,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        animation_mode: AnimationMode,
    },
    CursorTransition {
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
    },
    GlyphPayload {
        payload: GlyphAnimationPayload,
        animation_mode: AnimationMode,
    },
    ClusterPayload {
        payload: ClusterAnimationPayload,
        animation_mode: AnimationMode,
    },
    RunPayload {
        payload: RunAnimationPayload,
        animation_mode: AnimationMode,
    },
    LineReflowPayload {
        payload: LineReflowAnimationPayload,
        animation_mode: AnimationMode,
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
        animation_mode: AnimationMode,
        old_text: &str,
        _font_family: &str,
        shaped_glyphs: &[ShapedGlyphInfo],
    ) -> Self {
        let insert_runs: Vec<VisualRunSnapshot> = insert_glyph_rects.iter().map(|g| {
            let para_text = extract_paragraph_for_glyph(old_text, g.byte_start, g.byte_end);
            let shaping = shaped_glyphs.iter().find(|s|
                s.string_index <= g.byte_end && s.string_index >= g.byte_start
            );
            VisualRunSnapshot::from_glyph_rect_with_shaping(g, shaping)
                .with_old_paragraph(para_text)
        }).collect();
        let reflow_runs: Vec<ReflowRunSnapshot> = reflow_glyph_rects.iter().map(|r| {
            let shaping = shaped_glyphs.iter().find(|s|
                s.string_index <= r.byte_end && s.string_index >= r.byte_start
            );
            ReflowRunSnapshot::from_reflow_glyph_rect_with_shaping(r, shaping)
        }).collect();

        if has_reflow && !reflow_runs.is_empty() {
            VisualPayload::ReflowRuns {
                insert_runs,
                reflow_runs,
                inserted_range,
                old_cursor_rect,
                new_cursor_rect,
                animation_mode,
            }
        } else {
            VisualPayload::InsertRuns {
                insert_runs,
                reflow_runs,
                inserted_range,
                old_cursor_rect,
                new_cursor_rect,
                animation_mode,
            }
        }
    }

    pub fn from_delete_transaction(
        deleted_glyph_rects: &[GlyphRect],
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        animation_mode: AnimationMode,
        old_text: &str,
        _font_family: &str,
        shaped_glyphs: &[ShapedGlyphInfo],
    ) -> Self {
        let delete_runs: Vec<VisualRunSnapshot> = deleted_glyph_rects.iter().map(|g| {
            let para_text = extract_paragraph_for_glyph(old_text, g.byte_start, g.byte_end);
            let shaping = shaped_glyphs.iter().find(|s|
                s.string_index <= g.byte_end && s.string_index >= g.byte_start
            );
            VisualRunSnapshot::from_glyph_rect_with_shaping(g, shaping)
                .with_old_paragraph(para_text)
        }).collect();
        VisualPayload::DeleteRuns {
            delete_runs,
            old_cursor_rect,
            new_cursor_rect,
            animation_mode,
        }
    }

    pub fn from_glyph_payload(
        snapshot: VisualRunSnapshot,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        animation_mode: AnimationMode,
    ) -> Self {
        VisualPayload::GlyphPayload {
            payload: GlyphAnimationPayload {
                snapshot,
                old_cursor_rect,
                new_cursor_rect,
            },
            animation_mode,
        }
    }

    pub fn from_cluster_payload(
        snapshots: Vec<VisualRunSnapshot>,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        animation_mode: AnimationMode,
    ) -> Self {
        VisualPayload::ClusterPayload {
            payload: ClusterAnimationPayload {
                snapshots,
                old_cursor_rect,
                new_cursor_rect,
            },
            animation_mode,
        }
    }

    pub fn from_run_payload(
        shaped_run: ShapedVisualRun,
        reflow_snapshot: Option<ReflowVisualSnapshot>,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        animation_mode: AnimationMode,
    ) -> Self {
        VisualPayload::RunPayload {
            payload: RunAnimationPayload {
                shaped_run,
                reflow_snapshot,
                old_cursor_rect,
                new_cursor_rect,
            },
            animation_mode,
        }
    }

    pub fn from_line_reflow_payload(
        insert_shaped_runs: Vec<ShapedVisualRun>,
        reflow_snapshot: ReflowVisualSnapshot,
        inserted_range: Option<(usize, usize)>,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        animation_mode: AnimationMode,
    ) -> Self {
        VisualPayload::LineReflowPayload {
            payload: LineReflowAnimationPayload {
                insert_shaped_runs,
                reflow_snapshot,
                inserted_range,
                old_cursor_rect,
                new_cursor_rect,
            },
            animation_mode,
        }
    }

    pub fn cursor_rects(&self) -> (Option<&CursorRect>, Option<&CursorRect>) {
        match self {
            VisualPayload::InsertRuns { old_cursor_rect, new_cursor_rect, .. } => (old_cursor_rect.as_ref(), new_cursor_rect.as_ref()),
            VisualPayload::DeleteRuns { old_cursor_rect, new_cursor_rect, .. } => (old_cursor_rect.as_ref(), new_cursor_rect.as_ref()),
            VisualPayload::ReflowRuns { old_cursor_rect, new_cursor_rect, .. } => (old_cursor_rect.as_ref(), new_cursor_rect.as_ref()),
            VisualPayload::CursorTransition { old_cursor_rect, new_cursor_rect } => (old_cursor_rect.as_ref(), new_cursor_rect.as_ref()),
            VisualPayload::GlyphPayload { payload, .. } => (payload.old_cursor_rect.as_ref(), payload.new_cursor_rect.as_ref()),
            VisualPayload::ClusterPayload { payload, .. } => (payload.old_cursor_rect.as_ref(), payload.new_cursor_rect.as_ref()),
            VisualPayload::RunPayload { payload, .. } => (payload.old_cursor_rect.as_ref(), payload.new_cursor_rect.as_ref()),
            VisualPayload::LineReflowPayload { payload, .. } => (payload.old_cursor_rect.as_ref(), payload.new_cursor_rect.as_ref()),
        }
    }

    pub fn total_glyph_count(&self) -> usize {
        match self {
            VisualPayload::InsertRuns { insert_runs, reflow_runs, .. } => insert_runs.len() + reflow_runs.len(),
            VisualPayload::DeleteRuns { delete_runs, .. } => delete_runs.len(),
            VisualPayload::ReflowRuns { insert_runs, reflow_runs, .. } => insert_runs.len() + reflow_runs.len(),
            VisualPayload::CursorTransition { .. } => 0,
            VisualPayload::GlyphPayload { .. } => 1,
            VisualPayload::ClusterPayload { payload, .. } => payload.snapshots.len(),
            VisualPayload::RunPayload { payload, .. } => payload.shaped_run.glyphs.len(),
            VisualPayload::LineReflowPayload { payload, .. } => {
                payload.insert_shaped_runs.iter().map(|r| r.glyphs.len()).sum::<usize>()
                    + payload.reflow_snapshot.new_shaped_runs.iter().map(|r| r.glyphs.len()).sum::<usize>()
            }
        }
    }

    pub fn animation_mode(&self) -> AnimationMode {
        match self {
            VisualPayload::InsertRuns { animation_mode, .. } => *animation_mode,
            VisualPayload::DeleteRuns { animation_mode, .. } => *animation_mode,
            VisualPayload::ReflowRuns { animation_mode, .. } => *animation_mode,
            VisualPayload::CursorTransition { .. } => AnimationMode::GlyphAnimation,
            VisualPayload::GlyphPayload { animation_mode, .. } => *animation_mode,
            VisualPayload::ClusterPayload { animation_mode, .. } => *animation_mode,
            VisualPayload::RunPayload { animation_mode, .. } => *animation_mode,
            VisualPayload::LineReflowPayload { animation_mode, .. } => *animation_mode,
        }
    }

    pub fn is_insert(&self) -> bool {
        matches!(self, VisualPayload::InsertRuns { .. } | VisualPayload::GlyphPayload { .. } | VisualPayload::ClusterPayload { .. } | VisualPayload::RunPayload { .. } | VisualPayload::LineReflowPayload { .. })
    }

    pub fn is_delete(&self) -> bool {
        matches!(self, VisualPayload::DeleteRuns { .. })
    }

    pub fn insert_runs(&self) -> &[VisualRunSnapshot] {
        match self {
            VisualPayload::InsertRuns { insert_runs, .. } => insert_runs,
            VisualPayload::ReflowRuns { insert_runs, .. } => insert_runs,
            _ => &[],
        }
    }

    pub fn reflow_runs(&self) -> &[ReflowRunSnapshot] {
        match self {
            VisualPayload::InsertRuns { reflow_runs, .. } => reflow_runs,
            VisualPayload::ReflowRuns { reflow_runs, .. } => reflow_runs,
            _ => &[],
        }
    }

    pub fn delete_runs(&self) -> &[VisualRunSnapshot] {
        match self {
            VisualPayload::DeleteRuns { delete_runs, .. } => delete_runs,
            _ => &[],
        }
    }

    pub fn inserted_range(&self) -> Option<(usize, usize)> {
        match self {
            VisualPayload::InsertRuns { inserted_range, .. } => *inserted_range,
            VisualPayload::ReflowRuns { inserted_range, .. } => *inserted_range,
            VisualPayload::LineReflowPayload { payload, .. } => payload.inserted_range,
            _ => None,
        }
    }

    pub fn reflow_byte_ranges(&self) -> Vec<(usize, usize)> {
        match self {
            VisualPayload::InsertRuns { reflow_runs, .. } => {
                reflow_runs.iter().map(|r| (r.byte_start, r.byte_end)).collect()
            }
            VisualPayload::ReflowRuns { reflow_runs, .. } => {
                reflow_runs.iter().map(|r| (r.byte_start, r.byte_end)).collect()
            }
            VisualPayload::LineReflowPayload { payload, .. } => {
                payload.reflow_snapshot.new_shaped_runs.iter()
                    .map(|r| r.string_range())
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    pub fn shaped_runs_for_texture(&self) -> Vec<&ShapedVisualRun> {
        let mut runs = Vec::new();
        match self {
            VisualPayload::InsertRuns { insert_runs, reflow_runs, .. } => {
                for r in insert_runs {
                    if let Some(ref sr) = r.shaped_run {
                        runs.push(sr);
                    }
                }
                for r in reflow_runs {
                    if let Some(ref sr) = r.new_shaped_run {
                        runs.push(sr);
                    }
                }
            }
            VisualPayload::DeleteRuns { delete_runs, .. } => {
                for r in delete_runs {
                    if let Some(ref sr) = r.shaped_run {
                        runs.push(sr);
                    }
                }
            }
            VisualPayload::ReflowRuns { insert_runs, reflow_runs, .. } => {
                for r in insert_runs {
                    if let Some(ref sr) = r.shaped_run {
                        runs.push(sr);
                    }
                }
                for r in reflow_runs {
                    if let Some(ref sr) = r.new_shaped_run {
                        runs.push(sr);
                    }
                }
            }
            VisualPayload::GlyphPayload { payload, .. } => {
                if let Some(ref sr) = payload.snapshot.shaped_run {
                    runs.push(sr);
                }
            }
            VisualPayload::ClusterPayload { payload, .. } => {
                for s in &payload.snapshots {
                    if let Some(ref sr) = s.shaped_run {
                        runs.push(sr);
                    }
                }
            }
            VisualPayload::RunPayload { payload, .. } => {
                runs.push(&payload.shaped_run);
            }
            VisualPayload::LineReflowPayload { payload, .. } => {
                for r in &payload.insert_shaped_runs {
                    runs.push(r);
                }
                for r in &payload.reflow_snapshot.new_shaped_runs {
                    runs.push(r);
                }
            }
            VisualPayload::CursorTransition { .. } => {}
        }
        runs
    }
}

fn extract_paragraph_for_glyph(text: &str, byte_start: usize, byte_end: usize) -> String {
    let text_bytes = text.as_bytes();
    let mut para_start = byte_start;
    while para_start > 0 && text_bytes.get(para_start - 1).copied() != Some(b'\n') {
        para_start -= 1;
    }
    let mut para_end = byte_end.min(text_bytes.len());
    while para_end < text_bytes.len() && text_bytes.get(para_end).copied() != Some(b'\n') {
        para_end += 1;
    }
    text[para_start..para_end].to_string()
}
