use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::layout_snapshot::{ShapingIdentity, SourceRect, LineClusterSnapshot};
use super::shaped_visual_run::ShapedVisualRun;

#[derive(Clone, Debug)]
pub(crate) struct OffsetMap {
    pub unchanged_segments: Vec<UnchangedSegment>,
    pub inserted_ranges: Vec<(usize, usize)>,
    pub deleted_ranges: Vec<(usize, usize)>,
}

#[derive(Clone, Debug)]
pub(crate) struct UnchangedSegment {
    pub old_byte_start: usize,
    pub old_byte_end: usize,
    pub new_byte_start: usize,
    pub new_byte_end: usize,
}

impl OffsetMap {
    pub fn from_changes(old_text: &str, new_text: &str) -> Self {
        let changes = writer_core::editor::diff_plain_text(old_text, new_text);
        let mut unchanged_segments = Vec::new();
        let mut inserted_ranges = Vec::new();
        let mut deleted_ranges = Vec::new();

        let mut old_cursor: usize = 0;
        let mut new_cursor: usize = 0;
        let mut pending_insert_len: usize = 0;

        for change in &changes {
            match change {
                writer_core::editor::EditorChange::Delete { index, text } => {
                    let delete_pos = *index;
                    if old_cursor < delete_pos {
                        let old_seg_end = delete_pos;
                        let new_seg_end = new_cursor + (delete_pos - old_cursor);
                        unchanged_segments.push(UnchangedSegment {
                            old_byte_start: old_cursor,
                            old_byte_end: old_seg_end,
                            new_byte_start: new_cursor,
                            new_byte_end: new_seg_end,
                        });
                        new_cursor = new_seg_end;
                        old_cursor = old_seg_end;
                    }
                    let delete_end = old_cursor + text.len();
                    deleted_ranges.push((old_cursor, delete_end));
                    old_cursor = delete_end;
                }
                writer_core::editor::EditorChange::Insert { index, text } => {
                    let insert_old_pos = *index;
                    if old_cursor < insert_old_pos {
                        let old_seg_end = insert_old_pos;
                        let new_seg_end = new_cursor + (insert_old_pos - old_cursor);
                        unchanged_segments.push(UnchangedSegment {
                            old_byte_start: old_cursor,
                            old_byte_end: old_seg_end,
                            new_byte_start: new_cursor,
                            new_byte_end: new_seg_end,
                        });
                        new_cursor = new_seg_end;
                        old_cursor = old_seg_end;
                    }
                    let insert_new_start = new_cursor;
                    let insert_new_end = new_cursor + text.len();
                    inserted_ranges.push((insert_new_start, insert_new_end));
                    new_cursor = insert_new_end;
                    pending_insert_len += text.len();
                }
            }
        }

        if old_cursor < old_text.len() {
            let remaining = old_text.len() - old_cursor;
            unchanged_segments.push(UnchangedSegment {
                old_byte_start: old_cursor,
                old_byte_end: old_text.len(),
                new_byte_start: new_cursor,
                new_byte_end: new_cursor + remaining,
            });
        }

        OffsetMap {
            unchanged_segments,
            inserted_ranges,
            deleted_ranges,
        }
    }

    pub fn old_byte_to_new(&self, old_byte: usize) -> Option<usize> {
        for seg in &self.unchanged_segments {
            if old_byte >= seg.old_byte_start && old_byte < seg.old_byte_end {
                let offset = old_byte - seg.old_byte_start;
                return Some(seg.new_byte_start + offset);
            }
        }
        None
    }

    pub fn new_byte_to_old(&self, new_byte: usize) -> Option<usize> {
        for seg in &self.unchanged_segments {
            if new_byte >= seg.new_byte_start && new_byte < seg.new_byte_end {
                let offset = new_byte - seg.new_byte_start;
                return Some(seg.old_byte_start + offset);
            }
        }
        None
    }

    pub fn is_unchanged_new(&self, new_byte_start: usize, new_byte_end: usize) -> bool {
        self.unchanged_segments.iter().any(|seg| {
            new_byte_start >= seg.new_byte_start && new_byte_end <= seg.new_byte_end
        })
    }

    pub fn find_unchanged_segments_in_new_range(
        &self,
        new_byte_start: usize,
        new_byte_end: usize,
    ) -> Vec<&UnchangedSegment> {
        self.unchanged_segments
            .iter()
            .filter(|seg| seg.new_byte_end > new_byte_start && seg.new_byte_start < new_byte_end)
            .collect()
    }
}

pub(crate) fn compute_shaping_identity(
    cluster: &LineClusterSnapshot,
    run: &ShapedVisualRun,
) -> ShapingIdentity {
    let mut text_hasher = DefaultHasher::new();
    cluster.byte_start.hash(&mut text_hasher);
    cluster.byte_end.hash(&mut text_hasher);
    let text_content_hash = text_hasher.finish();

    let raw_font_fingerprint = run.raw_font_key.as_stable_id();

    let mut glyph_hasher = DefaultHasher::new();
    for g in &run.glyphs {
        g.glyph_index.hash(&mut glyph_hasher);
    }
    let glyph_indexes_hash = glyph_hasher.finish();

    let cluster_glyph_count = cluster.byte_end - cluster.byte_start;
    let direction_rtl = run.is_rtl();

    let mut fmt_hasher = DefaultHasher::new();
    run.flags.bits().hash(&mut fmt_hasher);
    let format_fingerprint = fmt_hasher.finish();

    ShapingIdentity {
        text_content_hash,
        raw_font_fingerprint,
        glyph_indexes_hash,
        cluster_glyph_count,
        direction_rtl,
        format_fingerprint,
    }
}

#[derive(Clone, Debug)]
pub(crate) enum AnimatedSliceKind {
    Move {
        old_snapshot_id: super::layout_snapshot::LineSnapshotId,
        old_source_rect: SourceRect,
        new_destination_rect: SourceRect,
    },
    Insert {
        new_snapshot_id: super::layout_snapshot::LineSnapshotId,
        new_source_rect: SourceRect,
        new_destination_rect: SourceRect,
    },
    Delete {
        old_snapshot_id: super::layout_snapshot::LineSnapshotId,
        old_source_rect: SourceRect,
        old_destination_rect: SourceRect,
    },
    CrossfadeOld {
        old_snapshot_id: super::layout_snapshot::LineSnapshotId,
        old_source_rect: SourceRect,
        old_destination_rect: SourceRect,
    },
    CrossfadeNew {
        new_snapshot_id: super::layout_snapshot::LineSnapshotId,
        new_source_rect: SourceRect,
        new_destination_rect: SourceRect,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct AnimatedSlice {
    pub kind: AnimatedSliceKind,
    pub byte_start: usize,
    pub byte_end: usize,
    pub shaping_identity: Option<ShapingIdentity>,
}

impl AnimatedSlice {
    pub fn source_rect(&self) -> &SourceRect {
        match &self.kind {
            AnimatedSliceKind::Move { old_source_rect, .. } => old_source_rect,
            AnimatedSliceKind::Insert { new_source_rect, .. } => new_source_rect,
            AnimatedSliceKind::Delete { old_source_rect, .. } => old_source_rect,
            AnimatedSliceKind::CrossfadeOld { old_source_rect, .. } => old_source_rect,
            AnimatedSliceKind::CrossfadeNew { new_source_rect, .. } => new_source_rect,
        }
    }

    pub fn destination_rect(&self) -> &SourceRect {
        match &self.kind {
            AnimatedSliceKind::Move { new_destination_rect, .. } => new_destination_rect,
            AnimatedSliceKind::Insert { new_destination_rect, .. } => new_destination_rect,
            AnimatedSliceKind::Delete { old_destination_rect, .. } => old_destination_rect,
            AnimatedSliceKind::CrossfadeOld { old_destination_rect, .. } => old_destination_rect,
            AnimatedSliceKind::CrossfadeNew { new_destination_rect, .. } => new_destination_rect,
        }
    }

    pub fn snapshot_id(&self) -> super::layout_snapshot::LineSnapshotId {
        match &self.kind {
            AnimatedSliceKind::Move { old_snapshot_id, .. } => *old_snapshot_id,
            AnimatedSliceKind::Insert { new_snapshot_id, .. } => *new_snapshot_id,
            AnimatedSliceKind::Delete { old_snapshot_id, .. } => *old_snapshot_id,
            AnimatedSliceKind::CrossfadeOld { old_snapshot_id, .. } => *old_snapshot_id,
            AnimatedSliceKind::CrossfadeNew { new_snapshot_id, .. } => *new_snapshot_id,
        }
    }

    pub fn is_insert(&self) -> bool {
        matches!(self.kind, AnimatedSliceKind::Insert { .. })
    }

    pub fn is_delete(&self) -> bool {
        matches!(self.kind, AnimatedSliceKind::Delete { .. })
    }

    pub fn is_move(&self) -> bool {
        matches!(self.kind, AnimatedSliceKind::Move { .. })
    }

    pub fn is_crossfade(&self) -> bool {
        matches!(
            self.kind,
            AnimatedSliceKind::CrossfadeOld { .. } | AnimatedSliceKind::CrossfadeNew { .. }
        )
    }
}

#[derive(Clone, Debug)]
pub(crate) struct StaticLinePatch {
    pub snapshot_id: super::layout_snapshot::LineSnapshotId,
    pub hidden_source_rects: Vec<SourceRect>,
}

#[derive(Clone, Debug)]
pub(crate) struct EditorVisualTransactionData {
    pub slices: Vec<AnimatedSlice>,
    pub static_patches: Vec<StaticLinePatch>,
    pub old_cursor_rect: Option<writer_core::editor::CursorRect>,
    pub new_cursor_rect: Option<writer_core::editor::CursorRect>,
}

impl EditorVisualTransactionData {
    pub fn new() -> Self {
        Self {
            slices: Vec::new(),
            static_patches: Vec::new(),
            old_cursor_rect: None,
            new_cursor_rect: None,
        }
    }
}

pub(crate) fn build_slices_for_insert_no_reflow(
    old_snapshot: &super::layout_snapshot::EditorLayoutSnapshot,
    new_snapshot: &super::layout_snapshot::EditorLayoutSnapshot,
    inserted_range: (usize, usize),
    offset_map: &OffsetMap,
) -> EditorVisualTransactionData {
    let mut data = EditorVisualTransactionData::new();
    let (insert_start, insert_end) = inserted_range;

    if let Some(new_line) = new_snapshot.line_for_byte(insert_start) {
        if let Some(clusters) = new_line.source_rect_for_byte_range(insert_start, insert_end) {
            data.slices.push(AnimatedSlice {
                kind: AnimatedSliceKind::Insert {
                    new_snapshot_id: new_line.id,
                    new_source_rect: clusters.clone(),
                    new_destination_rect: clusters,
                },
                byte_start: insert_start,
                byte_end: insert_end,
                shaping_identity: None,
            });

            data.static_patches.push(StaticLinePatch {
                snapshot_id: new_line.id,
                hidden_source_rects: vec![data.slices.last().unwrap().source_rect().clone()],
            });
        }
    }

    data
}

pub(crate) fn build_slices_for_delete_no_reflow(
    old_snapshot: &super::layout_snapshot::EditorLayoutSnapshot,
    _new_snapshot: &super::layout_snapshot::EditorLayoutSnapshot,
    deleted_range: (usize, usize),
) -> EditorVisualTransactionData {
    let mut data = EditorVisualTransactionData::new();
    let (delete_start, delete_end) = deleted_range;

    if let Some(old_line) = old_snapshot.line_for_byte(delete_start) {
        if let Some(source_rect) = old_line.source_rect_for_byte_range(delete_start, delete_end) {
            data.slices.push(AnimatedSlice {
                kind: AnimatedSliceKind::Delete {
                    old_snapshot_id: old_line.id,
                    old_source_rect: source_rect.clone(),
                    old_destination_rect: source_rect,
                },
                byte_start: delete_start,
                byte_end: delete_end,
                shaping_identity: None,
            });
        }
    }

    data
}

pub(crate) fn build_slices_for_reflow(
    old_snapshot: &super::layout_snapshot::EditorLayoutSnapshot,
    new_snapshot: &super::layout_snapshot::EditorLayoutSnapshot,
    offset_map: &OffsetMap,
    inserted_range: Option<(usize, usize)>,
    deleted_ranges: &[(usize, usize)],
) -> EditorVisualTransactionData {
    let mut data = EditorVisualTransactionData::new();

    if let Some((insert_start, insert_end)) = inserted_range {
        for new_line in new_snapshot.lines_in_byte_range(insert_start, insert_end) {
            if let Some(source_rect) = new_line.source_rect_for_byte_range(insert_start, insert_end) {
                data.slices.push(AnimatedSlice {
                    kind: AnimatedSliceKind::Insert {
                        new_snapshot_id: new_line.id,
                        new_source_rect: source_rect.clone(),
                        new_destination_rect: source_rect,
                    },
                    byte_start: insert_start,
                    byte_end: insert_end,
                    shaping_identity: None,
                });
            }
        }
    }

    for (del_start, del_end) in deleted_ranges {
        for old_line in old_snapshot.lines_in_byte_range(*del_start, *del_end) {
            if let Some(source_rect) = old_line.source_rect_for_byte_range(*del_start, *del_end) {
                data.slices.push(AnimatedSlice {
                    kind: AnimatedSliceKind::Delete {
                        old_snapshot_id: old_line.id,
                        old_source_rect: source_rect.clone(),
                        old_destination_rect: source_rect,
                    },
                    byte_start: *del_start,
                    byte_end: *del_end,
                    shaping_identity: None,
                });
            }
        }
    }

    for seg in &offset_map.unchanged_segments {
        let old_lines = old_snapshot.lines_in_byte_range(seg.old_byte_start, seg.old_byte_end);
        let new_lines = new_snapshot.lines_in_byte_range(seg.new_byte_start, seg.new_byte_end);

        for old_line in &old_lines {
            for new_line in &new_lines {
                let old_seg_start = seg.old_byte_start.max(old_line.byte_start);
                let old_seg_end = seg.old_byte_end.min(old_line.byte_end);
                let new_seg_start = seg.new_byte_start.max(new_line.byte_start);
                let new_seg_end = seg.new_byte_end.min(new_line.byte_end);

                if old_seg_start >= old_seg_end || new_seg_start >= new_seg_end {
                    continue;
                }

                let old_source = old_line.source_rect_for_byte_range(old_seg_start, old_seg_end);
                let new_source = new_line.source_rect_for_byte_range(new_seg_start, new_seg_end);
                let new_source_for_patch = new_source.clone();

                match (old_source, new_source) {
                    (Some(old_sr), Some(new_sr)) => {
                        let same_line = old_line.id.visual_line_id == new_line.id.visual_line_id;
                        let same_shaping = old_line.clusters.iter()
                            .zip(new_line.clusters.iter())
                            .all(|(oc, nc)| oc.shaping_identity.is_same_shaping(&nc.shaping_identity));

                        if same_shaping {
                            data.slices.push(AnimatedSlice {
                                kind: AnimatedSliceKind::Move {
                                    old_snapshot_id: old_line.id,
                                    old_source_rect: old_sr,
                                    new_destination_rect: new_sr,
                                },
                                byte_start: new_seg_start,
                                byte_end: new_seg_end,
                                shaping_identity: old_line.clusters.first().map(|c| c.shaping_identity.clone()),
                            });
                        } else {
                            data.slices.push(AnimatedSlice {
                                kind: AnimatedSliceKind::CrossfadeOld {
                                    old_snapshot_id: old_line.id,
                                    old_source_rect: old_sr.clone(),
                                    old_destination_rect: old_sr,
                                },
                                byte_start: new_seg_start,
                                byte_end: new_seg_end,
                                shaping_identity: None,
                            });
                            data.slices.push(AnimatedSlice {
                                kind: AnimatedSliceKind::CrossfadeNew {
                                    new_snapshot_id: new_line.id,
                                    new_source_rect: new_sr.clone(),
                                    new_destination_rect: new_sr,
                                },
                                byte_start: new_seg_start,
                                byte_end: new_seg_end,
                                shaping_identity: None,
                            });
                        }

                        if !same_line {
                            if let Some(ns) = new_source_for_patch {
                                data.static_patches.push(StaticLinePatch {
                                    snapshot_id: new_line.id,
                                    hidden_source_rects: vec![ns],
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_offset_map_insert() {
        let map = OffsetMap::from_changes("hello", "hello world");
        assert_eq!(map.inserted_ranges.len(), 1);
        assert_eq!(map.inserted_ranges[0], (5, 11));
        assert_eq!(map.unchanged_segments.len(), 1);
        assert_eq!(map.unchanged_segments[0].old_byte_start, 0);
        assert_eq!(map.unchanged_segments[0].old_byte_end, 5);
    }

    #[test]
    fn test_offset_map_delete() {
        let map = OffsetMap::from_changes("hello world", "hello");
        assert_eq!(map.deleted_ranges.len(), 1);
        assert_eq!(map.deleted_ranges[0], (5, 11));
        assert_eq!(map.unchanged_segments.len(), 1);
        assert_eq!(map.unchanged_segments[0].old_byte_start, 0);
        assert_eq!(map.unchanged_segments[0].old_byte_end, 5);
    }

    #[test]
    fn test_offset_map_replace() {
        let map = OffsetMap::from_changes("abc", "xyz");
        assert_eq!(map.deleted_ranges.len(), 1);
        assert_eq!(map.inserted_ranges.len(), 1);
        assert!(map.unchanged_segments.is_empty());
    }

    #[test]
    fn test_offset_map_old_to_new() {
        let map = OffsetMap::from_changes("abc", "abXc");
        assert_eq!(map.old_byte_to_new(0), Some(0));
        assert_eq!(map.old_byte_to_new(2), Some(3));
        assert_eq!(map.old_byte_to_new(3), None);
    }

    #[test]
    fn test_offset_map_new_to_old() {
        let map = OffsetMap::from_changes("abc", "abXc");
        assert_eq!(map.new_byte_to_old(0), Some(0));
        assert_eq!(map.new_byte_to_old(2), None);
        assert_eq!(map.new_byte_to_old(3), Some(2));
    }

    #[test]
    fn test_animated_slice_kinds() {
        use crate::sujian_editor_item::layout_snapshot::{LineSnapshotId, LayoutRevision};
        let id = LineSnapshotId::new(LayoutRevision::new(), 0);
        let sr = SourceRect::zero();

        let insert = AnimatedSlice {
            kind: AnimatedSliceKind::Insert {
                new_snapshot_id: id,
                new_source_rect: sr.clone(),
                new_destination_rect: sr.clone(),
            },
            byte_start: 0,
            byte_end: 1,
            shaping_identity: None,
        };
        assert!(insert.is_insert());
        assert!(!insert.is_delete());

        let delete = AnimatedSlice {
            kind: AnimatedSliceKind::Delete {
                old_snapshot_id: id,
                old_source_rect: sr.clone(),
                old_destination_rect: sr.clone(),
            },
            byte_start: 0,
            byte_end: 1,
            shaping_identity: None,
        };
        assert!(delete.is_delete());
        assert!(!delete.is_insert());
    }
}
