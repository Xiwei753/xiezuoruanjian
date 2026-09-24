//! Linux Qt 文字动画协调器 — 核心结构与非组合编辑方法。
//!
//! 组合编辑（composition update/commit/cancel）方法在 `composition.rs`。
//! rebase/cursor_motion/transaction_builder/render_plan_builder 在各自子模块。

use std::collections::HashMap;
use std::time::Instant;

use writer_core::editor::OffsetMap;

use crate::sujian_editor_item::animated_slice::AnimatedSliceKind;
use crate::sujian_editor_item::layout_revision::LayoutRevision;
use crate::sujian_editor_item::layout_snapshot::{
    EditorLayoutSnapshot, LineSnapshotId,
};
use crate::sujian_editor_item::transaction_key::VisualTransactionKey;
use crate::sujian_editor_item::animation::{
    PreparedTransactionQueue, TextVisualOperationKind,
    TextVisualTransactionState,
};
use crate::sujian_editor_item::editor_animation_debug_log;

/// Issue #722 评论 5750218208: 从 `EditorLayoutSnapshot` 的 `line_snapshots` 中
/// 按 `visual_line_id` 查找行几何（文档坐标的 top/bottom）。
///
/// 直接返回 `PreparedLineSnapshot.visual_line_top` / `visual_line_bottom`
/// （= `VisualLine.y` / `VisualLine.y + VisualLine.height`），不再扫描 cluster
/// ink bounds 猜行高——空行没有 cluster，cluster bounds 也不含行间距，
/// 用 cluster 会导致空行高度为 0、软换行附近行边界错误。
/// 找不到对应行时返回 `(0.0, 0.0)`。
pub(crate) fn find_line_geometry_in_snapshot(
    snapshot: &EditorLayoutSnapshot,
    visual_line_id: Option<usize>,
) -> (f64, f64) {
    match visual_line_id {
        Some(id) => {
            for line in &snapshot.line_snapshots {
                if line.visual_line_id == id {
                    return (line.visual_line_top, line.visual_line_bottom);
                }
            }
            (0.0, 0.0)
        }
        None => (0.0, 0.0),
    }
}

/// Issue #690 评论 5675007226 步骤 1: 同一帧的统一时间采样。
///
/// `update_paint_node()` 入口处取一次 `Instant::now()` 作为 `frame_now`，
/// 后续文字 progress、光标 progress、cursor timeline sample 全部从这一个时间点计算。
/// 消除 GUI 线程 FrameAnimation tick 和 Scene Graph 渲染帧之间的采样偏差。
///
/// 每个 active 正文事务的 progress 在 `build_render_plan_full()` 入口处只算一次，
/// 写入 `progress_by_key`，后面 `build_text_animation_plan` / `compute_coordinated_cursor`
/// 都从同一个 sample 读取，不再各自 `Instant::now()`。
pub(crate) struct AnimationFrameSample {
    /// 本帧统一采样时间点。
    pub frame_now: Instant,
    progress_by_key: HashMap<VisualTransactionKey, f64>,
}

impl AnimationFrameSample {
    pub fn new(frame_now: Instant) -> Self {
        Self {
            frame_now,
            progress_by_key: HashMap::new(),
        }
    }

    pub fn set_progress(&mut self, key: VisualTransactionKey, progress: f64) {
        self.progress_by_key.insert(key, progress);
    }

    /// 读取本帧该事务的预计算 progress（未记录则视为 0）。
    pub fn progress(&self, key: VisualTransactionKey) -> f64 {
        *self.progress_by_key.get(&key).unwrap_or(&0.0)
    }
}

/// Linux Qt 文字动画协调器 — 管理动画事务的生命周期和 rebase。
///
/// - `next_key_id`：事务键 ID 分配器（单调递增），每个事务有唯一键用于取消和 rebase。
/// - `prepared_queue`：已准备好的动画事务队列，按时间线顺序执行。
/// - `layout_revision`：上次处理事务时的布局修订号，用于检测布局是否已变化。
///   与 `EditorKernel.revision` 不同——`layout_revision` 跟踪排版结果变更（含窗口宽度、字号等），
///   `revision` 跟踪文本内容变更。两者独立递增。
pub(crate) struct LinuxEditorAnimationCoordinator {
    next_key_id: u64,
    pub(crate) prepared_queue: PreparedTransactionQueue,
    /// 打字/预输入动画时长（毫秒）。本地生成的事务不来自 Core 的
    /// `EditorVisualTransaction`（已删除），因此在此持有该视觉配置，
    /// 与 `PreparedEditMotion` 把 `duration_ms` 放进结构体的设计方向一致。
    pub(crate) typing_animation_duration_ms: u32,
    /// 光标平滑移动动画时长（毫秒）。
    pub(crate) cursor_animation_duration_ms: u32,
}

impl LinuxEditorAnimationCoordinator {
    pub fn new() -> Self {
        Self {
            next_key_id: 1,
            prepared_queue: PreparedTransactionQueue::new(),
            typing_animation_duration_ms: 160,
            cursor_animation_duration_ms: 120,
        }
    }

    pub(crate) fn alloc_key(&mut self) -> VisualTransactionKey {
        let id = self.next_key_id;
        self.next_key_id += 1;
        VisualTransactionKey::new(id, id)
    }

    pub(crate) fn set_typing_animation_duration_ms(&mut self, ms: u32) {
        self.typing_animation_duration_ms = ms;
    }

    pub(crate) fn set_cursor_animation_duration_ms(&mut self, ms: u32) {
        self.cursor_animation_duration_ms = ms;
    }

    /// Issue #690 评论 5675007226 步骤 3+5: 冲突事务交棒给新事务，只留一条紧凑诊断事件。
    ///
    /// 逐单元采集当前可见比例与单元时间线（[`PreparedTextVisualTransaction::collect_rebase_frames`]），
    /// 再取消旧事务。四个正文编辑入口共用本函数，避免各自拿事务级 progress 重算可见比例
    /// ——那正是"上一笔吐到 60% 的字被重启"的来源。
    ///
    /// `preserve` 为 `Some((编辑的 old 坐标范围, OffsetMap))` 时（Insert/Delete 入口），
    /// 若旧事务里仍在播放的单元都没被本次编辑覆盖，就完全不取消它：事务 key 保留自己的
    /// snapshot/纹理所有权，单元沿自己的时间线播完（`editor.anim.keep`）。预输入入口传
    /// `None`——preedit 文本整体被替换，旧单元必然失效。
    ///
    /// Issue #690 评论 5680276931 + 5681206040: 返回值同时带上 `RebaseCaretHandoff`——
    /// 在取消旧事务之前用同一个 `now` 采样旧事务当前真正显示的 coordinated cursor rect
    /// 和旧 caret track 剩余时长，交给新事务作为 caret track 的起点和 duration。
    /// 这样 rebase 交棒后光标不再从逻辑 old caret 重新起步，而是与文字 reflow 同帧
    /// 从屏幕位置续播；连续交棒也精确，因为新 caret track 自带时间状态。
    ///
    /// Issue #710 评论 5733833897: 一次新编辑可能同时撞上多笔旧事务。`conflicting`
    /// 现在接收**全部**冲突事务的 key（`&[VisualTransactionKey]`），逐笔处理：
    /// - untouched 的 tx：emit `editor.anim.keep` 诊断，**不取消**，继续下一笔。
    /// - 受影响的 tx：采集 rebase frames，把 frame 的 byte_start/byte_end 从该旧事务
    ///   new 坐标系映射到 current-old 坐标系（用 `OffsetMap::build(&tx.new_text,
    ///   current_old_text)`），追加到累计 `all_rebase_frames`；cancel 该 tx。
    /// - caret handoff：在所有被取消的冲突事务中，找 `cursor_owner_epoch ==
    ///   current_cursor_epoch` 的事务。如果有多个，取 `key.transaction_id` 最大的
    ///   （最新创建的）。对选中的那一笔采样 caret 构造 `RebaseCaretHandoff`。
    ///   如果没有冲突事务拥有 coordinated caret，handoff 为 None。

    /// Issue #738 评论 5796693007 问题1: 正文编辑路径 prepare 阶段——采 rebase frame +
    /// caret handoff，取消真正被覆盖的冲突事务，但还不创建新事务。
    ///
    /// 在旧事务还活着时调用（CaretDriven 还没被推到终态），采到的是旧事务真实当前帧。
    /// 返回 `PreparedRebaseHandoff` 供后续 `create_transaction_from_prepared_handoff` 使用。
    /// 返回 `None` 表示不创建新事务（early return 条件命中或 Cursor 分支）。
    ///
    /// `take_rebase_frames` 内部会 cancel 冲突事务，所以 prepare 阶段取消的旧事务在
    /// 后续 reconcile 阶段已经不在 active_transactions 里了。
    #[allow(clippy::too_many_arguments)]

    /// Issue #738 评论 5796693007 问题1: 正文编辑路径 create 阶段——用 prepare 阶段
    /// 采好的 rebase frame + caret handoff 创建新事务。
    ///
    /// 必须在 `prepare_rebase_handoff_for_edit` 之后、`reconcile_active_transactions_with_canonical`
    /// 之后调用。`prepared` 为 None 时直接返回 None（prepare 阶段 early return 或 Cursor 分支）。
    #[allow(clippy::too_many_arguments)]

    /// Issue #738 评论 5796693007 问题1: `process_transaction` 保留原内联 match 结构
    /// 作为 issue687/issue702 白盒测试的锚点（`EditorAnimationKind::Insert/Delete/Cursor =>`
    /// + `build_cluster_reflow_slices` 调用）。
    ///
    /// 正文编辑主路径 `prepare_edit_motion` 已改为显式调
    /// `prepare_rebase_handoff_for_edit` → `reconcile_active_transactions_with_canonical` →
    /// `create_transaction_from_prepared_handoff`，保证旧事务 CaretDriven 在 rebase frame
    /// 采好之后才 retire。此方法保留供测试锚点和潜在的未来直接调用，语义与
    /// prepare → create（中间不插 reconcile）等价。
    #[allow(clippy::too_many_arguments)]

    /// Issue #738 评论 5787277777: 把全部活动事务从上一份 canonical 几何重绑到这份新 canonical。
    ///
    /// 在 `record_visual_transaction` 里 `new_doc_snapshot` 已完成后、创建本次新事务之前调：
    /// 先把所有旧活动事务的 Timed Reflow unit 从"上一份 canonical 几何"重绑到这份新 canonical，
    /// 再处理本次新事务自己的 conflict/rebase。
    ///
    /// 流程：
    /// 1. 遍历全部 active transactions（不只遍历和新 edit byte range 相交的事务）。
    /// 2. CaretDriven 仍按现在的 owner/epoch 规则处理；旧事务失去 caret owner 后继续 retire
    ///    到 canonical（由 `build_text_animation_plan_with_sample` 每帧采样时处理）。
    /// 3. 对仍存活的 Timed Reflow unit 调 `rebind_timed_units_to_canonical`。
    /// 4. rebind 后已经没有 unit 的事务直接完成；还有 Timed unit 的继续播放。
    /// 5. 收集完后再生成本帧 clip rect / glyph plan（由 `build_render_plan_full` 完成）。
    ///    禁止 basis revision 旧于当前 canonical revision 的 unit 进入 `build_render_plan_full()`。
    pub(crate) fn reconcile_active_transactions_with_canonical(
        &mut self,
        current_text: &str,
        canonical_snapshot: &crate::editor::layout::CanonicalDocumentVisualSnapshot,
        layout_revision: LayoutRevision,
        now: Instant,
    ) {
        let mut keys_to_complete: Vec<VisualTransactionKey> = Vec::new();
        // Issue #738 评论 5795950264 问题1: 先收集需要处理的事务 key，再逐个
        // retire + rebind。retire_caret_driven_units_for_transaction 需要 &mut self，
        // 不能在 active_transactions_mut() 的循环里直接调。
        let keys: Vec<VisualTransactionKey> = self
            .prepared_queue
            .active_transactions()
            .iter()
            .filter(|t| {
                t.state != TextVisualTransactionState::Cancelled
                    && t.state != TextVisualTransactionState::Completed
            })
            .map(|t| t.key)
            .collect();
        for key in keys {
            // Issue #738 评论 5795950264 问题1: 先 retire CaretDriven units，让旧 caret
            // track 永久失去 ownership，再 rebind Timed Reflow。如果先 rebind 会把
            // layout_basis_revision 提升到当前 canonical，导致 basis 守卫
            //（build_text_animation_plan_with_sample / find_cursor_transaction_for_target）
            // 不再 retire 旧 CaretDriven，旧 caret track 重新拿到 ownership 在新 canonical
            // 上继续用旧布局几何。retire 把 CaretDriven 落到终态并置 caret_motion_retired=true，
            // ReflowMove/ReflowCrossFade 保留不动继续播。
            self.retire_caret_driven_units_for_transaction(key);
            let tx = match self
                .prepared_queue
                .active_transactions_mut()
                .iter_mut()
                .find(|t| t.key == key)
            {
                Some(t) => t,
                None => continue,
            };
            // 把 Timed Reflow unit 重绑到当前 canonical。
            tx.rebind_timed_units_to_canonical(
                current_text,
                canonical_snapshot,
                layout_revision,
                now,
            );
            // rebind 后已经没有 unit 的事务直接完成。
            if tx.units.is_empty() {
                keys_to_complete.push(tx.key);
            }
        }
        // 完成空事务（rebind 移除了全部 unit，让 canonical 正文接管）。
        for key in keys_to_complete {
            self.prepared_queue.complete(key);
        }
    }

    /// Issue #738 评论 5798704669 问题2: 只读入口，收集所有未完成 Timed Reflow
    ///（ReflowMove / ReflowCrossFade）在 current text 中的目标 byte ranges。
    ///
    /// 遍历所有活动 transaction（非 Cancelled / Completed），对每笔 transaction
    /// 用 `OffsetMap::build(tx.new_snapshot.virtual_text, current_text)` 把每个
    /// `reflow_anchor.byte_start/end` 映射到 current text。没有 anchors 的 fallback
    /// unit 用 slice 的 byte range。返回去重后的 current-text byte ranges。
    ///
    /// `build_editor_layout_snapshot_with_canonical` 把这些 ranges 对应的新 canonical
    /// line ids 并入 clusters 注入覆盖，确保远处 Reflow 的目标行在 canonical 里有 clusters，
    /// `find_clusters_in_canonical` 不再返回空，`rebind_timed_units_to_canonical`
    /// 不再误判 `RebindDecision::Remove`。
    pub(crate) fn collect_active_rebind_ranges(&self, current_text: &str) -> Vec<(usize, usize)> {
        let mut ranges: Vec<(usize, usize)> = Vec::new();
        for tx in self.prepared_queue.active_transactions() {
            if tx.state == TextVisualTransactionState::Cancelled
                || tx.state == TextVisualTransactionState::Completed
            {
                continue;
            }
            let tx_new_text = match tx.new_snapshot.as_ref() {
                Some(s) => s.virtual_text.as_str(),
                None => continue,
            };
            let per_tx_map = OffsetMap::build(tx_new_text, current_text);
            for unit in &tx.units {
                if !matches!(
                    unit.slice.kind,
                    AnimatedSliceKind::ReflowMove | AnimatedSliceKind::ReflowCrossFade
                ) {
                    continue;
                }
                if unit.slice.reflow_anchors.is_empty() {
                    // fallback: 用 slice 整体 byte range
                    if unit.slice.byte_start < unit.slice.byte_end {
                        if let Some((ms, me)) = per_tx_map
                            .map_old_range_to_new(unit.slice.byte_start, unit.slice.byte_end)
                        {
                            ranges.push((ms, me));
                        }
                    }
                    continue;
                }
                for anchor in &unit.slice.reflow_anchors {
                    if let Some((ms, me)) =
                        per_tx_map.map_old_range_to_new(anchor.byte_start, anchor.byte_end)
                    {
                        ranges.push((ms, me));
                    }
                }
            }
        }
        // 去重 + 合并重叠 ranges
        ranges.sort_by_key(|r| r.0);
        let mut merged: Vec<(usize, usize)> = Vec::new();
        for r in ranges {
            if let Some(last) = merged.last_mut() {
                if r.0 <= last.1 {
                    last.1 = last.1.max(r.1);
                    continue;
                }
            }
            merged.push(r);
        }
        merged
    }

    pub fn finish_by_key(&mut self, key: VisualTransactionKey) -> Option<Vec<LineSnapshotId>> {
        self.prepared_queue.complete(key)
    }

    /// Issue #736 评论 5786231506: 返回当前所有 active transaction 实际还引用的
    /// 去重 LineSnapshotId。事务完成、cancel、rebase 后，队列是"哪些视觉资源
    /// 仍有人用"的唯一事实源。
    pub(crate) fn collect_active_snapshot_ids(&self) -> Vec<LineSnapshotId> {
        let mut ids: Vec<LineSnapshotId> = Vec::new();
        for tx in self.prepared_queue.active_transactions() {
            if !matches!(
                tx.state,
                TextVisualTransactionState::Completed | TextVisualTransactionState::Cancelled
            ) {
                ids.extend(tx.snapshot_ids());
            }
        }
        ids.sort_by_key(|id| (id.layout_revision, id.paragraph_id, id.visual_line_ordinal));
        ids.dedup();
        ids
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

    /// Issue #702 评论 5708436497: 只做队列方法 `PreparedTransactionQueue::has_active_insert()`
    /// 的转发，不再承载"任意正文事务"的语义（那由 `has_active_text_transaction()` 负责）。
    /// 本方法只认 `TextVisualOperationKind::Insert`，用于输入时的光标 blink 抑制。
    pub fn has_active_insert(&self) -> bool {
        self.prepared_queue.has_active_insert()
    }

    /// Issue #702 评论 5708209114: 判断是否存在任意活跃正文视觉事务
    /// （Insert / Delete / CompositionUpdate / CompositionCommitOrCancel）。
    ///
    /// 与 [`has_active_insert`] 的区别：`has_active_insert()` 现在真正只查 Insert
    /// （Issue #702 评论 5708436497 修正了队列实现里缺失的 `operation_kind` 过滤），
    /// 只保留给"输入时抑制光标闪烁"这种 Insert 专属语义。本方法覆盖所有正文事务类型，
    /// 供 `build_cursor_plan()` 判断"正文协同是否活跃"——只要存在任意活跃正文事务且
    /// coordinated_enabled，就不能创建纯光标 Tween，正文光标只由
    /// `compute_coordinated_cursor_position()` 驱动。
    ///
    /// Issue #705 评论 5717380886: 本方法**不**检查 `cursor_owner_epoch`，保持
    /// "有没有活动事务"的语义。理由：本方法用于 `build_cursor_plan` 中的
    /// `_blink_mode` 计算，blink mode 不应因 epoch 变化而改变——文字动画还在播
    /// 就应该 suppress blink。直接遍历 active_transactions 判断，不调
    /// `active_text_transaction_key()`（后者现在带 epoch 检查）。
    pub(crate) fn has_active_text_transaction(&self) -> bool {
        for tx in self.prepared_queue.active_transactions().iter().rev() {
            if matches!(
                tx.state,
                TextVisualTransactionState::Completed | TextVisualTransactionState::Cancelled
            ) {
                continue;
            }
            return true;
        }
        false
    }

    /// Issue #686 评论 5664857575 领域2：返回当前活动的正文编辑事务（Insert/Delete，
    /// 不含 Cursor）的 key。光标按事务身份绑定，不靠浮点坐标反查。
    ///
    /// 从新到旧找最近一条 state 不是 Completed/Cancelled 的正文事务（operation_kind
    /// 为 Insert/Delete/CompositionUpdate/CompositionCommitOrCancel）。
    /// Issue #702 评论 5707449688 问题 2: TextVisualOperationKind::Cursor 已删除，
    /// 所有非 Completed/Cancelled 的事务都是正文事务。
    ///
    /// Issue #705 评论 5717380886: 本方法**不**检查 `cursor_owner_epoch`，
    /// 只返回最近一条活动正文事务的 key。epoch 检查由调用方负责
    /// （`find_cursor_transaction_for_target` / `compute_coordinated_cursor_position`
    /// 在拿到 key 后检查 `tx.cursor_owner_epoch != current_cursor_epoch`）。
    /// 保留不带参数的签名是为了让 `has_active_text_transaction` 和结构守卫测试
    /// 能继续用"有没有活动事务"的语义判断。
    pub(crate) fn active_text_transaction_key(&self) -> Option<VisualTransactionKey> {
        for tx in self.prepared_queue.active_transactions().iter().rev() {
            if matches!(
                tx.state,
                TextVisualTransactionState::Completed | TextVisualTransactionState::Cancelled
            ) {
                continue;
            }
            // Issue #727 评论 5760650874 方案 A / Issue #735 评论 5773604666 问题3:
            // 永久退休 caret motion 的事务不再被选为 active caret owner。
            // find_cursor_transaction_for_target / compute_coordinated_cursor_position
            // 都通过本方法取事务后驱动 caret，retired 事务不应再驱动 caret
            // （CaretDriven units 已落到终态，已 Snap 回 canonical）。
            if tx.caret_motion_retired {
                continue;
            }
            return Some(tx.key);
        }
        None
    }

    /// Issue #705 评论 5717380886: 返回当前活动的正文编辑事务的 key，
    /// 且其 `cursor_owner_epoch == current_cursor_epoch`。
    ///
    /// Issue #727 约束 1 / Issue #735 评论 5773604666 问题3: epoch 不一致时返回 None——
    /// 事务立刻失去 caret motion ownership，CaretDriven units（InsertReveal/DeleteConceal）
    /// 立即落到 canonical final state（`start_fraction = target_fraction`），
    /// 不再继续播放。ReflowMove/ReflowCrossFade 作为独立 passive reflow track 继续。
    ///
    /// 本方法供 `build_cursor_plan` 内部判断"正文协同是否活跃（epoch 一致）"用。
    /// `find_cursor_transaction_for_target` / `compute_coordinated_cursor_position`
    /// 直接调 `active_text_transaction_key()` 后手动检查 epoch，以保留结构守卫测试
    /// 期望的 `self.active_text_transaction_key()` 调用形式。

    /// Issue #679 评论 5657313927 (3d): 按当前光标 target 查找对应的事务。
    ///
    /// Issue #686 评论 5664857575 领域2：当存在活动正文事务时，直接返回该事务的 key
    /// 和它的 old/new cursor rect，不靠浮点坐标相等反查。光标按事务身份绑定。
    /// 只有在没有正文事务时才走原来的 CursorOnly 查找逻辑（按 target x/y 匹配）。
    ///
    /// Issue #705 评论 5717380886: 增加 `current_cursor_epoch` 参数。
    /// 调 `active_text_transaction_key()` 取活动事务后，检查其 `cursor_owner_epoch`
    /// 是否等于 `current_cursor_epoch`。
    ///
    /// Issue #735 评论 5773604666 问题3: epoch 不一致时不再只是 fall through，
    /// 而是触发收口逻辑——调用 `retire_caret_driven_units_for_transaction` 把
    /// CaretDriven units（InsertReveal/DeleteConceal）的 `start_fraction` 设为
    /// `target_fraction`（终态），并置 `caret_motion_retired = true`。
    /// ReflowMove/ReflowCrossFade 保留不动，作为独立 passive reflow track 继续。
    /// 不再存在"同一笔正文吞吐 transaction 还活着，但 caret_owner 已经不是它"
    /// 的状态。
    /// Issue #738 评论 5789470425 问题1: 增加 `current_layout_revision` 参数，
    /// 和 `active_text_transaction_key_with_epoch` 一样跳过 basis 不一致的事务。
    /// 旧事务即使 cursor_owner_epoch 一致，若 layout basis 已过期，也不能继续拥有
    /// coordinated caret——否则旧事务用旧 caret track 驱动光标，与 canonical 新布局分叉。

    /// Issue #735 评论 5773604666 问题3: 收口指定事务的 CaretDriven units。
    ///
    /// 当正文 edit motion 失去 caret ownership 时调用。把指定事务的
    /// CaretDriven units（InsertReveal/DeleteConceal）的 `start_fraction` 设为
    /// `target_fraction`（终态），并置 `caret_motion_retired = true`。
    /// ReflowMove/ReflowCrossFade 保留不动，作为独立 passive reflow track 继续。
    ///
    /// 调用后：
    /// - CaretDriven units 立即落到 canonical final state，不再继续播。
    /// - `active_text_transaction_key_with_epoch` / `active_text_transaction_key`
    ///   永远跳过此事务（`caret_motion_retired == true`）。
    /// - 如果事务中还有 Timed unit（Reflow），事务不立即 Completed，等它们播完。
    /// - 如果没有 Timed unit，事务可在下一帧 Completed。
    ///
    /// 幂等：对已 retired 的事务再次调用是 no-op（start_fraction 已是 target_fraction）。
    pub(crate) fn retire_caret_driven_units_for_transaction(&mut self, key: VisualTransactionKey) {
        let tx = match self
            .prepared_queue
            .active_transactions_mut()
            .iter_mut()
            .find(|t| t.key == key)
        {
            Some(t) => t,
            None => return,
        };
        // 已 retired 的事务无需重复收口。
        if tx.caret_motion_retired {
            return;
        }
        // 只有含 CaretDriven units 的事务才需要收口。
        if !tx.has_caret_driven_units() {
            // 纯 Reflow 事务本来就不驱动 caret，只置 retired 标记防止重新被选为 owner。
            tx.caret_motion_retired = true;
            return;
        }
        tx.retire_caret_driven_units();
        tx.caret_motion_retired = true;
        editor_animation_debug_log(&format!(
            "retire_caret_driven: key={:?} op={:?} units={} — CaretDriven units 已落到终态",
            tx.key,
            tx.operation_kind,
            tx.units.len(),
        ));
    }

    pub fn has_prepared_or_rendering(&self) -> bool {
        self.prepared_queue.active_transactions().iter().any(|t| {
            t.state == TextVisualTransactionState::Prepared
                || t.state == TextVisualTransactionState::Rendering
        })
    }

    #[allow(clippy::too_many_arguments)]

    /// Issue #727 约束 7: 滚动开始时，CaretDriven 事务（InsertReveal/DeleteConceal）
    /// 依赖 caret motion track，caret track 被终止时它们必须立即完成到 canonical 状态，
    /// 不能只 pause（pause 后 resume 时 caret track 已不存在，数据依赖链断裂）。
    /// Timed 事务（ReflowMove/ReflowCrossFade）有独立时间线，可以正常 pause/resume。
    ///
    /// 此方法先完成所有含 CaretDriven unit 的事务，再 pause 剩下的 Timed 事务。
    /// 返回被完成事务的 snapshot IDs，供调用方清理 texture cache。
    pub(crate) fn pause_all(&mut self) -> Vec<LineSnapshotId> {
        use crate::sujian_editor_item::animation::VisualUnitTiming;

        // 1. 找出所有含 CaretDriven unit 的活跃事务，完成它们到 canonical 状态。
        let caret_driven_keys: Vec<VisualTransactionKey> = self
            .prepared_queue
            .active_transactions()
            .iter()
            .filter(|t| {
                t.state != TextVisualTransactionState::Completed
                    && t.state != TextVisualTransactionState::Cancelled
                    && t.units
                        .iter()
                        .any(|u| matches!(u.timing, VisualUnitTiming::CaretDriven { .. }))
            })
            .map(|t| t.key)
            .collect();
        let mut freed_snapshot_ids = Vec::new();
        for key in caret_driven_keys {
            if let Some(ids) = self.prepared_queue.complete(key) {
                freed_snapshot_ids.extend(ids);
            }
        }
        // 2. pause 剩下的活跃事务（纯 Timed：ReflowMove/ReflowCrossFade）。
        for tx in self.prepared_queue.active_transactions_mut() {
            tx.pause();
        }
        freed_snapshot_ids
    }

    pub(crate) fn resume_all(&mut self) {
        for tx in self.prepared_queue.active_transactions_mut() {
            tx.resume();
        }
    }

    /// Issue #727 评论 5760020833 问题2: 把本帧 Prepared 事务切到 Rendering，
    /// 并用当前 frame_now 启动 transaction timeline / Timed units / cursor track。
    /// 必须在 `sample_coordinated_motion_frame` 之前调用，否则刚进入 Prepared 的
    /// 新事务第一帧采样时状态仍为 Prepared → caret=None → InsertReveal/DeleteConceal
    /// 不画、clip 也不藏 → 第一帧直接显示最终正文 → 下一帧才从 progress≈0 开始动画，
    /// 形成固定的一帧"先显示最终文字，再开始动画"的闪烁。

    /// Issue #690 评论 5675007226 步骤 1+2: 接受 `frame_now`，统一采样文字和光标 progress。
    ///
    /// 文字和光标的 progress 全部从同一个 `frame_now` 计算，消除 GUI 线程 tick 和
    /// Scene Graph 渲染帧之间的采样偏差。当正文编辑事务活跃且 coordinated 动画启用时，
    /// 光标位置直接从 text animation progress 计算（跟随文字吞吐边界），
    /// 不再使用 GUI 线程上一帧留下的 `cursor_ctrl.visual_x/y`。

    /// Issue #727 约束 3: 采样本帧统一的 CoordinatedMotionFrame。
    ///
    /// 在 `build_render_plan_full` 入口处调用，先采样 caret motion 得到一份
    /// `SampledCaretFrame`，供 cursor layer 和文字 reveal/conceal 共享。
    ///
    /// Issue #727 约束 1 / Issue #735 评论 5773604666 问题3: epoch 不一致时返回 None——
    /// 事务立刻失去 caret motion ownership，CaretDriven units（InsertReveal/DeleteConceal）
    /// 已落到 canonical final state（不再继续播放）。

    /// Issue #701 评论 5699573227 第三阶段 (F5): 用同一份 `AnimationFrameSample`
    /// 采样 CursorOnly 光标位置。
    ///
    /// 当没有活跃文字事务但有 `cursor_ctrl.animation`（CursorOnly）时，从
    /// `frame_sample` 读取 driver 事务的 progress，按 ease-out-cubic 插值光标位置。
    /// 文字层和光标层都使用同一份 frame state。
    /// Issue #702 评论 5707449688 问题 2: 不再用 `anim.driver_key` 查事务，
    /// 直接用 CursorAnimationState 自己的 timeline（started_at + duration_ms）推进。

    /// Issue #690 评论 5675007226 步骤 1+3: 从同一 `AnimationFrameSample` 读取文字 progress，
    ///
    /// 替代原来的 `build_text_animation_plan()`（内部各自 `Instant::now()`）。
    /// Issue #722 评论 5747719529 核心语义：光标本身就是吞字/吐字的视觉边界
    /// （caret_geometry_determines_clip / clip_from_coordinated_caret）。
    /// 文字不能再维护一套会和 caret 分叉的独立 timeline 进度。InsertReveal/DeleteConceal
    /// 的裁切边界直接消费本帧 coordinated caret 的位置（`compute_frame_caret_driven`），
    /// caret 与文字使用同一个 frame_now 和同一个 from→to 几何轨迹。reflow/crossfade
    /// 继续用 unit 的时间线做几何插值；`start_fraction` 由 rebase 决定（新单元为 0，
    /// 被连续输入覆盖的单元从已显示比例继续）。

    /// Issue #690 评论 5675007226 步骤 2 + 5681206040: 协同光标直接计算最终屏幕位置。
    ///
    /// Issue #722 评论 5747719529 改法 4 + 核心语义：光标本身就是吞字/吐字的视觉边界。
    /// 不再从文字 glyph 切片反推光标位置（删除 rightmost_x.max() / conceal_edge.min()）。
    /// 光标位置只由 `PreparedCursorVisualTrack`（canonical old caret → canonical new caret）
    /// 插值决定，使用同一个 `frame_now` 和同一个 from→to 几何轨迹。
    /// - 有 cursor_visual_track 时：用 `sampled_rect(frame_now)` 插值（caret_driven_clip）。
    /// - 没有 track 时（首次事务未经过 rebase）：按事务 progress 插值 old/new cursor rect。
    /// - 前向 Delete（conceal_to_left_edge=false）：逻辑光标不移动，固定在 new_cursor_rect。
    ///
    /// 文字的 InsertReveal/DeleteConceal 裁切边界直接消费本帧 coordinated caret 的位置
    /// （caret_driven_clip），caret 与文字使用同一个 frame_now 和同一个 from→to 几何
    /// 轨迹。快速 rebase 时先采样当前 caret 边界，再把这个边界作为下一段动画起点。
    ///
    /// 返回 `(x, y, h)` 供 `build_render_plan_full` 直接写入 `CursorRenderState`。
    ///
    /// Issue #722 评论 5748596920 问题1: 返回的 `y` 是文档坐标（不减 scroll_y），
    /// `build_render_plan_full` 在写入 `CursorRenderState` 时用当前 scroll_y 转成视口 y。
    /// `x` 是水平坐标，不受 scroll_y 影响。
    ///
    /// Issue #705 评论 5717380886: 增加 `current_cursor_epoch` 参数。
    /// Issue #727 约束 1 / Issue #735 评论 5773604666 问题3: epoch 不一致时返回 None——
    /// 事务立刻失去 caret motion ownership，CaretDriven units 已落到 canonical
    /// final state（不再继续播放）。

    /// 返回当前最新正文编辑事务的操作类型，用于决定光标 blink mode。
    /// Issue #702 评论 5707449688 问题 2: TextVisualOperationKind::Cursor 已删除，
    /// 所有非 Completed/Cancelled 的事务都是正文事务。
    pub(crate) fn active_operation_kind(&self) -> Option<TextVisualOperationKind> {
        self.prepared_queue
            .active_transactions()
            .iter()
            .rev()
            .find(|t| {
                !matches!(
                    t.state,
                    TextVisualTransactionState::Completed | TextVisualTransactionState::Cancelled
                )
            })
            .map(|t| t.operation_kind)
    }

}

#[cfg(test)]

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sujian_editor_item::animated_slice::AnimatedSlice;
    use crate::sujian_editor_item::animation_mode::AnimationMode;
    use crate::sujian_editor_item::layout_snapshot::EditorLayoutSnapshot;
    use crate::sujian_editor_item::animation::{
        PreparedTextVisualTransaction, PreparedVisualUnit,
        TextVisualOperationKind, TransactionTimeline, VisualUnitTiming,
    };
    use crate::sujian_editor_item::transaction_key::VisualTransactionKey;

    /// 构造一笔处于 Pending 状态（活跃，非 Completed/Cancelled）的最小事务，
    /// 只设置 key 与 operation_kind，其余字段取最小值。用于 `has_active_insert()`
    /// 的语义验证——该方法只看 `operation_kind` 与 `state`，不依赖 units/snapshots。
    fn make_minimal_active_tx(
        key: VisualTransactionKey,
        operation_kind: TextVisualOperationKind,
    ) -> PreparedTextVisualTransaction {
        PreparedTextVisualTransaction {
            key,
            state: TextVisualTransactionState::Pending,
            operation_kind,
            timeline: TransactionTimeline::new(100),
            units: Vec::new(),
            old_cursor_rect: None,
            new_cursor_rect: None,
            cursor_visual_track: None,
            cancel_reason: None,
            texture_prepared: false,
            old_snapshot: None,
            new_snapshot: None,
            cursor_owner_epoch: 0,
            caret_motion_retired: false,
            visual_affected_byte_range_old: None,
            visual_affected_byte_range_new: None,
            layout_basis_revision: LayoutRevision::initial(),
        }
    }

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
    fn test_has_active_insert_returns_false_for_delete_only() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.prepared_queue.enqueue(make_minimal_active_tx(
            VisualTransactionKey::new(1, 1),
            TextVisualOperationKind::Delete,
        ));
        assert!(
            !coord.has_active_insert(),
            "只有 Delete 事务时 has_active_insert() 应为 false"
        );
        println!(
            "[BUGFIX_VERIFY] 只有 Delete 事务时 has_active_insert()==false \
             (Issue702 评论5708436497)"
        );
    }

#[test]
    fn test_has_active_insert_returns_false_for_composition_update_only() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.prepared_queue.enqueue(make_minimal_active_tx(
            VisualTransactionKey::new(2, 1),
            TextVisualOperationKind::CompositionUpdate,
        ));
        assert!(
            !coord.has_active_insert(),
            "只有 CompositionUpdate 事务时 has_active_insert() 应为 false"
        );
        println!(
            "[BUGFIX_VERIFY] 只有 CompositionUpdate 事务时 has_active_insert()==false \
             (Issue702 评论5708436497)"
        );
    }

#[test]
    fn test_has_active_insert_returns_false_for_composition_commit_or_cancel_only() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.prepared_queue.enqueue(make_minimal_active_tx(
            VisualTransactionKey::new(3, 1),
            TextVisualOperationKind::CompositionCommitOrCancel,
        ));
        assert!(
            !coord.has_active_insert(),
            "只有 CompositionCommitOrCancel 事务时 has_active_insert() 应为 false"
        );
        println!(
            "[BUGFIX_VERIFY] 只有 CompositionCommitOrCancel 事务时 has_active_insert()==false \
             (Issue702 评论5708436497)"
        );
    }

#[test]
    fn test_has_active_insert_returns_true_for_insert() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.prepared_queue.enqueue(make_minimal_active_tx(
            VisualTransactionKey::new(4, 1),
            TextVisualOperationKind::Insert,
        ));
        assert!(
            coord.has_active_insert(),
            "有活跃 Insert 事务时 has_active_insert() 应为 true"
        );
        println!(
            "[BUGFIX_VERIFY] 有活跃 Insert 事务时 has_active_insert()==true \
             (Issue702 评论5708436497)"
        );
    }

#[test]
    fn test_has_active_insert_returns_false_after_insert_completed() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let key = VisualTransactionKey::new(5, 1);
        coord
            .prepared_queue
            .enqueue(make_minimal_active_tx(key, TextVisualOperationKind::Insert));
        assert!(
            coord.has_active_insert(),
            "完成前 has_active_insert() 应为 true"
        );
        let removed = coord.prepared_queue.complete(key);
        assert!(removed.is_some(), "complete 应返回 Some（事务曾存在）");
        assert!(
            !coord.has_active_insert(),
            "Insert 事务 Completed 后 has_active_insert() 应为 false"
        );
        println!(
            "[BUGFIX_VERIFY] Insert 事务 Completed 后 has_active_insert()==false \
             (Issue702 评论5708436497)"
        );
    }

#[test]
    fn test_has_active_insert_returns_false_after_insert_cancelled() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        let key = VisualTransactionKey::new(6, 1);
        coord
            .prepared_queue
            .enqueue(make_minimal_active_tx(key, TextVisualOperationKind::Insert));
        assert!(
            coord.has_active_insert(),
            "取消前 has_active_insert() 应为 true"
        );
        let cancelled = coord.prepared_queue.cancel(key, "test_cancel");
        assert!(cancelled, "cancel 应返回 true（事务曾存在）");
        assert!(
            !coord.has_active_insert(),
            "Insert 事务 Cancelled 后 has_active_insert() 应为 false"
        );
        println!(
            "[BUGFIX_VERIFY] Insert 事务 Cancelled 后 has_active_insert()==false \
             (Issue702 评论5708436497)"
        );
    }

#[test]
    fn test_has_active_insert_returns_true_when_insert_mixed_with_other_kinds() {
        let mut coord = LinuxEditorAnimationCoordinator::new();
        coord.prepared_queue.enqueue(make_minimal_active_tx(
            VisualTransactionKey::new(7, 1),
            TextVisualOperationKind::Delete,
        ));
        coord.prepared_queue.enqueue(make_minimal_active_tx(
            VisualTransactionKey::new(7, 2),
            TextVisualOperationKind::CompositionUpdate,
        ));
        coord.prepared_queue.enqueue(make_minimal_active_tx(
            VisualTransactionKey::new(7, 3),
            TextVisualOperationKind::Insert,
        ));
        assert!(
            coord.has_active_insert(),
            "Insert 与其他类型事务共存时 has_active_insert() 应为 true"
        );
        println!(
            "[BUGFIX_VERIFY] Insert 与 Delete/Composition 共存时 has_active_insert()==true \
             (Issue702 评论5708436497)"
        );
    }

#[test]
    fn test_has_active_insert_only_returns_true_for_insert_kind() {
        // 综合断言：空队列返回 false；逐个入队非 Insert 事务仍返回 false；
        // 入队 Insert 事务后翻转为 true。覆盖队列层面 `any` 迭代顺序无关性。
        let mut coord = LinuxEditorAnimationCoordinator::new();
        assert!(
            !coord.has_active_insert(),
            "空队列 has_active_insert() 应为 false"
        );
        coord.prepared_queue.enqueue(make_minimal_active_tx(
            VisualTransactionKey::new(8, 1),
            TextVisualOperationKind::Delete,
        ));
        coord.prepared_queue.enqueue(make_minimal_active_tx(
            VisualTransactionKey::new(8, 2),
            TextVisualOperationKind::CompositionUpdate,
        ));
        coord.prepared_queue.enqueue(make_minimal_active_tx(
            VisualTransactionKey::new(8, 3),
            TextVisualOperationKind::CompositionCommitOrCancel,
        ));
        assert!(
            !coord.has_active_insert(),
            "仅有 Delete/CompositionUpdate/CompositionCommitOrCancel 时 has_active_insert() 应为 false"
        );
        coord.prepared_queue.enqueue(make_minimal_active_tx(
            VisualTransactionKey::new(8, 4),
            TextVisualOperationKind::Insert,
        ));
        assert!(
            coord.has_active_insert(),
            "追加 Insert 事务后 has_active_insert() 应翻转为 true"
        );
        println!(
            "[BUGFIX_VERIFY] has_active_insert() 综合语义：仅 Insert 翻转 true \
             (Issue702 评论5708436497 FIXED)"
        );
    }

}
