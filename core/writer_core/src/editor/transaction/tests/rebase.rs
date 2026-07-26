use crate::editor::transaction::visual::*;
use crate::editor::transaction::rebase::*;
use crate::editor::transaction::engine::*;

# [allow(deprecated)]

        #[test]
        fn transaction_rebase_serializes_camel_case() {
            let rebase = TransactionRebase {
                cancelled_transaction_id: 42,
                old_progress: 0.6,
                old_frame_snapshot: Some(RebaseFrameSnapshot {
                    slice_rects: vec![Rect { x: 10.0, y: 20.0, w: 30.0, h: 40.0 }],
                    slice_alphas: vec![0.8],
                    cursor_rect: Some(CursorRect { x: 10.0, top: 5.0, bottom: 25.0, baseline_y: 20.0 }),
                }),
            };
            let json = serde_json::to_string(&rebase).unwrap();
            assert!(json.contains("\"cancelledTransactionId\":"));
            assert!(json.contains("\"oldProgress\":"));
            assert!(json.contains("\"oldFrameSnapshot\":"));
            assert!(json.contains("\"sliceRects\":"));
            assert!(json.contains("\"sliceAlphas\":"));
            assert!(json.contains("\"cursorRect\":"));
        }

        #[test]
        fn transaction_rebase_skips_none() {
            let rebase = TransactionRebase {
                cancelled_transaction_id: 1,
                old_progress: 0.0,
                old_frame_snapshot: None,
            };
            let json = serde_json::to_string(&rebase).unwrap();
            assert!(!json.contains("\"oldFrameSnapshot\":"));
        }

        #[test]
        fn compute_rebase_creates_transaction_rebase() {
            let rebase = compute_rebase(42, 0.6, Some(RebaseFrameSnapshot {
                slice_rects: vec![Rect { x: 10.0, y: 20.0, w: 30.0, h: 40.0 }],
                slice_alphas: vec![0.8],
                cursor_rect: None,
            }));
            assert_eq!(rebase.cancelled_transaction_id, 42);
            assert!((rebase.old_progress - 0.6).abs() < f64::EPSILON);
            assert!(rebase.old_frame_snapshot.is_some());
        }

        #[test]
        fn transactions_overlap_cursor_only_always_conflicts() {
            assert!(transactions_overlap(
                UnifiedTransactionKind::CursorOnly,
                (0, 0),
                UnifiedTransactionKind::BodyEdit,
                (5, 10),
            ));
        }

        #[test]
        fn transactions_overlap_overlapping_ranges() {
            assert!(transactions_overlap(
                UnifiedTransactionKind::BodyEdit,
                (0, 10),
                UnifiedTransactionKind::BodyEdit,
                (5, 15),
            ));
        }

        #[test]
        fn transactions_overlap_non_overlapping_ranges() {
            assert!(!transactions_overlap(
                UnifiedTransactionKind::BodyEdit,
                (0, 5),
                UnifiedTransactionKind::BodyEdit,
                (10, 15),
            ));
        }

        #[test]
        fn rebase_covers_all_transaction_kinds() {
            // #516: rebase 必须覆盖四种事务
            // 测试 CursorOnly 与 BodyEdit 冲突
            assert!(transactions_overlap(
                UnifiedTransactionKind::CursorOnly,
                (0, 0),
                UnifiedTransactionKind::BodyEdit,
                (0, 5),
            ));
            // 测试 CompositionUpdate 与 CompositionCommitOrCancel 冲突
            assert!(transactions_overlap(
                UnifiedTransactionKind::CompositionUpdate,
                (0, 5),
                UnifiedTransactionKind::CompositionCommitOrCancel,
                (3, 8),
            ));
        }
