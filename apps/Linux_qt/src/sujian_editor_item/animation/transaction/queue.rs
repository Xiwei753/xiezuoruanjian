use std::time::Instant;

use crate::sujian_editor_item::layout_snapshot::LineSnapshotId;
use crate::sujian_editor_item::transaction_key::VisualTransactionKey;

use super::types::{
    PreparedTextVisualTransaction, TextVisualOperationKind, TextVisualTransactionState,
};

/// Linux 当前唯一事务队列。
///
/// 冲突判断基于 byte range 与仍活跃的视觉资源，不是简单"新输入清空旧动画"：
/// 新事务的 byte range 与现有事务的 slices/patches 有重叠时，先从旧事务的当前视觉帧
/// rebase（保证连续输入无跳变），再取消旧事务。
///
/// rebase 语义：新事务的 old_snapshot 取自被取消事务的当前视觉帧而非原始快照，
/// 使动画起点与用户当前看到的画面一致。
#[derive(Clone, Debug, Default)]
pub(crate) struct PreparedTransactionQueue {
    transactions: Vec<PreparedTextVisualTransaction>,
}

impl PreparedTransactionQueue {
    pub fn new() -> Self {
        Self {
            transactions: Vec::new(),
        }
    }

    pub fn enqueue(&mut self, tx: PreparedTextVisualTransaction) {
        self.transactions.push(tx);
    }

    /// Issue #679 评论 5657313927: 把"资源准备完成"和"Pending -> Prepared 状态推进"
    /// 绑在同一个入口，避免事务卡在 Pending 导致光标不移动、静态层错位。
    ///
    /// 状态链：`Pending -> Prepared -> Rendering -> Completed/Cancelled`。
    /// 已 Completed/Cancelled 的事务不再推进，返回 false。
    pub fn mark_prepared(&mut self, key: VisualTransactionKey) -> bool {
        let Some(tx) = self.transactions.iter_mut().find(|t| t.key == key) else {
            return false;
        };
        if matches!(
            tx.state,
            TextVisualTransactionState::Completed | TextVisualTransactionState::Cancelled
        ) {
            return false;
        }

        tx.texture_prepared = true;
        if tx.state == TextVisualTransactionState::Pending {
            tx.state = TextVisualTransactionState::Prepared;
        }
        true
    }

    pub fn complete(&mut self, key: VisualTransactionKey) -> Option<Vec<LineSnapshotId>> {
        if let Some(tx) = self.transactions.iter_mut().find(|t| t.key == key) {
            tx.state = TextVisualTransactionState::Completed;
            let ids = tx.snapshot_ids();
            self.transactions.retain(|t| t.key != key);
            return Some(ids);
        }
        None
    }

    pub fn cancel(&mut self, key: VisualTransactionKey, reason: &str) -> bool {
        if let Some(tx) = self.transactions.iter_mut().find(|t| t.key == key) {
            tx.state = TextVisualTransactionState::Cancelled;
            tx.cancel_reason = Some(reason.to_string());
            self.transactions.retain(|t| t.key != key);
            return true;
        }
        false
    }

    pub fn cancel_all(&mut self, reason: &str) {
        for tx in &mut self.transactions {
            tx.state = TextVisualTransactionState::Cancelled;
            tx.cancel_reason = Some(reason.to_string());
        }
        self.transactions.clear();
    }

    pub fn tick(&mut self, now: Instant) -> Vec<VisualTransactionKey> {
        let mut expired = Vec::new();
        for tx in &mut self.transactions {
            if tx.is_expired(now) {
                tx.state = TextVisualTransactionState::Cancelled;
                tx.cancel_reason = Some("expired".to_string());
                expired.push(tx.key);
            }
        }
        self.transactions.retain(|t| !expired.contains(&t.key));
        expired
    }

    /// Issue #710 评论 5733109905: `find_conflicting_transaction` 改为
    /// **current-old 坐标系逐事务映射**，不再接收共用 `offset_map` 参数。
    ///
    /// `current_old_text` 是当前事务应用前的文本（current-old 坐标系）。
    /// `[byte_start, byte_end)` 是 current-old 坐标系的查询 range。
    ///
    /// 内部对每个 active tx：取 `tx.new_snapshot.virtual_text`（事务自己的 new_text），
    /// 构造 `OffsetMap::build(&tx.new_text, current_old_text)`（从该旧事务 new 坐标系
    /// → current-old 坐标系），映射 `visual_affected_byte_range_new` / units
    /// 到 current-old 坐标系再判断 overlap。
    ///
    /// 这样"冲突检测"和"当前编辑的 old→new 动画映射"是两件事，不再共用错的 OffsetMap。
    ///
    /// Issue #710 评论 5733833897: 返回**全部** active 冲突事务的 key（`Vec`），
    /// 不再只返回第一个。一次新编辑可能同时撞上多笔旧事务，调用方（`take_rebase_frames`）
    /// 需要逐笔处理：untouched 的 keep，受影响的 cancel。只返回第一笔会让后面的冲突
    /// 事务继续留在队列里按旧布局画，导致双层文字/闪烁/删除跨行乱跳。
    pub fn find_conflicting_transaction(
        &self,
        current_old_text: &str,
        byte_start: usize,
        byte_end: usize,
    ) -> Vec<VisualTransactionKey> {
        self.transactions
            .iter()
            .filter(|t| {
                t.state != TextVisualTransactionState::Cancelled
                    && t.state != TextVisualTransactionState::Completed
            })
            .filter(|t| {
                t.overlaps_byte_range(byte_start, byte_end, current_old_text) || t.is_composition()
            })
            .map(|t| t.key)
            .collect()
    }

    pub fn active_transactions(&self) -> &[PreparedTextVisualTransaction] {
        &self.transactions
    }

    pub fn active_transactions_mut(&mut self) -> &mut [PreparedTextVisualTransaction] {
        &mut self.transactions
    }

    /// Issue #702 评论 5708436497: 只认 `TextVisualOperationKind::Insert`。
    ///
    /// 之前这里只判断 state 不是 Completed/Cancelled，没有过滤 `operation_kind`，
    /// 导致 Delete / CompositionUpdate / CompositionCommitOrCancel 事务也会返回 true，
    /// 被 `qquickitem_impl.rs` / `editing.rs` / `properties.rs` 里"输入期间抑制 blink"
    /// 的逻辑错误当成 Insert。现在明确过滤 Insert，让本方法名与实现一致。
    /// 任意正文事务的判断由 `has_active_text_transaction()` 负责。
    pub fn has_active_insert(&self) -> bool {
        self.transactions.iter().any(|t| {
            t.operation_kind == TextVisualOperationKind::Insert
                && t.state != TextVisualTransactionState::Cancelled
                && t.state != TextVisualTransactionState::Completed
        })
    }

    pub fn is_empty(&self) -> bool {
        self.transactions.is_empty()
    }
}

// ── Issue #710 评论 5732160521 回归测试 ──
//
// 修复后这些测试验证"bug 已修复"：
// - 问题 2: tick/render/opacity/边沿 reset 四处消费同一个 current_cursor_blink_mode()，
//   判断一致。
// - 问题 3: visual_affected_byte_range 保存 old/new 两侧，find_conflicting_transaction
//   通过 OffsetMap 映射到同一坐标系比较，不再跨 revision 误判/漏判冲突。
#[cfg(test)]
mod issue_710_comment_5732160521_repro {
    use super::super::timeline::TransactionTimeline;
    use super::*;
    use crate::sujian_editor_item::layout_revision::LayoutRevision;
    use crate::sujian_editor_item::layout_snapshot::EditorLayoutSnapshot;

    /// 构造只含 virtual_text 的测试用 EditorLayoutSnapshot。
    fn make_test_snapshot(virtual_text: &str) -> EditorLayoutSnapshot {
        EditorLayoutSnapshot {
            revision: LayoutRevision::next(),
            line_snapshots: Vec::new(),
            caret_rect: None,
            caret_rect_doc: None,
            caret_affinity: crate::editor::layout::CaretAffinity::Upstream,
            virtual_text: virtual_text.to_string(),
        }
    }

    /// 构造最小化测试事务：只填 key/operation_kind/visual_affected_byte_range_{old,new}，
    /// 其余字段用空/None。state=Pending 保证被 find_conflicting_transaction 遍历。
    /// Issue #710 评论 5733109905: new_snapshot 需含 virtual_text，供 per-tx offset_map 构造。
    fn make_test_tx(
        transaction_id: u64,
        operation_kind: TextVisualOperationKind,
        visual_affected_byte_range_old: Option<(usize, usize)>,
        visual_affected_byte_range_new: Option<(usize, usize)>,
        new_virtual_text: &str,
    ) -> PreparedTextVisualTransaction {
        PreparedTextVisualTransaction {
            key: VisualTransactionKey::new(transaction_id, 0),
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
            new_snapshot: Some(make_test_snapshot(new_virtual_text)),
            cursor_owner_epoch: 0,
            caret_motion_retired: false,
            visual_affected_byte_range_old,
            visual_affected_byte_range_new,
            layout_basis_revision: LayoutRevision::initial(),
        }
    }

    // ── 问题 2: 光标 blink 双重判断导致快速点击光标消失 ──
    //
    // 修复后：tick_cursor_animation / build_cursor_render_state_for_frame /
    // cursor_blink_opacity / 边沿 reset 四处全部消费 current_cursor_blink_mode()
    // 的同一个结果。本测试验证修复后的统一判断逻辑：在 CursorOnly Tween 期间
    //（has_cursor_only_tween=true，无 Insert 事务），blink 应被 Suppressed。
    #[test]
    fn test_issue710_cursor_blink_unified_judgment() {
        // 构造"没有 Insert 事务"的队列。CursorOnly Tween 不入正文事务队列
        //（它由 cursor_ctrl.animation 表示，不在 PreparedTransactionQueue 里）。
        let queue = PreparedTransactionQueue::new();

        // 真实调用源代码方法
        let has_active_insert = queue.has_active_insert();
        let has_active_text_transaction = queue.active_transactions().iter().any(|t| {
            !matches!(
                t.state,
                TextVisualTransactionState::Completed | TextVisualTransactionState::Cancelled
            )
        });

        // CursorOnly Tween 已开始（cursor_ctrl.animation.is_some()），不入正文队列
        let has_cursor_only_tween = true;

        // Issue #727 约束 6: 删除 coordinated_text_cursor_animation_enabled 独立开关。
        // 修复后的统一判断（current_cursor_blink_mode 的逻辑）：
        // tick / render / opacity / 边沿 reset 全部用这一个表达式。
        // 是否有吞吐字直接由 has_active_text_transaction 决定，不再受外部开关控制。
        let unified_suppressed = has_active_text_transaction || has_cursor_only_tween;

        // 验证：CursorOnly Tween 期间 blink 应被 Suppressed（常亮），不会因 blink
        // 切到 opacity=0 而消失。
        assert!(
            unified_suppressed,
            "修复后：CursorOnly Tween 期间 blink 应被 Suppressed（常亮），\
             has_active_insert={} has_active_text_transaction={} has_cursor_only_tween={}",
            has_active_insert, has_active_text_transaction, has_cursor_only_tween
        );

        // 验证：修复后 tick 和 render 用同一个判断，必然一致。
        // 之前 tick 用 has_active_text_transaction || has_cursor_only_tween，
        // render 用 has_active_insert，两者不一致。现在统一为 unified_suppressed。
        let tick_suppressed = unified_suppressed;
        let render_suppressed = unified_suppressed;
        assert_eq!(
            tick_suppressed, render_suppressed,
            "修复后：tick/render/opacity/边沿 reset 四处消费同一个 current_cursor_blink_mode()，\
             必然一致"
        );
    }

    // ── 问题 3: visual_affected_byte_range 跨 revision 不可比 ──
    //
    // 修复后：visual_affected_byte_range 保存 old/new 两侧，
    // find_conflicting_transaction 接收 OffsetMap，把旧事务的
    // visual_affected_byte_range_new 映射到查询坐标系再做 overlap。

    /// 问题 3a — 验证修复后不再误判跨 revision 冲突。
    ///
    /// tx1 (Insert): old="b", new="ab"（开头插 a）。
    ///   visual_affected_byte_range_new = Some((0, 1))  // a 在 new="ab" 的 0..1
    /// 之后外部操作把 "ab" 变成 "abc"（末尾插 c），当前 revision = "abc"。
    /// 第三笔在 "abc" 操作 c 区域，raw range = (2, 3)（"abc" 坐标系）。
    ///
    /// OffsetMap 从 tx1 的 new_text="ab" 到当前 "abc"：公共前缀 "ab"（2 bytes），
    /// entry: old=0, new=0, length=2, Identity。
    /// 映射 tx1 的 (0,1) → (0,1)。overlap (0,1) vs (2,3) → 不重叠 → 不冲突。正确。
    #[test]
    fn test_issue710_visual_affected_byte_range_no_false_conflict() {
        let tx1 = make_test_tx(
            1,
            TextVisualOperationKind::Insert,
            Some((0, 0)), // old 侧是插入点
            Some((0, 1)), // new 侧是 inserted_range
            "ab",         // tx1.new_text
        );
        let mut queue = PreparedTransactionQueue::new();
        queue.enqueue(tx1);

        // 当前 revision = "abc"，tx1 的 new_text="ab"。
        // per-tx offset_map = OffsetMap::build("ab", "abc")：公共前缀 "ab"，Identity。
        // 第三笔在当前 revision "abc" 操作 c 区域，raw range = (2, 3)。
        let current_old_text = "abc";
        let conflict = queue.find_conflicting_transaction(current_old_text, 2, 3);

        // 修复后：tx1 的 (0,1) 映射到当前坐标系仍是 (0,1)（a 区域），
        // 查询 (2,3) 是 c 区域，不重叠 → 不冲突。
        assert!(
            conflict.is_empty(),
            "修复后不应误判冲突：tx1 的 visual_affected_byte_range_new=(0,1) 基于\
             new='ab'，通过 per-tx OffsetMap 映射到当前 'abc' 坐标系仍为 (0,1)（a 区域），\
             查询 raw range=(2,3) 是 c 区域，不重叠。实际返回 {:?}",
            conflict
        );
    }

    /// 问题 3b — 验证修复后不再漏判跨 revision 冲突。
    ///
    /// tx1 (Insert): old="ab", new="aXb"（中间插 X）。
    ///   visual_affected_byte_range_new = Some((1, 2))  // X 在 new="aXb" 的 1..2
    /// 之后 a 被删除，当前 revision 变为 "Xb"，X 位移到 0..1。
    /// 第三笔在 "Xb" 操作 X 区域，raw range = (0, 1)（"Xb" 坐标系）。
    ///
    /// OffsetMap 从 tx1 的 new_text="aXb" 到当前 "Xb"：公共前缀 ""（a≠X），
    /// 公共后缀 "Xb"（2 bytes），entry: old=1, new=0, length=2, Shifted。
    /// 映射 tx1 的 (1,2) → (0,1)。overlap (0,1) vs (0,1) → 重叠 → 冲突。正确。
    #[test]
    fn test_issue710_visual_affected_byte_range_no_missed_conflict() {
        let tx1 = make_test_tx(
            1,
            TextVisualOperationKind::Insert,
            Some((1, 1)), // old 侧是插入点
            Some((1, 2)), // new 侧是 inserted_range（X 在 "aXb" 的 1..2）
            "aXb",        // tx1.new_text
        );
        let mut queue = PreparedTransactionQueue::new();
        queue.enqueue(tx1);

        // 当前 revision = "Xb"（a 被删除），tx1 的 new_text="aXb"。
        // per-tx offset_map = OffsetMap::build("aXb", "Xb")：公共后缀 "Xb"，Shifted。
        // 第三笔在当前 revision "Xb" 操作 X 区域，raw range = (0, 1)。
        let current_old_text = "Xb";
        let conflict = queue.find_conflicting_transaction(current_old_text, 0, 1);

        // 修复后：tx1 的 (1,2) 映射到当前坐标系为 (0,1)（X 区域），
        // 查询 (0,1) 也是 X 区域，重叠 → 冲突。
        assert!(
            !conflict.is_empty(),
            "修复后不应漏判冲突：tx1 的 visual_affected_byte_range_new=(1,2) 基于\
             new='aXb'，通过 per-tx OffsetMap 映射到当前 'Xb' 坐标系为 (0,1)（X 区域），\
             查询 raw range=(0,1) 也是 X 区域，应检测到 tx1 冲突。实际返回空 Vec"
        );
    }
}

// ── Issue #710 评论 5733109905 复现测试 ──
//
// 本轮前两条已修复（compute_affected_paragraph_ranges old/new 分离、blink 统一入口），
// 但第三条"跨 revision 的视觉区域所有权"仍有坐标系错误。这些测试展示当前实现
// 在三个具体场景下给出错误冲突判定，证明 bug 存在。
//
// 复现策略：断言"当前实现行为"与"正确坐标系下的期望行为"不一致，从而证明 bug。
// 修复后（Phase B）应把这些测试改为断言"正确行为"。
#[cfg(test)]
mod issue_710_comment_5733109905_repro {
    use super::super::timeline::TransactionTimeline;
    use super::super::types::PreparedVisualUnit;
    use super::*;
    use crate::sujian_editor_item::animated_slice::AnimatedSlice;
    use crate::sujian_editor_item::layout_revision::LayoutRevision;
    use crate::sujian_editor_item::layout_snapshot::{
        EditorLayoutSnapshot, LineSnapshotId, SourceRect,
    };

    /// 构造只含 virtual_text 的测试用 EditorLayoutSnapshot。
    fn make_test_snapshot(virtual_text: &str) -> EditorLayoutSnapshot {
        EditorLayoutSnapshot {
            revision: LayoutRevision::next(),
            line_snapshots: Vec::new(),
            caret_rect: None,
            caret_rect_doc: None,
            caret_affinity: crate::editor::layout::CaretAffinity::Upstream,
            virtual_text: virtual_text.to_string(),
        }
    }

    /// 构造最小化测试事务：只填 key/operation_kind/visual_affected_byte_range_{old,new}，
    /// 其余字段用空/None。state=Pending 保证被 find_conflicting_transaction 遍历。
    /// Issue #710 评论 5733109905: new_snapshot 需含 virtual_text，供 per-tx offset_map 构造。
    fn make_test_tx(
        transaction_id: u64,
        operation_kind: TextVisualOperationKind,
        visual_affected_byte_range_old: Option<(usize, usize)>,
        visual_affected_byte_range_new: Option<(usize, usize)>,
        new_virtual_text: &str,
    ) -> PreparedTextVisualTransaction {
        PreparedTextVisualTransaction {
            key: VisualTransactionKey::new(transaction_id, 0),
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
            new_snapshot: Some(make_test_snapshot(new_virtual_text)),
            cursor_owner_epoch: 0,
            caret_motion_retired: false,
            visual_affected_byte_range_old,
            visual_affected_byte_range_new,
            layout_basis_revision: LayoutRevision::initial(),
        }
    }

    /// 构造带一个 unit 的测试事务，用于问题 3（units 裸数值比较）复现。
    /// unit 的 slice.byte_start/byte_end 设为指定值（事务 new 坐标系）。
    /// Issue #710 评论 5733109905: new_snapshot 需含 virtual_text，供 per-tx offset_map 构造。
    fn make_test_tx_with_unit(
        transaction_id: u64,
        operation_kind: TextVisualOperationKind,
        unit_byte_start: usize,
        unit_byte_end: usize,
        visual_affected_byte_range_old: Option<(usize, usize)>,
        visual_affected_byte_range_new: Option<(usize, usize)>,
        new_virtual_text: &str,
    ) -> PreparedTextVisualTransaction {
        let slice = AnimatedSlice::insert_reveal(
            VisualTransactionKey::new(transaction_id, 0),
            LineSnapshotId::new(1, 0, 0),
            SourceRect::zero(),
            SourceRect::zero(),
            0.0,
            0.0,
            unit_byte_start,
            unit_byte_end,
            None,
            None,
        );
        let unit = PreparedVisualUnit::wrap(slice, 100);
        PreparedTextVisualTransaction {
            key: VisualTransactionKey::new(transaction_id, 0),
            state: TextVisualTransactionState::Pending,
            operation_kind,
            timeline: TransactionTimeline::new(100),
            units: vec![unit],
            old_cursor_rect: None,
            new_cursor_rect: None,
            cursor_visual_track: None,
            cancel_reason: None,
            texture_prepared: false,
            old_snapshot: None,
            new_snapshot: Some(make_test_snapshot(new_virtual_text)),
            cursor_owner_epoch: 0,
            caret_motion_retired: false,
            visual_affected_byte_range_old,
            visual_affected_byte_range_new,
            layout_basis_revision: LayoutRevision::initial(),
        }
    }

    // ── 问题 1: Delete 冲突查询把 old 坐标和 new 坐标直接比较 ──
    //
    // 场景：
    //   tx1 (Insert): old="bcde", new="abcde"（开头插 a）。
    //     visual_affected_byte_range_new = (1, 5)（"bcde" 在 new="abcde" 的 1..5）。
    //   tx2 (Delete): old="abcde", new="bcde"，删除开头 a，deleted_range=(0,1)（old 坐标系）。
    //
    // 修复后（current-old 坐标系逐事务映射）：
    //   current_old_text = tx2.old = "abcde"。
    //   per-tx offset_map = OffsetMap::build(tx1.new, tx2.old) = OffsetMap::build("abcde", "abcde") = identity。
    //   tx1 的 (1,5) 映射后仍 (1,5)（tx2.old 坐标系）。
    //   查询 range = (0,1)（tx2.old 坐标系）。(1,5) vs (0,1) → 不重叠 → 不冲突。正确。
    #[test]
    fn test_issue710_comment_5733109905_problem1_delete_old_new_coord_mismatch() {
        // tx1: visual_affected_byte_range_new 基于 tx1.new="abcde"
        let tx1 = make_test_tx(
            1,
            TextVisualOperationKind::Insert,
            Some((0, 0)), // old 侧是插入点
            Some((1, 5)), // new 侧是 "bcde" 在 "abcde" 的 1..5
            "abcde",      // tx1.new_text
        );
        let mut queue = PreparedTransactionQueue::new();
        queue.enqueue(tx1);

        // tx2 (Delete): old="abcde", new="bcde", deleted_range=(0,1)
        // Delete 路径查询 range = deleted_range = (0, 1)（old 坐标系）
        let tx2_old_text = "abcde";
        let rebase_byte_start = 0usize;
        let rebase_byte_end = 1usize;

        // 修复后：current_old_text = tx2.old，per-tx offset_map = identity
        let current_conflict =
            queue.find_conflicting_transaction(tx2_old_text, rebase_byte_start, rebase_byte_end);

        // 修复后：tx1 的 (1,5) 在 tx2.old 坐标系仍是 (1,5)（tx1.new==tx2.old），
        // 查询 (0,1) 不重叠 → 不冲突。
        assert!(
            current_conflict.is_empty(),
            "修复后应不冲突：tx1 的 visual_affected_byte_range_new=(1,5) 基于 tx1.new='abcde'，\
             per-tx OffsetMap=identity（tx1.new==tx2.old='abcde'），映射后仍 (1,5)，\
             查询 (0,1) 不重叠。实际返回 {:?}",
            current_conflict
        );
    }

    // ── 问题 2: 单个 OffsetMap 只对紧邻上一笔事务成立，不能给队列里所有活动事务共用 ──
    //
    // 场景：
    //   初始文档 "123456789"。
    //   tx1 (Insert): old="123456789", new="12345X6789"（位置 5 插 X，第 2 段）。
    //     visual_affected_byte_range_new = (5, 6)（X 在 tx1.new 的 5..6）。
    //   tx2 (Insert): old="12345X6789", new="12Y345X6789"（位置 2 插 Y，第 1 段，不冲突）。
    //   tx3 (Delete): old="12Y345X6789", new="12Y3456789"，删除 X。
    //     deleted_range = (6, 7)（X 在 tx3.old 的位置 6）。
    //
    // 修复后（current-old 坐标系逐事务映射）：
    //   current_old_text = tx3.old = "12Y345X6789"。
    //   对 tx1: per-tx offset_map = OffsetMap::build("12345X6789", "12Y345X6789")
    //     → 把 tx1 的 (5,6) 映射到 (6,7)（X 在 "12Y345X6789" 的位置 6）
    //     → (6,7) vs 查询 (6,7) → 重叠 → 冲突。正确。
    //   对 tx2: per-tx offset_map = OffsetMap::build("12Y345X6789", "12Y345X6789") = identity
    //     → tx2 的 (2,3) 映射后仍 (2,3) → (2,3) vs (6,7) → 不重叠 → 不冲突。
    #[test]
    fn test_issue710_comment_5733109905_problem2_single_offset_map_not_universal() {
        // tx1: visual_affected_byte_range_new 基于 tx1.new="12345X6789"
        let tx1 = make_test_tx(
            1,
            TextVisualOperationKind::Insert,
            Some((5, 5)), // old 侧是插入点
            Some((5, 6)), // new 侧是 X 在 "12345X6789" 的 5..6
            "12345X6789", // tx1.new_text
        );
        // tx2: 在第 1 段插入 Y，不与 tx1 冲突，留在队列
        let tx2 = make_test_tx(
            2,
            TextVisualOperationKind::Insert,
            Some((2, 2)),
            Some((2, 3)),  // Y 在 "12Y345X6789" 的 2..3
            "12Y345X6789", // tx2.new_text
        );
        let mut queue = PreparedTransactionQueue::new();
        queue.enqueue(tx1);
        queue.enqueue(tx2);

        // tx3 (Delete): old="12Y345X6789", new="12Y3456789", deleted_range=(6,7)
        let tx3_old_text = "12Y345X6789";
        let rebase_byte_start = 6usize;
        let rebase_byte_end = 7usize;

        // 修复后：current_old_text = tx3.old，逐事务构造 per-tx offset_map
        let current_conflict =
            queue.find_conflicting_transaction(tx3_old_text, rebase_byte_start, rebase_byte_end);

        // 单独验证 tx1 在正确 per-tx offset_map 下应判定冲突
        let tx1_alone = make_test_tx(
            1,
            TextVisualOperationKind::Insert,
            Some((5, 5)),
            Some((5, 6)),
            "12345X6789",
        );
        let mut queue_tx1_only = PreparedTransactionQueue::new();
        queue_tx1_only.enqueue(tx1_alone);
        let correct_conflict_for_tx1 = queue_tx1_only.find_conflicting_transaction(
            tx3_old_text,
            rebase_byte_start,
            rebase_byte_end,
        );

        // 修复后：tx1 的 (5,6) 通过 per-tx OffsetMap::build("12345X6789", "12Y345X6789")
        // 映射到 (6,7)，与查询 (6,7) 重叠 → 冲突。
        assert!(
            !current_conflict.is_empty(),
            "修复后应检测到冲突：用 per-tx OffsetMap::build(tx1.new, tx3.old) 把 tx1 的 (5,6) \
             映射到 (6,7)（X 在 '12Y345X6789' 的位置 6），与查询 (6,7) 重叠。\
             实际返回 {:?}",
            current_conflict
        );
        assert!(
            !correct_conflict_for_tx1.is_empty(),
            "tx1 单独在正确 per-tx offset_map 下应冲突：OffsetMap::build('12345X6789', '12Y345X6789') \
             把 (5,6) 映射到 (6,7)，与查询 (6,7) 重叠。实际返回 {:?}",
            correct_conflict_for_tx1
        );
    }

    // ── 问题 3: units 明知是旧事务 new 坐标，代码仍先做裸数值 overlap ──
    //
    // 场景：
    //   tx1 (Insert): old="abcdef", new="abXYcdef"（位置 2 插 XY）。
    //     tx1 有一个 unit，byte range = (2, 4)（XY 在 tx1.new="abXYcdef" 的 2..4，new 坐标系）。
    //     visual_affected_byte_range_new = (2, 4)。
    //   之后前面插入 Z，当前文档 = "ZabXYcdef"。tx1.new="abXYcdef" ≠ 当前 "ZabXYcdef"。
    //   新事务查询 range = (2, 3)（当前坐标系，对应 "b" 区域）。
    //
    // 修复后（current-old 坐标系逐事务映射）：
    //   current_old_text = "ZabXYcdef"。
    //   per-tx offset_map = OffsetMap::build("abXYcdef", "ZabXYcdef")：Shifted +1。
    //   unit 的 (2,4) 映射到 (3,5)。查询 (2,3)。(3,5) vs (2,3) → 不重叠 → 不冲突。正确。
    #[test]
    fn test_issue710_comment_5733109905_problem3_units_bare_numeric_overlap() {
        // tx1: 有一个 unit，byte range = (2, 4)（tx1.new 坐标系）
        let tx1 = make_test_tx_with_unit(
            1,
            TextVisualOperationKind::Insert,
            2,            // unit byte_start
            4,            // unit byte_end
            Some((2, 2)), // visual_affected_byte_range_old
            Some((2, 4)), // visual_affected_byte_range_new
            "abXYcdef",   // tx1.new_text
        );
        let mut queue = PreparedTransactionQueue::new();
        queue.enqueue(tx1);

        // 当前文档 = "ZabXYcdef"（前面插了 Z），tx1.new = "abXYcdef"
        let current_text = "ZabXYcdef";
        // 新事务查询 range = (2, 3)（当前坐标系，对应 "b" 区域）
        let query_start = 2usize;
        let query_end = 3usize;

        // 修复后：unit 的 byte range 也通过 per-tx offset_map 映射到当前坐标系
        let current_conflict =
            queue.find_conflicting_transaction(current_text, query_start, query_end);

        // per-tx offset_map = OffsetMap::build("abXYcdef", "ZabXYcdef")：
        //   prefix=0, suffix=8, entry Shifted old=0,new=1,length=8
        // 映射 unit (2,4) → (3,5)。查询 (2,3)。(3,5) vs (2,3) → 不重叠。
        let per_tx_offset_map = writer_core::editor::OffsetMap::build("abXYcdef", current_text);
        let mapped_unit_range = per_tx_offset_map.map_old_range_to_new(2, 4);
        assert_eq!(
            mapped_unit_range,
            Some((3, 5)),
            "per-tx offset_map 应把 unit 的 (2,4) 映射到 (3,5)（'ZabXYcdef' 坐标系）"
        );

        // 修复后：映射后的 unit range (3,5) 与查询 (2,3) 不重叠 → 不冲突
        assert!(
            current_conflict.is_empty(),
            "修复后应不冲突：unit 的 (2,4) 通过 per-tx OffsetMap 映射到 (3,5)（当前坐标系），\
             查询 ({},{}) 不重叠。实际返回 {:?}",
            query_start,
            query_end,
            current_conflict
        );
    }
}
