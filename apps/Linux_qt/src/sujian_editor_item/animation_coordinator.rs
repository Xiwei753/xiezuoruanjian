//! Linux Qt 文字动画协调器。
//!
//! 主链：
//! ```text
//! Core EditorVisualTransaction
//! → 捕获 old/new layout snapshot
//! → 生成 AnimatedSlice + StaticLinePatch
//! → 准备平台视觉资源
//! → PreparedTransactionQueue
//! → rendering overlay + cursor transition
//! ```
//!
//! 关键约束：
//! - 先完成视觉资源准备，再允许静态层隐藏：`texture_prepared` 为 true 前静态层不裁剪，
//!   否则准备纹理与第一帧 overlay 之间会出现空白帧。
//! - 连续输入从当前视觉帧 rebase：新事务与旧事务 byte range 重叠时，先从旧事务当前
//!   progress 计算已显示帧位置，rebase 新 slice 的 from_document_rect，再取消旧事务，
//!   保证视觉无跳变。
//! - scrolling/window inactive 使用 pause/resume 而非销毁事务：滚动结束后 revision
//!   未变则累加 paused duration 继续，避免重新创建事务的开销和视觉跳变。
//! - revision 不匹配时必须取消：旧 source rect 是旧布局的产物，不能套到新布局上，
//!   否则坐标和 shaping 全部错误。
//! - shaping identity 变化走 crossfade 而非强行 move：字体、glyph、方向、格式任一变化
//!   都意味着旧视觉资源与新排版结果不是同一视觉对象，强行移动会导致 ligature/RTL/emoji
//!   渲染错误。

use std::time::Instant;

use writer_core::editor::{
    CursorRect, EditorAnimationKind, EditorVisualTransaction, OffsetMap,
};

pub(crate) use super::transaction_key::VisualTransactionKey;
pub(crate) use super::animation_mode::AnimationMode;
pub(crate) use super::cursor_animation::{CursorAnimationPlan, CursorBlinkMode, CursorTransition};
pub(crate) use super::render_plan::{
    HiddenClipRect, StaticTextPlan, TextAnimationPlan, TextAnimationGlyphInfo,
    SelectionPreeditPlan, SelectionRange, PreeditRange,
    ImeUpdateKind, ImeUpdatePlan, RenderPlan,
};
pub(crate) use super::texture_cache::TextureCache;
use super::animated_slice::{AnimatedSlice, AnimatedSliceFrame, AnimatedSliceKind};
use super::text_visual_transaction::{
    PreparedTextVisualTransaction, PreparedTransactionQueue,
    TextVisualTransactionState, TextVisualOperationKind, TransactionTimeline,
};
use super::decoration_slice::DecorationSlice;
use super::static_line_patch::StaticLinePatch;
use super::layout_revision::LayoutRevision;
use super::layout_snapshot::{EditorLayoutSnapshot, LineSnapshotId, SourceRect, ShapingIdentity};

pub(crate) struct LinuxEditorAnimationCoordinator {
    next_key_id: u64,
    pub(crate) prepared_queue: PreparedTransactionQueue,
    layout_revision: LayoutRevision,
}

impl LinuxEditorAnimationCoordinator {
    pub fn new() -> Self {
        Self {
            next_key_id: 1,
            prepared_queue: PreparedTransactionQueue::new(),
            layout_revision: LayoutRevision::initial(),
        }
    }

    pub(crate) fn alloc_key(&mut self) -> VisualTransactionKey {
        let id = self.next_key_id;
        self.next_key_id += 1;
        VisualTransactionKey::new(id, id)
    }

    pub fn process_transaction(
        &mut self,
        vt: &EditorVisualTransaction,
        typing_animation_enabled: bool,
        is_scrolling: bool,
        is_loading: bool,
        is_applying_format: bool,
        is_applying_settings: bool,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        old_snapshot: &EditorLayoutSnapshot,
        new_snapshot: &EditorLayoutSnapshot,
    ) -> Option<VisualTransactionKey> {
        if !typing_animation_enabled || is_scrolling || is_loading || is_applying_format || is_applying_settings {
            return None;
        }

        let mode = AnimationMode::from_core(vt.animation_mode);
        if !mode.should_create_transaction() {
            return None;
        }

        let new_revision = LayoutRevision::next();

        match vt.kind {
            EditorAnimationKind::Insert => {
                if let Some((range_start, range_end)) = vt.inserted_range {
                    let conflicting = self.prepared_queue.find_conflicting_transaction(range_start, range_end);
                    let rebase_frames: Vec<(usize, usize, f64, f64, f64, Option<ShapingIdentity>)> = if let Some(old_key) = conflicting {
                        let mut frames = Vec::new();
                        if let Some(old_tx) = self.prepared_queue.active_transactions().iter().find(|t| t.key == old_key) {
                            let old_progress = old_tx.progress(Instant::now());
                            if old_progress > 0.0 && old_progress < 1.0 {
                                for old_slice in &old_tx.slices {
                                    let frame = old_slice.compute_frame(old_progress);
                                    frames.push((old_slice.byte_start, old_slice.byte_end, frame.x, frame.y, frame.opacity, old_slice.shaping_identity.clone()));
                                }
                            }
                        }
                        self.prepared_queue.cancel(old_key, "rebased");
                        frames
                    } else {
                        Vec::new()
                    };

                    let key = self.alloc_key();
                    let mut slices = Vec::new();
                    let mut static_patches = Vec::new();

                    let old_cx = old_cursor_rect.as_ref().map(|c| c.x).unwrap_or(0.0);
                    let old_cy = old_cursor_rect.as_ref().map(|c| c.top).unwrap_or(0.0);

                    for new_line in new_snapshot.lines_in_byte_range(range_start, range_end) {
                        if let Some(source_rect) = new_line.source_rect_for_byte_range(range_start, range_end) {
                            let to_doc = new_line.source_rect_to_document_rect(&source_rect);
                            slices.push(AnimatedSlice::insert_fade_in(
                                key,
                                new_line.id,
                                source_rect.clone(),
                                to_doc,
                                old_cx,
                                old_cy,
                                range_start,
                                range_end,
                                new_line.clusters.first().map(|c| c.shaping_identity.clone()),
                            ));

                            static_patches.push(StaticLinePatch::insert_patch(
                                key,
                                new_line.id,
                                vec![source_rect],
                                range_start,
                                range_end,
                            ));
                        }
                    }

                    let reflow_start = range_end;
                    let insert_reflow_offset_map = OffsetMap::build(&vt.old_text, &vt.new_text);
                    for new_line in &new_snapshot.line_snapshots {
                        if new_line.byte_end <= range_end {
                            continue;
                        }
                        if new_line.byte_start < range_end {
                            continue;
                        }

                        let old_line = {
                            let mapped_old_byte_start = insert_reflow_offset_map.map_new_to_old(new_line.byte_start);
                            let mapped_old_byte_end = insert_reflow_offset_map.map_new_to_old(new_line.byte_end);
                            if let (Some(mobs), Some(mobe)) = (mapped_old_byte_start, mapped_old_byte_end) {
                                old_snapshot.line_for_byte_range(mobs, mobe).cloned()
                            } else if let Some(mobs) = mapped_old_byte_start {
                                old_snapshot.line_for_byte(mobs).cloned()
                            } else {
                                None
                            }
                        };
                        if let Some(ol) = old_line {
                            let old_sr = ol.source_rect_for_byte_range(ol.byte_start, ol.byte_end);
                            let new_sr = new_line.source_rect_for_byte_range(new_line.byte_start, new_line.byte_end);

                            match (old_sr, new_sr) {
                                (Some(old_src), Some(new_src)) => {
                                    let same_shaping = ol.clusters.len() == new_line.clusters.len()
                                        && ol.clusters.iter()
                                            .zip(new_line.clusters.iter())
                                            .all(|(oc, nc)| oc.shaping_identity.is_same_shaping(&nc.shaping_identity));

                                    let old_doc = ol.source_rect_to_document_rect(&old_src);
                                    let new_doc = new_line.source_rect_to_document_rect(&new_src);

                                    if same_shaping {
                                        slices.push(AnimatedSlice::reflow_move(
                                            key,
                                            ol.id,
                                            old_src,
                                            old_doc,
                                            new_line.id,
                                            new_src.clone(),
                                            new_doc,
                                            new_line.byte_start,
                                            new_line.byte_end,
                                            ol.clusters.first().map(|c| c.shaping_identity.clone()),
                                        ));
                                    } else {
                                        slices.push(AnimatedSlice::reflow_crossfade_old(
                                            key,
                                            ol.id,
                                            old_src.clone(),
                                            old_doc.clone(),
                                            new_doc.clone(),
                                            new_line.byte_start,
                                            new_line.byte_end,
                                        ));
                                        slices.push(AnimatedSlice::reflow_crossfade_new(
                                            key,
                                            new_line.id,
                                            new_src.clone(),
                                            old_doc,
                                            new_doc,
                                            new_line.byte_start,
                                            new_line.byte_end,
                                        ));
                                    }

                                    static_patches.push(StaticLinePatch::reflow_patch(
                                        key,
                                        new_line.id,
                                        vec![new_src],
                                        new_line.byte_start,
                                        new_line.byte_end,
                                    ));
                                }
                                _ => {}
                            }
                        }
                    }

                    let cursor_transition = if old_cursor_rect.is_some() && new_cursor_rect.is_some() {
                        CursorTransition::Tween {
                            old_rect: old_cursor_rect.clone().unwrap(),
                            new_rect: new_cursor_rect.clone().unwrap(),
                            duration_ms: vt.duration_ms,
                        }
                    } else {
                        CursorTransition::Snap
                    };

                    let insert_offset_map = OffsetMap::build(&vt.old_text, &vt.new_text);
                    let mut consumed_indices: Vec<usize> = Vec::new();
                    for (bs, be, fx, fy, fo, ref shaping) in &rebase_frames {
                        if let Some(new_slice) = slices.iter_mut().find(|ns| ns.byte_start == *bs && ns.byte_end == *be) {
                            new_slice.rebase_from(*fx, *fy, *fo);
                        } else if let (Some(mbs), Some(mbe)) = (insert_offset_map.map_old_to_new(*bs), insert_offset_map.map_old_to_new(*be)) {
                            if let Some(new_slice) = slices.iter_mut().find(|ns| ns.byte_start == mbs && ns.byte_end == mbe) {
                                new_slice.rebase_from(*fx, *fy, *fo);
                            } else if let Some(ref sid) = shaping {
                                let mapped_center = (mbs + mbe) / 2;
                                let best = slices.iter_mut().enumerate()
                                    .filter(|(idx, ns)| !consumed_indices.contains(idx))
                                    .filter(|(_, ns)| ns.shaping_identity.as_ref() == Some(sid))
                                    .filter(|(_, ns)| ns.byte_start >= mbs && ns.byte_end <= mbe.max(mbs + 1))
                                    .min_by_key(|(_, ns)| (ns.byte_start + ns.byte_end) as i64 / 2 - mapped_center as i64);
                                if let Some((idx, new_slice)) = best {
                                    new_slice.rebase_from(*fx, *fy, *fo);
                                    consumed_indices.push(idx);
                                }
                            }
                        }
                    }

                    let prepared = PreparedTextVisualTransaction {
                        key,
                        state: TextVisualTransactionState::Pending,
                        operation_kind: TextVisualOperationKind::Insert,
                        animation_mode: mode,
                        timeline: TransactionTimeline::new(vt.duration_ms),
                        old_revision: self.layout_revision,
                        new_revision,
                        slices,
                        static_patches,
                        decoration_slices: Vec::new(),
                        cursor_transition,
                        old_cursor_rect,
                        new_cursor_rect,
                        cancel_reason: None,
                        texture_prepared: false,
                        old_snapshot: Some(old_snapshot.clone()),
                        new_snapshot: Some(new_snapshot.clone()),
                    };

                    self.layout_revision = new_revision;
                    self.prepared_queue.enqueue(prepared);

                    return Some(key);
                }
            }
            EditorAnimationKind::Delete => {
                let deleted_ranges: Vec<(usize, usize)> = if let Some((ds, de)) = vt.deleted_range {
                    vec![(ds, de)]
                } else {
                    let changes = writer_core::editor::diff_plain_text(&vt.old_text, &vt.new_text);
                    let mut ranges = Vec::new();
                    for change in &changes {
                        if let writer_core::editor::EditorChange::Delete { index, text } = change {
                            let range_start = *index;
                            let range_end = range_start + text.len();
                            ranges.push((range_start, range_end));
                        }
                    }
                    ranges
                };

                let rebase_byte_start = deleted_ranges.first().map(|(s, _)| *s).unwrap_or(0);
                let rebase_byte_end = deleted_ranges.last().map(|(_, e)| *e).unwrap_or(0);
                let conflicting = self.prepared_queue.find_conflicting_transaction(rebase_byte_start, rebase_byte_end);
                let rebase_frames: Vec<(usize, usize, f64, f64, f64, Option<ShapingIdentity>)> = if let Some(old_key) = conflicting {
                    let mut frames = Vec::new();
                    if let Some(old_tx) = self.prepared_queue.active_transactions().iter().find(|t| t.key == old_key) {
                        let old_progress = old_tx.progress(Instant::now());
                        if old_progress > 0.0 && old_progress < 1.0 {
                            for old_slice in &old_tx.slices {
                                let frame = old_slice.compute_frame(old_progress);
                                frames.push((old_slice.byte_start, old_slice.byte_end, frame.x, frame.y, frame.opacity, old_slice.shaping_identity.clone()));
                            }
                        }
                    }
                    self.prepared_queue.cancel(old_key, "rebased");
                    frames
                } else {
                    Vec::new()
                };

                let key = self.alloc_key();
                let new_revision = LayoutRevision::next();

                let mut slices = Vec::new();
                let mut static_patches = Vec::new();
                let new_cx = new_cursor_rect.as_ref().map(|c| c.x).unwrap_or(0.0);
                let new_cy = new_cursor_rect.as_ref().map(|c| c.top).unwrap_or(0.0);

                for (del_start, del_end) in &deleted_ranges {
                    for old_line in old_snapshot.lines_in_byte_range(*del_start, *del_end) {
                        if let Some(source_rect) = old_line.source_rect_for_byte_range(*del_start, *del_end) {
                            let from_doc = old_line.source_rect_to_document_rect(&source_rect);
                            slices.push(AnimatedSlice::delete_fade_out(
                                key,
                                old_line.id,
                                source_rect,
                                from_doc,
                                new_cx,
                                new_cy,
                                *del_start,
                                *del_end,
                                old_line.clusters.first().map(|c| c.shaping_identity.clone()),
                            ));
                        }
                    }
                }

                let delete_reflow_offset_map = OffsetMap::build(&vt.old_text, &vt.new_text);
                for new_line in &new_snapshot.line_snapshots {
                    let old_line = {
                        let mapped_old_byte_start = delete_reflow_offset_map.map_new_to_old(new_line.byte_start);
                        let mapped_old_byte_end = delete_reflow_offset_map.map_new_to_old(new_line.byte_end);
                        if let (Some(mobs), Some(mobe)) = (mapped_old_byte_start, mapped_old_byte_end) {
                            old_snapshot.line_for_byte_range(mobs, mobe).cloned()
                        } else if let Some(mobs) = mapped_old_byte_start {
                            old_snapshot.line_for_byte(mobs).cloned()
                        } else {
                            None
                        }
                    };
                    if let Some(ol) = old_line {
                        let old_sr = ol.source_rect_for_byte_range(ol.byte_start, ol.byte_end);
                        let new_sr = new_line.source_rect_for_byte_range(new_line.byte_start, new_line.byte_end);

                        match (old_sr, new_sr) {
                            (Some(old_src), Some(new_src)) => {
                                let same_shaping = ol.clusters.len() == new_line.clusters.len()
                                    && ol.clusters.iter()
                                        .zip(new_line.clusters.iter())
                                        .all(|(oc, nc)| oc.shaping_identity.is_same_shaping(&nc.shaping_identity));

                                let old_doc = ol.source_rect_to_document_rect(&old_src);
                                let new_doc = new_line.source_rect_to_document_rect(&new_src);

                                if same_shaping {
                                    slices.push(AnimatedSlice::reflow_move(
                                        key,
                                        ol.id,
                                        old_src,
                                        old_doc,
                                        new_line.id,
                                        new_src.clone(),
                                        new_doc,
                                        new_line.byte_start,
                                        new_line.byte_end,
                                        ol.clusters.first().map(|c| c.shaping_identity.clone()),
                                    ));
                                } else {
                                    slices.push(AnimatedSlice::reflow_crossfade_old(
                                        key,
                                        ol.id,
                                        old_src.clone(),
                                        old_doc.clone(),
                                        new_doc.clone(),
                                        new_line.byte_start,
                                        new_line.byte_end,
                                    ));
                                    slices.push(AnimatedSlice::reflow_crossfade_new(
                                        key,
                                        new_line.id,
                                        new_src.clone(),
                                        old_doc,
                                        new_doc,
                                        new_line.byte_start,
                                        new_line.byte_end,
                                    ));
                                }

                                static_patches.push(StaticLinePatch::reflow_patch(
                                    key,
                                    new_line.id,
                                    vec![new_src],
                                    new_line.byte_start,
                                    new_line.byte_end,
                                ));
                            }
                            _ => {}
                        }
                    }
                }

                let cursor_transition = if old_cursor_rect.is_some() && new_cursor_rect.is_some() {
                    CursorTransition::Tween {
                        old_rect: old_cursor_rect.clone().unwrap(),
                        new_rect: new_cursor_rect.clone().unwrap(),
                        duration_ms: vt.duration_ms,
                    }
                } else {
                    CursorTransition::Snap
                };

                let delete_offset_map = OffsetMap::build(&vt.old_text, &vt.new_text);
                let mut consumed_indices: Vec<usize> = Vec::new();
                for (bs, be, fx, fy, fo, ref shaping) in &rebase_frames {
                    if let Some(new_slice) = slices.iter_mut().find(|ns| ns.byte_start == *bs && ns.byte_end == *be) {
                        new_slice.rebase_from(*fx, *fy, *fo);
                    } else if let (Some(mbs), Some(mbe)) = (delete_offset_map.map_old_to_new(*bs), delete_offset_map.map_old_to_new(*be)) {
                        if let Some(new_slice) = slices.iter_mut().find(|ns| ns.byte_start == mbs && ns.byte_end == mbe) {
                            new_slice.rebase_from(*fx, *fy, *fo);
                        } else if let Some(ref sid) = shaping {
                            let mapped_center = (mbs + mbe) / 2;
                            let best = slices.iter_mut().enumerate()
                                .filter(|(idx, ns)| !consumed_indices.contains(idx))
                                .filter(|(_, ns)| ns.shaping_identity.as_ref() == Some(sid))
                                .filter(|(_, ns)| ns.byte_start >= mbs && ns.byte_end <= mbe.max(mbs + 1))
                                .min_by_key(|(_, ns)| (ns.byte_start + ns.byte_end) as i64 / 2 - mapped_center as i64);
                            if let Some((idx, new_slice)) = best {
                                new_slice.rebase_from(*fx, *fy, *fo);
                                consumed_indices.push(idx);
                            }
                        }
                    }
                }

                let prepared = PreparedTextVisualTransaction {
                    key,
                    state: TextVisualTransactionState::Pending,
                    operation_kind: TextVisualOperationKind::Delete,
                    animation_mode: mode,
                    timeline: TransactionTimeline::new(vt.duration_ms),
                    old_revision: self.layout_revision,
                    new_revision,
                    slices,
                    static_patches,
                    decoration_slices: Vec::new(),
                    cursor_transition,
                    old_cursor_rect: old_cursor_rect.clone(),
                    new_cursor_rect: new_cursor_rect.clone(),
                    cancel_reason: None,
                    texture_prepared: false,
                    old_snapshot: Some(old_snapshot.clone()),
                    new_snapshot: Some(new_snapshot.clone()),
                };

                self.layout_revision = new_revision;
                self.prepared_queue.enqueue(prepared);
                return Some(key);
            }
            EditorAnimationKind::Cursor => {
                let key = self.alloc_key();
                let new_revision = LayoutRevision::next();

                let cursor_transition = if old_cursor_rect.is_some() && new_cursor_rect.is_some() {
                    CursorTransition::Tween {
                        old_rect: old_cursor_rect.clone().unwrap(),
                        new_rect: new_cursor_rect.clone().unwrap(),
                        duration_ms: vt.duration_ms,
                    }
                } else {
                    CursorTransition::Snap
                };

                let prepared = PreparedTextVisualTransaction {
                    key,
                    state: TextVisualTransactionState::Pending,
                    operation_kind: TextVisualOperationKind::Cursor,
                    animation_mode: mode,
                    timeline: TransactionTimeline::new(vt.duration_ms),
                    old_revision: self.layout_revision,
                    new_revision,
                    slices: Vec::new(),
                    static_patches: Vec::new(),
                    decoration_slices: Vec::new(),
                    cursor_transition,
                    old_cursor_rect,
                    new_cursor_rect,
                    cancel_reason: None,
                    texture_prepared: false,
                    old_snapshot: None,
                    new_snapshot: None,
                };

                self.layout_revision = new_revision;
                self.prepared_queue.enqueue(prepared);
                return Some(key);
            }
        }
        None
    }

    pub fn handle_composition_update(
        &mut self,
        duration_ms: u64,
        old_snapshot: &EditorLayoutSnapshot,
        new_snapshot: &EditorLayoutSnapshot,
        composition_byte_start: usize,
        composition_byte_end: usize,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
    ) -> Option<VisualTransactionKey> {
        let conflicting = self.prepared_queue.find_conflicting_transaction(composition_byte_start, composition_byte_end);
        let rebase_frames: Vec<(usize, usize, f64, f64, f64, Option<ShapingIdentity>)> = if let Some(old_key) = conflicting {
            let mut frames = Vec::new();
            if let Some(old_tx) = self.prepared_queue.active_transactions().iter().find(|t| t.key == old_key) {
                let old_progress = old_tx.progress(Instant::now());
                if old_progress > 0.0 && old_progress < 1.0 {
                    for old_slice in &old_tx.slices {
                        let frame = old_slice.compute_frame(old_progress);
                        frames.push((old_slice.byte_start, old_slice.byte_end, frame.x, frame.y, frame.opacity, old_slice.shaping_identity.clone()));
                    }
                }
            }
            self.prepared_queue.cancel(old_key, "rebased");
            frames
        } else {
            Vec::new()
        };

        let offset_map = OffsetMap::build(&old_snapshot.virtual_text, &new_snapshot.virtual_text);

        let key = self.alloc_key();
        let new_revision = LayoutRevision::next();

        let cursor_transition = match (&old_cursor_rect, &new_cursor_rect) {
            (Some(old), Some(new)) => CursorTransition::Tween {
                old_rect: old.clone(),
                new_rect: new.clone(),
                duration_ms,
            },
            _ => CursorTransition::Snap,
        };

        let mut slices = Vec::new();
        let mut static_patches = Vec::new();
        let mut decoration_slices = Vec::new();

        let old_cx = old_cursor_rect.as_ref().map(|c| c.x).unwrap_or(0.0);
        let old_cy = old_cursor_rect.as_ref().map(|c| c.top).unwrap_or(0.0);

        for new_line in new_snapshot.lines_in_byte_range(composition_byte_start, composition_byte_end) {
            if let Some(source_rect) = new_line.source_rect_for_byte_range(composition_byte_start, composition_byte_end) {
                let to_doc = new_line.source_rect_to_document_rect(&source_rect);
                slices.push(AnimatedSlice::insert_fade_in(
                    key,
                    new_line.id,
                    source_rect.clone(),
                    to_doc,
                    old_cx,
                    old_cy,
                    composition_byte_start,
                    composition_byte_end,
                    new_line.clusters.first().map(|c| c.shaping_identity.clone()),
                ));

                static_patches.push(StaticLinePatch::insert_patch(
                    key,
                    new_line.id,
                    vec![source_rect],
                    composition_byte_start,
                    composition_byte_end,
                ));
            }

            decoration_slices.push(DecorationSlice::underline(
                key,
                composition_byte_start.max(new_line.byte_start),
                composition_byte_end.min(new_line.byte_end),
                new_line.visual_x,
                new_line.document_origin_y + new_line.line_height - 2.0,
                new_line.line_width,
                2.0,
                "#000000".to_string(),
            ));
        }

        let reflow_start = composition_byte_end;
        for new_line in &new_snapshot.line_snapshots {
            if new_line.byte_end <= composition_byte_end {
                continue;
            }
            if new_line.byte_start < composition_byte_end {
                continue;
            }

            let old_line = {
                let mapped_old_byte_start = offset_map.map_new_to_old(new_line.byte_start);
                let mapped_old_byte_end = offset_map.map_new_to_old(new_line.byte_end);
                let offset_matched = if let (Some(mobs), Some(mobe)) = (mapped_old_byte_start, mapped_old_byte_end) {
                    old_snapshot.line_for_byte_range(mobs, mobe).cloned()
                } else if let Some(mobs) = mapped_old_byte_start {
                    old_snapshot.line_for_byte(mobs).cloned()
                } else {
                    None
                };
                offset_matched
            };
            if let Some(ol) = old_line {
                let old_sr = ol.source_rect_for_byte_range(ol.byte_start, ol.byte_end);
                let new_sr = new_line.source_rect_for_byte_range(new_line.byte_start, new_line.byte_end);

                match (old_sr, new_sr) {
                    (Some(old_src), Some(new_src)) => {
                        let same_shaping = ol.clusters.len() == new_line.clusters.len()
                            && ol.clusters.iter()
                                .zip(new_line.clusters.iter())
                                .all(|(oc, nc)| oc.shaping_identity.is_same_shaping(&nc.shaping_identity));

                        let old_doc = ol.source_rect_to_document_rect(&old_src);
                        let new_doc = new_line.source_rect_to_document_rect(&new_src);

                        if same_shaping {
                            slices.push(AnimatedSlice::reflow_move(
                                key,
                                ol.id,
                                old_src,
                                old_doc,
                                new_line.id,
                                new_src.clone(),
                                new_doc,
                                new_line.byte_start,
                                new_line.byte_end,
                                ol.clusters.first().map(|c| c.shaping_identity.clone()),
                            ));
                        } else {
                            slices.push(AnimatedSlice::reflow_crossfade_old(
                                key,
                                ol.id,
                                old_src.clone(),
                                old_doc.clone(),
                                new_doc.clone(),
                                new_line.byte_start,
                                new_line.byte_end,
                            ));
                            slices.push(AnimatedSlice::reflow_crossfade_new(
                                key,
                                new_line.id,
                                new_src.clone(),
                                old_doc,
                                new_doc,
                                new_line.byte_start,
                                new_line.byte_end,
                            ));
                        }

                        static_patches.push(StaticLinePatch::reflow_patch(
                            key,
                            new_line.id,
                            vec![new_src],
                            new_line.byte_start,
                            new_line.byte_end,
                        ));
                    }
                    _ => {}
                }
            }
        }

        let mut consumed_indices: Vec<usize> = Vec::new();
        for (bs, be, fx, fy, fo, ref shaping) in &rebase_frames {
            if let Some(new_slice) = slices.iter_mut().find(|ns| ns.byte_start == *bs && ns.byte_end == *be) {
                new_slice.rebase_from(*fx, *fy, *fo);
            } else if let (Some(mbs), Some(mbe)) = (offset_map.map_old_to_new(*bs), offset_map.map_old_to_new(*be)) {
                if let Some(new_slice) = slices.iter_mut().find(|ns| ns.byte_start == mbs && ns.byte_end == mbe) {
                    new_slice.rebase_from(*fx, *fy, *fo);
                } else if let Some(ref sid) = shaping {
                    let mapped_center = (mbs + mbe) / 2;
                    let best = slices.iter_mut().enumerate()
                        .filter(|(idx, ns)| !consumed_indices.contains(idx))
                        .filter(|(_, ns)| ns.shaping_identity.as_ref() == Some(sid))
                        .filter(|(_, ns)| ns.byte_start >= mbs && ns.byte_end <= mbe.max(mbs + 1))
                        .min_by_key(|(_, ns)| (ns.byte_start + ns.byte_end) as i64 / 2 - mapped_center as i64);
                    if let Some((idx, new_slice)) = best {
                        new_slice.rebase_from(*fx, *fy, *fo);
                        consumed_indices.push(idx);
                    }
                }
            }
        }

        let prepared = PreparedTextVisualTransaction {
            key,
            state: TextVisualTransactionState::Pending,
            operation_kind: TextVisualOperationKind::CompositionUpdate,
            animation_mode: AnimationMode::GlyphAnimation,
            timeline: TransactionTimeline::new(duration_ms),
            old_revision: self.layout_revision,
            new_revision,
            slices,
            static_patches,
            decoration_slices,
            cursor_transition,
            old_cursor_rect,
            new_cursor_rect,
            cancel_reason: None,
            texture_prepared: false,
            old_snapshot: Some(old_snapshot.clone()),
            new_snapshot: Some(new_snapshot.clone()),
        };

        self.layout_revision = new_revision;
        self.prepared_queue.enqueue(prepared);
        Some(key)
    }

    pub fn handle_composition_commit_or_cancel(
        &mut self,
        duration_ms: u64,
        old_snapshot: &EditorLayoutSnapshot,
        new_snapshot: &EditorLayoutSnapshot,
        composition_byte_start: usize,
        composition_byte_end: usize,
        is_commit: bool,
        visual_text_unchanged: bool,
        candidate_byte_start: usize,
        candidate_byte_end: usize,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
    ) -> Option<VisualTransactionKey> {
        let conflicting = self.prepared_queue.find_conflicting_transaction(composition_byte_start, composition_byte_end);
        let rebase_frames: Vec<(usize, usize, f64, f64, f64, Option<ShapingIdentity>)> = if let Some(old_key) = conflicting {
            let mut frames = Vec::new();
            if let Some(old_tx) = self.prepared_queue.active_transactions().iter().find(|t| t.key == old_key) {
                let old_progress = old_tx.progress(Instant::now());
                if old_progress > 0.0 && old_progress < 1.0 {
                    for old_slice in &old_tx.slices {
                        let frame = old_slice.compute_frame(old_progress);
                        frames.push((old_slice.byte_start, old_slice.byte_end, frame.x, frame.y, frame.opacity, old_slice.shaping_identity.clone()));
                    }
                }
            }
            self.prepared_queue.cancel(old_key, "rebased");
            frames
        } else {
            Vec::new()
        };

        let offset_map = OffsetMap::build(&old_snapshot.virtual_text, &new_snapshot.virtual_text);

        let key = self.alloc_key();
        let new_revision = LayoutRevision::next();

        let cursor_transition = match (&old_cursor_rect, &new_cursor_rect) {
            (Some(old), Some(new)) => CursorTransition::Tween {
                old_rect: old.clone(),
                new_rect: new.clone(),
                duration_ms,
            },
            _ => CursorTransition::Snap,
        };

        let mut slices = Vec::new();
        let mut static_patches = Vec::new();

        if !is_commit {
            let shrink_x = new_cursor_rect.as_ref().map(|c| c.x).unwrap_or(0.0);
            let shrink_y = new_cursor_rect.as_ref().map(|c| c.top).unwrap_or(0.0);

            for old_line in old_snapshot.lines_in_byte_range(composition_byte_start, composition_byte_end) {
                if let Some(source_rect) = old_line.source_rect_for_byte_range(composition_byte_start, composition_byte_end) {
                    let from_doc = old_line.source_rect_to_document_rect(&source_rect);
                    slices.push(AnimatedSlice::delete_fade_out(
                        key,
                        old_line.id,
                        source_rect,
                        from_doc,
                        shrink_x,
                        shrink_y,
                        composition_byte_start,
                        composition_byte_end,
                        old_line.clusters.first().map(|c| c.shaping_identity.clone()),
                    ));
                }
            }
        } else {
            if visual_text_unchanged {
            } else {
                let insert_cx = old_cursor_rect.as_ref().map(|c| c.x).unwrap_or(0.0);
                let insert_cy = old_cursor_rect.as_ref().map(|c| c.top).unwrap_or(0.0);
                let shrink_x = new_cursor_rect.as_ref().map(|c| c.x).unwrap_or(0.0);
                let shrink_y = new_cursor_rect.as_ref().map(|c| c.top).unwrap_or(0.0);

                for old_line in old_snapshot.lines_in_byte_range(composition_byte_start, composition_byte_end) {
                    for old_cluster in old_line.clusters_in_byte_range(composition_byte_start, composition_byte_end) {
                        let mapped_new_bs = offset_map.map_old_to_new(old_cluster.byte_start);
                        let mapped_new_be = offset_map.map_old_to_new(old_cluster.byte_end);
                        let matched_in_new = if let (Some(mbs), Some(mbe)) = (mapped_new_bs, mapped_new_be) {
                            new_snapshot.line_snapshots.iter().any(|nl| {
                                nl.clusters.iter().any(|nc| nc.byte_start == mbs && nc.byte_end == mbe)
                            })
                        } else {
                            false
                        };
                        if !matched_in_new {
                            if let Some(old_sr) = old_line.source_rect_for_byte_range(old_cluster.byte_start, old_cluster.byte_end) {
                                let from_doc = old_line.source_rect_to_document_rect(&old_sr);
                                slices.push(AnimatedSlice::delete_fade_out(
                                    key,
                                    old_line.id,
                                    old_sr,
                                    from_doc,
                                    shrink_x,
                                    shrink_y,
                                    old_cluster.byte_start,
                                    old_cluster.byte_end,
                                    Some(old_cluster.shaping_identity.clone()),
                                ));
                            }
                        } else if let (Some(mbs), Some(mbe)) = (mapped_new_bs, mapped_new_be) {
                            if let Some((new_line, new_cluster)) = new_snapshot.line_snapshots.iter()
                                .filter_map(|nl| nl.clusters.iter()
                                    .find(|nc| nc.byte_start == mbs && nc.byte_end == mbe)
                                    .map(|nc| (nl, nc)))
                                .next()
                            {
                                if !old_cluster.shaping_identity.is_same_shaping(&new_cluster.shaping_identity) {
                                    if let Some(old_sr) = old_line.source_rect_for_byte_range(old_cluster.byte_start, old_cluster.byte_end) {
                                        let old_doc = old_line.source_rect_to_document_rect(&old_sr);
                                        if let Some(new_sr) = new_line.source_rect_for_byte_range(new_cluster.byte_start, new_cluster.byte_end) {
                                            let new_doc = new_line.source_rect_to_document_rect(&new_sr);
                                            slices.push(AnimatedSlice::reflow_crossfade_old(
                                                key,
                                                old_line.id,
                                                old_sr,
                                                old_doc,
                                                new_doc,
                                                old_cluster.byte_start,
                                                old_cluster.byte_end,
                                            ));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                for new_line in new_snapshot.lines_in_byte_range(candidate_byte_start, candidate_byte_end) {
                    for new_cluster in new_line.clusters_in_byte_range(candidate_byte_start, candidate_byte_end) {
                        let mapped_old_bs = offset_map.map_new_to_old(new_cluster.byte_start);
                        let mapped_old_be = offset_map.map_new_to_old(new_cluster.byte_end);
                        let found_in_old = if let (Some(mbs), Some(mbe)) = (mapped_old_bs, mapped_old_be) {
                            old_snapshot.line_snapshots.iter().any(|ol| {
                                ol.clusters.iter().any(|oc| oc.byte_start == mbs && oc.byte_end == mbe)
                            })
                        } else {
                            false
                        };
                        if !found_in_old {
                            if let Some(new_sr) = new_line.source_rect_for_byte_range(new_cluster.byte_start, new_cluster.byte_end) {
                                let to_doc = new_line.source_rect_to_document_rect(&new_sr);
                                slices.push(AnimatedSlice::insert_fade_in(
                                    key,
                                    new_line.id,
                                    new_sr.clone(),
                                    to_doc,
                                    insert_cx,
                                    insert_cy,
                                    new_cluster.byte_start,
                                    new_cluster.byte_end,
                                    Some(new_cluster.shaping_identity.clone()),
                                ));
                                static_patches.push(StaticLinePatch::insert_patch(
                                    key,
                                    new_line.id,
                                    vec![new_sr],
                                    new_cluster.byte_start,
                                    new_cluster.byte_end,
                                ));
                            }
                        } else if let (Some(mbs), Some(mbe)) = (mapped_old_bs, mapped_old_be) {
                            if let Some((old_line, old_cluster)) = old_snapshot.line_snapshots.iter()
                                .filter_map(|ol| ol.clusters.iter()
                                    .find(|oc| oc.byte_start == mbs && oc.byte_end == mbe)
                                    .map(|oc| (ol, oc)))
                                .next()
                            {
                                if !old_cluster.shaping_identity.is_same_shaping(&new_cluster.shaping_identity) {
                                    if let Some(new_sr) = new_line.source_rect_for_byte_range(new_cluster.byte_start, new_cluster.byte_end) {
                                        let old_doc = old_line.source_rect_to_document_rect(
                                            &old_line.source_rect_for_byte_range(old_cluster.byte_start, old_cluster.byte_end).unwrap_or(SourceRect::zero())
                                        );
                                        let new_doc = new_line.source_rect_to_document_rect(&new_sr);
                                        slices.push(AnimatedSlice::reflow_crossfade_new(
                                            key,
                                            new_line.id,
                                            new_sr,
                                            old_doc,
                                            new_doc,
                                            new_cluster.byte_start,
                                            new_cluster.byte_end,
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }

                let reflow_start = candidate_byte_end;
                for new_line in &new_snapshot.line_snapshots {
                    if new_line.byte_end <= reflow_start {
                        continue;
                    }
                    if new_line.byte_start < reflow_start {
                        continue;
                    }

                    let old_line = {
                        let mapped_old_byte_start = offset_map.map_new_to_old(new_line.byte_start);
                        let mapped_old_byte_end = offset_map.map_new_to_old(new_line.byte_end);
                        let offset_matched = if let (Some(mobs), Some(mobe)) = (mapped_old_byte_start, mapped_old_byte_end) {
                            old_snapshot.line_for_byte_range(mobs, mobe).cloned()
                        } else if let Some(mobs) = mapped_old_byte_start {
                            old_snapshot.line_for_byte(mobs).cloned()
                        } else {
                            None
                        };
                        offset_matched
                    };
                    if let Some(ol) = old_line {
                        let old_sr = ol.source_rect_for_byte_range(ol.byte_start, ol.byte_end);
                        let new_sr = new_line.source_rect_for_byte_range(new_line.byte_start, new_line.byte_end);

                        match (old_sr, new_sr) {
                            (Some(old_src), Some(new_src)) => {
                                let same_shaping = ol.clusters.len() == new_line.clusters.len()
                                    && ol.clusters.iter()
                                        .zip(new_line.clusters.iter())
                                        .all(|(oc, nc)| oc.shaping_identity.is_same_shaping(&nc.shaping_identity));

                                let old_doc = ol.source_rect_to_document_rect(&old_src);
                                let new_doc = new_line.source_rect_to_document_rect(&new_src);

                                if same_shaping {
                                    slices.push(AnimatedSlice::reflow_move(
                                        key,
                                        ol.id,
                                        old_src,
                                        old_doc,
                                        new_line.id,
                                        new_src.clone(),
                                        new_doc,
                                        new_line.byte_start,
                                        new_line.byte_end,
                                        ol.clusters.first().map(|c| c.shaping_identity.clone()),
                                    ));
                                } else {
                                    slices.push(AnimatedSlice::reflow_crossfade_old(
                                        key,
                                        ol.id,
                                        old_src.clone(),
                                        old_doc.clone(),
                                        new_doc.clone(),
                                        new_line.byte_start,
                                        new_line.byte_end,
                                    ));
                                    slices.push(AnimatedSlice::reflow_crossfade_new(
                                        key,
                                        new_line.id,
                                        new_src.clone(),
                                        old_doc,
                                        new_doc,
                                        new_line.byte_start,
                                        new_line.byte_end,
                                    ));
                                }

                                static_patches.push(StaticLinePatch::reflow_patch(
                                    key,
                                    new_line.id,
                                    vec![new_src],
                                    new_line.byte_start,
                                    new_line.byte_end,
                                ));
                            }
                            _ => {}
                        }
                    }
                }
            }
        }

        if !is_commit {
            for new_line in &new_snapshot.line_snapshots {
                let animated_byte_ranges: Vec<(usize, usize)> = slices.iter().map(|s| (s.byte_start, s.byte_end)).collect();
                let hidden_source_rects: Vec<SourceRect> = new_line
                    .clusters
                    .iter()
                    .filter(|c| animated_byte_ranges.iter().any(|(s, e)| !(c.byte_end <= *s || c.byte_start >= *e)))
                    .map(|c| c.source_rect.clone())
                    .collect();

                if !hidden_source_rects.is_empty() {
                    static_patches.push(StaticLinePatch::reflow_patch(
                        key,
                        new_line.id,
                        hidden_source_rects,
                        new_line.byte_start,
                        new_line.byte_end,
                    ));
                }
            }
        }

        let mut consumed_indices: Vec<usize> = Vec::new();
        for (bs, be, fx, fy, fo, ref shaping) in &rebase_frames {
            if let Some(new_slice) = slices.iter_mut().find(|ns| ns.byte_start == *bs && ns.byte_end == *be) {
                new_slice.rebase_from(*fx, *fy, *fo);
            } else if let (Some(mbs), Some(mbe)) = (offset_map.map_old_to_new(*bs), offset_map.map_old_to_new(*be)) {
                if let Some(new_slice) = slices.iter_mut().find(|ns| ns.byte_start == mbs && ns.byte_end == mbe) {
                    new_slice.rebase_from(*fx, *fy, *fo);
                } else if let Some(ref sid) = shaping {
                    let mapped_center = (mbs + mbe) / 2;
                    let best = slices.iter_mut().enumerate()
                        .filter(|(idx, ns)| !consumed_indices.contains(idx))
                        .filter(|(_, ns)| ns.shaping_identity.as_ref() == Some(sid))
                        .filter(|(_, ns)| ns.byte_start >= mbs && ns.byte_end <= mbe.max(mbs + 1))
                        .min_by_key(|(_, ns)| (ns.byte_start + ns.byte_end) as i64 / 2 - mapped_center as i64);
                    if let Some((idx, new_slice)) = best {
                        new_slice.rebase_from(*fx, *fy, *fo);
                        consumed_indices.push(idx);
                    }
                }
            }
        }

        let prepared = PreparedTextVisualTransaction {
            key,
            state: TextVisualTransactionState::Pending,
            operation_kind: TextVisualOperationKind::CompositionCommitOrCancel,
            animation_mode: AnimationMode::GlyphAnimation,
            timeline: TransactionTimeline::new(duration_ms),
            old_revision: self.layout_revision,
            new_revision,
            slices,
            static_patches,
            decoration_slices: Vec::new(),
            cursor_transition,
            old_cursor_rect,
            new_cursor_rect,
            cancel_reason: None,
            texture_prepared: false,
            old_snapshot: Some(old_snapshot.clone()),
            new_snapshot: Some(new_snapshot.clone()),
        };

        self.layout_revision = new_revision;
        self.prepared_queue.enqueue(prepared);
        Some(key)
    }

    pub fn has_active_composition(&self) -> bool {
        self.prepared_queue.active_transactions()
            .iter()
            .any(|t| t.is_composition()
                && t.state != TextVisualTransactionState::Cancelled
                && t.state != TextVisualTransactionState::Completed)
    }

    pub fn active_composition_new_snapshot(&self) -> Option<&EditorLayoutSnapshot> {
        self.prepared_queue.active_transactions()
            .iter()
            .filter(|t| t.operation_kind == TextVisualOperationKind::CompositionUpdate
                && t.state != TextVisualTransactionState::Cancelled
                && t.state != TextVisualTransactionState::Completed)
            .filter_map(|t| t.new_snapshot.as_ref())
            .last()
    }

    pub fn cancel_active_composition(&mut self, reason: &str) {
        let keys: Vec<VisualTransactionKey> = self.prepared_queue.active_transactions()
            .iter()
            .filter(|t| t.operation_kind == TextVisualOperationKind::CompositionUpdate
                && t.state != TextVisualTransactionState::Cancelled
                && t.state != TextVisualTransactionState::Completed)
            .map(|t| t.key)
            .collect();
        for key in keys {
            self.prepared_queue.cancel(key, reason);
        }
    }

    pub fn handle_cursor_only(
        &mut self,
        duration_ms: u64,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
    ) -> Option<VisualTransactionKey> {
        let key = self.alloc_key();
        let new_revision = LayoutRevision::next();

        let cursor_transition = match (&old_cursor_rect, &new_cursor_rect) {
            (Some(old), Some(new)) => CursorTransition::Tween {
                old_rect: old.clone(),
                new_rect: new.clone(),
                duration_ms,
            },
            _ => CursorTransition::Snap,
        };

        let prepared = PreparedTextVisualTransaction {
            key,
            state: TextVisualTransactionState::Pending,
            operation_kind: TextVisualOperationKind::Cursor,
            animation_mode: AnimationMode::GlyphAnimation,
            timeline: TransactionTimeline::new(duration_ms),
            old_revision: self.layout_revision,
            new_revision,
            slices: Vec::new(),
            static_patches: Vec::new(),
            decoration_slices: Vec::new(),
            cursor_transition,
            old_cursor_rect: old_cursor_rect.clone(),
            new_cursor_rect: new_cursor_rect.clone(),
            cancel_reason: None,
            texture_prepared: true,
            old_snapshot: None,
            new_snapshot: None,
        };

        self.layout_revision = new_revision;
        self.prepared_queue.enqueue(prepared);
        Some(key)
    }

    pub fn finish_by_key(&mut self, key: VisualTransactionKey) -> Option<Vec<LineSnapshotId>> {
        self.prepared_queue.complete(key)
    }

    pub fn cancel_by_key(&mut self, key: VisualTransactionKey, reason: &str) -> bool {
        self.prepared_queue.cancel(key, reason)
    }

    pub fn suppress_all(&mut self) -> bool {
        if self.prepared_queue.is_empty() {
            return false;
        }
        self.prepared_queue.cancel_all("suppress_all");
        true
    }

    pub fn tick(&mut self, now: Instant) -> bool {
        let expired = self.prepared_queue.tick(now);
        !expired.is_empty()
    }

    pub fn has_active_insert(&self) -> bool {
        self.prepared_queue.has_active_insert()
    }

    pub fn active_cursor_progress(&self) -> Option<f64> {
        let now = Instant::now();
        self.prepared_queue.active_transactions()
            .iter()
            .filter(|t| t.state != TextVisualTransactionState::Cancelled
                && t.state != TextVisualTransactionState::Completed
                && t.state != TextVisualTransactionState::Pending)
            .filter_map(|t| {
                let p = t.progress(now);
                if p > 0.0 && p < 1.0 { Some(p) } else { None }
            })
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    }

    pub fn is_empty(&self) -> bool {
        self.prepared_queue.is_empty()
    }

    fn collect_decoration_slices(&self) -> Vec<DecorationSlice> {
        self.prepared_queue.active_transactions()
            .iter()
            .filter(|t| t.state != TextVisualTransactionState::Cancelled && t.state != TextVisualTransactionState::Completed)
            .flat_map(|t| t.decoration_slices.clone())
            .collect()
    }

    pub fn has_prepared_or_rendering(&self) -> bool {
        self.prepared_queue.active_transactions()
            .iter()
            .any(|t| t.state == TextVisualTransactionState::Prepared || t.state == TextVisualTransactionState::Rendering)
    }

    pub fn current_static_render_plan(&self) -> StaticTextPlan {
        self.build_static_render_plan()
    }

    pub fn insert_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.prepared_queue.insert_byte_ranges()
    }

    pub fn reflow_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.prepared_queue.active_transactions()
            .iter()
            .filter(|t| t.state != TextVisualTransactionState::Cancelled && t.state != TextVisualTransactionState::Completed)
            .flat_map(|t| t.reflow_byte_ranges())
            .collect()
    }

    fn build_static_render_plan(&self) -> StaticTextPlan {
        let mut hidden_clip_rects = Vec::new();

        for tx in self.prepared_queue.active_transactions() {
            if tx.state == TextVisualTransactionState::Cancelled || tx.state == TextVisualTransactionState::Completed {
                continue;
            }
            if !tx.texture_prepared {
                continue;
            }
            for patch in &tx.static_patches {
                let snapshot = if let Some(ref snap) = tx.new_snapshot {
                    snap.line_snapshots.iter().find(|l| l.id == patch.snapshot_id)
                } else {
                    None
                };
                if let Some(line_snap) = snapshot {
                    for sr in &patch.hidden_source_rects {
                        let doc_x = sr.x / line_snap.dpr + line_snap.visual_x;
                        let doc_y = line_snap.document_origin_y + sr.y / line_snap.dpr;
                        let doc_w = sr.w / line_snap.dpr;
                        let doc_h = sr.h / line_snap.dpr;
                        hidden_clip_rects.push(HiddenClipRect {
                            key: tx.key,
                            x: doc_x,
                            y: doc_y,
                            w: doc_w,
                            h: doc_h,
                            byte_start: patch.byte_start,
                            byte_end: patch.byte_end,
                        });
                    }
                } else {
                    hidden_clip_rects.push(HiddenClipRect {
                        key: tx.key,
                        x: 0.0,
                        y: 0.0,
                        w: 0.0,
                        h: 0.0,
                        byte_start: patch.byte_start,
                        byte_end: patch.byte_end,
                    });
                }
            }
        }

        StaticTextPlan { hidden_clip_rects }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn build_cursor_plan(
        &self,
        old_cursor_rect: Option<CursorRect>,
        new_cursor_rect: Option<CursorRect>,
        cursor_x: f64,
        cursor_y: f64,
        cursor_h: f64,
        editor_enabled: bool,
        has_selection: bool,
        viewport_height: f64,
        is_scrolling: bool,
        is_selecting: bool,
        is_preediting: bool,
        smooth_cursor_enabled: bool,
        smooth_cursor_duration_ms: u32,
        coordinated_enabled: bool,
        scroll_y: f64,
        old_scroll_y: f64,
        old_visible: bool,
        old_blink_visible: bool,
        old_visual_x: f64,
        old_visual_y: f64,
        force_snap_next: bool,
        cursor_animation: Option<&super::rendering::CursorAnimationState>,
    ) -> CursorAnimationPlan {
        let in_viewport = cursor_y + cursor_h > 0.0 && cursor_y < viewport_height;
        let should_be_visible = editor_enabled && !has_selection && in_viewport && !is_scrolling;

        let has_active = self.has_active_insert();
        let blink_mode = if coordinated_enabled && has_active {
            CursorBlinkMode::Suppressed
        } else {
            CursorBlinkMode::Normal
        };

        let scroll_changed = (old_scroll_y - scroll_y).abs() > 0.01;
        let should_snap = is_scrolling || is_selecting || !old_visible || scroll_changed;

        let dx = (cursor_x - old_visual_x).abs();
        let dy = (cursor_y - old_visual_y).abs();

        let large_distance = force_snap_next && (dx > 80.0 || dy > cursor_h * 1.5);
        let cross_line_snap = dy > cursor_h * 3.0;

        let transition = if !should_be_visible {
            CursorTransition::Snap
        } else if should_snap || !smooth_cursor_enabled || large_distance || cross_line_snap {
            if coordinated_enabled
                && has_active
                && old_cursor_rect.is_some()
                && new_cursor_rect.is_some()
            {
                CursorTransition::Tween {
                    old_rect: old_cursor_rect.clone().unwrap(),
                    new_rect: new_cursor_rect.clone().unwrap(),
                    duration_ms: smooth_cursor_duration_ms as u64,
                }
            } else {
                CursorTransition::Snap
            }
        } else if let Some(anim) = cursor_animation {
            if (anim.target_x - cursor_x).abs() > 0.01
                || (anim.target_y - cursor_y).abs() > 0.01
            {
                if coordinated_enabled
                    && has_active
                    && old_cursor_rect.is_some()
                    && new_cursor_rect.is_some()
                {
                    CursorTransition::Tween {
                        old_rect: old_cursor_rect.clone().unwrap(),
                        new_rect: new_cursor_rect.clone().unwrap(),
                        duration_ms: smooth_cursor_duration_ms as u64,
                    }
                } else {
                    CursorTransition::Tween {
                        old_rect: CursorRect {
                            x: anim.start_x,
                            top: anim.start_y,
                            bottom: anim.start_y + cursor_h,
                            baseline_y: anim.start_y + cursor_h * 0.8,
                        },
                        new_rect: CursorRect {
                            x: cursor_x,
                            top: cursor_y,
                            bottom: cursor_y + cursor_h,
                            baseline_y: cursor_y + cursor_h * 0.8,
                        },
                        duration_ms: smooth_cursor_duration_ms as u64,
                    }
                }
            } else {
                CursorTransition::Snap
            }
        } else if (old_visual_x - cursor_x).abs() > 0.01
            || (old_visual_y - cursor_y).abs() > 0.01
        {
            if coordinated_enabled
                && has_active
                && old_cursor_rect.is_some()
                && new_cursor_rect.is_some()
            {
                CursorTransition::Tween {
                    old_rect: old_cursor_rect.clone().unwrap(),
                    new_rect: new_cursor_rect.clone().unwrap(),
                    duration_ms: smooth_cursor_duration_ms as u64,
                }
            } else {
                CursorTransition::Tween {
                    old_rect: CursorRect {
                        x: old_visual_x,
                        top: old_visual_y,
                        bottom: old_visual_y + cursor_h,
                        baseline_y: old_visual_y + cursor_h * 0.8,
                    },
                    new_rect: CursorRect {
                        x: cursor_x,
                        top: cursor_y,
                        bottom: cursor_y + cursor_h,
                        baseline_y: cursor_y + cursor_h * 0.8,
                    },
                    duration_ms: smooth_cursor_duration_ms as u64,
                }
            }
        } else {
            CursorTransition::Snap
        };

        let _ = (is_preediting, old_blink_visible);

        CursorAnimationPlan {
            should_be_visible,
            blink_mode,
            transition,
            cursor_x,
            cursor_y,
            cursor_h,
        }
    }

    pub(crate) fn build_ime_plan(
        &self,
        position_changed: bool,
        scroll_changed: bool,
    ) -> ImeUpdatePlan {
        ImeUpdatePlan {
            kind: if position_changed || scroll_changed {
                ImeUpdateKind::QueryInput
            } else {
                ImeUpdateKind::None
            },
            cursor_changed: position_changed,
            anchor_changed: false,
        }
    }

    fn active_transaction_duration_ms(&self) -> Option<u64> {
        self.prepared_queue.active_transactions()
            .iter()
            .filter(|t| t.state != TextVisualTransactionState::Cancelled && t.state != TextVisualTransactionState::Completed)
            .map(|t| t.duration_ms())
            .min()
    }

    pub(crate) fn pause_all(&mut self) {
        for tx in self.prepared_queue.active_transactions_mut() {
            tx.pause();
        }
    }

    pub(crate) fn resume_all(&mut self) {
        for tx in self.prepared_queue.active_transactions_mut() {
            tx.resume();
        }
    }

    pub(crate) fn build_render_plan_full(
        &mut self,
        cursor_plan: CursorAnimationPlan,
        ime_plan: ImeUpdatePlan,
        selection_preedit: SelectionPreeditPlan,
        mut frame_context: super::render_plan::FrameContext,
        cursor_style: super::render_plan::CursorStyle,
    ) -> RenderPlan {
        let static_text = self.build_static_render_plan();
        let (text_animation, keys_to_complete) = self.build_text_animation_plan();
        let decorations = self.collect_decoration_slices();
        frame_context.keys_to_complete = keys_to_complete;
        let active_keys: Vec<VisualTransactionKey> = self.prepared_queue.active_transactions()
            .iter().map(|t| t.key).collect();
        frame_context.active_transaction_keys = active_keys;
        RenderPlan {
            static_text,
            text_animation,
            selection_preedit,
            decorations,
            cursor: cursor_plan,
            ime: ime_plan,
            frame_context,
            cursor_style,
        }
    }

    fn build_text_animation_plan(&mut self) -> (TextAnimationPlan, Vec<VisualTransactionKey>) {
        let mut glyphs = Vec::new();
        let mut keys_to_complete = Vec::new();
        let now = Instant::now();

        for tx in self.prepared_queue.active_transactions_mut() {
            if tx.state == TextVisualTransactionState::Cancelled || tx.state == TextVisualTransactionState::Completed {
                continue;
            }

            if tx.state == TextVisualTransactionState::Prepared {
                tx.state = TextVisualTransactionState::Rendering;
                if !tx.timeline.is_started() {
                    tx.timeline.mark_first_frame();
                }
            }

            let progress = tx.progress(now);

            if progress >= 1.0 {
                keys_to_complete.push(tx.key);
                continue;
            }

            for slice in &tx.slices {
                let frame = slice.compute_frame(progress);
                glyphs.push(TextAnimationGlyphInfo {
                    key: tx.key,
                    x: frame.x,
                    y: frame.y,
                    w: frame.w,
                    h: frame.h,
                    opacity: frame.opacity,
                    animation_mode: tx.animation_mode,
                    is_delete: tx.is_delete(),
                    snapshot_id: frame.snapshot_id,
                    source_rect: frame.source_rect,
                });
            }
        }

        (TextAnimationPlan { glyphs }, keys_to_complete)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordinator_suppress_all() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        assert!(!coord.has_active_insert());
        let suppressed = coord.suppress_all();
        assert!(!suppressed);
    }

    #[test]
    fn test_coordinator_finish_by_key() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let key = VisualTransactionKey::new(1, 1);
        let removed = coord.finish_by_key(key);
        assert!(removed.is_none());
    }

    #[test]
    fn test_ime_plan_position_changed() {
        let coord = LinuxEditorAnimationCoordinator::new();
        let plan = coord.build_ime_plan(true, false);
        assert_eq!(plan.kind, ImeUpdateKind::QueryInput);
        assert!(plan.cursor_changed);
    }

    #[test]
    fn test_ime_plan_no_change() {
        let coord = LinuxEditorAnimationCoordinator::new();
        let plan = coord.build_ime_plan(false, false);
        assert_eq!(plan.kind, ImeUpdateKind::None);
    }

    #[test]
    fn test_animation_mode_system_suppressed_no_transaction() {
        let mode = AnimationMode::SystemSuppressed;
        assert!(!mode.should_create_transaction());
    }

    #[test]
    fn test_animation_mode_glyph_creates_transaction() {
        let mode = AnimationMode::GlyphAnimation;
        assert!(mode.should_create_transaction());
    }

    #[test]
    fn test_rebase_uses_offset_map_and_shaping_identity() {
        let sid_a = ShapingIdentity { text_content_hash: 1, raw_font_fingerprint: "font_a".to_string(), glyph_indexes_hash: 10, cluster_glyph_count: 3, direction_rtl: false, format_fingerprint: 100 };
        let sid_b = ShapingIdentity { text_content_hash: 2, raw_font_fingerprint: "font_b".to_string(), glyph_indexes_hash: 20, cluster_glyph_count: 2, direction_rtl: false, format_fingerprint: 200 };

        let rebase_frames: Vec<(usize, usize, f64, f64, f64, Option<ShapingIdentity>)> = vec![
            (10, 20, 100.0, 200.0, 0.5, Some(sid_a.clone())),
            (30, 40, 150.0, 250.0, 0.7, Some(sid_b.clone())),
        ];

        let mut slices = vec![
            AnimatedSlice::insert_fade_in(
                VisualTransactionKey::new(1, 1),
                LineSnapshotId::new(1, 0, 0),
                SourceRect { x: 0.0, y: 0.0, w: 100.0, h: 20.0 },
                SourceRect { x: 0.0, y: 0.0, w: 100.0, h: 20.0 },
                0.0, 0.0, 50, 60,
                Some(sid_a.clone()),
            ),
            AnimatedSlice::insert_fade_in(
                VisualTransactionKey::new(1, 2),
                LineSnapshotId::new(1, 0, 1),
                SourceRect { x: 0.0, y: 20.0, w: 100.0, h: 20.0 },
                SourceRect { x: 0.0, y: 20.0, w: 100.0, h: 20.0 },
                0.0, 0.0, 70, 80,
                Some(sid_b.clone()),
            ),
        ];

        let offset_map = OffsetMap::build("old_text_with_padding", "new_text_with_padding");
        let mut consumed_indices: Vec<usize> = Vec::new();
        for (bs, be, fx, fy, fo, ref shaping) in &rebase_frames {
            if let Some(new_slice) = slices.iter_mut().find(|ns| ns.byte_start == *bs && ns.byte_end == *be) {
                new_slice.rebase_from(*fx, *fy, *fo);
            } else if let (Some(mbs), Some(mbe)) = (offset_map.map_old_to_new(*bs), offset_map.map_old_to_new(*be)) {
                if let Some(new_slice) = slices.iter_mut().find(|ns| ns.byte_start == mbs && ns.byte_end == mbe) {
                    new_slice.rebase_from(*fx, *fy, *fo);
                } else if let Some(ref sid) = shaping {
                    let mapped_center = (mbs + mbe) / 2;
                    let best = slices.iter_mut().enumerate()
                        .filter(|(idx, ns)| !consumed_indices.contains(idx))
                        .filter(|(_, ns)| ns.shaping_identity.as_ref() == Some(sid))
                        .filter(|(_, ns)| ns.byte_start >= mbs && ns.byte_end <= mbe.max(mbs + 1))
                        .min_by_key(|(_, ns)| (ns.byte_start + ns.byte_end) as i64 / 2 - mapped_center as i64);
                    if let Some((idx, new_slice)) = best {
                        new_slice.rebase_from(*fx, *fy, *fo);
                        consumed_indices.push(idx);
                    }
                }
            }
        }

        assert!((slices[0].from_document_rect.x - 0.0).abs() < 0.01);
        assert!((slices[1].from_document_rect.x - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_rebase_tier3_closest_position_match_with_duplicate_shaping() {
        let sid_dup = ShapingIdentity { text_content_hash: 99, raw_font_fingerprint: "font_x".to_string(), glyph_indexes_hash: 50, cluster_glyph_count: 1, direction_rtl: false, format_fingerprint: 500 };

        let rebase_frames: Vec<(usize, usize, f64, f64, f64, Option<ShapingIdentity>)> = vec![
            (5, 10, 10.0, 100.0, 0.3, Some(sid_dup.clone())),
            (15, 20, 20.0, 200.0, 0.5, Some(sid_dup.clone())),
            (25, 30, 30.0, 300.0, 0.7, Some(sid_dup.clone())),
        ];

        let mut slices = vec![
            AnimatedSlice::insert_fade_in(
                VisualTransactionKey::new(1, 1),
                LineSnapshotId::new(1, 0, 0),
                SourceRect { x: 0.0, y: 0.0, w: 100.0, h: 20.0 },
                SourceRect { x: 0.0, y: 0.0, w: 100.0, h: 20.0 },
                0.0, 0.0, 5, 10,
                Some(sid_dup.clone()),
            ),
            AnimatedSlice::insert_fade_in(
                VisualTransactionKey::new(1, 2),
                LineSnapshotId::new(1, 0, 1),
                SourceRect { x: 0.0, y: 20.0, w: 100.0, h: 20.0 },
                SourceRect { x: 0.0, y: 20.0, w: 100.0, h: 20.0 },
                0.0, 0.0, 15, 20,
                Some(sid_dup.clone()),
            ),
            AnimatedSlice::insert_fade_in(
                VisualTransactionKey::new(1, 3),
                LineSnapshotId::new(1, 0, 2),
                SourceRect { x: 0.0, y: 40.0, w: 100.0, h: 20.0 },
                SourceRect { x: 0.0, y: 40.0, w: 100.0, h: 20.0 },
                0.0, 0.0, 25, 30,
                Some(sid_dup.clone()),
            ),
        ];

        let offset_map = OffsetMap::build("same_text_for_identity", "same_text_for_identity");
        let mut consumed_indices: Vec<usize> = Vec::new();
        for (bs, be, fx, fy, fo, ref shaping) in &rebase_frames {
            if let Some(new_slice) = slices.iter_mut().find(|ns| ns.byte_start == *bs && ns.byte_end == *be) {
                new_slice.rebase_from(*fx, *fy, *fo);
            } else if let (Some(mbs), Some(mbe)) = (offset_map.map_old_to_new(*bs), offset_map.map_old_to_new(*be)) {
                if let Some(new_slice) = slices.iter_mut().find(|ns| ns.byte_start == mbs && ns.byte_end == mbe) {
                    new_slice.rebase_from(*fx, *fy, *fo);
                } else if let Some(ref sid) = shaping {
                    let mapped_center = (mbs + mbe) / 2;
                    let best = slices.iter_mut().enumerate()
                        .filter(|(idx, ns)| !consumed_indices.contains(idx))
                        .filter(|(_, ns)| ns.shaping_identity.as_ref() == Some(sid))
                        .filter(|(_, ns)| ns.byte_start >= mbs && ns.byte_end <= mbe.max(mbs + 1))
                        .min_by_key(|(_, ns)| (ns.byte_start + ns.byte_end) as i64 / 2 - mapped_center as i64);
                    if let Some((idx, new_slice)) = best {
                        new_slice.rebase_from(*fx, *fy, *fo);
                        consumed_indices.push(idx);
                    }
                }
            }
        }

        assert!((slices[0].from_document_rect.x - 10.0).abs() < 0.01, "slice 0 should get rebase frame 10.0, got {}", slices[0].from_document_rect.x);
        assert!((slices[1].from_document_rect.x - 20.0).abs() < 0.01, "slice 1 should get rebase frame 20.0, got {}", slices[1].from_document_rect.x);
        assert!((slices[2].from_document_rect.x - 30.0).abs() < 0.01, "slice 2 should get rebase frame 30.0, got {}", slices[2].from_document_rect.x);
    }
}
