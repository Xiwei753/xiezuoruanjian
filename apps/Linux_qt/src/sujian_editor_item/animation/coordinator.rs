//! Linux Qt 正文动画协调器 — Issue #826 四层模型。
//!
//! 本协调器**不再**拥有 prepared transaction 队列、rebase、carried unit、ingest
//! stage 或 caret ownership。正文动画只有两样东西：
//!
//! - [`EditFrontierState`]：唯一的遮罩前沿，控制「本轮改掉的字现在露出多少」。
//! - [`ReflowState`]：独立的 Reflow 层，控制「没改的字移动到哪」。
//!
//! 光标动画由 `cursor_animation.rs` / `cursor_controller.rs` 自己拥有；IME preedit
//! 由 `ime_visual.rs` 自己拥有。四层互相不拥有对方状态。
//!
//! 普通正文编辑的完整流程（见 `pipeline::prepare_edit_motion`）：
//! 1. Core 立即提交正文（正文永远是最新真实内容）；
//! 2. 读 `display_patches`；
//! 3. 算最新 canonical layout；
//! 4. 判断 Insert / Delete / Replace；
//! 5. 能并入当前连续编辑就更新唯一前沿 / Reflow，否则结束当前再开新的。

use std::collections::HashSet;
use std::time::Instant;

use writer_core::editor::OffsetMap;

use crate::sujian_editor_item::animation::edit_frontier::{
    EditFrontierKind, EditFrontierSample, EditFrontierState, FrontierGlyph,
};
use crate::sujian_editor_item::animation::reflow_motion::{ReflowSpanFrame, ReflowState};
use crate::sujian_editor_item::cursor_animation::{
    CursorAnimationPlan, CursorBlinkMode, CursorTransition,
};
use crate::sujian_editor_item::edit_motion::{CursorRect, EditorAnimationKind};
use crate::sujian_editor_item::editor_animation_debug_log;
use crate::sujian_editor_item::layout_snapshot::{EditorLayoutSnapshot, LineSnapshotId};

/// Issue #826: 一轮正文编辑的完整事实，由 `pipeline` 从 Core `display_patches`
/// + old/new canonical layout 派生后交给协调器。
pub(crate) struct EditFrontierRequest {
    /// 从 patch 事实派生的动画类别（`CursorOnly` 不进这里）。
    pub kind: EditorAnimationKind,
    /// 连续 burst 开始前的旧正文（Delete / Replace 的 overlay 用）。
    pub base_snapshot: EditorLayoutSnapshot,
    /// 当前最新正文。
    pub target_snapshot: EditorLayoutSnapshot,
    /// 旧正文坐标系里被删掉的全部范围。
    pub deleted_ranges: Vec<(usize, usize)>,
    /// 最新正文坐标系里新增的全部范围。
    pub inserted_ranges: Vec<(usize, usize)>,
    /// 前沿起点：本轮改动开始前的位置。
    pub start_frontier: CursorRect,
    /// 前沿最新目标：本轮改动结束后的位置。
    pub target_frontier: CursorRect,
    /// old → new 的偏移映射（Reflow 层用）。
    pub offset_map: OffsetMap,
    /// 本帧时间。
    pub now: Instant,
}

/// Issue #826: 光标逻辑位置与视觉位置的解耦输入。
#[derive(Clone, Copy, Debug)]
pub(crate) struct CursorMoveInputs {
    /// 当前 caret 的文档坐标 x。
    pub cursor_x: f64,
    /// 当前 caret 的文档坐标 y。
    pub cursor_y: f64,
    /// 当前 caret 高度。
    pub cursor_h: f64,
    pub editor_enabled: bool,
    pub has_selection: bool,
    pub is_scrolling: bool,
    pub selection_gesture_active: bool,
    pub is_preediting: bool,
    pub smooth_cursor_enabled: bool,
    pub duration_ms: u64,
    pub visual_x: f64,
    pub visual_y: f64,
    pub force_snap_next: bool,
    pub baseline_y: f64,
}

pub(crate) struct LinuxEditorAnimationCoordinator {
    /// Issue #826: 唯一的遮罩前沿。连续同方向编辑只更新这一个对象。
    pub(crate) active_edit_frontier: Option<EditFrontierState>,
    /// Issue #826: 独立的 Reflow 层。与前沿共享同一次 old/new layout，状态独立。
    pub(crate) active_reflow: Option<ReflowState>,
    /// 打字动画时长（毫秒）。
    pub(crate) typing_animation_duration_ms: u32,
    /// 光标平滑移动动画时长（毫秒）。
    pub(crate) cursor_animation_duration_ms: u32,
    /// 窗口失焦 / 加载等场景下暂停正文动画的时间点。
    paused_at: Option<Instant>,
}

impl LinuxEditorAnimationCoordinator {
    pub fn new() -> Self {
        Self {
            active_edit_frontier: None,
            active_reflow: None,
            typing_animation_duration_ms: 160,
            cursor_animation_duration_ms: 120,
            paused_at: None,
        }
    }

    pub(crate) fn set_typing_animation_duration_ms(&mut self, ms: u32) {
        self.typing_animation_duration_ms = ms;
    }

    pub(crate) fn set_cursor_animation_duration_ms(&mut self, ms: u32) {
        self.cursor_animation_duration_ms = ms;
    }

    // ── 遮罩前沿 ─────────────────────────────────────────────────────────────

    /// Issue #826: 普通正文编辑的唯一入口。
    ///
    /// - 能并入当前连续编辑（同种类 + 未结束）→ 先采样当前前沿当新起点，只更新
    ///   最新 target，**不生成第二个历史动画对象**。
    /// - 否则先 `finish_edit_frontier_to_canonical()` 收掉当前，再开新的。
    pub(crate) fn begin_or_extend_edit_frontier(&mut self, request: EditFrontierRequest) {
        let kind = match request.kind {
            EditorAnimationKind::Insert => EditFrontierKind::Insert,
            EditorAnimationKind::Delete => EditFrontierKind::Delete,
            EditorAnimationKind::Replace => EditFrontierKind::Replace,
            // CursorOnly 没有正文改动，不进前沿，也不重开 Reflow。
            EditorAnimationKind::CursorOnly => return,
        };
        let duration_ms = u64::from(self.typing_animation_duration_ms);

        // 连续同方向编辑才并入：种类相同，且当前前沿还没走完（走完了就是上一笔动画
        // 已经结束，必须开新的）。
        let can_extend = self
            .active_edit_frontier
            .as_ref()
            .map(|frontier| frontier.kind.can_extend(kind) && !frontier.is_finished(request.now))
            .unwrap_or(false);

        if can_extend {
            let frontier = self
                .active_edit_frontier
                .as_mut()
                .expect("can_extend 为真时 active_edit_frontier 必然存在");
            match kind {
                EditFrontierKind::Insert => {
                    let range = first_range(&request.inserted_ranges);
                    if let Some(range) = range {
                        frontier.extend_insert(
                            request.target_snapshot.clone(),
                            range,
                            request.target_frontier,
                            request.now,
                        );
                    }
                }
                EditFrontierKind::Delete => {
                    let range = first_range(&request.deleted_ranges);
                    if let Some(range) = range {
                        frontier.extend_delete(
                            request.base_snapshot.clone(),
                            request.target_snapshot.clone(),
                            range,
                            request.target_frontier,
                            request.now,
                        );
                    }
                }
                EditFrontierKind::Replace => {
                    // Replace 的连续扩展按 Delete 处理（旧 overlay 继续收），新字
                    // 走下一轮。真实 IME 连续输入不会产生 Replace。
                    let range = first_range(&request.deleted_ranges);
                    if let Some(range) = range {
                        frontier.extend_delete(
                            request.base_snapshot.clone(),
                            request.target_snapshot.clone(),
                            range,
                            request.target_frontier,
                            request.now,
                        );
                    }
                }
            }
        } else {
            self.finish_edit_frontier_to_canonical();
            self.active_edit_frontier = Some(match kind {
                EditFrontierKind::Insert => {
                    let range = first_range(&request.inserted_ranges).unwrap_or((0, 0));
                    EditFrontierState::begin_insert(
                        request.target_snapshot.clone(),
                        range,
                        request.start_frontier,
                        request.target_frontier,
                        request.now,
                        duration_ms,
                    )
                }
                EditFrontierKind::Delete => {
                    let range = first_range(&request.deleted_ranges).unwrap_or((0, 0));
                    EditFrontierState::begin_delete(
                        request.base_snapshot.clone(),
                        request.target_snapshot.clone(),
                        range,
                        request.start_frontier,
                        request.target_frontier,
                        request.now,
                        duration_ms,
                    )
                }
                EditFrontierKind::Replace => {
                    let old_range = first_range(&request.deleted_ranges).unwrap_or((0, 0));
                    let new_range = first_range(&request.inserted_ranges).unwrap_or((0, 0));
                    EditFrontierState::begin_replace(
                        request.base_snapshot.clone(),
                        request.target_snapshot.clone(),
                        old_range,
                        new_range,
                        request.start_frontier,
                        request.target_frontier,
                        request.now,
                        duration_ms,
                    )
                }
            });
        }

        self.begin_or_extend_reflow(&request, duration_ms);
        record_frontier_diagnostic(&request, kind, self.active_edit_frontier.as_ref());
    }

    /// Issue #826: Reflow 层独立更新。没改的字移动，不参与前沿。
    fn begin_or_extend_reflow(&mut self, request: &EditFrontierRequest, duration_ms: u64) {
        // Changed range 必须排除，不允许同一 glyph 同时进 Reveal/Conceal 和 Reflow：
        // inserted 在 new 侧、deleted 在 old 侧，两侧都要排掉。
        let reflow = ReflowState::build(
            &request.base_snapshot,
            &request.target_snapshot,
            &request.offset_map,
            &request.deleted_ranges,
            &request.inserted_ranges,
            request.now,
            duration_ms,
        );
        if reflow.is_empty() {
            self.active_reflow = None;
        } else {
            self.active_reflow = Some(reflow);
        }
    }

    /// Issue #826: 采样本帧遮罩前沿。
    pub(crate) fn sample_edit_frontier(&self, frame_now: Instant) -> Option<EditFrontierSample> {
        self.active_edit_frontier
            .as_ref()
            .map(|frontier| frontier.sample(frame_now))
    }

    /// Issue #826: 按已采好的前沿样本算本帧吐字要从 canonical 静态层裁掉的矩形。
    ///
    /// 渲染计划构建入口会先 `sample_edit_frontier` 采一次，再把同一份样本交给
    /// 这里和 `old_overlay_glyphs_for`——同帧只采样一次，遮罩与 overlay 不会
    /// 因为两次 `Instant` 采样而错位。
    pub(crate) fn hidden_canonical_rects_for(
        &self,
        sample: &EditFrontierSample,
    ) -> Vec<(f64, f64, f64, f64, LineSnapshotId)> {
        let Some(frontier) = self.active_edit_frontier.as_ref() else {
            return Vec::new();
        };
        frontier
            .hidden_new_text_rects(sample)
            .into_iter()
            .map(|rect| {
                let snapshot_id = frontier
                    .target_snapshot
                    .line_snapshots
                    .iter()
                    .find(|line| line.visual_line_top <= rect.y && rect.y < line.visual_line_bottom)
                    .map(|line| line.id)
                    .unwrap_or(LineSnapshotId::new(0, 0, 0));
                (rect.x, rect.y, rect.w, rect.h, snapshot_id)
            })
            .collect()
    }

    /// Issue #826: 按已采好的前沿样本算本帧要额外画的旧正文 overlay glyph。
    pub(crate) fn old_overlay_glyphs_for(&self, sample: &EditFrontierSample) -> Vec<FrontierGlyph> {
        let Some(frontier) = self.active_edit_frontier.as_ref() else {
            return Vec::new();
        };
        frontier
            .old_overlay_glyphs(sample)
            .into_iter()
            .filter(FrontierGlyph::is_visible)
            .collect()
    }

    /// Issue #826: 本帧 Reflow 层要画的 glyph。
    pub(crate) fn reflow_glyphs(&self, frame_now: Instant) -> Vec<ReflowSpanFrame> {
        self.active_reflow
            .as_ref()
            .map(|reflow| reflow.sample(frame_now))
            .unwrap_or_default()
    }

    /// Issue #826: 把当前前沿立刻收成 canonical 终态。
    ///
    /// 指针点击 / 选区变化 / 滚动等场景必须先收口，不能让遮罩挂在旧正文上。
    pub(crate) fn finish_edit_frontier_to_canonical(&mut self) {
        self.active_edit_frontier = None;
        self.active_reflow = None;
    }

    pub(crate) fn has_active_edit_frontier(&self) -> bool {
        self.active_edit_frontier.is_some()
    }

    /// Issue #826: 当前前沿的 base（旧正文）布局快照。
    ///
    /// 吞字 / 替换的旧 overlay 要从它取旧行 QImage 补进纹理缓存。
    /// 吐字不需要旧 overlay，返回 `None`，调用方直接跳过纹理准备。
    pub(crate) fn active_edit_frontier_base_snapshot(&self) -> Option<&EditorLayoutSnapshot> {
        self.active_edit_frontier
            .as_ref()
            .filter(|frontier| frontier.kind.needs_old_overlay())
            .map(|frontier| &frontier.base_snapshot)
    }

    /// 当前前沿种类（光标 blink 抑制等诊断用）。
    pub(crate) fn active_edit_frontier_kind(&self) -> Option<EditFrontierKind> {
        self.active_edit_frontier.as_ref().map(|f| f.kind)
    }

    /// 本帧是否还有任何正文动画在跑。
    pub(crate) fn has_active_text_animation(&self, frame_now: Instant) -> bool {
        let frontier_running = self
            .active_edit_frontier
            .as_ref()
            .map(|f| !f.is_finished(frame_now))
            .unwrap_or(false);
        let reflow_running = self
            .active_reflow
            .as_ref()
            .map(|r| !r.is_finished(frame_now))
            .unwrap_or(false);
        frontier_running || reflow_running
    }

    /// Issue #826: 活跃正文动画**额外**需要保留的行纹理。
    ///
    /// 只收"静态层不会自己画"的那些：
    /// - 吞字 / 替换的旧正文 overlay —— 它画的是本轮删除前的旧行，静态层完全
    ///   不覆盖这些像素，必须从纹理缓存里取旧行图。
    /// - Reflow span —— `ReflowSpan.snapshot_id` 指向新行，新行本来就会被静态层
    ///   栅格化并缓存，这里一并登记只为保证中途不被回收。
    ///
    /// 吐字（纯 Insert）不需要任何额外纹理：最新 canonical 正文只画一份，由静态层
    /// 自己持有纹理，前沿只用 clip 把还没打开的部分裁掉。
    pub(crate) fn collect_active_snapshot_ids(&self) -> Vec<LineSnapshotId> {
        let mut ids: Vec<LineSnapshotId> = Vec::new();
        let mut seen = HashSet::new();
        let push = |id: LineSnapshotId, ids: &mut Vec<LineSnapshotId>, seen: &mut HashSet<_>| {
            if seen.insert(id) {
                ids.push(id);
            }
        };
        if let Some(frontier) = self.active_edit_frontier.as_ref() {
            if frontier.kind.needs_old_overlay() {
                if let Some(old_range) = frontier.old_range {
                    for line in frontier
                        .base_snapshot
                        .lines_in_byte_range(old_range.0, old_range.1)
                    {
                        push(line.id, &mut ids, &mut seen);
                    }
                }
            }
        }
        if let Some(reflow) = self.active_reflow.as_ref() {
            for span in &reflow.spans {
                push(span.snapshot_id, &mut ids, &mut seen);
            }
        }
        ids
    }

    /// 当前活跃 Reflow 在**新文本**坐标系里涉及的 byte 范围。
    ///
    /// 纹理准备按这些范围决定哪些新行需要重新栅格化。
    pub(crate) fn active_reflow_new_ranges(&self) -> Vec<(usize, usize)> {
        self.active_reflow
            .as_ref()
            .map(|reflow| reflow.spans.iter().map(|span| span.new_range).collect())
            .unwrap_or_default()
    }

    /// Issue #826: 抑制全部正文动画（失焦 / 加载 / 模式切换）。
    ///
    /// 直接落 canonical 终态，不留半开的遮罩。
    pub(crate) fn suppress_all(&mut self) -> bool {
        let had = self.active_edit_frontier.is_some() || self.active_reflow.is_some();
        self.finish_edit_frontier_to_canonical();
        had
    }

    /// 暂停正文动画（窗口失焦）。返回仍需保留的旧行纹理。
    pub(crate) fn pause_all(&mut self) -> Vec<LineSnapshotId> {
        self.paused_at = Some(Instant::now());
        self.collect_active_snapshot_ids()
    }

    pub(crate) fn resume_all(&mut self) {
        self.paused_at = None;
    }

    pub(crate) fn is_paused(&self) -> bool {
        self.paused_at.is_some()
    }

    /// 推进一帧。返回是否还需要继续请求下一帧。
    pub(crate) fn tick(&mut self, frame_now: Instant) -> bool {
        if self.is_paused() {
            return false;
        }
        if self
            .active_edit_frontier
            .as_ref()
            .map(|f| f.is_finished(frame_now))
            .unwrap_or(false)
        {
            self.active_edit_frontier = None;
        }
        if self
            .active_reflow
            .as_ref()
            .map(|r| r.is_finished(frame_now))
            .unwrap_or(false)
        {
            self.active_reflow = None;
        }
        self.active_edit_frontier.is_some() || self.active_reflow.is_some()
    }

    // ── 光标（只管视觉 Tween，不决定文字显示多少） ──────────────────────────

    /// Issue #826: 构造光标视觉动画计划。
    ///
    /// 逻辑 caret 已经在 Core 里立即变成当前 selection；本方法只负责让**视觉**
    /// 光标从当前 `visual_x/y` 平滑追到新的 caret rect。它不拥有正文动画，也不
    /// 决定任何文字显示多少。
    pub(crate) fn build_cursor_plan(&self, inputs: &CursorMoveInputs) -> CursorAnimationPlan {
        let should_be_visible =
            inputs.editor_enabled && !inputs.has_selection && !inputs.is_preediting;
        let old_rect = CursorRect {
            x: inputs.visual_x,
            top: inputs.visual_y,
            bottom: inputs.visual_y + inputs.cursor_h,
            baseline_y: inputs.baseline_y,
        };
        let new_rect = CursorRect {
            x: inputs.cursor_x,
            top: inputs.cursor_y,
            bottom: inputs.cursor_y + inputs.cursor_h,
            baseline_y: inputs.baseline_y,
        };
        let hard_snap = inputs.force_snap_next || inputs.selection_gesture_active;
        let allow_cross_line_tween = inputs.smooth_cursor_enabled && !inputs.is_scrolling;
        let needs_tween = (old_rect.x - new_rect.x).abs() > f64::EPSILON
            || (old_rect.top - new_rect.top).abs() > f64::EPSILON;
        let can_tween = !hard_snap
            && needs_tween
            && (allow_cross_line_tween || (old_rect.top - new_rect.top).abs() <= f64::EPSILON);

        let transition = if can_tween && inputs.duration_ms > 0 {
            CursorTransition::Tween {
                old_rect,
                new_rect,
                duration_ms: inputs.duration_ms,
            }
        } else {
            CursorTransition::Snap
        };

        CursorAnimationPlan {
            should_be_visible,
            transition,
            cursor_x: new_rect.x,
            cursor_y: new_rect.top,
            cursor_h: inputs.cursor_h,
            cursor_baseline_y: inputs.baseline_y,
            hidden_by_selection: inputs.has_selection,
        }
    }
}

impl Default for LinuxEditorAnimationCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

/// 首个非空 range。
fn first_range(ranges: &[(usize, usize)]) -> Option<(usize, usize)> {
    ranges.iter().copied().find(|&(start, end)| end > start)
}

/// Issue #826: 遮罩前沿的正式诊断事件。
fn record_frontier_diagnostic(
    request: &EditFrontierRequest,
    kind: EditFrontierKind,
    frontier: Option<&EditFrontierState>,
) {
    let mut fields: std::collections::BTreeMap<String, serde_json::Value> =
        std::collections::BTreeMap::new();
    fields.insert("frontier_kind".to_string(), serde_json::json!(kind.label()));
    fields.insert(
        "start_frontier".to_string(),
        serde_json::json!([request.start_frontier.x, request.start_frontier.top]),
    );
    fields.insert(
        "target_frontier".to_string(),
        serde_json::json!([request.target_frontier.x, request.target_frontier.top]),
    );
    fields.insert(
        "inserted_ranges".to_string(),
        serde_json::json!(request.inserted_ranges),
    );
    fields.insert(
        "deleted_ranges".to_string(),
        serde_json::json!(request.deleted_ranges),
    );
    fields.insert(
        "reflow_span_count".to_string(),
        serde_json::json!(request.target_snapshot.line_snapshots.len()),
    );
    fields.insert(
        "duration_ms".to_string(),
        serde_json::json!(frontier.map(|f| f.duration_ms).unwrap_or(0)),
    );
    writer_diagnostics::record_event(writer_diagnostics::DiagnosticEvent {
        timestamp_ms: chrono::Utc::now().timestamp_millis(),
        sequence: 0,
        session_id: String::new(),
        level: writer_diagnostics::DiagnosticLevel::Info,
        origin: writer_diagnostics::DiagnosticOrigin::App,
        event: "editor.anim.frontier".to_string(),
        target: "editor.anim".to_string(),
        message: Some(format!(
            "Issue #826: 遮罩前沿 {}，inserted={:?} deleted={:?}",
            kind.label(),
            request.inserted_ranges,
            request.deleted_ranges,
        )),
        fields,
    });
    editor_animation_debug_log(&format!(
        "anim_frontier: kind={} start=({:.1},{:.1}) target=({:.1},{:.1}) inserted={:?} deleted={:?}",
        kind.label(),
        request.start_frontier.x,
        request.start_frontier.top,
        request.target_frontier.x,
        request.target_frontier.top,
        request.inserted_ranges,
        request.deleted_ranges,
    ));
}

/// Issue #826: 光标 blink 在正文前沿跑完期间抑制。
pub(crate) fn blink_mode_for_frontier(frontier: Option<EditFrontierKind>) -> CursorBlinkMode {
    match frontier {
        Some(_) => CursorBlinkMode::Suppressed,
        None => CursorBlinkMode::Normal,
    }
}
