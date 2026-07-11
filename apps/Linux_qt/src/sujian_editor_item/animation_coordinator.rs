use std::time::Instant;

use serde::Serialize;
use writer_core::editor::{
    AnimationMode as CoreAnimationMode, CursorRect, EditorAnimationKind, EditorVisualTransaction,
    GlyphRect, ReflowGlyphRect,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VisualTransactionKey {
    pub transaction_id: u64,
    pub generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum AnimationMode {
    GlyphAnimation,
    ClusterAnimation,
    RunAnimation,
    LineReflowAnimation,
    SnapshotAnimation,
    SystemSuppressed,
}

impl AnimationMode {
    pub(crate) fn from_core(mode: CoreAnimationMode) -> Self {
        match mode {
            CoreAnimationMode::GlyphAnimation => AnimationMode::GlyphAnimation,
            CoreAnimationMode::ClusterAnimation => AnimationMode::ClusterAnimation,
            CoreAnimationMode::RunAnimation => AnimationMode::RunAnimation,
            CoreAnimationMode::LineReflowAnimation => AnimationMode::LineReflowAnimation,
            CoreAnimationMode::SnapshotAnimation | CoreAnimationMode::SystemSuppressed => {
                AnimationMode::SystemSuppressed
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum OverlayAnimationKind {
    Insert,
    Delete,
    Cursor,
}

impl From<EditorAnimationKind> for OverlayAnimationKind {
    fn from(k: EditorAnimationKind) -> Self {
        match k {
            EditorAnimationKind::Insert => OverlayAnimationKind::Insert,
            EditorAnimationKind::Delete => OverlayAnimationKind::Delete,
            EditorAnimationKind::Cursor => OverlayAnimationKind::Cursor,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextAnimationKind {
    Insert,
    Delete,
}

#[derive(Clone, Debug)]
pub(crate) struct AnimationRangeEntry {
    pub key: VisualTransactionKey,
    pub core_range_id: Option<u64>,
    pub kind: TextAnimationKind,
    pub byte_range: (usize, usize),
    pub reflow_hidden_ranges: Vec<ReflowHiddenRangeEntry>,
    pub animation_mode: AnimationMode,
    pub start_time: Instant,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReflowHiddenRangeEntry {
    pub byte_range: (usize, usize),
}

// =============================================================================
// ActiveVisualTransactionQueue — Rust-owned animation transaction queue
// =============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VisualTransactionState {
    Pending,
    Prepared,
    Rendering,
    Completed,
    Cancelled,
}

#[derive(Clone, Debug)]
pub(crate) struct ActiveVisualTransaction {
    pub key: VisualTransactionKey,
    pub state: VisualTransactionState,
    pub kind: OverlayAnimationKind,
    pub animation_mode: AnimationMode,
    pub duration_ms: u64,
    pub start_time: Instant,
    pub insert_glyph_rects: Vec<GlyphRect>,
    pub deleted_glyph_rects: Vec<GlyphRect>,
    pub reflow_glyph_rects: Vec<ReflowGlyphRect>,
    pub inserted_range: Option<(usize, usize)>,
    pub old_cursor_rect: Option<CursorRect>,
    pub new_cursor_rect: Option<CursorRect>,
    pub cancel_reason: Option<String>,
    pub texture_prepared: bool,
}

impl ActiveVisualTransaction {
    pub fn transaction_id(&self) -> u64 {
        self.key.transaction_id
    }

    pub fn generation(&self) -> u64 {
        self.key.generation
    }
}

pub(crate) struct ActiveVisualTransactionQueue {
    transactions: Vec<ActiveVisualTransaction>,
}

impl ActiveVisualTransactionQueue {
    pub fn new() -> Self {
        Self {
            transactions: Vec::new(),
        }
    }

    pub fn enqueue(
        &mut self,
        key: VisualTransactionKey,
        vt: &EditorVisualTransaction,
        animation_mode: AnimationMode,
    ) {
        let kind = vt.kind.into();
        self.transactions.push(ActiveVisualTransaction {
            key,
            state: VisualTransactionState::Prepared,
            kind,
            animation_mode,
            duration_ms: vt.duration_ms,
            start_time: Instant::now(),
            insert_glyph_rects: vt.insert_glyph_rects.clone().unwrap_or_default(),
            deleted_glyph_rects: vt.deleted_glyph_rects.clone().unwrap_or_default(),
            reflow_glyph_rects: vt.reflow_glyph_rects.clone().unwrap_or_default(),
            inserted_range: vt.inserted_range,
            old_cursor_rect: vt.old_cursor_rect.clone(),
            new_cursor_rect: vt.new_cursor_rect.clone(),
            cancel_reason: None,
            texture_prepared: false,
        });

        super::editor_animation_debug_log(&format!(
            "VTQueue::enqueue: tid={}, gen={}, kind={:?}, mode={:?}, duration_ms={}",
            key.transaction_id, key.generation, kind, animation_mode, vt.duration_ms
        ));
    }

    pub fn mark_rendering(&mut self, key: VisualTransactionKey) {
        if let Some(tx) = self.transactions.iter_mut().find(|t| t.key == key) {
            tx.state = VisualTransactionState::Rendering;
        }
    }

    pub fn mark_texture_prepared(&mut self, key: VisualTransactionKey) {
        if let Some(tx) = self.transactions.iter_mut().find(|t| t.key == key) {
            tx.texture_prepared = true;
        }
    }

    pub fn complete(&mut self, key: VisualTransactionKey) -> bool {
        let before = self.transactions.len();
        self.transactions.retain(|t| t.key != key);
        let removed = self.transactions.len() < before;
        if removed {
            super::editor_animation_debug_log(&format!(
                "VTQueue::complete: tid={}, gen={}",
                key.transaction_id, key.generation
            ));
        }
        removed
    }

    pub fn cancel(&mut self, key: VisualTransactionKey, reason: &str) -> bool {
        let before = self.transactions.len();
        self.transactions.retain(|t| t.key != key);
        let removed = self.transactions.len() < before;
        if removed {
            super::editor_animation_debug_log(&format!(
                "VTQueue::cancel: tid={}, gen={}, reason={}",
                key.transaction_id, key.generation, reason
            ));
        }
        removed
    }

    pub fn cancel_all(&mut self, reason: &str) {
        if !self.transactions.is_empty() {
            super::editor_animation_debug_log(&format!(
                "VTQueue::cancel_all: count={}, reason={}",
                self.transactions.len(),
                reason
            ));
        }
        self.transactions.clear();
    }

    pub fn active_transactions(&self) -> &[ActiveVisualTransaction] {
        &self.transactions
    }

    pub fn has_active(&self) -> bool {
        !self.transactions.is_empty()
    }

    pub fn tick(&mut self, now: Instant) -> bool {
        if self.transactions.is_empty() {
            return false;
        }
        let before = self.transactions.len();
        self.transactions.retain(|t| {
            let elapsed = now.duration_since(t.start_time).as_millis() as u64;
            let timeout = t.duration_ms * 3 + 500;
            if elapsed > timeout {
                super::editor_animation_debug_log(&format!(
                    "VTQueue::tick: expired tid={}, gen={}, elapsed={}ms, timeout={}ms",
                    t.key.transaction_id, t.key.generation, elapsed, timeout
                ));
                false
            } else {
                true
            }
        });
        self.transactions.len() != before
    }

    pub fn is_empty(&self) -> bool {
        self.transactions.is_empty()
    }
}

pub(crate) struct AnimationRangeRegistry {
    entries: Vec<AnimationRangeEntry>,
}

impl AnimationRangeRegistry {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn start_insert(
        &mut self,
        key: VisualTransactionKey,
        core_range_id: Option<u64>,
        byte_range: (usize, usize),
        reflow_byte_ranges: Vec<(usize, usize)>,
        animation_mode: AnimationMode,
        duration_ms: u64,
    ) {
        let reflow_hidden_ranges = reflow_byte_ranges
            .into_iter()
            .map(|br| ReflowHiddenRangeEntry {
                byte_range: br,
            })
            .collect();
        self.entries.push(AnimationRangeEntry {
            key,
            core_range_id,
            kind: TextAnimationKind::Insert,
            byte_range,
            reflow_hidden_ranges,
            animation_mode,
            start_time: Instant::now(),
            duration_ms,
        });
    }

    pub fn start_delete(
        &mut self,
        key: VisualTransactionKey,
        byte_range: (usize, usize),
        animation_mode: AnimationMode,
        duration_ms: u64,
    ) {
        self.entries.push(AnimationRangeEntry {
            key,
            core_range_id: None,
            kind: TextAnimationKind::Delete,
            byte_range,
            reflow_hidden_ranges: Vec::new(),
            animation_mode,
            start_time: Instant::now(),
            duration_ms,
        });
    }

    pub fn finish_by_key(&mut self, key: VisualTransactionKey) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.key != key);
        self.entries.len() < before
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn tick(&mut self, now: Instant) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let before = self.entries.len();
        self.entries.retain(|e| {
            let elapsed = now.duration_since(e.start_time).as_millis() as u64;
            elapsed < e.duration_ms * 2 + 200
        });
        self.entries.len() != before
    }

    pub fn has_active_insert(&self) -> bool {
        self.entries
            .iter()
            .any(|e| e.kind == TextAnimationKind::Insert)
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn insert_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.entries
            .iter()
            .filter(|e| e.kind == TextAnimationKind::Insert)
            .map(|e| e.byte_range)
            .collect()
    }

    pub fn reflow_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.entries
            .iter()
            .filter(|e| e.kind == TextAnimationKind::Insert)
            .flat_map(|e| e.reflow_hidden_ranges.iter().map(|r| r.byte_range))
            .collect()
    }

    pub fn map_ranges_for_insert(&mut self, pos: usize, len: usize) {
        let new_ranges: Vec<Option<(usize, usize)>> = self
            .entries
            .iter()
            .map(|e| map_range_for_insert(e.byte_range, pos, len))
            .collect();

        for (i, e) in self.entries.iter_mut().enumerate() {
            if let Some(nr) = new_ranges[i] {
                e.byte_range = nr;
            }
        }
        let mut i = 0;
        self.entries.retain(|_| {
            let keep = new_ranges[i].is_some();
            i += 1;
            keep
        });

        for e in &mut self.entries {
            e.reflow_hidden_ranges = e
                .reflow_hidden_ranges
                .iter()
                .filter_map(|r| {
                    map_range_for_insert(r.byte_range, pos, len).map(|br| ReflowHiddenRangeEntry {
                        byte_range: br,
                    })
                })
                .collect();
        }
    }

    pub fn map_ranges_for_delete(&mut self, pos: usize, len: usize) {
        let new_ranges: Vec<Option<(usize, usize)>> = self
            .entries
            .iter()
            .map(|e| map_range_for_delete(e.byte_range, pos, len))
            .collect();

        for (i, e) in self.entries.iter_mut().enumerate() {
            if let Some(nr) = new_ranges[i] {
                e.byte_range = nr;
            }
        }
        let mut i = 0;
        self.entries.retain(|_| {
            let keep = new_ranges[i].is_some();
            i += 1;
            keep
        });

        for e in &mut self.entries {
            e.reflow_hidden_ranges = e
                .reflow_hidden_ranges
                .iter()
                .filter_map(|r| {
                    map_range_for_delete(r.byte_range, pos, len).map(|br| ReflowHiddenRangeEntry {
                        byte_range: br,
                    })
                })
                .collect();
        }
    }

    pub fn entries_with_texture_prepared(&self) -> Vec<&AnimationRangeEntry> {
        self.entries
            .iter()
            .filter(|e| e.kind == TextAnimationKind::Insert)
            .collect()
    }
}

fn map_range_for_insert(range: (usize, usize), pos: usize, len: usize) -> Option<(usize, usize)> {
    if pos <= range.0 {
        Some((range.0 + len, range.1 + len))
    } else if pos >= range.1 {
        Some(range)
    } else {
        None
    }
}

fn map_range_for_delete(range: (usize, usize), pos: usize, len: usize) -> Option<(usize, usize)> {
    let delete_end = pos + len;
    if delete_end <= range.0 {
        Some((range.0 - len, range.1 - len))
    } else if pos >= range.1 {
        Some(range)
    } else {
        None
    }
}

// =============================================================================
// Plans — read-only outputs from the coordinator
// =============================================================================

#[derive(Clone, Debug)]
pub(crate) struct HiddenRangeInfo {
    pub key: VisualTransactionKey,
    pub byte_range: (usize, usize),
}

#[derive(Clone, Debug, Default)]
pub(crate) struct StaticTextRenderPlan {
    pub hidden_ranges: Vec<HiddenRangeInfo>,
}

impl StaticTextRenderPlan {
    pub fn merged_byte_ranges(&self) -> Vec<(usize, usize)> {
        let mut all: Vec<(usize, usize)> = self
            .hidden_ranges
            .iter()
            .map(|r| r.byte_range)
            .collect();
        all.sort_by_key(|r| r.0);
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for (rs, re) in all {
            if let Some(last) = merged.last_mut() {
                if rs <= last.1 {
                    last.1 = last.1.max(re);
                    continue;
                }
            }
            merged.push((rs, re));
        }
        merged
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OverlayGlyphInfo {
    pub key: VisualTransactionKey,
    pub duration_ms: u64,
    pub glyph_rects: Vec<GlyphRect>,
    pub deleted_glyph_rects: Vec<GlyphRect>,
    pub reflow_glyph_rects: Vec<ReflowGlyphRect>,
    pub old_cursor_rect: Option<CursorRect>,
    pub new_cursor_rect: Option<CursorRect>,
    pub animation_mode: AnimationMode,
    pub inserted_range: Option<(usize, usize)>,
    pub kind: OverlayAnimationKind,
    pub cluster_rects: Option<Vec<writer_core::editor::ClusterRect>>,
    pub cluster_runs: Option<Vec<writer_core::editor::ClusterRun>>,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OverlayAnimationPlan {
    pub entries: Vec<OverlayGlyphInfo>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CursorBlinkMode {
    Normal,
    Suppressed,
}

#[derive(Clone, Debug)]
pub(crate) enum CursorTransition {
    Snap,
    Tween {
        old_rect: CursorRect,
        new_rect: CursorRect,
        duration_ms: u64,
    },
}

#[derive(Clone, Debug)]
pub(crate) struct CursorAnimationPlan {
    pub should_be_visible: bool,
    pub blink_mode: CursorBlinkMode,
    pub transition: CursorTransition,
    pub cursor_x: f64,
    pub cursor_y: f64,
    pub cursor_h: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImeUpdateKind {
    None,
    QueryInput,
}

#[derive(Clone, Debug)]
pub(crate) struct ImeUpdatePlan {
    pub kind: ImeUpdateKind,
    pub cursor_changed: bool,
    pub anchor_changed: bool,
}

// =============================================================================
// CoordinatorOutput — all four plans produced atomically
// =============================================================================

#[derive(Clone, Debug)]
pub(crate) struct CoordinatorOutput {
    pub static_render_plan: StaticTextRenderPlan,
    pub overlay_plan: OverlayAnimationPlan,
    pub cursor_plan: CursorAnimationPlan,
    pub ime_plan: ImeUpdatePlan,
}

// =============================================================================
// LinuxEditorAnimationCoordinator
// =============================================================================

pub(crate) struct LinuxEditorAnimationCoordinator {
    registry: AnimationRangeRegistry,
    next_key_id: u64,
    pub(crate) vt_queue: ActiveVisualTransactionQueue,
}

impl LinuxEditorAnimationCoordinator {
    pub fn new() -> Self {
        Self {
            registry: AnimationRangeRegistry::new(),
            next_key_id: 1,
            vt_queue: ActiveVisualTransactionQueue::new(),
        }
    }

    fn alloc_key(&mut self) -> VisualTransactionKey {
        let id = self.next_key_id;
        self.next_key_id += 1;
        VisualTransactionKey {
            transaction_id: id,
            generation: id,
        }
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
        cursor_x: f64,
        cursor_y: f64,
        cursor_h: f64,
        editor_enabled: bool,
        has_selection: bool,
        viewport_height: f64,
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
    ) -> CoordinatorOutput {
        if typing_animation_enabled && !is_scrolling {
            match vt.kind {
                EditorAnimationKind::Insert => {
                    if let Some((range_start, range_end)) = vt.inserted_range {
                        let insert_len = range_end - range_start;
                        let had_active_before = self.registry.has_active_insert();
                        self.registry.map_ranges_for_insert(range_start, insert_len);
                        if had_active_before && !self.registry.has_active_insert() {
                        }
                        let mut mode = AnimationMode::from_core(vt.animation_mode);
                        if is_scrolling
                            || is_loading
                            || is_applying_format
                            || is_applying_settings
                            || !typing_animation_enabled
                        {
                            mode = AnimationMode::SystemSuppressed;
                        }
                        if mode != AnimationMode::SystemSuppressed {
                            let key = self.alloc_key();
                            let reflow_ranges: Vec<(usize, usize)> = vt
                                .reflow_glyph_rects
                                .as_ref()
                                .map(|rects| {
                                    rects.iter().map(|r| (r.byte_start, r.byte_end)).collect()
                                })
                                .unwrap_or_default();
                            let core_range_id =
                                vt.hidden_visual_ranges.first().map(|r| r.id);
                            self.registry.start_insert(
                                key,
                                core_range_id,
                                (range_start, range_end),
                                reflow_ranges,
                                mode,
                                vt.duration_ms,
                            );
                            self.vt_queue.enqueue(key, vt, mode);
                        }
                    }
                }
                EditorAnimationKind::Delete => {
                    let mut mode = AnimationMode::from_core(vt.animation_mode);
                    if is_scrolling
                        || is_loading
                        || is_applying_format
                        || is_applying_settings
                        || !typing_animation_enabled
                    {
                        mode = AnimationMode::SystemSuppressed;
                    }
                    if mode != AnimationMode::SystemSuppressed {
                        let key = self.alloc_key();
                        let changes =
                            writer_core::editor::diff_plain_text(&vt.old_text, &vt.new_text);
                        for change in &changes {
                            if let writer_core::editor::EditorChange::Delete { index, text } =
                                change
                            {
                                let range_start = *index;
                                let delete_len = text.len();
                                self.registry.map_ranges_for_delete(range_start, delete_len);
                            }
                        }
                        for change in &changes {
                            if let writer_core::editor::EditorChange::Delete { index, text } =
                                change
                            {
                                let range_start = *index;
                                let range_end = range_start + text.len();
                                self.registry.start_delete(
                                    key,
                                    (range_start, range_end),
                                    mode,
                                    vt.duration_ms,
                                );
                            }
                        }
                        self.vt_queue.enqueue(key, vt, mode);
                    }
                }
                EditorAnimationKind::Cursor => {}
            }
        }

        self.build_output(
            old_cursor_rect,
            new_cursor_rect,
            cursor_x,
            cursor_y,
            cursor_h,
            editor_enabled,
            has_selection,
            viewport_height,
            is_scrolling,
            is_selecting,
            is_preediting,
            smooth_cursor_enabled,
            smooth_cursor_duration_ms,
            coordinated_enabled,
            scroll_y,
            old_scroll_y,
            old_visible,
            old_blink_visible,
            old_visual_x,
            old_visual_y,
            force_snap_next,
            cursor_animation,
        )
    }

    pub fn finish_by_key(&mut self, key: VisualTransactionKey) -> bool {
        let registry_result = self.registry.finish_by_key(key);
        if registry_result {
            self.vt_queue.complete(key);
        }
        registry_result
    }

    pub fn suppress_all(&mut self) -> bool {
        if self.registry.is_empty() && self.vt_queue.is_empty() {
            return false;
        }
        self.registry.clear();
        self.vt_queue.cancel_all("suppress_all");
        true
    }

    pub fn tick(&mut self, now: Instant) -> bool {
        let registry_changed = self.registry.tick(now);
        let queue_changed = self.vt_queue.tick(now);
        registry_changed || queue_changed
    }

    pub fn has_active_insert(&self) -> bool {
        self.registry.has_active_insert()
    }

    pub fn is_empty(&self) -> bool {
        self.registry.is_empty()
    }

    pub fn current_static_render_plan(&self) -> StaticTextRenderPlan {
        self.build_static_render_plan()
    }

    pub fn insert_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.registry.insert_byte_ranges()
    }

    pub fn reflow_byte_ranges(&self) -> Vec<(usize, usize)> {
        self.registry.reflow_byte_ranges()
    }

    fn build_static_render_plan(&self) -> StaticTextRenderPlan {
        let mut hidden_ranges = Vec::new();

        for e in self.registry.entries.iter() {
            if e.kind == TextAnimationKind::Insert {
                let tx = self.vt_queue.active_transactions().iter().find(|t| t.key == e.key);
                let texture_prepared = tx.map(|t| t.texture_prepared).unwrap_or(false);
                if !texture_prepared {
                    continue;
                }
                hidden_ranges.push(HiddenRangeInfo {
                    key: e.key,
                    byte_range: e.byte_range,
                });
                for r in &e.reflow_hidden_ranges {
                    hidden_ranges.push(HiddenRangeInfo {
                        key: e.key,
                        byte_range: r.byte_range,
                    });
                }
            }
        }

        StaticTextRenderPlan { hidden_ranges }
    }

    fn build_overlay_plan(&self, vt: &EditorVisualTransaction) -> OverlayAnimationPlan {
        let mut entries = Vec::new();

        if vt.kind == EditorAnimationKind::Insert || vt.kind == EditorAnimationKind::Delete {
            let mode = AnimationMode::from_core(vt.animation_mode);
            if mode != AnimationMode::SystemSuppressed {
                let key = self.vt_queue.active_transactions().iter()
                    .find(|t| t.kind == vt.kind.into()
                        && t.inserted_range == vt.inserted_range
                        && t.duration_ms == vt.duration_ms)
                    .map(|t| t.key)
                    .unwrap_or(VisualTransactionKey { transaction_id: 0, generation: 0 });

                entries.push(OverlayGlyphInfo {
                    key,
                    duration_ms: vt.duration_ms,
                    glyph_rects: vt.insert_glyph_rects.clone().unwrap_or_default(),
                    deleted_glyph_rects: vt.deleted_glyph_rects.clone().unwrap_or_default(),
                    reflow_glyph_rects: vt.reflow_glyph_rects.clone().unwrap_or_default(),
                    old_cursor_rect: vt.old_cursor_rect.clone(),
                    new_cursor_rect: vt.new_cursor_rect.clone(),
                    animation_mode: mode,
                    inserted_range: vt.inserted_range,
                    kind: vt.kind.into(),
                    cluster_rects: vt.cluster_rects.clone(),
                    cluster_runs: vt.cluster_runs.clone(),
                });
            }
        }

        OverlayAnimationPlan { entries }
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

        let has_active = self.registry.has_active_insert();
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
        let safety_snap = dy > cursor_h * 3.0;

        let transition = if !should_be_visible {
            CursorTransition::Snap
        } else if should_snap || !smooth_cursor_enabled || large_distance || safety_snap {
            if coordinated_enabled
                && has_active
                && old_cursor_rect.is_some()
                && new_cursor_rect.is_some()
            {
                CursorTransition::Tween {
                    old_rect: old_cursor_rect.unwrap(),
                    new_rect: new_cursor_rect.unwrap(),
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
                        old_rect: old_cursor_rect.unwrap(),
                        new_rect: new_cursor_rect.unwrap(),
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
                    old_rect: old_cursor_rect.unwrap(),
                    new_rect: new_cursor_rect.unwrap(),
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

    #[allow(clippy::too_many_arguments)]
    fn build_output(
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
    ) -> CoordinatorOutput {
        let static_render_plan = self.build_static_render_plan();
        let cursor_plan = self.build_cursor_plan(
            old_cursor_rect,
            new_cursor_rect,
            cursor_x,
            cursor_y,
            cursor_h,
            editor_enabled,
            has_selection,
            viewport_height,
            is_scrolling,
            is_selecting,
            is_preediting,
            smooth_cursor_enabled,
            smooth_cursor_duration_ms,
            coordinated_enabled,
            scroll_y,
            old_scroll_y,
            old_visible,
            old_blink_visible,
            old_visual_x,
            old_visual_y,
            force_snap_next,
            cursor_animation,
        );

        let position_changed = (old_visual_x - cursor_x).abs() > 0.01
            || (old_visual_y - cursor_y).abs() > 0.01;
        let scroll_changed = (old_scroll_y - scroll_y).abs() > 0.01;
        let ime_plan = self.build_ime_plan(position_changed, scroll_changed);

        CoordinatorOutput {
            static_render_plan,
            overlay_plan: OverlayAnimationPlan::default(),
            cursor_plan,
            ime_plan,
        }
    }

    pub fn build_overlay_plan_for_vt(
        &mut self,
        vt: &EditorVisualTransaction,
    ) -> OverlayAnimationPlan {
        self.build_overlay_plan(vt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn make_key(id: u64) -> VisualTransactionKey {
        VisualTransactionKey { transaction_id: id, generation: id }
    }

    #[test]
    fn test_registry_insert_creates_hidden_range() {
        let mut reg = AnimationRangeRegistry::new();
        reg.start_insert(make_key(1), None, (10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        assert_eq!(reg.insert_byte_ranges(), vec![(10, 20)]);
        assert!(reg.has_active_insert());
        assert!(!reg.is_empty());
    }

    #[test]
    fn test_registry_delete_no_hidden_range() {
        let mut reg = AnimationRangeRegistry::new();
        reg.start_delete(make_key(1), (5, 15), AnimationMode::GlyphAnimation, 100);
        assert!(reg.insert_byte_ranges().is_empty());
        assert!(!reg.has_active_insert());
        assert!(!reg.is_empty());
    }

    #[test]
    fn test_registry_clear() {
        let mut reg = AnimationRangeRegistry::new();
        reg.start_insert(make_key(1), None, (10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        reg.start_delete(make_key(2), (30, 40), AnimationMode::GlyphAnimation, 100);
        assert!(!reg.is_empty());
        reg.clear();
        assert!(reg.is_empty());
    }

    #[test]
    fn test_registry_finish_by_key() {
        let mut reg = AnimationRangeRegistry::new();
        let key = make_key(1);
        reg.start_insert(key, None, (10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        assert!(reg.has_active_insert());
        let removed = reg.finish_by_key(key);
        assert!(removed);
        assert!(reg.is_empty());
    }

    #[test]
    fn test_registry_finish_wrong_key_no_effect() {
        let mut reg = AnimationRangeRegistry::new();
        reg.start_insert(make_key(1), None, (10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        let removed = reg.finish_by_key(make_key(999));
        assert!(!removed);
        assert!(reg.has_active_insert());
    }

    #[test]
    fn test_registry_timeout() {
        let mut reg = AnimationRangeRegistry::new();
        reg.start_insert(make_key(1), None, (10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        let now = Instant::now() + Duration::from_millis(401);
        assert!(reg.tick(now));
        assert!(reg.is_empty());
    }

    #[test]
    fn test_registry_map_ranges_for_insert() {
        let mut reg = AnimationRangeRegistry::new();
        reg.start_insert(make_key(1), None, (10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        reg.map_ranges_for_insert(5, 3);
        assert_eq!(reg.insert_byte_ranges(), vec![(13, 23)]);
    }

    #[test]
    fn test_registry_map_ranges_insert_inside_cancels() {
        let mut reg = AnimationRangeRegistry::new();
        reg.start_insert(make_key(1), None, (10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        reg.map_ranges_for_insert(15, 3);
        assert!(reg.is_empty());
    }

    #[test]
    fn test_registry_map_ranges_for_delete() {
        let mut reg = AnimationRangeRegistry::new();
        reg.start_insert(make_key(1), None, (10, 20), vec![], AnimationMode::GlyphAnimation, 100);
        reg.map_ranges_for_delete(5, 3);
        assert_eq!(reg.insert_byte_ranges(), vec![(7, 17)]);
    }

    #[test]
    fn test_registry_reflow_ranges() {
        let mut reg = AnimationRangeRegistry::new();
        reg.start_insert(make_key(1), None, (10, 20), vec![(20, 25), (25, 30)], AnimationMode::GlyphAnimation, 100);
        assert_eq!(reg.reflow_byte_ranges(), vec![(20, 25), (25, 30)]);
    }

    #[test]
    fn test_concurrent_inserts_different_keys() {
        let mut reg = AnimationRangeRegistry::new();
        reg.start_insert(make_key(1), None, (5, 6), vec![], AnimationMode::GlyphAnimation, 100);
        reg.start_insert(make_key(2), None, (15, 16), vec![], AnimationMode::GlyphAnimation, 100);
        assert_eq!(reg.insert_byte_ranges().len(), 2);
        let removed = reg.finish_by_key(make_key(2));
        assert!(removed);
        assert!(reg.has_active_insert());
        assert_eq!(reg.insert_byte_ranges(), vec![(5, 6)]);
    }

    #[test]
    fn test_visual_transaction_key_uniqueness() {
        let key1 = make_key(1);
        let key2 = make_key(2);
        assert_ne!(key1, key2);
        let key1_dup = make_key(1);
        assert_eq!(key1, key1_dup);
    }

    #[test]
    fn test_static_render_plan_merged_byte_ranges() {
        let plan = StaticTextRenderPlan {
            hidden_ranges: vec![
                HiddenRangeInfo {
                    key: make_key(1),
                    byte_range: (10, 20),
                },
                HiddenRangeInfo {
                    key: make_key(1),
                    byte_range: (18, 25),
                },
                HiddenRangeInfo {
                    key: make_key(2),
                    byte_range: (30, 40),
                },
            ],
        };
        let merged = plan.merged_byte_ranges();
        assert_eq!(merged, vec![(10, 25), (30, 40)]);
    }

    #[test]
    fn test_cursor_plan_coordinated_tween() {
        let coord = LinuxEditorAnimationCoordinator::new();
        let plan = coord.build_cursor_plan(
            Some(CursorRect { x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
            Some(CursorRect { x: 50.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
            50.0, 5.0, 20.0,
            true, false, 1000.0,
            false, false, false,
            true, 120,
            true,
            0.0, 0.0,
            true, true,
            10.0, 5.0,
            false, None,
        );
        assert!(plan.should_be_visible);
        assert_eq!(plan.blink_mode, CursorBlinkMode::Normal);
    }

    #[test]
    fn test_cursor_plan_coordinated_suppressed_blink() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.registry.start_insert(make_key(1), None, (5, 6), vec![], AnimationMode::GlyphAnimation, 100);

        let plan = coord.build_cursor_plan(
            Some(CursorRect { x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
            Some(CursorRect { x: 50.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
            50.0, 5.0, 20.0,
            true, false, 1000.0,
            false, false, false,
            true, 120,
            true,
            0.0, 0.0,
            true, true,
            10.0, 5.0,
            false, None,
        );
        assert!(plan.should_be_visible);
        assert_eq!(plan.blink_mode, CursorBlinkMode::Suppressed);
    }

    #[test]
    fn test_cursor_plan_not_visible_when_disabled() {
        let coord = LinuxEditorAnimationCoordinator::new();
        let plan = coord.build_cursor_plan(
            None, None,
            50.0, 5.0, 20.0,
            false, false, 1000.0,
            false, false, false,
            true, 120,
            true,
            0.0, 0.0,
            false, true,
            50.0, 5.0,
            false, None,
        );
        assert!(!plan.should_be_visible);
    }

    #[test]
    fn test_coordinator_suppress_all() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.registry.start_insert(make_key(1), None, (5, 6), vec![], AnimationMode::GlyphAnimation, 100);
        assert!(coord.has_active_insert());
        let suppressed = coord.suppress_all();
        assert!(suppressed);
        assert!(!coord.has_active_insert());
        assert!(coord.is_empty());
    }

    #[test]
    fn test_coordinator_finish_by_key() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let key = make_key(1);
        coord.registry.start_insert(key, None, (5, 6), vec![], AnimationMode::GlyphAnimation, 100);
        let removed = coord.finish_by_key(key);
        assert!(removed);
        assert!(coord.is_empty());
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
    fn test_multiple_reflow_ranges_independent_lifecycle() {
        let mut reg = AnimationRangeRegistry::new();
        let key = make_key(1);
        reg.start_insert(key, None, (5, 10), vec![(10, 15), (15, 20)], AnimationMode::GlyphAnimation, 100);
        assert_eq!(reg.reflow_byte_ranges(), vec![(10, 15), (15, 20)]);
        reg.finish_by_key(key);
        assert!(reg.reflow_byte_ranges().is_empty());
    }

    #[test]
    fn test_map_ranges_insert_before_reflow() {
        let mut reg = AnimationRangeRegistry::new();
        reg.start_insert(make_key(1), None, (10, 20), vec![(20, 25), (25, 30)], AnimationMode::GlyphAnimation, 100);
        reg.map_ranges_for_insert(5, 3);
        assert_eq!(reg.insert_byte_ranges(), vec![(13, 23)]);
        assert_eq!(reg.reflow_byte_ranges(), vec![(23, 28), (28, 33)]);
    }

    #[test]
    fn test_map_ranges_delete_before_reflow() {
        let mut reg = AnimationRangeRegistry::new();
        reg.start_insert(make_key(1), None, (10, 20), vec![(20, 25), (25, 30)], AnimationMode::GlyphAnimation, 100);
        reg.map_ranges_for_delete(5, 3);
        assert_eq!(reg.insert_byte_ranges(), vec![(7, 17)]);
        assert_eq!(reg.reflow_byte_ranges(), vec![(17, 22), (22, 27)]);
    }

    #[test]
    fn test_map_ranges_insert_inside_reflow_removes_that_reflow() {
        let mut reg = AnimationRangeRegistry::new();
        reg.start_insert(make_key(1), None, (10, 20), vec![(20, 25), (25, 30)], AnimationMode::GlyphAnimation, 100);
        reg.map_ranges_for_insert(22, 3);
        assert_eq!(reg.insert_byte_ranges(), vec![(10, 20)]);
        assert_eq!(reg.reflow_byte_ranges(), vec![(28, 33)]);
    }

    #[test]
    fn test_coordinated_tween_uses_visual_transaction_cursor_rects() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.registry.start_insert(make_key(1), None, (5, 6), vec![], AnimationMode::GlyphAnimation, 100);

        let plan = coord.build_cursor_plan(
            Some(CursorRect { x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
            Some(CursorRect { x: 50.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
            50.0, 5.0, 20.0,
            true, false, 1000.0,
            false, false, false,
            true, 120,
            true,
            0.0, 0.0,
            true, true,
            10.0, 5.0,
            false, None,
        );
        assert!(plan.should_be_visible);
        assert_eq!(plan.blink_mode, CursorBlinkMode::Suppressed);
        match plan.transition {
            CursorTransition::Tween { old_rect, new_rect, duration_ms } => {
                assert_eq!(old_rect.x, 10.0);
                assert_eq!(new_rect.x, 50.0);
                assert_eq!(duration_ms, 120);
            }
            CursorTransition::Snap => panic!("expected Tween, got Snap"),
        }
    }

    #[test]
    fn test_coordinated_false_snaps_even_with_active_insert() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.registry.start_insert(make_key(1), None, (5, 6), vec![], AnimationMode::GlyphAnimation, 100);

        let plan = coord.build_cursor_plan(
            Some(CursorRect { x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
            Some(CursorRect { x: 50.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
            50.0, 5.0, 20.0,
            true, false, 1000.0,
            false, false, false,
            false, 0,
            false,
            0.0, 0.0,
            true, true,
            50.0, 5.0,
            false, None,
        );
        assert!(plan.should_be_visible);
        assert_eq!(plan.blink_mode, CursorBlinkMode::Normal);
        assert!(matches!(plan.transition, CursorTransition::Snap));
    }

    #[test]
    fn test_scroll_suppresses_insert_animation() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let vt = EditorVisualTransaction {
            id: 1,
            kind: EditorAnimationKind::Insert,
            cause: writer_core::editor::EditorTransactionCause::Typing,
            old_text: "a".into(),
            new_text: "ab".into(),
            old_selection: writer_core::editor::EditorSelection::collapsed("a", 1),
            new_selection: writer_core::editor::EditorSelection::collapsed("ab", 2),
            inserted_range: Some((1, 2)),
            deleted_glyph_rects: None,
            insert_glyph_rects: Some(vec![]),
            reflow_glyph_rects: None,
            animation_mode: CoreAnimationMode::GlyphAnimation,
            cluster_rects: None,
            cluster_runs: None,
            hidden_visual_ranges: vec![],
            old_cursor_rect: None,
            new_cursor_rect: None,
            duration_ms: 160,
            coordinate_mode: writer_core::editor::VisualCoordinateMode::Baseline,
        };
        let output = coord.process_transaction(
            &vt,
            true,
            true,
            false, false, false,
            None, None,
            50.0, 5.0, 20.0,
            true, false, 1000.0,
            false, false, true,
            120,
            true,
            0.0, 0.0,
            true, true,
            50.0, 5.0,
            false, None,
        );
        assert!(coord.is_empty());
        assert!(output.static_render_plan.hidden_ranges.is_empty());
        assert!(output.overlay_plan.entries.is_empty());
    }

    #[test]
    fn test_loading_suppresses_insert_animation() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let vt = EditorVisualTransaction {
            id: 1,
            kind: EditorAnimationKind::Insert,
            cause: writer_core::editor::EditorTransactionCause::Typing,
            old_text: "a".into(),
            new_text: "ab".into(),
            old_selection: writer_core::editor::EditorSelection::collapsed("a", 1),
            new_selection: writer_core::editor::EditorSelection::collapsed("ab", 2),
            inserted_range: Some((1, 2)),
            deleted_glyph_rects: None,
            insert_glyph_rects: Some(vec![]),
            reflow_glyph_rects: None,
            animation_mode: CoreAnimationMode::GlyphAnimation,
            cluster_rects: None,
            cluster_runs: None,
            hidden_visual_ranges: vec![],
            old_cursor_rect: None,
            new_cursor_rect: None,
            duration_ms: 160,
            coordinate_mode: writer_core::editor::VisualCoordinateMode::Baseline,
        };
        let output = coord.process_transaction(
            &vt,
            true,
            false,
            true, false, false,
            None, None,
            50.0, 5.0, 20.0,
            true, false, 1000.0,
            false, false, true,
            120,
            true,
            0.0, 0.0,
            true, true,
            50.0, 5.0,
            false, None,
        );
        assert!(coord.is_empty());
        assert!(output.overlay_plan.entries.is_empty());
    }

    #[test]
    fn test_ime_plan_independent_of_cursor_blink() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.registry.start_insert(make_key(1), None, (5, 6), vec![], AnimationMode::GlyphAnimation, 100);

        let cursor_plan = coord.build_cursor_plan(
            Some(CursorRect { x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
            Some(CursorRect { x: 50.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
            50.0, 5.0, 20.0,
            true, false, 1000.0,
            true, false, false,
            true, 120,
            true,
            0.0, 0.0,
            true, true,
            10.0, 5.0,
            false, None,
        );
        assert_eq!(cursor_plan.blink_mode, CursorBlinkMode::Suppressed);

        let ime_plan = coord.build_ime_plan(true, false);
        assert_eq!(ime_plan.kind, ImeUpdateKind::QueryInput);
        assert!(ime_plan.cursor_changed);
    }

    #[test]
    fn test_ime_plan_no_update_when_position_unchanged() {
        let coord = LinuxEditorAnimationCoordinator::new();
        let ime_plan = coord.build_ime_plan(false, false);
        assert_eq!(ime_plan.kind, ImeUpdateKind::None);
        assert!(!ime_plan.cursor_changed);
    }

    #[test]
    fn test_key_atomic_cleanup_finish_by_key() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let key = coord.alloc_key();
        coord.registry.start_insert(key, None, (5, 6), vec![], AnimationMode::GlyphAnimation, 100);
        let vt = EditorVisualTransaction {
            id: 1,
            kind: EditorAnimationKind::Insert,
            cause: writer_core::editor::EditorTransactionCause::Typing,
            old_text: "a".into(),
            new_text: "ab".into(),
            old_selection: writer_core::editor::EditorSelection::collapsed("a", 1),
            new_selection: writer_core::editor::EditorSelection::collapsed("ab", 2),
            inserted_range: Some((1, 2)),
            deleted_glyph_rects: None,
            insert_glyph_rects: Some(vec![]),
            reflow_glyph_rects: None,
            animation_mode: CoreAnimationMode::GlyphAnimation,
            cluster_rects: None,
            cluster_runs: None,
            hidden_visual_ranges: vec![],
            old_cursor_rect: None,
            new_cursor_rect: None,
            duration_ms: 160,
            coordinate_mode: writer_core::editor::VisualCoordinateMode::Baseline,
        };
        coord.vt_queue.enqueue(key, &vt, AnimationMode::GlyphAnimation);

        assert!(coord.has_active_insert());
        assert!(coord.vt_queue.has_active());

        let removed = coord.finish_by_key(key);
        assert!(removed);
        assert!(coord.is_empty());
        assert!(coord.vt_queue.is_empty());
    }
}
