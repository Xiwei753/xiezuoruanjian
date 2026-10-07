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
//! Issue #826 评论 38：协同模式例外 —— 同一笔正文编辑的光标与文字前沿共享
//! [`CoordinatedCaretMotion`]（`active_coordinated_caret`），一条 caret 运动轨迹
//! 同时驱动视觉光标位置与前沿吞吐边界。非协同模式不用它。
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

use crate::sujian_editor_item::animation::coordinated_caret::{
    project_onto_layer, CaretMotionSegment, CoordinatedBoundary, CoordinatedCaretMotion,
    CoordinatedCaretSample,
};
use crate::sujian_editor_item::animation::edit_frontier::{
    ConcealDirection, ConcealSourceLine, ConcealVisualHandoff, EditFrontierKind,
    EditFrontierSample, EditFrontierState, FrontierGlyph, FrontierRegion,
};
use crate::sujian_editor_item::animation::reflow_motion::ReflowCurrentGeometry;
use crate::sujian_editor_item::animation::reflow_motion::{ReflowSpanFrame, ReflowState};
use crate::sujian_editor_item::animation::shaping_transition::{
    visible_source_slice, CurrentVisualCluster, ShapingTransitionFrame, ShapingTransitionState,
};
use crate::sujian_editor_item::cursor_animation::{
    CursorAnimationPlan, CursorBlinkMode, CursorTransition,
};
use crate::sujian_editor_item::edit_motion::{CursorRect, EditorAnimationKind};
use crate::sujian_editor_item::editor_animation_debug_log;
use crate::sujian_editor_item::layout_snapshot::{EditorLayoutSnapshot, LineSnapshotId};
use crate::sujian_editor_item::qt_text_node::{AnimationClipRect, StaticClipKind};

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
    /// old → new 的偏移映射（Reflow 层用）。
    pub offset_map: OffsetMap,
    /// Issue #826 评论 3 问题 1/2：`base_snapshot` 对应的正文纯文本。
    ///
    /// 连续吞字时用它构造「burst 最初 base 文本 → 本次编辑前文本」的 OffsetMap，
    /// 把每次编辑的 old range 映射回同一个基准再累计。
    pub base_text: String,
    /// `target_snapshot` 对应的正文纯文本。
    ///
    /// 连续吐字时 `offset_map` 本身就是「上一次 target 文本 → 本次 target 文本」，
    /// 用它把已累计的 new range 映射到最新坐标。
    pub target_text: String,
    /// Issue #826 评论 8 阻塞 1：本轮吞字的路径行进方向。
    ///
    /// Backspace 时 old range 向左扩（Backward），Delete 键时向右扩（Forward）。
    /// 连续删除时方向必须跟着变，否则跨自动换行扩展 old range 会让已吞掉的
    /// 那一行被重绑到别的字符上。
    pub conceal_direction: ConcealDirection,
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
    /// Issue #826 评论 24：不可拆 shaping cluster 的原子视觉交接层。
    ///
    /// 只有「逻辑改动只覆盖 cluster 一部分」时才非空。整块 cluster 从
    /// EditFrontier（Reveal carry / scalar / Conceal）与 Reflow 全部退出，
    /// 独占这一帧的视觉所有权。
    pub(crate) active_shaping_transition: Option<ShapingTransitionState>,
    /// Issue #826 评论 38：协同模式下光标与前沿共享的单条 caret 运动轨迹。
    ///
    /// `Some` ⟺ 本轮正文编辑走协同（`active_edit_frontier` 必同在）。
    /// 非协同模式恒为 `None`。`Option` 本身保证活跃 motion 数量最多 1，
    /// 不存在 per-key queue。时钟（`started_at`/`duration_ms`）永远与
    /// `active_edit_frontier` 一致，所以同一帧采到的 progress 天然相等。
    pub(crate) active_coordinated_caret: Option<CoordinatedCaretMotion>,
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
            active_shaping_transition: None,
            active_coordinated_caret: None,
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
    /// - 否则先 `finish_frontier_burst_only()` 结束当前 Frontier burst 再开新的。
    ///   **不动 Reflow** —— 见 Issue #826 评论 12：Reflow 是否连续由
    ///   `previous.target_text == request.base_text` 独立判定。
    pub(crate) fn begin_or_extend_edit_frontier(&mut self, request: EditFrontierRequest) {
        let kind = match request.kind {
            EditorAnimationKind::Insert => EditFrontierKind::Insert,
            EditorAnimationKind::Delete => EditFrontierKind::Delete,
            EditorAnimationKind::Replace => EditFrontierKind::Replace,
            // CursorOnly 没有正文改动，不进前沿，也不重开 Reflow。
            EditorAnimationKind::CursorOnly => return,
        };
        let duration_ms = u64::from(self.typing_animation_duration_ms);

        // Issue #826 评论 13 阻塞：Reflow -> Conceal 的当前帧几何交接。
        //
        // 一个正在 Reflow 的字，这一笔被删除时，所有权要从 Reflow 切到
        // Conceal。此刻必须先把「它这一帧实际在哪」采下来，否则新的
        // ConcealTrack 只能从 base_snapshot 的 canonical 几何起步，
        // 屏幕会出现「半路位置 -> canonical 位置 -> 再开始吞」的瞬移
        // （自动换行时可能跨整行）。
        //
        // 先采样再改状态：一旦进入 extend/begin，active_reflow 就被换掉了。
        // 只在 revision 链连续（上一轮 target == 本次 base）时才有交接依据；
        // 否则这段 Reflow 与本笔无关，不该拿它的几何。
        let reflow_current: Vec<ReflowCurrentGeometry> = self
            .active_reflow
            .as_ref()
            .filter(|reflow| reflow.target_text() == request.base_text)
            .map(|reflow| reflow.current_geometry(request.now))
            .unwrap_or_default();

        // 连续同方向编辑才并入：种类相同、吞字方向相同，且当前前沿还没走完
        // （走完了就是上一笔动画已经结束，必须开新的）。
        //
        // Issue #826 评论 9 阻塞 4：Delete 键（Forward）之后立刻 Backspace
        // （Backward）方向相反，必须另开一轮。否则 `extend_delete()` 会把
        // `conceal_direction` 改掉并重建整条路径，已吞掉的视觉状态会被换到
        // 另一端去。
        // Issue #826 评论 24/25：先派生「视觉 affected cluster」层。
        //
        // Core 的 inserted/deleted range 是逻辑字符事实，可以按字符切；Qt 的
        // shaping cluster 不能切。改动只覆盖 cluster 一部分（或边界 / shaping
        // 变了）时，整块 cluster 退出 EditFrontier（Reveal carry / scalar /
        // Conceal 全部不许碰），作为一组 old/new atom 进入这一层。
        //
        // **必须先采当前屏幕事实再改任何状态**：本层的第一帧要接着上一帧
        // 真正画出来的像素，而不是回到 canonical 起步。评论 25 的反例：
        // `af` 80ms 时 `f` 只露了 8.75px，若本层拿 base_snapshot 里的完整 `f`
        // 当 old 侧，第二笔同一帧就会「8.75px 突然补满 10px，再开始 f -> fi」。
        let current_visuals = self.collect_current_visuals(request.now);
        // Issue #826 评论 25 阻塞 2：revision 链连续（上一份的 target 正好是
        // 本次的 base）才能 retarget。上一笔还在淡出的 f/fi 遇到下一笔**无关**
        // 编辑时不能被清掉 —— 那样会「播到一半突然跳终态」。
        let previous_shaping = self
            .active_shaping_transition
            .as_ref()
            .filter(|previous| previous.target_text() == request.base_text)
            .cloned();
        // Issue #826 评论 31：**先采**当前屏幕事实 —— 被删 cluster 上一帧如果
        // 正由上一份 ShapingTransition 的 new side 在画，本笔的 owner 下一站是
        // 单一 Conceal frontier，而它必须从上一帧真实 rect/source/opacity 起步，
        // 不能从 canonical 的完整 1.0 起步。必须在 `build_or_retarget` 之前算，
        // 因为那一步会改掉 `current_visuals` 的消费状态。
        let conceal_handoffs = Self::collect_conceal_visual_handoffs(
            &current_visuals,
            previous_shaping.as_ref(),
            &request.base_snapshot,
            &request.deleted_ranges,
        );
        let shaping = ShapingTransitionState::build_or_retarget(
            previous_shaping.as_ref(),
            &current_visuals,
            &request.base_snapshot,
            &request.target_snapshot,
            &request.deleted_ranges,
            &request.inserted_ranges,
            &request.offset_map,
            request.now,
            duration_ms,
            request.target_text.clone(),
        );
        // Issue #826 评论 29：这两个是**本次 revision 坐标系**的 ownership claims
        // （`old` 落在 `base_snapshot`、`new` 落在 `target_snapshot`），不是渲染
        // atoms 的 source range。下面 insert/delete/replace 全部直接用它们。
        let shaping_new_owned = shaping.owned_new_clusters();
        let shaping_old_owned = shaping.owned_old_clusters();

        let can_extend = self
            .active_edit_frontier
            .as_ref()
            .map(|frontier| {
                let same_conceal_direction = !kind.needs_old_overlay()
                    || frontier.conceal_direction == request.conceal_direction;
                frontier.kind.can_extend(kind)
                    && same_conceal_direction
                    && !frontier.is_finished(request.now)
                    // Issue #826 评论 11 阻塞 2：身份不连续就是新的 burst 边界。
                    && frontier.can_extend_identity(kind, &request)
            })
            .unwrap_or(false);

        if can_extend {
            let frontier = self
                .active_edit_frontier
                .as_mut()
                .expect("can_extend 为真时 active_edit_frontier 必然存在");
            match kind {
                EditFrontierKind::Insert => {
                    // Issue #826 评论 10 阻塞 1：`base_to_current` 必须用
                    // 前沿自己累计的精确映射，不能每笔 `OffsetMap::build` 重新
                    // 全文 diff（那会丢掉多 patch 中间的 unchanged island）。
                    let base_to_current = frontier.base_to_target_map.clone();
                    frontier.extend_insert(
                        request.target_snapshot.clone(),
                        request.target_text.clone(),
                        request.inserted_ranges.clone(),
                        &request.offset_map,
                        &base_to_current,
                        shaping_new_owned,
                        request.now,
                    );
                }
                EditFrontierKind::Delete => {
                    // Issue #826 评论 3 问题 2：本次 old ranges 用「这一次编辑前」
                    // 的坐标，必须映射回 burst 最初 base 文本的坐标再累计。
                    // Issue #826 评论 10 阻塞 1：用前沿累计的精确 map，不再
                    // `OffsetMap::build(base_text, current_base_text)` 重新全文 diff。
                    let base_to_current = frontier.base_to_target_map.clone();
                    frontier.extend_delete(
                        request.target_snapshot.clone(),
                        request.target_text.clone(),
                        request.deleted_ranges.clone(),
                        &request.base_snapshot,
                        &base_to_current,
                        &request.offset_map,
                        &reflow_current,
                        request.conceal_direction,
                        shaping_old_owned,
                        &conceal_handoffs,
                        request.now,
                    );
                }
                EditFrontierKind::Replace => {
                    // Issue #826 评论 8 阻塞 2：Replace 必须**双侧**累计。
                    // 之前它走 extend_delete，新插入的字根本不进 reveal mask，
                    // canonical 会把这次新字直接完整显示。
                    // Issue #826 评论 10 阻塞 1：同 extend_delete，用累计的精确 map。
                    let base_to_current = frontier.base_to_target_map.clone();
                    frontier.extend_replace(
                        request.target_snapshot.clone(),
                        request.target_text.clone(),
                        request.deleted_ranges.clone(),
                        request.inserted_ranges.clone(),
                        &request.base_snapshot,
                        &base_to_current,
                        &request.offset_map,
                        &reflow_current,
                        request.conceal_direction,
                        shaping_old_owned,
                        shaping_new_owned,
                        &conceal_handoffs,
                        request.now,
                    );
                }
            }
        } else {
            // Issue #826 评论 12：Frontier 换 burst 时**只结束 Frontier**，
            // 不能顺手把独立的 Reflow 也清掉。
            //
            // Frontier 的 burst 边界（kind / direction / identity）与 Reflow 的
            // 连续性是两件独立的事。反例 `A|B` 输入 X 得 `AX|B`：
            // X 走 Insert Frontier、B 走 Reflow 往右移动；动画跑到一半立刻
            // Backspace 删掉 X —— Frontier 因 kind 不同换 burst 完全正常，
            // 但此时 `previous.target_text == request.base_text == "AXB"`，
            // Reflow 的 revision 链**仍然连续**，应该「从当前屏幕半路位置
            // retarget 到最新位置」。全局 finish 会先 `active_reflow = None`，
            // 导致 B 从屏幕半路**瞬移**回 `AXB` 的 canonical 位置再往回走。
            self.finish_frontier_burst_only();
            // Issue #826 评论 8 阻塞 3：不再只取 `first_range`。
            // Core 一次编辑可能给出多条 display_patches（Undo/Redo batch、
            // replace-all、apply 原子 batch、IME commit…），全部都要被前沿接管，
            // 每条不相邻的 patch 各走自己的视觉路径，共享同一个 progress。
            self.active_edit_frontier = Some(match kind {
                EditFrontierKind::Insert => EditFrontierState::begin_insert(
                    request.base_text.clone(),
                    request.target_snapshot.clone(),
                    request.target_text.clone(),
                    request.inserted_ranges.clone(),
                    request.offset_map.clone(),
                    shaping_new_owned,
                    request.now,
                    duration_ms,
                ),
                EditFrontierKind::Delete => EditFrontierState::begin_delete(
                    request.base_snapshot.clone(),
                    request.base_text.clone(),
                    request.target_snapshot.clone(),
                    request.target_text.clone(),
                    request.deleted_ranges.clone(),
                    request.offset_map.clone(),
                    &reflow_current,
                    request.conceal_direction,
                    shaping_old_owned,
                    &conceal_handoffs,
                    request.now,
                    duration_ms,
                ),
                EditFrontierKind::Replace => EditFrontierState::begin_replace(
                    request.base_snapshot.clone(),
                    request.base_text.clone(),
                    request.target_snapshot.clone(),
                    request.target_text.clone(),
                    request.deleted_ranges.clone(),
                    request.inserted_ranges.clone(),
                    request.offset_map.clone(),
                    &reflow_current,
                    request.conceal_direction,
                    shaping_old_owned,
                    shaping_new_owned,
                    &conceal_handoffs,
                    request.now,
                    duration_ms,
                ),
            });
        }

        // Issue #826 评论 24/25：把这一笔的交接层挂上。
        //
        // 与 Frontier burst 的连续性无关：只要有不可拆 cluster 就必须有这一层，
        // 否则那块视觉 cluster 会被前沿/reflow 同时碰。空的时候直接清掉，避免留一个
        // 什么都不画的「运行中动画」让渲染链一直请求下一帧。
        self.active_shaping_transition = (!shaping.is_empty()).then_some(shaping);

        self.begin_or_extend_reflow(&request, duration_ms);
        record_frontier_diagnostic(
            &request,
            kind,
            self.active_edit_frontier.as_ref(),
            self.active_shaping_transition.as_ref(),
        );
    }

    /// Issue #826 评论 25：**这一帧**屏幕上真实存在的全部视觉原子。
    ///
    /// 这是「owner 换手」的唯一交接凭据。任一视觉 owner 换成另一个 owner 时，
    /// 新 owner 的第一帧必须等于旧 owner 上一帧真正画出来的像素，不能回
    /// canonical 再起步 —— 这是 #826 从 Reflow→Conceal、Reveal rewrap 到
    /// Reveal→ShapingTransition 一路贯穿的那条原则。
    ///
    /// 它**不是**历史动画状态：没有 `started_at`、没有 remaining duration、
    /// 没有 historical stage、没有第二个动画对象，就是一帧采样。
    /// Issue #826 评论 31：被整块删除、且上一帧正由 ShapingTransition new side
    /// 持有的 cluster，交给 Conceal 的**当前帧视觉事实**。
    ///
    /// 判据必须与 `shaping_transition::build_or_retarget()` 的 pure-delete 分支
    /// **完全一致**（同一个「视觉身份 = previous new atom 的
    /// `(snapshot_id, visual_cluster_range)`」判据），否则 owner 会两头脱钩：
    /// 一边以为自己在吞，另一边以为自己还在淡。
    fn collect_conceal_visual_handoffs(
        current_visuals: &[CurrentVisualCluster],
        previous_shaping: Option<&ShapingTransitionState>,
        base_snapshot: &EditorLayoutSnapshot,
        deleted_ranges: &[(usize, usize)],
    ) -> Vec<ConcealVisualHandoff> {
        let Some(previous) = previous_shaping else {
            return Vec::new();
        };
        let previous_new_keys: Vec<(LineSnapshotId, (usize, usize))> = previous
            .groups
            .iter()
            .flat_map(|group| {
                group
                    .new_atoms
                    .iter()
                    .map(|atom| (atom.snapshot_id, atom.visual_cluster_range))
            })
            .collect();
        if previous_new_keys.is_empty() {
            return Vec::new();
        }
        let mut handoffs: Vec<ConcealVisualHandoff> = Vec::new();
        for &range in deleted_ranges {
            for line in base_snapshot.lines_in_byte_range(range.0, range.1) {
                for cluster in line.clusters_contained_in_range(range.0, range.1) {
                    // 吞字侧同样只认**完整覆盖**的 cluster（评论 24）。
                    let cluster_range = (cluster.byte_start, cluster.byte_end);
                    let key = (line.id, cluster_range);
                    if !previous_new_keys.contains(&key) {
                        continue;
                    }
                    if handoffs
                        .iter()
                        .any(|handoff| handoff.range == cluster_range)
                    {
                        continue;
                    }
                    // 判据只回答「上一帧它归谁」；真正接手还得**这一帧真的在画**。
                    let Some(visual) = current_visuals.iter().find(|visual| {
                        visual.snapshot_id == key.0 && visual.visual_cluster_range == key.1
                    }) else {
                        continue;
                    };
                    handoffs.push(ConcealVisualHandoff {
                        range: cluster_range,
                        snapshot_id: visual.snapshot_id,
                        source_rect: visual.source_rect.clone(),
                        dest_rect: visual.dest_rect.clone(),
                        opacity: visual.opacity,
                    });
                }
            }
        }
        handoffs
    }

    pub(crate) fn collect_current_visuals(&self, frame_now: Instant) -> Vec<CurrentVisualCluster> {
        // Issue #826 评论 35：paused 时正文视觉时间钉在 paused_at，
        // 无论谁触发重绘（滚动会持续 request_static_repaint）。
        let frame_now = self.effective_text_animation_time(frame_now);
        let mut out: Vec<CurrentVisualCluster> = Vec::new();

        // 1. 吐字侧：scalar reveal 正在打开的 cluster + carry 那一小段可见前缀。
        if let (Some(frontier), Some(sample)) = (
            self.active_edit_frontier.as_ref(),
            self.sample_edit_frontier(frame_now),
        ) {
            for reveal in frontier.current_reveal_visuals(sample.progress) {
                out.push(CurrentVisualCluster {
                    logical_range: reveal.range,
                    // 前沿的像素就取自这一块 cluster，视觉身份 = 逻辑身份。
                    visual_cluster_range: reveal.range,
                    snapshot_id: reveal.snapshot_id,
                    // Issue #826 评论 26 阻塞 2：语义统一成 exact slice。
                    source_rect: visible_source_slice(
                        &reveal.source_rect,
                        reveal.rect.w,
                        reveal.visible_width,
                    ),
                    dest_rect: reveal.rect,
                    // 吐字遮罩只裁可见宽度，屏幕上的实际不透明度仍是 1。
                    opacity: 1.0,
                    visible_clip: reveal.visible_width,
                });
            }
            // 2. 吞字侧：还没被遮罩收掉的旧 glyph。
            for glyph in frontier.old_overlay_glyphs(&sample) {
                if !glyph.is_visible() {
                    continue;
                }
                out.push(CurrentVisualCluster {
                    logical_range: glyph.range,
                    // 吞字侧画的正是这块 old cluster，视觉身份 = 逻辑身份。
                    visual_cluster_range: glyph.range,
                    snapshot_id: glyph.snapshot_id,
                    source_rect: glyph.source_rect,
                    dest_rect: glyph.dest_rect.clone(),
                    opacity: 1.0,
                    visible_clip: glyph.dest_rect.w,
                });
            }
        }

        // 3. Reflow 层：正在移动的字，这一帧的真实位置。
        if let Some(reflow) = self.active_reflow.as_ref() {
            for current in reflow.current_geometry(frame_now) {
                out.push(CurrentVisualCluster {
                    logical_range: current.current_range,
                    // Reflow 平移的就是这一块 cluster，视觉身份 = 逻辑身份。
                    visual_cluster_range: current.current_range,
                    snapshot_id: current.snapshot_id,
                    source_rect: current.source_rect,
                    dest_rect: current.dest_rect.clone(),
                    opacity: 1.0,
                    visible_clip: current.dest_rect.w,
                });
            }
        }

        // 4. 交接层自己：正在淡出 / 淡入的两侧。
        if let Some(shaping) = self.active_shaping_transition.as_ref() {
            out.extend(shaping.current_visuals(frame_now));
        }
        out
    }

    /// Issue #826: Reflow 层独立更新。没改的字移动，不参与前沿。
    ///
    /// 连续编辑只保留**一份** `ReflowState`：上一笔还在 A -> B 半路时来了新一笔，
    /// 不能重新 `build` 从 canonical 的 B 开始 B -> C（那会先跳一下再动）。
    /// 这里先 `sample(now)` 拿到当前屏幕上每个 unchanged cluster 的真实位置，
    /// 再以它为新起点指向最新目标 —— 永远只做「当前屏幕 -> 最新目标」。
    fn begin_or_extend_reflow(&mut self, request: &EditFrontierRequest, duration_ms: u64) {
        // Changed range 必须排除，不允许同一 glyph 同时进 Reveal/Conceal 和 Reflow。
        // 用**前沿已累计的** burst 范围（old_range / new_range），而不是单笔的
        // deleted/inserted —— 同一轮连打时，先前插入的字仍由前沿负责吐字，
        // 不能被 Reflow 抢走。
        // Issue #826 评论 5: 两侧 exclusion 各自只使用自己所在 revision 的坐标。
        //
        // `excluded_old` 属于 `request.base_snapshot`（本次编辑前的旧正文）坐标系，
        // 而 `active_edit_frontier.old_range` 固定在**burst 最初** base_snapshot 坐标系。
        // 两者不能直接拿来 overlaps：`ABCDEF` 连删 D、E 时 frontier.old_range 累计成
        // burst 初始坐标 [3,5]，但在第二次的 current old 坐标（`ABCEF`）里 [3,5] 是 E+F，
        // 会把本该参与回流的 F 误当 changed text 排除掉、导致 F 直接瞬移。
        // 所以旧侧只用本次 `deleted_ranges`；若将来还有别的旧侧 overlay 需要排除，
        // 必须先显式 map 到 `request.base_text` 坐标再传入。
        // Reflow 只排除「本帧仍未吐完」的部分（评论 17）。
        let frontier_progress = self
            .active_edit_frontier
            .as_ref()
            .map(|frontier| frontier.sample(request.now).progress)
            .unwrap_or(1.0);
        // Issue #826 评论 24：mixed visual cluster 整块归 `shaping_transition`，
        // Reflow 绝不能碰。
        //
        // 现在 `excluded_*` 实际上**已经**能挡掉它 —— mixed cluster 必然与本笔
        // changed range 重叠，而 `overlaps_any` 是按重叠判定的。但那只是巧合：
        // 一旦 `deleted_ranges` 归一化方式变化，或者将来「只部分改动」的判定与
        // changed range 脱钩，Reflow 就会把一块正被交接层淡出的 cluster 再插值
        // 一次。这里显式并进去，让「每块视觉 cluster 每帧一个 owner」是**写出来的
        // 约束**，不是推出来的。
        //
        // Issue #826 评论 29：`excluded_old` 要求的坐标系是**本次
        // `request.base_snapshot`**，而 `owned_old_clusters()` 返回的
        // `old_owned_in_base` 正是为此构造的。绝不能换成去遍历
        // `groups[*].old_atoms[*].cluster` —— 历史 old atom 的 `cluster` 停在
        // 更早 revision 上，一个 stale `0..4` 就会把当前 base 里同坐标的新字
        // 一起排掉，表现是当前活字瞬移。
        let shaping_old_owned: Vec<(usize, usize)> = self
            .active_shaping_transition
            .as_ref()
            .map(|shaping| shaping.owned_old_clusters())
            .unwrap_or_default();
        let shaping_new_owned: Vec<(usize, usize)> = self
            .active_shaping_transition
            .as_ref()
            .map(|shaping| shaping.owned_new_clusters())
            .unwrap_or_default();
        let excluded_old: Vec<(usize, usize)> = request
            .deleted_ranges
            .iter()
            .copied()
            .chain(shaping_old_owned)
            .collect();
        // Issue #826 评论 8：`new_ranges` 现在是按 overlap / adjacent 归一化的
        // 集合，全部落在最新 target 坐标系里，可以直接用来做 excludes。
        // Issue #826 评论 17：只用**仍未吐完**的 pending reveal region。
        // 已经完整露出的字对本次编辑已经是 unchanged text，应该让 Reflow 正常
        // 接管它从旧位置移到新行；之前用整轮历史插入会让上一笔已吐完的字在
        // 下一笔触发自动换行时既不能 Reveal 也不能 Reflow，直接瞬移。
        let carried_new: Vec<(usize, usize)> = self
            .active_edit_frontier
            .as_ref()
            .map(|f| f.pending_reveal_ranges(frontier_progress))
            .unwrap_or_default();
        let excluded_new: Vec<(usize, usize)> = carried_new
            .iter()
            .copied()
            .chain(
                request
                    .inserted_ranges
                    .iter()
                    .copied()
                    .filter(|(s, e)| !carried_new.iter().any(|acc| acc.0 <= *s && *e <= acc.1)),
            )
            .chain(shaping_new_owned)
            .collect();

        // Issue #826 评论 10 阻塞 2：只有上一份 Reflow 的 target 文本**正好等于**
        // 本次编辑前的 base 文本，才说明这一笔与上一笔在同一条编辑 revision 链上，
        // 这时它需要的 `prev_target_to_new` 就是 Core 本次给的精确 `offset_map`。
        //
        // 反过来，之前是 `OffsetMap::build(previous.target_text(), request.target_text)`
        // 重新全文 diff（只是最长公共前后缀），多 patch 中间的 unchanged island
        // 直接消失 —— Reflow retarget 找不到旧 span，退化成「新进入 Reflow」分支，
        // 从 `request.base_snapshot` 的 canonical 位置重新起步，于是中间的字
        // 「上一轮动画半路位置 → 突然跳回 canonical → 再向新 target 动」。
        //
        // 两者不等时上一份 Reflow 已经不和当前编辑 revision 连续，强接没有正确
        // 身份依据，直接从本次 base -> target 重新 build。
        let reflow_is_continuous = self
            .active_reflow
            .as_ref()
            .is_some_and(|previous| previous.target_text() == request.base_text);

        let mut next = match self.active_reflow.as_ref() {
            Some(previous) if reflow_is_continuous && !previous.is_finished(request.now) => {
                // 语义虽然不同（old_to_new 是本次 base -> target，
                // prev_target_to_new 是上一 Reflow target -> 本次 target），
                // 但在上面的 invariant 成立时它们就是同一份 map。
                previous.retarget(
                    request.now,
                    &request.base_snapshot,
                    &request.offset_map,
                    &request.target_snapshot,
                    &request.offset_map,
                    &excluded_old,
                    &excluded_new,
                    duration_ms,
                )
            }
            _ => ReflowState::build(
                &request.base_snapshot,
                &request.target_snapshot,
                &request.offset_map,
                &excluded_old,
                &excluded_new,
                request.now,
                duration_ms,
            ),
        };
        next.set_target_text(request.target_text.clone());
        self.active_reflow = if next.is_empty() { None } else { Some(next) };
    }

    /// Issue #826 评论 38/39：采样本帧遮罩前沿。
    ///
    /// 协同 motion 活跃时，吐字/吞字两侧的边界都由本帧 caret 位置经
    /// [`project_onto_layer`] 投影得到（评论 38 的投影设计 + 评论 39 的
    /// first-y-match 修正）：caret 在哪，边界就在哪，天然精确相等，不存在
    /// glyph/caret 几何差。`CaretOnly`（Enter / shaping 全接管）时两侧 path
    /// 本来就空，无实际消费者。投影命不中（理论上 caret y 恒在某行带内，
    /// 只有浮点边界情形）时回退到 progress 时钟。
    pub(crate) fn sample_edit_frontier(&self, frame_now: Instant) -> Option<EditFrontierSample> {
        let frame_now = self.effective_text_animation_time(frame_now);
        let frontier = self.active_edit_frontier.as_ref()?;
        let mut sample = frontier.sample(frame_now);
        if let Some(motion) = self.active_coordinated_caret.as_ref() {
            let progress = motion.sample_progress(frame_now);
            let (caret_x, caret_y) =
                motion.position_at_distance(motion.distance_at_progress(progress));
            let reveal_distance =
                project_onto_layer(&frontier.reveal.regions, caret_x, caret_y)
                    .unwrap_or_else(|| frontier.reveal.advanced(progress));
            let conceal_distance =
                project_onto_layer(&frontier.conceal.regions, caret_x, caret_y)
                    .unwrap_or_else(|| frontier.conceal.advanced(progress));
            sample.coordinated = Some(CoordinatedBoundary {
                reveal_distance,
                conceal_distance,
            });
        }
        Some(sample)
    }

    /// Issue #826: 按已采好的前沿样本算本帧吐字要从 canonical 静态层裁掉的矩形。
    ///
    /// 渲染计划构建入口会先 `sample_edit_frontier` 采一次，再把同一份样本交给
    /// 这里和 `old_overlay_glyphs_for`——同帧只采样一次，遮罩与 overlay 不会
    /// 因为两次 `Instant` 采样而错位。
    pub(crate) fn hidden_canonical_rects_for(
        &self,
        sample: &EditFrontierSample,
    ) -> Vec<AnimationClipRect> {
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
                // Issue #826 评论 6：吐字遮罩只是把还没露出的 inserted glyph 从
                // canonical 静态层暂时裁掉，动画层不画第二份正文，所以**不依赖
                // 任何动画纹理**。renderer 不得因为 texture_cache 里没有这个
                // snapshot_id 就把它过滤掉，否则吐字动画直接失效。
                AnimationClipRect {
                    x: rect.x,
                    y: rect.y,
                    w: rect.w,
                    h: rect.h,
                    snapshot_id,
                    kind: StaticClipKind::FrontierMask,
                }
            })
            .collect()
    }

    /// Issue #826 评论 6 阻塞 2: 当前前沿的最新 target snapshot。
    ///
    /// Reflow 的动画 glyph 从最新 target/new 行图取纹理（`ReflowSpan.snapshot_id`
    /// 就是 `new_line.id`），所以纹理准备必须能拿到这一份，不能只看 base_snapshot。
    pub(crate) fn active_edit_frontier_target_snapshot(&self) -> Option<&EditorLayoutSnapshot> {
        self.active_edit_frontier
            .as_ref()
            .map(|frontier| &frontier.target_snapshot)
    }

    /// Issue #826 评论 6 阻塞 2: 当前活跃 Reflow 真正需要动画纹理的行 id。
    ///
    /// 这些 id 指向最新 target/new 行，与 Delete overlay 用的 base/old 行是不同
    /// 批次（id 带 revision）。纹理准备必须分别覆盖两批。
    pub(crate) fn active_reflow_snapshot_ids(&self) -> Vec<LineSnapshotId> {
        let mut ids: Vec<LineSnapshotId> = Vec::new();
        let mut seen = HashSet::new();
        if let Some(reflow) = self.active_reflow.as_ref() {
            for span in &reflow.spans {
                if seen.insert(span.snapshot_id) {
                    ids.push(span.snapshot_id);
                }
            }
        }
        ids
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
        let frame_now = self.effective_text_animation_time(frame_now);
        self.active_reflow
            .as_ref()
            .map(|reflow| reflow.sample(frame_now))
            .unwrap_or_default()
    }

    /// Issue #826: 把**全部**正文视觉层立刻收成 canonical 终态。
    ///
    /// 指针点击 / 手动 selection-caret 跳转 / composition 切 preedit /
    /// suppress_all / 加载 / 失焦等场景必须先收口，不能让遮罩挂在旧正文上。
    /// 这些是真正要「正文所有视觉层一起结束」。
    ///
    /// **不要**用它表达「正文又来一笔编辑，只是 Frontier 换 burst」——
    /// 那是 [`Self::finish_frontier_burst_only`] 的语义，两者不能混用。
    pub(crate) fn finish_edit_frontier_to_canonical(&mut self) {
        self.active_edit_frontier = None;
        self.active_reflow = None;
        // Issue #826 评论 24：交接层也属于「正文视觉层」，一起收成 canonical。
        self.active_shaping_transition = None;
        // Issue #826 评论 38：协同 motion 与前沿同生共死。前沿收掉时 motion
        // 必须一起清，否则下一帧协同采样会拿着旧 start/target 往新 canonical
        // 上画，把光标带到一个已经不存在的 caret 位置。
        self.active_coordinated_caret = None;
    }

    /// Issue #826 评论 12：**只**结束当前 Frontier burst，保留 Reflow。
    ///
    /// Frontier 与 Reflow 是 #826 拆开的两个独立视觉层，各自的连续性判据不同：
    /// - Frontier：由 kind / conceal direction / 编辑身份连续性决定；
    /// - Reflow：由 `previous.target_text == request.base_text`（revision 链）
    ///   决定。
    ///
    /// 正文又来一笔编辑、只是 Frontier 需要换 burst 时，Reflow 的 revision 链
    /// 可能仍然连续（Insert -> Delete、Forward Delete -> Backspace、
    /// identity 断裂、Replace -> 其它 kind 都会遇到），此时必须让 Reflow 继续
    /// 「当前屏幕位置 -> 最新 target」，不能把它清掉重建成
    /// 「canonical 位置 -> 最新 target」（那会造成肉眼可见的瞬移）。
    pub(crate) fn finish_frontier_burst_only(&mut self) {
        self.active_edit_frontier = None;
        // Issue #826 评论 24：mixed cluster 交接层是**独立**的一层，不随 Frontier
        // 换 burst 一起清 —— 它画的是「不可拆 cluster 的 old/new 视觉交接」，
        // 与本轮是 Insert 还是 Delete 无关。真正要整体收口时用
        // `finish_edit_frontier_to_canonical`。
    }

    pub(crate) fn has_active_edit_frontier(&self) -> bool {
        self.active_edit_frontier.is_some()
    }

    /// Issue #826 评论 14 阻塞 4：当前活跃的旧正文 overlay 引用的行纹理 **id**
    /// （生命周期）。
    ///
    /// 直接从 `conceal_tracks[].glyphs[].snapshot_id` 收集 —— ConcealTrack 已经
    /// 明确知道自己画什么，不再从 `old_ranges + base_snapshot` 推测。
    ///
    /// 这个区别在**同 burst** 时是致命的：`base_snapshot` 是 burst 第一笔之前的
    /// 快照，而 track 的 glyph source 来自「track 创建这一刻的 current old
    /// snapshot」，两者 line id 不同。之前只收 burst base 的 id，于是
    /// `texture_cache.retain_active_snapshot_ids()`（实现就是 `line_store.retain`）
    /// 会先把 overlay 那张图删掉。
    ///
    /// 注意这只是**生命周期**（retain 时别删）；**资源**见
    /// [`Self::active_conceal_source_lines`] —— retain 不能凭空创建缺失的行图。
    /// 纯吐字（Insert）没有旧 overlay，两者都返回空。
    pub(crate) fn active_old_overlay_snapshot_ids(&self) -> Vec<LineSnapshotId> {
        let Some(frontier) = self
            .active_edit_frontier
            .as_ref()
            .filter(|f| f.kind.needs_old_overlay())
        else {
            return Vec::new();
        };
        frontier.active_conceal_snapshot_ids()
    }

    /// Issue #826 评论 15：本轮活跃吞字 overlay 的**纹理资源**。
    ///
    /// `LineSnapshotId` 只是钥匙，不是图 —— pipeline 据此在缺失时把行图重新插进
    /// `TextureCache`。评论 14 之后 glyph 的 source 来自「track 创建这一刻的
    /// current old snapshot」，不再统一来自 burst base，所以缺失时不能回头猜
    /// 某份 snapshot 有没有这个 id。
    pub(crate) fn active_conceal_source_lines(&self) -> Vec<ConcealSourceLine> {
        self.active_edit_frontier
            .as_ref()
            .filter(|f| f.kind.needs_old_overlay())
            .map(|f| f.active_conceal_source_lines())
            .unwrap_or_default()
    }

    /// 当前前沿种类（光标 blink 抑制等诊断用）。
    /// Issue #826 评论 17：测试用 —— 当前前沿的吞字 region 范围。
    #[cfg(test)]
    pub(crate) fn active_edit_frontier_base_ranges_for_test(&self) -> Vec<(usize, usize)> {
        self.active_edit_frontier
            .as_ref()
            .map(|f| f.old_ranges())
            .unwrap_or_default()
    }

    /// Issue #826 评论 17：测试用 —— 当前**仍未吐完**的 reveal range。
    #[cfg(test)]
    pub(crate) fn active_reveal_pending_ranges_for_test(
        &self,
        now: std::time::Instant,
    ) -> Vec<(usize, usize)> {
        self.active_edit_frontier
            .as_ref()
            .map(|f| f.pending_reveal_ranges(f.sample(now).progress))
            .unwrap_or_default()
    }

    /// Issue #826 评论 18 阻塞 2：测试用 —— 当前仍持有的旧字 glyph / 行图数量。
    #[cfg(test)]
    pub(crate) fn active_conceal_glyphs_for_test(&self) -> usize {
        self.active_edit_frontier
            .as_ref()
            .map(|f| f.conceal_glyphs.len())
            .unwrap_or(0)
    }

    /// Issue #826 评论 18 阻塞 2：测试用 —— 当前仍持有的行图来源数量。
    #[cfg(test)]
    pub(crate) fn active_conceal_sources_for_test(&self) -> usize {
        self.active_edit_frontier
            .as_ref()
            .map(|f| f.conceal_sources.len())
            .unwrap_or(0)
    }

    /// Issue #826 评论 19 阻塞 3：测试用 —— 当前吞字前沿**所有 region 的路径总长**。
    ///
    /// 它必须等于「这一帧屏幕上仍然可见的旧 glyph 宽度之和」。一旦某条历史 region
    /// 没有 glyph 却还留着，它就会凭空吃掉一段单前沿 distance —— 肉眼是「前沿在
    /// 没有旧字 overlay 的位置空跑，删除中间顿一下」，而 overlay 断言完全看不出来。
    #[cfg(test)]
    pub(crate) fn active_conceal_total_length_for_test(&self) -> f64 {
        self.active_edit_frontier
            .as_ref()
            .map(|f| f.conceal.regions.iter().map(|r| r.path.total_length).sum())
            .unwrap_or(0.0)
    }

    pub(crate) fn active_edit_frontier_kind(&self) -> Option<EditFrontierKind> {
        self.active_edit_frontier.as_ref().map(|f| f.kind)
    }

    /// 本帧是否还有任何正文动画在跑。
    pub(crate) fn has_active_text_animation(&self, frame_now: Instant) -> bool {
        let frame_now = self.effective_text_animation_time(frame_now);
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
        // Issue #826 评论 24：交接层还在跑就必须继续请求下一帧，否则它会卡在
        // 半透明状态、静态层却已经挖掉了 canonical 目标位置。
        let shaping_running = self
            .active_shaping_transition
            .as_ref()
            .map(|s| !s.is_finished(frame_now))
            .unwrap_or(false);
        frontier_running || reflow_running || shaping_running
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
        // Issue #826 评论 24：mixed cluster 交接层的旧侧行图静态层不会画
        // （正文已经是最新的了），必须保活，否则旧 cluster 淡出时直接消失。
        if let Some(shaping) = self.active_shaping_transition.as_ref() {
            for id in shaping.active_snapshot_ids() {
                push(id, &mut ids, &mut seen);
            }
        }
        if let Some(frontier) = self.active_edit_frontier.as_ref() {
            if frontier.kind.needs_old_overlay() {
                // Issue #826 评论 14 阻塞 4：从 track 自己的 glyphs 收，
                // 同 burst handoff 的 source texture 不在 burst base 里。
                for id in frontier.active_conceal_snapshot_ids() {
                    push(id, &mut ids, &mut seen);
                }
            }
        }
        if let Some(reflow) = self.active_reflow.as_ref() {
            for span in &reflow.spans {
                push(span.snapshot_id, &mut ids, &mut seen);
            }
        }
        // Issue #826 评论 21 阻塞 1：吐字 carry 的目标行纹理也要保活。
        //
        // 纯 Insert rewrap 场景（X 半吐 -> 输入 Y）里 `Conceal ids = []`、Reflow 也
        // 没有 X 的 span（X 未吐完，仍被 pending Reveal 排除出 Reflow），所以这
        // 个 id 在这里之前根本没人登记。retain 之后纹理可能已被回收，
        // `prepare_frontier_textures` 也不会插回去 —— renderer 里 carry glyph 被
        // 直接跳过，而它的 `ReflowTarget` clip 又因纹理 miss 被过滤，
        // 「旧行半个 X -> 新行完整 X」的瞬移就在真实渲染链里复活了。
        //
        // 不加 `needs_old_overlay()` 判断：纯 Insert 也需要它。
        if let Some(frontier) = self.active_edit_frontier.as_ref() {
            for id in frontier.active_reveal_carried_snapshot_ids() {
                push(id, &mut ids, &mut seen);
            }
        }
        ids
    }

    /// Issue #826 评论 21 阻塞 1：吐字 carry 真正引用的最新 target 行纹理 id。
    ///
    /// 生命周期（`collect_active_snapshot_ids`）负责别删；资源由 pipeline 的
    /// `prepare_frontier_textures` 从 `active_edit_frontier_target_snapshot()` 里
    /// 缺失时重新插入。两件事分开，不能靠其中一件顺带完成另一件。
    pub(crate) fn active_reveal_carried_snapshot_ids(&self) -> Vec<LineSnapshotId> {
        self.active_edit_frontier
            .as_ref()
            .map(|f| f.active_reveal_carried_snapshot_ids())
            .unwrap_or_default()
    }

    /// Issue #826 评论 4 问题 3：Reflow 接管期间要从静态正文层挖掉的 canonical 目标位置。
    ///
    /// 动画层画"正在移动的那一份"，静态层不能同时再画一份最终位置，否则重影。
    /// Issue #826 评论 6 阻塞 3: Reflow 接管的 canonical 最终位置。
    ///
    /// 这些区域静态层要让位给动画层正在移动的那一份 glyph，所以**必须确认对应
    /// target 行纹理存在**——renderer 只对 `ReflowTarget` 做纹理可用性守卫，
    /// 纹理 miss 时同帧恢复 canonical，绝不影响 `FrontierMask`。
    pub(crate) fn reflow_target_clip_rects(&self) -> Vec<AnimationClipRect> {
        self.active_reflow
            .as_ref()
            .map(ReflowState::target_clip_rects)
            .unwrap_or_default()
            .into_iter()
            .map(|(x, y, w, h, snapshot_id)| AnimationClipRect {
                x,
                y,
                w,
                h,
                snapshot_id,
                kind: StaticClipKind::ReflowTarget,
            })
            .collect()
    }

    /// Issue #826 评论 20：吐字 carry 的 canonical 目标位置，静态层要让位。
    ///
    /// carry 用最新 target 的行纹理在**旧屏幕位置**画已可见前缀，canonical 在
    /// **新位置**画同一个字 —— 两边都画就是重影，所以新位置必须挖掉。
    ///
    /// kind 用 `ReflowTarget` 而不是 `FrontierMask`：carry 真的需要那张纹理，
    /// 纹理 miss 时应该恢复 canonical（字重新出现），而不是留一块空白。
    pub(crate) fn reveal_carried_target_clip_rects(
        &self,
        sample: &EditFrontierSample,
    ) -> Vec<AnimationClipRect> {
        let Some(frontier) = self.active_edit_frontier.as_ref() else {
            return Vec::new();
        };
        frontier
            .reveal_carried_target_rects(sample)
            .into_iter()
            .map(|(rect, snapshot_id)| AnimationClipRect {
                x: rect.x,
                y: rect.y,
                w: rect.w,
                h: rect.h,
                snapshot_id,
                kind: StaticClipKind::ReflowTarget,
            })
            .collect()
    }

    /// Issue #826 评论 20：本帧要额外画的「已可见前缀」glyph。
    pub(crate) fn reveal_carried_glyphs_for(
        &self,
        sample: &EditFrontierSample,
    ) -> Vec<FrontierGlyph> {
        let Some(frontier) = self.active_edit_frontier.as_ref() else {
            return Vec::new();
        };
        frontier
            .reveal_carried_glyphs(sample)
            .into_iter()
            .filter(FrontierGlyph::is_visible)
            .collect()
    }

    /// Issue #826 评论 24：mixed cluster 交接层本帧要画的画面。
    pub(crate) fn shaping_transition_glyphs(
        &self,
        frame_now: Instant,
    ) -> Vec<ShapingTransitionFrame> {
        let frame_now = self.effective_text_animation_time(frame_now);
        self.active_shaping_transition
            .as_ref()
            .map(|shaping| shaping.sample(frame_now))
            .unwrap_or_default()
    }

    /// Issue #826 评论 24：mixed cluster 的 canonical 目标位置，静态层要让位。
    ///
    /// 动画层正在画「正在淡入的那一份」，静态层同时画最终位置就是重影。
    /// kind 用 `ReflowTarget`：新侧真的需要那张纹理，纹理 miss 时应该恢复
    /// canonical，而不是留一块空白。
    pub(crate) fn shaping_transition_target_clip_rects(&self) -> Vec<AnimationClipRect> {
        self.active_shaping_transition
            .as_ref()
            .map(ShapingTransitionState::target_clip_rects)
            .unwrap_or_default()
            .into_iter()
            .map(|(rect, snapshot_id)| AnimationClipRect {
                x: rect.x,
                y: rect.y,
                w: rect.w,
                h: rect.h,
                snapshot_id,
                kind: StaticClipKind::ReflowTarget,
            })
            .collect()
    }

    /// Issue #826 评论 24：mixed cluster 旧侧的**纹理资源**。
    pub(crate) fn active_shaping_transition_source_lines(&self) -> Vec<ConcealSourceLine> {
        self.active_shaping_transition
            .as_ref()
            .map(ShapingTransitionState::old_source_lines)
            .unwrap_or_default()
    }

    /// Issue #826 评论 24：mixed cluster 交接层引用的行纹理 id（新侧行图由
    /// canonical 栅格化，但保活由 [`Self::collect_active_snapshot_ids`] 负责）。
    pub(crate) fn active_shaping_transition_snapshot_ids(&self) -> Vec<LineSnapshotId> {
        self.active_shaping_transition
            .as_ref()
            .map(ShapingTransitionState::active_snapshot_ids)
            .unwrap_or_default()
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
        let had = self.active_edit_frontier.is_some()
            || self.active_reflow.is_some()
            || self.active_shaping_transition.is_some();
        self.finish_edit_frontier_to_canonical();
        had
    }

    /// Issue #826 评论 34：暂停正文动画时间轴，返回仍需保留的旧行纹理。
    ///
    /// `now` 由调用方给出（生产传 `std::time::Instant::now()`，测试传可控时刻）：
    /// 只记 pause 瞬间，各层 progress 停在这一刻；恢复时由 [`Self::resume_all`]
    /// 把暂停时长平移回 `started_at`，所以这是**真 pause 时间轴**，不是
    /// 「先不 tick、恢复后按墙钟直接跳终态」。
    /// Issue #826 评论 35：正文动画的**实际**采样时刻。
    ///
    /// pause 只记 `paused_at` 是不够的：`set_is_scrolling(true)` 自己会
    /// `request_static_repaint()`，滚动位置变化也会持续触发 Scene Graph 重绘，
    /// 于是 paused 期间 `build_render_plan_full(frame_now)` 仍按墙钟算
    /// `frame_now - started_at`，画面会先跳到终态、resume 后又回到 pause 时刻。
    /// 原则：paused 时无论谁触发重绘，正文视觉时间永远钉在 `paused_at`。
    pub(crate) fn effective_text_animation_time(&self, frame_now: Instant) -> Instant {
        self.paused_at.unwrap_or(frame_now)
    }

    pub(crate) fn pause_all(&mut self, now: Instant) -> Vec<LineSnapshotId> {
        self.paused_at = Some(now);
        self.collect_active_snapshot_ids()
    }

    /// Issue #826 评论 34：恢复正文动画时间轴。
    ///
    /// 没有 pause 过（`paused_at == None`）什么都不做。
    /// 有 pause：算 `delta = now - paused_at`，把三层 active state 的
    /// `started_at` 整体平移 `delta` —— 40ms 处 pause、500ms 后 resume，
    /// 恢复后仍从 40ms 继续，而不是按墙钟 540ms 被 `tick()` 判 finished。
    /// 只平移起点，不重建动画对象、不排历史队列。
    pub(crate) fn resume_all(&mut self, now: Instant) {
        let Some(paused_at) = self.paused_at.take() else {
            return;
        };
        let delta = now.saturating_duration_since(paused_at);
        if delta.is_zero() {
            return;
        }
        if let Some(frontier) = self.active_edit_frontier.as_mut() {
            frontier.shift_started_at(delta);
        }
        if let Some(reflow) = self.active_reflow.as_mut() {
            reflow.shift_started_at(delta);
        }
        if let Some(shaping) = self.active_shaping_transition.as_mut() {
            shaping.shift_started_at(delta);
        }
        // Issue #826 评论 38：协同 motion 与前沿同 started_at，必须一起平移，
        // 否则 resume 后光标与吞吐边界立刻分叉。
        if let Some(motion) = self.active_coordinated_caret.as_mut() {
            motion.shift_started_at(delta);
        }
    }

    /// Issue #826 评论 34：把「滚动期间被 suppress 的正文编辑」之前的旧正文
    /// 动画连同 pause 状态一起收成最新 canonical。
    ///
    /// 滚动中的正文编辑 Core edit 已应用、正文已经变了，但完全不进
    /// `prepare_edit_motion`：既不 retarget 也不 finish。若把上一笔
    /// Frontier/Reflow/Shaping 留到 resume，它们会拿旧正文的
    /// mask / overlay / 行图身份在新 canonical 上播（Reveal 裁错新正文、
    /// Reflow glyph 错位重现、Shaping clip 挖掉新字、纹理 owner 全是旧编辑链）。
    ///
    /// 语义与「这一笔明确不做正文动画」一致：最新 canonical 直接接管。
    /// 返回收口前是否真的有活动正文动画（调用方据此决定要不要 retain 纹理）。
    pub(crate) fn finish_paused_text_animation_to_canonical(&mut self) -> bool {
        let had = self.active_edit_frontier.is_some()
            || self.active_reflow.is_some()
            || self.active_shaping_transition.is_some();
        self.finish_edit_frontier_to_canonical();
        self.paused_at = None;
        had
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
        if self
            .active_shaping_transition
            .as_ref()
            .map(|s| s.is_finished(frame_now))
            .unwrap_or(false)
        {
            self.active_shaping_transition = None;
        }
        // Issue #826 评论 39 BLOCKER 1：motion 自己就是本笔正文协同 clock，
        // progress==1 时由采样侧自己结束；这里**不得**因为前沿没了就无条件清
        // motion —— 零可见 path 的编辑（Enter / shaping 全接管）第一帧前沿就
        // finished，motion 必须继续走到 target，否则协同光标只动一帧就停。
        // 显式收口（suppress / 点击 / 独立光标接管 / finish-to-canonical）会清它。
        if self.active_edit_frontier.is_some()
            || self.active_reflow.is_some()
            || self.active_shaping_transition.is_some()
            || self.active_coordinated_caret.is_some()
        {
            return true;
        }
        false
    }

    // ── 协同 caret（评论 38：光标与前沿共享的单条运动轨迹） ──────────────

    /// Issue #826 评论 38/39：为本轮正文编辑创建或 retarget 协同 caret motion。
    ///
    /// 调用方（`update_cursor_visual_position` 的协同分支）传入当前真实
    /// visual 位置当起点参考、最新 canonical caret 当终点、`now` 当采样时刻。
    /// 这里只做三件事：
    /// 1. 没有活跃前沿 → 返回 false（调用方回退到独立光标 Tween）；
    /// 2. target 与时钟都没变 → 保持现有 motion（同一笔编辑重复调光标更新
    ///    不重启时钟、不重建路径）；
    /// 3. 否则创建/retarget：`started_at` / `duration_ms` 直接取当前前沿
    ///    （连续编辑 extend 已把前沿时钟移到最新，这里跟着走，天然与前沿同
    ///    progress）；轨迹按当前前沿路径重建（评论 39 BLOCKER 2：与 changed
    ///    visual path 同一视觉顺序，不是对起点终点拉斜线）；`travelled_base`
    ///    取旧 motion 在 `now` 的当前距离（屏幕真相，不退回调用方可能滞后
    ///    一帧的 visual —— 两帧之间没有 Scene Graph 帧写回 visual 时，
    ///    调用方的 visual 是上一帧的旧值，直接拿它当 start 会让光标跳回去）。
    ///
    /// 返回 true ⟺ 协同接管成功，调用方不得再建独立 Tween。
    pub(crate) fn begin_or_retarget_coordinated_caret(
        &mut self,
        start_x: f64,
        start_y: f64,
        target_x: f64,
        target_y: f64,
        now: Instant,
    ) -> bool {
        let Some(frontier) = self.active_edit_frontier.as_ref() else {
            return false;
        };
        let (started_at, duration_ms) = (frontier.started_at, frontier.duration_ms);
        if let Some(motion) = self.active_coordinated_caret.as_ref() {
            let same_target = (motion.target_x - target_x).abs() <= 0.01
                && (motion.target_y - target_y).abs() <= 0.01;
            let same_clock =
                motion.started_at == started_at && motion.duration_ms == duration_ms;
            if same_target && same_clock {
                return true;
            }
            let now = self.effective_text_animation_time(now);
            let old = self
                .active_coordinated_caret
                .as_ref()
                .expect("retarget 分支必有旧 motion");
            let (current_x, current_y) =
                old.position_at_distance(old.distance_at_progress(old.sample_progress(now)));
            let (segments, total_length) = Self::coordinated_path_from_frontier(
                frontier,
                current_x,
                current_y,
                target_x,
                target_y,
            );
            self.active_coordinated_caret = Some(CoordinatedCaretMotion {
                segments,
                total_length,
                started_at,
                duration_ms,
                target_x,
                target_y,
            });
            return true;
        }
        let (segments, total_length) =
            Self::coordinated_path_from_frontier(frontier, start_x, start_y, target_x, target_y);
        self.active_coordinated_caret = Some(CoordinatedCaretMotion {
            segments,
            total_length,
            started_at,
            duration_ms,
            target_x,
            target_y,
        });
        true
    }

    /// Issue #826 评论 39 BLOCKER 2：按当前前沿路径构造 caret 分段轨迹。
    ///
    /// 吐字侧有可见 path 就跟着吐字侧走，纯吞字跟着吞字侧走（方向天然一致，
    /// Backspace 的 Backward 段序直接可用）；两侧都没有可见 path（Enter /
    /// shaping 全接管）时退成单段 caret-only（旧 caret 直达新 caret，文字侧
    /// 只剩 Reflow/Shaping 独立跑）。
    ///
    /// 首段起点钉死在 `start`（创建时是屏幕 visual，retarget 时是旧 motion
    /// 当前位置），末段终点钉死在 `target`（最新 canonical caret）：glyph
    /// 矩形与 caret 矩形的定位基准天然差几个像素，不对齐首尾第一帧跳、
    /// 最后一帧还得 snap。钉死后途中仍在行段上走，首尾精确无跳变。
    fn coordinated_path_from_frontier(
        frontier: &EditFrontierState,
        start_x: f64,
        start_y: f64,
        target_x: f64,
        target_y: f64,
    ) -> (Vec<CaretMotionSegment>, f64) {
        fn segments_of(regions: &[FrontierRegion]) -> (Vec<CaretMotionSegment>, f64) {
            let mut segments = Vec::new();
            let mut total = 0.0;
            for region in regions {
                for segment in &region.path.segments {
                    total += segment.visual_length;
                    segments.push(CaretMotionSegment {
                        x_from: segment.x_from,
                        x_to: segment.x_to,
                        y: segment.y,
                        h: segment.h,
                        visual_length: segment.visual_length,
                    });
                }
            }
            (segments, total)
        }
        let (reveal_segments, reveal_total) = segments_of(&frontier.reveal.regions);
        let (mut segments, _) = if reveal_total > 1e-9 {
            (reveal_segments, reveal_total)
        } else {
            let (conceal_segments, conceal_total) = segments_of(&frontier.conceal.regions);
            if conceal_total <= 1e-9 {
                let dx = target_x - start_x;
                let dy = target_y - start_y;
                let length = dx.hypot(dy).max(f64::MIN_POSITIVE);
                return (
                    vec![CaretMotionSegment {
                        x_from: start_x,
                        x_to: target_x,
                        y: start_y,
                        h: dy.abs(),
                        visual_length: length,
                    }],
                    length,
                );
            }
            (conceal_segments, conceal_total)
        };
        // 钉死首尾并重算首末段长度（中间段原样保留行段几何）。
        // 注意：钉死只改 caret 自己的距离系（首点=屏幕 caret、末点=canonical），
        // 前沿 reveal/conceal 的距离系原样不动 —— 两边本来就是两套几何
        // （bearings 差），边界永远由 caret 位置投影得到，与距离系无关。
        if let Some(first) = segments.first_mut() {
            first.x_from = start_x;
            first.visual_length = (first.x_to - first.x_from).abs();
        }
        if let Some(last) = segments.last_mut() {
            last.x_to = target_x;
            last.visual_length = (last.x_to - last.x_from).abs();
        }
        let total = segments.iter().map(|s| s.visual_length).sum();
        (segments, total)
    }

    /// Issue #826 评论 38/39：Scene Graph 每帧采样协同 caret 的**唯一入口**。
    ///
    /// 用 `effective_text_animation_time` 采样：滚动 pause 期间正文时间钉在
    /// `paused_at`，协同光标跟着冻住（评论 35 原则同样适用于协同光标）。
    /// caret 位置沿分段轨迹由同一 distance 求出（评论 39 BLOCKER 2，不拉斜线）。
    /// progress 到 1 时把 visual 精确落到 canonical target 并清掉 motion。
    /// 返回 None ⟺ 当前没有协同 motion（调用方走独立 `tick_animation`）。
    pub(crate) fn sample_coordinated_caret(
        &mut self,
        frame_now: Instant,
    ) -> Option<CoordinatedCaretSample> {
        let now = self.effective_text_animation_time(frame_now);
        let motion = self.active_coordinated_caret.as_ref()?;
        let progress = motion.sample_progress(now);
        if progress >= 1.0 {
            let (x, y) = (motion.target_x, motion.target_y);
            self.active_coordinated_caret = None;
            Some(CoordinatedCaretSample {
                x,
                y,
                progress: 1.0,
                finished: true,
            })
        } else {
            let (x, y) = motion.position_at_distance(motion.distance_at_progress(progress));
            Some(CoordinatedCaretSample {
                x,
                y,
                progress,
                finished: false,
            })
        }
    }

    pub(crate) fn has_active_coordinated_caret(&self) -> bool {
        self.active_coordinated_caret.is_some()
    }

    /// 任何独立的光标决策（点击/方向键/Snap/纯光标 Tween）都会取代协同 motion：
    /// 用户亲自接管了光标，前沿继续按自己的时钟播完，不再要求同 progress。
    pub(crate) fn clear_coordinated_caret(&mut self) {
        self.active_coordinated_caret = None;
    }

    /// 测试用 —— 当前协同 motion 的 (target_x, target_y, duration_ms, total_length)。
    #[cfg(test)]
    pub(crate) fn coordinated_caret_for_test(&self) -> Option<(f64, f64, u64, f64)> {
        self.active_coordinated_caret.as_ref().map(|m| {
            (m.target_x, m.target_y, m.duration_ms, m.total_length)
        })
    }

    /// 测试用 —— 当前协同 motion 按 `now` 算的 (progress, distance)。
    #[cfg(test)]
    pub(crate) fn coordinated_distance_for_test(
        &self,
        now: std::time::Instant,
    ) -> Option<(f64, f64)> {
        let motion = self.active_coordinated_caret.as_ref()?;
        let now = self.effective_text_animation_time(now);
        let progress = motion.sample_progress(now);
        Some((progress, motion.distance_at_progress(progress)))
    }

    /// 测试用 —— 当前协同 motion 的分段轨迹（首尾已钉死 visual/target）。
    ///
    /// 返回每段 (x_from, x_to, y, h, visual_length)。caret 侧的段查找必须走
    /// 它，不能走前沿 regions —— 钉死端点后两边的距离系差几个像素
    /// （glyph/caret 几何差），混用会错段；生产边界走投影，不受此影响。
    #[cfg(test)]
    pub(crate) fn coordinated_segments_for_test(&self) -> Vec<(f64, f64, f64, f64, f64)> {
        self.active_coordinated_caret
            .as_ref()
            .map(|m| {
                m.segments
                    .iter()
                    .map(|s| (s.x_from, s.x_to, s.y, s.h, s.visual_length))
                    .collect()
            })
            .unwrap_or_default()
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

/// Issue #826: 遮罩前沿的正式诊断事件。
fn record_frontier_diagnostic(
    request: &EditFrontierRequest,
    kind: EditFrontierKind,
    frontier: Option<&EditFrontierState>,
    shaping: Option<&ShapingTransitionState>,
) {
    let mut fields: std::collections::BTreeMap<String, serde_json::Value> =
        std::collections::BTreeMap::new();
    fields.insert("frontier_kind".to_string(), serde_json::json!(kind.label()));
    // Issue #826 评论 7：前沿不再有二维起点/目标，改为记录两条视觉路径的段数。
    fields.insert(
        "reveal_segments".to_string(),
        serde_json::json!(frontier.map(|f| f.reveal.regions.len()).unwrap_or(0)),
    );
    fields.insert(
        "conceal_segments".to_string(),
        serde_json::json!(frontier.map(|f| f.conceal.regions.len()).unwrap_or(0)),
    );
    fields.insert(
        "reveal_length".to_string(),
        serde_json::json!(frontier
            .map(|f| f
                .reveal
                .regions
                .iter()
                .map(|t| t.path.total_length)
                .sum::<f64>())
            .unwrap_or(0.0)),
    );
    fields.insert(
        "conceal_length".to_string(),
        serde_json::json!(frontier
            .map(|f| f
                .conceal
                .regions
                .iter()
                .map(|t| t.path.total_length)
                .sum::<f64>())
            .unwrap_or(0.0)),
    );
    fields.insert(
        "conceal_direction".to_string(),
        serde_json::json!(frontier.map(|f| f.conceal_direction.label()).unwrap_or("")),
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
    // Issue #826 评论 24：mixed visual cluster 的交接对数与整块归属。
    //
    // 前沿的 `shaping_*_owned` 非空就说明有 cluster 被整块移交出去了 ——
    // 排查「某个字突然闪一下 / 形状跳变」时先看这两个字段：非零说明这一笔
    // 碰到了 Qt 合成 cluster，动画走的是交接层而不是普通 Reveal/Conceal。
    fields.insert(
        "shaping_transition_spans".to_string(),
        serde_json::json!(shaping.map(|s| s.groups.len()).unwrap_or(0)),
    );
    fields.insert(
        "shaping_old_owned".to_string(),
        serde_json::json!(shaping.map(|s| s.owned_old_clusters()).unwrap_or_default()),
    );
    fields.insert(
        "shaping_new_owned".to_string(),
        serde_json::json!(shaping.map(|s| s.owned_new_clusters()).unwrap_or_default()),
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
        "anim_frontier: kind={} reveal_len={:.1} conceal_len={:.1} inserted={:?} deleted={:?}",
        kind.label(),
        frontier
            .map(|f| f
                .reveal
                .regions
                .iter()
                .map(|t| t.path.total_length)
                .sum::<f64>())
            .unwrap_or(0.0),
        frontier
            .map(|f| f
                .conceal
                .regions
                .iter()
                .map(|t| t.path.total_length)
                .sum::<f64>())
            .unwrap_or(0.0),
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
