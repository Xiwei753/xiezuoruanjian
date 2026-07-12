use super::layout_revision::LayoutRevision;
use super::shaped_visual_run::ShapedVisualRun;
use qmetaobject::QImage;
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct LineSnapshotId {
    pub revision: LayoutRevision,
    pub line_index: usize,
}

impl LineSnapshotId {
    pub fn new(revision: LayoutRevision, line_index: usize) -> Self {
        Self { revision, line_index }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedLineSnapshot {
    pub id: LineSnapshotId,
    pub byte_start: usize,
    pub byte_end: usize,
    pub visual_x: f64,
    pub visual_y: f64,
    pub visual_w: f64,
    pub visual_h: f64,
    pub baseline_y: f64,
    pub shaped_runs: Vec<ShapedVisualRun>,
    pub para_text: String,
    pub qtextline_idx: i32,
    pub paragraph_wrap_w: f64,
    pub para_indent: f64,
}

impl PreparedLineSnapshot {
    pub fn string_range(&self) -> (usize, usize) {
        (self.byte_start, self.byte_end)
    }

    pub fn intersects_byte_range(&self, start: usize, end: usize) -> bool {
        self.byte_end > start && self.byte_start < end
    }

    pub fn runs_in_byte_range(&self, start: usize, end: usize) -> Vec<&ShapedVisualRun> {
        self.shaped_runs
            .iter()
            .filter(|r| r.source_string_end > start && r.source_string_start < end)
            .collect()
    }
}

#[derive(Clone, Debug)]
pub(crate) struct EditorLayoutSnapshot {
    pub revision: LayoutRevision,
    pub lines: Vec<PreparedLineSnapshot>,
}

impl EditorLayoutSnapshot {
    pub fn new(revision: LayoutRevision) -> Self {
        Self {
            revision,
            lines: Vec::new(),
        }
    }

    pub fn affected_lines(&self, byte_start: usize, byte_end: usize) -> Vec<&PreparedLineSnapshot> {
        self.lines
            .iter()
            .filter(|l| l.intersects_byte_range(byte_start, byte_end))
            .collect()
    }

    pub fn line_for_byte(&self, byte_offset: usize) -> Option<&PreparedLineSnapshot> {
        self.lines
            .iter()
            .find(|l| byte_offset >= l.byte_start && byte_offset < l.byte_end)
    }
}

pub(crate) struct LineTextureStore {
    textures: HashMap<LineSnapshotId, QImage>,
}

impl LineTextureStore {
    pub fn new() -> Self {
        Self {
            textures: HashMap::new(),
        }
    }

    pub fn insert(&mut self, id: LineSnapshotId, texture: QImage) {
        self.textures.insert(id, texture);
    }

    pub fn get(&self, id: &LineSnapshotId) -> Option<&QImage> {
        self.textures.get(id)
    }

    pub fn remove_revision(&mut self, revision: LayoutRevision) {
        self.textures.retain(|id, _| id.revision != revision);
    }

    pub fn clear(&mut self) {
        self.textures.clear();
    }
}

impl Default for LineTextureStore {
    fn default() -> Self {
        Self::new()
    }
}
