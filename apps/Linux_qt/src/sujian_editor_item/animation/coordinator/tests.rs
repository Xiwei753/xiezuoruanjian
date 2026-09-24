
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

