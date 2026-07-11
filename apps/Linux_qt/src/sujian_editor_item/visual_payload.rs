use writer_core::editor::{CursorRect, GlyphRect, ReflowGlyphRect};
use super::animation_mode::AnimationMode;

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
}

#[derive(Clone, Debug)]
pub(crate) struct ShapedGlyphInfo {
    pub font_id: String,
    pub glyph_index: u32,
    pub glyph_position_x: f64,
    pub glyph_position_y: f64,
    pub string_index: usize,
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
        }
    }

    pub fn with_font_id(mut self, id: &str) -> Self {
        self.font_id = id.to_string();
        self
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
        font_family: &str,
        shaped_glyphs: &[ShapedGlyphInfo],
    ) -> Self {
        let insert_runs: Vec<VisualRunSnapshot> = insert_glyph_rects.iter().map(|g| {
            let para_text = extract_paragraph_for_glyph(old_text, g.byte_start, g.byte_end);
            let shaping = shaped_glyphs.iter().find(|s|
                s.string_index <= g.byte_end && s.string_index >= g.byte_start
            );
            VisualRunSnapshot::from_glyph_rect_with_shaping(g, shaping)
                .with_old_paragraph(para_text)
                .with_font_id(font_family)
        }).collect();
        let reflow_runs: Vec<ReflowRunSnapshot> = reflow_glyph_rects.iter().map(|r| {
            let shaping = shaped_glyphs.iter().find(|s|
                s.string_index <= r.byte_end && s.string_index >= r.byte_start
            );
            ReflowRunSnapshot::from_reflow_glyph_rect_with_shaping(r, shaping)
                .with_font_id(font_family)
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
        font_family: &str,
        shaped_glyphs: &[ShapedGlyphInfo],
    ) -> Self {
        let delete_runs: Vec<VisualRunSnapshot> = deleted_glyph_rects.iter().map(|g| {
            let para_text = extract_paragraph_for_glyph(old_text, g.byte_start, g.byte_end);
            let shaping = shaped_glyphs.iter().find(|s|
                s.string_index <= g.byte_end && s.string_index >= g.byte_start
            );
            VisualRunSnapshot::from_glyph_rect_with_shaping(g, shaping)
                .with_old_paragraph(para_text)
                .with_font_id(font_family)
        }).collect();
        VisualPayload::DeleteRuns {
            delete_runs,
            old_cursor_rect,
            new_cursor_rect,
            animation_mode,
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

    pub fn animation_mode(&self) -> AnimationMode {
        match self {
            VisualPayload::InsertRuns { animation_mode, .. } => *animation_mode,
            VisualPayload::DeleteRuns { animation_mode, .. } => *animation_mode,
            VisualPayload::ReflowRuns { animation_mode, .. } => *animation_mode,
            VisualPayload::CursorTransition { .. } => AnimationMode::GlyphAnimation,
        }
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
