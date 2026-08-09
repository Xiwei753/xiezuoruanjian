#[cfg(test)]
#[allow(clippy::module_inception)]
mod tests {
    use super::super::result::{EditorEditOutcome, EditorInputError};
    use super::super::types::{DisplayPatch, EditorCommand, EditorOperationKind};
    use super::super::*;
    use crate::editor::strong_types::{
        EditorRevision, EditorSessionGeneration, EditorSessionId, Utf8ByteOffset, Utf8ByteRange,
    };
    use crate::editor::transaction::{AnimationMode, EditorTransactionCause};

    #[test]
    fn insert_command_produces_display_patch() {
        let mut kernel = EditorKernel::with_text("你好".to_string(), 6).unwrap();
        let result = kernel
            .apply(EditorCommand::Insert {
                byte_offset: Utf8ByteOffset::unchecked(6),
                text: "世界".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();

        assert_eq!(kernel.text(), "你好世界");
        assert_eq!(kernel.cursor(), 12);
        assert_eq!(result.base_revision.value(), 0);
        assert_eq!(result.new_revision.value(), 1);
        assert!(!result.display_patches.is_empty());
        assert_eq!(
            result.visual_intent.operation_kind,
            EditorOperationKind::Insert
        );
        assert_eq!(result.visual_intent.cause, EditorTransactionCause::Typing);
    }

    #[test]
    fn delete_command_produces_display_patch() {
        let mut kernel = EditorKernel::with_text("你好世界".to_string(), 12).unwrap();
        let result = kernel
            .apply(EditorCommand::Delete {
                byte_range: Utf8ByteRange::from_ordered(6, 12),
                deleted_text: "世界".to_string(),
                cause: EditorTransactionCause::Delete,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();

        assert_eq!(kernel.text(), "你好");
        assert_eq!(kernel.cursor(), 6);
        assert_eq!(
            result.visual_intent.operation_kind,
            EditorOperationKind::Delete
        );
    }

    #[test]
    fn replace_command_produces_display_patch() {
        let mut kernel = EditorKernel::with_text("你好世界".to_string(), 12).unwrap();
        let result = kernel
            .apply(EditorCommand::Replace {
                byte_range: Utf8ByteRange::from_ordered(6, 12),
                replacement_text: "朋友".to_string(),
                original_text: "世界".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();

        assert_eq!(kernel.text(), "你好朋友");
        assert_eq!(kernel.cursor(), 12);
        assert_eq!(
            result.visual_intent.operation_kind,
            EditorOperationKind::Replace
        );
    }

    #[test]
    fn set_selection_does_not_modify_text() {
        let mut kernel = EditorKernel::with_text("hello".to_string(), 5).unwrap();
        let result = kernel
            .apply(EditorCommand::SetSelection {
                anchor: Utf8ByteOffset::unchecked(0),
                head: Utf8ByteOffset::unchecked(3),
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(kernel.text(), "hello");
        assert_eq!(kernel.cursor(), 3);
        assert_eq!(result.display_patches.len(), 0);
        assert_eq!(
            result.visual_intent.operation_kind,
            EditorOperationKind::CursorOnly
        );
    }

    #[test]
    fn undo_restores_previous_state() {
        let mut kernel = EditorKernel::with_text("ab".to_string(), 2).unwrap();
        let r1 = kernel
            .apply(EditorCommand::Insert {
                byte_offset: Utf8ByteOffset::unchecked(2),
                text: "c".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(kernel.text(), "abc");

        let result = kernel
            .apply(EditorCommand::Undo {
                expected_revision: r1.new_revision,
            })
            .into_result();
        assert_eq!(kernel.text(), "ab");
        assert_eq!(kernel.cursor(), 2);
        assert_eq!(result.visual_intent.cause, EditorTransactionCause::Undo);
    }

    #[test]
    fn redo_restores_undone_state() {
        let mut kernel = EditorKernel::with_text("ab".to_string(), 2).unwrap();
        let r1 = kernel
            .apply(EditorCommand::Insert {
                byte_offset: Utf8ByteOffset::unchecked(2),
                text: "c".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        let r2 = kernel
            .apply(EditorCommand::Undo {
                expected_revision: r1.new_revision,
            })
            .into_result();
        assert_eq!(kernel.text(), "ab");

        let result = kernel
            .apply(EditorCommand::Redo {
                expected_revision: r2.new_revision,
            })
            .into_result();
        assert_eq!(kernel.text(), "abc");
        assert_eq!(result.visual_intent.cause, EditorTransactionCause::Redo);
    }

    #[test]
    fn load_text_resets_state() {
        let mut kernel = EditorKernel::with_text("old".to_string(), 3).unwrap();
        kernel
            .apply(EditorCommand::Insert {
                byte_offset: Utf8ByteOffset::unchecked(3),
                text: " text".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();

        let result = kernel.load_text("new content".to_string(), 0).into_result();
        assert_eq!(kernel.text(), "new content");
        assert_eq!(kernel.cursor(), 0);
        assert_eq!(
            result.visual_intent.operation_kind,
            EditorOperationKind::Load
        );
        assert_eq!(
            result.visual_intent.animation_mode,
            AnimationMode::SystemSuppressed
        );
    }

    #[test]
    fn revision_increments_on_each_edit() {
        let mut kernel = EditorKernel::new();
        assert_eq!(kernel.revision(), 0);

        let r1 = kernel
            .apply(EditorCommand::Insert {
                byte_offset: Utf8ByteOffset::unchecked(0),
                text: "a".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(kernel.revision(), 1);

        kernel
            .apply(EditorCommand::Insert {
                byte_offset: Utf8ByteOffset::unchecked(1),
                text: "b".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: r1.new_revision,
            })
            .into_result();
        assert_eq!(kernel.revision(), 2);
    }

    #[test]
    fn display_patch_contains_correct_ranges() {
        let mut kernel = EditorKernel::with_text("hello world".to_string(), 11).unwrap();
        let result = kernel
            .apply(EditorCommand::Replace {
                byte_range: Utf8ByteRange::from_ordered(6, 11),
                replacement_text: "rust".to_string(),
                original_text: "world".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();

        assert_eq!(result.display_patches.len(), 1);
        assert_eq!(
            result.display_patches[0].replace_byte_range,
            Utf8ByteRange::from_ordered(6, 11)
        );
        assert_eq!(result.display_patches[0].inserted_text, "rust");
    }

    #[test]
    fn coordinated_cursor_tracks_movement() {
        let mut kernel = EditorKernel::with_text("abc".to_string(), 3).unwrap();
        let result = kernel
            .apply(EditorCommand::SetSelection {
                anchor: Utf8ByteOffset::unchecked(0),
                head: Utf8ByteOffset::unchecked(0),
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(
            result.visual_intent.coordinated_cursor.old_offset.value(),
            3
        );
        assert_eq!(
            result.visual_intent.coordinated_cursor.new_offset.value(),
            0
        );
        assert!(result.visual_intent.coordinated_cursor.should_animate);
    }

    #[test]
    fn animation_disabled_suppresses_animation() {
        let mut kernel = EditorKernel::with_text("ab".to_string(), 2).unwrap();
        kernel.set_animation_enabled(false);

        let result = kernel
            .apply(EditorCommand::Insert {
                byte_offset: Utf8ByteOffset::unchecked(2),
                text: "c".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();

        assert_eq!(
            result.visual_intent.animation_mode,
            AnimationMode::SystemSuppressed
        );
        assert!(!result.visual_intent.coordinated_cursor.should_animate);
    }

    #[test]
    fn undo_then_new_edit_clears_redo() {
        let mut kernel = EditorKernel::with_text("ab".to_string(), 2).unwrap();
        let r1 = kernel
            .apply(EditorCommand::Insert {
                byte_offset: Utf8ByteOffset::unchecked(2),
                text: "c".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        let r2 = kernel
            .apply(EditorCommand::Undo {
                expected_revision: r1.new_revision,
            })
            .into_result();
        assert_eq!(kernel.text(), "ab");

        let r3 = kernel
            .apply(EditorCommand::Insert {
                byte_offset: Utf8ByteOffset::unchecked(2),
                text: "d".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: r2.new_revision,
            })
            .into_result();

        let result = kernel
            .apply(EditorCommand::Redo {
                expected_revision: r3.new_revision,
            })
            .into_result();
        assert_eq!(kernel.text(), "abd");
        assert_eq!(
            result.visual_intent.operation_kind,
            EditorOperationKind::CursorOnly
        );
    }

    #[test]
    fn edit_result_serializes_camel_case() {
        let mut kernel = EditorKernel::with_text("ab".to_string(), 2).unwrap();
        let result = kernel
            .apply(EditorCommand::Insert {
                byte_offset: Utf8ByteOffset::unchecked(2),
                text: "c".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();

        let json = serde_json::to_string(&result).unwrap();
        assert!(
            json.contains("\"transactionId\":"),
            "JSON should use camelCase for transactionId, got: {}",
            json
        );
        assert!(
            json.contains("\"baseRevision\":"),
            "JSON should use camelCase for baseRevision, got: {}",
            json
        );
        assert!(
            json.contains("\"newRevision\":"),
            "JSON should use camelCase for newRevision, got: {}",
            json
        );
        assert!(
            json.contains("\"displayPatches\":"),
            "JSON should use camelCase for displayPatches, got: {}",
            json
        );
        assert!(
            json.contains("\"visualIntent\":"),
            "JSON should use camelCase for visualIntent, got: {}",
            json
        );
        assert!(
            json.contains("\"operationKind\":"),
            "JSON should use camelCase for operationKind, got: {}",
            json
        );
        assert!(
            json.contains("\"animationMode\":"),
            "JSON should use camelCase for animationMode, got: {}",
            json
        );
        assert!(
            json.contains("\"durationMs\":"),
            "JSON should use camelCase for durationMs, got: {}",
            json
        );
        assert!(
            json.contains("\"coordinatedCursor\":"),
            "JSON should use camelCase for coordinatedCursor, got: {}",
            json
        );
        assert!(
            json.contains("\"oldOffset\":"),
            "JSON should use camelCase for oldOffset, got: {}",
            json
        );
        assert!(
            json.contains("\"newOffset\":"),
            "JSON should use camelCase for newOffset, got: {}",
            json
        );
        assert!(
            json.contains("\"shouldAnimate\":"),
            "JSON should use camelCase for shouldAnimate, got: {}",
            json
        );
        assert!(
            json.contains("\"replaceByteRange\":"),
            "JSON should use camelCase for replaceByteRange, got: {}",
            json
        );
        assert!(
            json.contains("\"insertedText\":"),
            "JSON should use camelCase for insertedText, got: {}",
            json
        );
        assert!(
            json.contains("\"resultingSelectionByteRange\":"),
            "JSON should use camelCase for resultingSelectionByteRange, got: {}",
            json
        );
    }

    #[test]
    fn display_patch_serializes_camel_case() {
        let patch = DisplayPatch {
            base_revision: EditorRevision::new(0),
            new_revision: EditorRevision::new(1),
            replace_byte_range: Utf8ByteRange::from_ordered(2, 3),
            inserted_text: "c".to_string(),
            resulting_selection_byte_range: Utf8ByteRange::point(3),
        };
        let json = serde_json::to_string(&patch).unwrap();
        assert!(
            json.contains("\"replaceByteRange\":"),
            "DisplayPatch JSON should use camelCase, got: {}",
            json
        );
        assert!(
            json.contains("\"insertedText\":"),
            "DisplayPatch JSON should use camelCase, got: {}",
            json
        );
        assert!(
            json.contains("\"resultingSelectionByteRange\":"),
            "DisplayPatch JSON should use camelCase, got: {}",
            json
        );
    }

    #[test]
    fn composition_update_does_not_modify_text() {
        let mut kernel = EditorKernel::with_text("你好".to_string(), 6).unwrap();
        let tx = kernel.composition_update(None, "", "nihao");
        assert_eq!(kernel.text(), "你好");
        assert_eq!(kernel.cursor(), 6);
        assert_eq!(tx.new_revision.preedit_text, "nihao");
    }

    #[test]
    fn composition_commit_modifies_text_via_replace() {
        let mut kernel = EditorKernel::with_text("你好".to_string(), 6).unwrap();
        let result = kernel
            .apply(EditorCommand::Replace {
                byte_range: Utf8ByteRange::point(6),
                replacement_text: "你好".to_string(),
                original_text: String::new(),
                cause: EditorTransactionCause::TypingCommit,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(kernel.text(), "你好你好");
        assert_eq!(
            result.visual_intent.operation_kind,
            EditorOperationKind::Insert
        );
        assert_eq!(
            result.visual_intent.cause,
            EditorTransactionCause::TypingCommit
        );
    }

    #[test]
    fn delete_empty_range_is_noop() {
        let mut kernel = EditorKernel::with_text("abc".to_string(), 3).unwrap();
        let outcome = kernel.apply(EditorCommand::Delete {
            byte_range: Utf8ByteRange::point(2),
            deleted_text: String::new(),
            cause: EditorTransactionCause::Delete,
            expected_revision: EditorRevision::new(0),
        });
        assert_eq!(kernel.text(), "abc");
        assert!(matches!(outcome, EditorEditOutcome::InvalidRange(_)));
    }

    #[test]
    fn insert_beyond_length_returns_invalid_offset() {
        let mut kernel = EditorKernel::with_text("ab".to_string(), 2).unwrap();
        let outcome = kernel.apply(EditorCommand::Insert {
            byte_offset: Utf8ByteOffset::unchecked(100),
            text: "c".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: EditorRevision::new(0),
        });
        assert!(matches!(outcome, EditorEditOutcome::InvalidOffset(_)));
        assert_eq!(kernel.text(), "ab");
    }

    #[test]
    fn replace_same_text_produces_patch() {
        let mut kernel = EditorKernel::with_text("abc".to_string(), 3).unwrap();
        let result = kernel
            .apply(EditorCommand::Replace {
                byte_range: Utf8ByteRange::from_ordered(1, 2),
                replacement_text: "X".to_string(),
                original_text: "b".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(kernel.text(), "aXc");
        assert!(!result.display_patches.is_empty());
    }

    #[test]
    fn undo_after_multiple_edits_restores_correctly() {
        let mut kernel = EditorKernel::new();
        let r1 = kernel
            .apply(EditorCommand::Insert {
                byte_offset: Utf8ByteOffset::unchecked(0),
                text: "a".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        let r2 = kernel
            .apply(EditorCommand::Insert {
                byte_offset: Utf8ByteOffset::unchecked(1),
                text: "b".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: r1.new_revision,
            })
            .into_result();
        let r3 = kernel
            .apply(EditorCommand::Insert {
                byte_offset: Utf8ByteOffset::unchecked(2),
                text: "c".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: r2.new_revision,
            })
            .into_result();
        assert_eq!(kernel.text(), "abc");

        let r4 = kernel
            .apply(EditorCommand::Undo {
                expected_revision: r3.new_revision,
            })
            .into_result();
        assert_eq!(kernel.text(), "ab");

        let r5 = kernel
            .apply(EditorCommand::Undo {
                expected_revision: r4.new_revision,
            })
            .into_result();
        assert_eq!(kernel.text(), "a");

        kernel
            .apply(EditorCommand::Undo {
                expected_revision: r5.new_revision,
            })
            .into_result();
        assert_eq!(kernel.text(), "");
    }

    #[test]
    fn cjk_insert_and_delete() {
        let mut kernel = EditorKernel::new();
        let result = kernel
            .apply(EditorCommand::Insert {
                byte_offset: Utf8ByteOffset::unchecked(0),
                text: "你好世界".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(kernel.text(), "你好世界");
        assert_eq!(kernel.cursor(), 12);

        let result = kernel
            .apply(EditorCommand::Delete {
                byte_range: Utf8ByteRange::from_ordered(6, 12),
                deleted_text: "世界".to_string(),
                cause: EditorTransactionCause::Delete,
                expected_revision: result.new_revision,
            })
            .into_result();
        assert_eq!(kernel.text(), "你好");
        assert_eq!(kernel.cursor(), 6);
        assert_eq!(
            result.visual_intent.operation_kind,
            EditorOperationKind::Delete
        );
    }

    #[test]
    fn set_selection_with_same_position_no_cursor_animation() {
        let mut kernel = EditorKernel::with_text("abc".to_string(), 1).unwrap();
        let result = kernel
            .apply(EditorCommand::SetSelection {
                anchor: Utf8ByteOffset::unchecked(1),
                head: Utf8ByteOffset::unchecked(1),
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert!(!result.visual_intent.coordinated_cursor.should_animate);
    }

    #[test]
    fn load_text_clears_undo_stack() {
        let mut kernel = EditorKernel::with_text("old".to_string(), 3).unwrap();
        kernel
            .apply(EditorCommand::Insert {
                byte_offset: Utf8ByteOffset::unchecked(3),
                text: " text".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        kernel.load_text("new".to_string(), 3).into_result();
        let result = kernel.apply(EditorCommand::Undo {
            expected_revision: EditorRevision::new(0),
        });
        assert!(matches!(result, EditorEditOutcome::StaleRevision(_)));
        assert_eq!(kernel.text(), "new");
    }

    #[test]
    fn with_text_rejects_invalid_cursor_offset() {
        let result = EditorKernel::with_text("你好".to_string(), 4);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            EditorInputError::InvalidCursorOffset { .. }
        ));
    }

    #[test]
    fn stale_revision_returns_stale_outcome() {
        let mut kernel = EditorKernel::with_text("abc".to_string(), 3).unwrap();
        let outcome = kernel.apply(EditorCommand::Insert {
            byte_offset: Utf8ByteOffset::unchecked(3),
            text: "d".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: EditorRevision::new(99),
        });
        assert!(matches!(outcome, EditorEditOutcome::StaleRevision(_)));
    }

    #[test]
    fn invalid_offset_returns_invalid_outcome() {
        let mut kernel = EditorKernel::with_text("abc".to_string(), 3).unwrap();
        let outcome = kernel.apply(EditorCommand::Insert {
            byte_offset: Utf8ByteOffset::unchecked(2),
            text: "X".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: EditorRevision::new(0),
        });
        assert!(matches!(outcome, EditorEditOutcome::Applied(_)));
    }

    #[test]
    fn atomic_display_patch_for_replace() {
        let mut kernel = EditorKernel::with_text("hello world".to_string(), 11).unwrap();
        let result = kernel
            .apply(EditorCommand::Replace {
                byte_range: Utf8ByteRange::from_ordered(6, 11),
                replacement_text: "rust".to_string(),
                original_text: "world".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(result.display_patches.len(), 1);
        let patch = &result.display_patches[0];
        assert_eq!(patch.replace_byte_range, Utf8ByteRange::from_ordered(6, 11));
        assert_eq!(patch.inserted_text, "rust");
    }

    #[test]
    fn compute_single_patch_cjk_replace() {
        let (range, inserted) = EditorKernel::compute_single_patch("你好", "你坏");
        assert_eq!(range.start().value(), 3);
        assert_eq!(range.end().value(), 6);
        assert_eq!(inserted, "坏");
    }

    #[test]
    fn compute_single_patch_cjk_insert() {
        let (range, inserted) = EditorKernel::compute_single_patch("你好", "你好世界");
        assert_eq!(range.start().value(), 6);
        assert_eq!(range.end().value(), 6);
        assert_eq!(inserted, "世界");
    }

    #[test]
    fn compute_single_patch_cjk_delete() {
        let (range, inserted) = EditorKernel::compute_single_patch("你好世界", "你好");
        assert_eq!(range.start().value(), 6);
        assert_eq!(range.end().value(), 12);
        assert_eq!(inserted, "");
    }

    #[test]
    fn compute_single_patch_mixed_ascii_cjk() {
        let (range, inserted) = EditorKernel::compute_single_patch("a你好b", "a你坏b");
        assert_eq!(range.start().value(), 4);
        assert_eq!(range.end().value(), 7);
        assert_eq!(inserted, "坏");
    }

    #[test]
    fn compute_single_patch_empty_old() {
        let (range, inserted) = EditorKernel::compute_single_patch("", "你好");
        assert_eq!(range.start().value(), 0);
        assert_eq!(range.end().value(), 0);
        assert_eq!(inserted, "你好");
    }

    #[test]
    fn compute_single_patch_empty_new() {
        let (range, inserted) = EditorKernel::compute_single_patch("你好", "");
        assert_eq!(range.start().value(), 0);
        assert_eq!(range.end().value(), 6);
        assert_eq!(inserted, "");
    }

    #[test]
    fn compute_single_patch_identical() {
        let (range, inserted) = EditorKernel::compute_single_patch("你好", "你好");
        assert_eq!(range.start().value(), 0);
        assert_eq!(range.end().value(), 0);
        assert_eq!(inserted, "");
    }

    #[test]
    fn delete_surrounding_both_sides_preserves_selection() {
        let mut kernel = EditorKernel::with_text("ABCDEFGHIJ".to_string(), 3).unwrap();
        kernel
            .apply(EditorCommand::SetSelection {
                anchor: Utf8ByteOffset::unchecked(3),
                head: Utf8ByteOffset::unchecked(6),
                expected_revision: EditorRevision::new(0),
            })
            .into_result();

        let outcome = kernel.apply(EditorCommand::DeleteSurrounding {
            before_byte_range: Utf8ByteRange::from_ordered(0, 2),
            after_byte_range: Utf8ByteRange::from_ordered(7, 9),
            cause: EditorTransactionCause::Delete,
            expected_revision: EditorRevision::new(0),
        });
        assert!(matches!(outcome, EditorEditOutcome::Applied(_)));
        assert_eq!(kernel.text(), "CDEFGJ");
        assert_eq!(kernel.selection_anchor(), 1);
        assert_eq!(kernel.cursor(), 4);
    }

    #[test]
    fn delete_surrounding_before_only_shifts_selection() {
        let mut kernel = EditorKernel::with_text("ABCDE".to_string(), 3).unwrap();
        kernel
            .apply(EditorCommand::SetSelection {
                anchor: Utf8ByteOffset::unchecked(3),
                head: Utf8ByteOffset::unchecked(5),
                expected_revision: EditorRevision::new(0),
            })
            .into_result();

        let outcome = kernel.apply(EditorCommand::DeleteSurrounding {
            before_byte_range: Utf8ByteRange::from_ordered(0, 2),
            after_byte_range: Utf8ByteRange::zero(),
            cause: EditorTransactionCause::Delete,
            expected_revision: EditorRevision::new(0),
        });
        assert!(matches!(outcome, EditorEditOutcome::Applied(_)));
        assert_eq!(kernel.text(), "CDE");
        assert_eq!(kernel.selection_anchor(), 1);
        assert_eq!(kernel.cursor(), 3);
    }

    #[test]
    fn delete_surrounding_after_only_preserves_selection() {
        let mut kernel = EditorKernel::with_text("ABCDE".to_string(), 3).unwrap();
        kernel
            .apply(EditorCommand::SetSelection {
                anchor: Utf8ByteOffset::unchecked(0),
                head: Utf8ByteOffset::unchecked(3),
                expected_revision: EditorRevision::new(0),
            })
            .into_result();

        let outcome = kernel.apply(EditorCommand::DeleteSurrounding {
            before_byte_range: Utf8ByteRange::zero(),
            after_byte_range: Utf8ByteRange::from_ordered(4, 5),
            cause: EditorTransactionCause::Delete,
            expected_revision: EditorRevision::new(0),
        });
        assert!(matches!(outcome, EditorEditOutcome::Applied(_)));
        assert_eq!(kernel.text(), "ABCD");
        assert_eq!(kernel.selection_anchor(), 0);
        assert_eq!(kernel.cursor(), 3);
    }

    #[test]
    fn commit_text_with_session_validation() {
        let mut kernel = EditorKernel::with_text("你好".to_string(), 6).unwrap();
        let begin = kernel
            .apply(EditorCommand::BeginComposition {
                replace_range: Utf8ByteRange::point(6),
                expected_revision: EditorRevision::new(0),
            })
            .into_result();

        let (session_id, base_rev, gen) = kernel.composition_session_info().unwrap();

        let outcome = kernel.apply(EditorCommand::CommitText {
            byte_range: Utf8ByteRange::point(6),
            replacement_text: "世界".to_string(),
            resulting_selection_anchor: Utf8ByteOffset::unchecked(12),
            resulting_selection_head: Utf8ByteOffset::unchecked(12),
            composition_session_id: EditorSessionId::new(session_id),
            composition_base_revision: EditorRevision::new(base_rev),
            composition_generation: EditorSessionGeneration::new(gen),
            cause: EditorTransactionCause::TypingCommit,
            expected_revision: begin.new_revision,
        });
        assert!(matches!(outcome, EditorEditOutcome::Applied(_)));
        assert_eq!(kernel.text(), "你好世界");
        assert_eq!(kernel.cursor(), 12);
        assert!(kernel.composition_session_info().is_none());
    }

    #[test]
    fn commit_text_wrong_session_returns_stale() {
        let mut kernel = EditorKernel::with_text("你好".to_string(), 6).unwrap();
        kernel
            .apply(EditorCommand::BeginComposition {
                replace_range: Utf8ByteRange::point(6),
                expected_revision: EditorRevision::new(0),
            })
            .into_result();

        let outcome = kernel.apply(EditorCommand::CommitText {
            byte_range: Utf8ByteRange::point(6),
            replacement_text: "世界".to_string(),
            resulting_selection_anchor: Utf8ByteOffset::unchecked(12),
            resulting_selection_head: Utf8ByteOffset::unchecked(12),
            composition_session_id: EditorSessionId::new(999),
            composition_base_revision: EditorRevision::new(0),
            composition_generation: EditorSessionGeneration::new(0),
            cause: EditorTransactionCause::TypingCommit,
            expected_revision: EditorRevision::new(1),
        });
        assert!(matches!(outcome, EditorEditOutcome::StaleRevision(_)));
        assert_eq!(kernel.text(), "你好");
    }

    #[test]
    fn commit_text_empty_string_deletes_range() {
        let mut kernel = EditorKernel::with_text("你好世界".to_string(), 12).unwrap();
        let begin = kernel
            .apply(EditorCommand::BeginComposition {
                replace_range: Utf8ByteRange::from_ordered(6, 12),
                expected_revision: EditorRevision::new(0),
            })
            .into_result();

        let (session_id, base_rev, gen) = kernel.composition_session_info().unwrap();

        let outcome = kernel.apply(EditorCommand::CommitText {
            byte_range: Utf8ByteRange::from_ordered(6, 12),
            replacement_text: String::new(),
            resulting_selection_anchor: Utf8ByteOffset::unchecked(6),
            resulting_selection_head: Utf8ByteOffset::unchecked(6),
            composition_session_id: EditorSessionId::new(session_id),
            composition_base_revision: EditorRevision::new(base_rev),
            composition_generation: EditorSessionGeneration::new(gen),
            cause: EditorTransactionCause::TypingCommit,
            expected_revision: begin.new_revision,
        });
        assert!(matches!(outcome, EditorEditOutcome::Applied(_)));
        assert_eq!(kernel.text(), "你好");
        assert_eq!(kernel.cursor(), 6);
    }

    #[test]
    fn finish_composition_materializes_preedit() {
        let mut kernel = EditorKernel::with_text("你好".to_string(), 6).unwrap();
        let begin = kernel
            .apply(EditorCommand::BeginComposition {
                replace_range: Utf8ByteRange::point(6),
                expected_revision: EditorRevision::new(0),
            })
            .into_result();

        let (session_id, _base_rev, gen) = kernel.composition_session_info().unwrap();

        kernel
            .apply(EditorCommand::UpdateComposition {
                composition_session_id: EditorSessionId::new(session_id),
                composition_generation: EditorSessionGeneration::new(gen),
                new_preedit_text: "世界".to_string(),
                new_preedit_cursor_offset: Utf8ByteOffset::unchecked(2),
                expected_revision: begin.new_revision,
            })
            .into_result();

        let (_, _, new_gen) = kernel.composition_session_info().unwrap();

        let outcome = kernel.apply(EditorCommand::FinishComposition {
            composition_session_id: EditorSessionId::new(session_id),
            composition_generation: EditorSessionGeneration::new(new_gen),
            expected_revision: begin.new_revision,
        });
        assert!(matches!(outcome, EditorEditOutcome::Applied(_)));
        assert_eq!(kernel.text(), "你好世界");
        assert_eq!(kernel.cursor(), 12);
        assert!(kernel.composition_session_info().is_none());
    }

    #[test]
    fn finish_composition_empty_preedit_no_text_change() {
        let mut kernel = EditorKernel::with_text("你好".to_string(), 6).unwrap();
        let begin = kernel
            .apply(EditorCommand::BeginComposition {
                replace_range: Utf8ByteRange::point(6),
                expected_revision: EditorRevision::new(0),
            })
            .into_result();

        let (session_id, _base_rev, gen) = kernel.composition_session_info().unwrap();

        let outcome = kernel.apply(EditorCommand::FinishComposition {
            composition_session_id: EditorSessionId::new(session_id),
            composition_generation: EditorSessionGeneration::new(gen),
            expected_revision: begin.new_revision,
        });
        assert!(matches!(outcome, EditorEditOutcome::Applied(_)));
        assert_eq!(kernel.text(), "你好");
        assert_eq!(kernel.cursor(), 6);
        assert!(kernel.composition_session_info().is_none());
    }

    #[test]
    fn cancel_composition_preserves_text() {
        let mut kernel = EditorKernel::with_text("你好".to_string(), 6).unwrap();
        let begin = kernel
            .apply(EditorCommand::BeginComposition {
                replace_range: Utf8ByteRange::from_ordered(3, 6),
                expected_revision: EditorRevision::new(0),
            })
            .into_result();

        let (session_id, _base_rev, gen) = kernel.composition_session_info().unwrap();

        kernel
            .apply(EditorCommand::UpdateComposition {
                composition_session_id: EditorSessionId::new(session_id),
                composition_generation: EditorSessionGeneration::new(gen),
                new_preedit_text: "坏".to_string(),
                new_preedit_cursor_offset: Utf8ByteOffset::unchecked(3),
                expected_revision: begin.new_revision,
            })
            .into_result();

        let (_, _, new_gen) = kernel.composition_session_info().unwrap();

        let outcome = kernel.apply(EditorCommand::CancelComposition {
            composition_session_id: EditorSessionId::new(session_id),
            composition_generation: EditorSessionGeneration::new(new_gen),
            expected_revision: begin.new_revision,
        });
        assert!(matches!(outcome, EditorEditOutcome::Applied(_)));
        assert_eq!(kernel.text(), "你好");
        assert!(kernel.composition_session_info().is_none());
    }

    #[test]
    fn stale_revision_returns_current_revision() {
        let mut kernel = EditorKernel::with_text("abc".to_string(), 3).unwrap();
        let r1 = kernel
            .apply(EditorCommand::Insert {
                byte_offset: Utf8ByteOffset::unchecked(3),
                text: "d".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(r1.new_revision.value(), 1);

        let outcome = kernel.apply(EditorCommand::Insert {
            byte_offset: Utf8ByteOffset::unchecked(4),
            text: "e".to_string(),
            cause: EditorTransactionCause::Typing,
            expected_revision: EditorRevision::new(99),
        });
        match outcome {
            EditorEditOutcome::StaleRevision(result) => {
                assert_eq!(result.base_revision.value(), 1);
                assert_eq!(result.new_revision.value(), 1);
            }
            _ => panic!("expected StaleRevision"),
        }
    }

    #[test]
    fn load_text_with_invalid_cursor_returns_adjusted() {
        let mut kernel = EditorKernel::with_text("old".to_string(), 3).unwrap();
        let outcome = kernel.load_text("new".to_string(), 100);
        assert!(matches!(
            outcome,
            EditorEditOutcome::AppliedWithAdjustedSelection(_)
        ));
        assert_eq!(kernel.text(), "new");
        assert_eq!(kernel.cursor(), 3);
    }

    #[test]
    fn load_text_with_valid_cursor_returns_applied() {
        let mut kernel = EditorKernel::with_text("old".to_string(), 3).unwrap();
        let outcome = kernel.load_text("new".to_string(), 2);
        assert!(matches!(outcome, EditorEditOutcome::Applied(_)));
        assert_eq!(kernel.text(), "new");
        assert_eq!(kernel.cursor(), 2);
    }

    #[test]
    fn commit_text_session_range_mismatch_returns_invalid_range() {
        let mut kernel = EditorKernel::with_text("你好世界".to_string(), 12).unwrap();
        let begin = kernel
            .apply(EditorCommand::BeginComposition {
                replace_range: Utf8ByteRange::from_ordered(3, 6),
                expected_revision: EditorRevision::new(0),
            })
            .into_result();

        let (session_id, base_rev, gen) = kernel.composition_session_info().unwrap();

        let outcome = kernel.apply(EditorCommand::CommitText {
            byte_range: Utf8ByteRange::from_ordered(6, 12),
            replacement_text: "XX".to_string(),
            resulting_selection_anchor: Utf8ByteOffset::unchecked(8),
            resulting_selection_head: Utf8ByteOffset::unchecked(8),
            composition_session_id: EditorSessionId::new(session_id),
            composition_base_revision: EditorRevision::new(base_rev),
            composition_generation: EditorSessionGeneration::new(gen),
            cause: EditorTransactionCause::TypingCommit,
            expected_revision: begin.new_revision,
        });
        assert!(matches!(outcome, EditorEditOutcome::InvalidRange(_)));
        assert_eq!(kernel.text(), "你好世界");
    }

    #[test]
    fn commit_text_empty_range_empty_text_no_session_returns_no_change() {
        let mut kernel = EditorKernel::with_text("abc".to_string(), 3).unwrap();
        let rev_before = kernel.revision();
        let outcome = kernel.apply(EditorCommand::CommitText {
            byte_range: Utf8ByteRange::point(3),
            replacement_text: String::new(),
            resulting_selection_anchor: Utf8ByteOffset::unchecked(3),
            resulting_selection_head: Utf8ByteOffset::unchecked(3),
            composition_session_id: EditorSessionId::new(0),
            composition_base_revision: EditorRevision::new(0),
            composition_generation: EditorSessionGeneration::new(0),
            cause: EditorTransactionCause::Typing,
            expected_revision: EditorRevision::new(0),
        });
        assert!(matches!(outcome, EditorEditOutcome::NoChange(_)));
        assert_eq!(kernel.revision(), rev_before);
        assert_eq!(kernel.text(), "abc");
    }

    #[test]
    fn finish_composition_cursor_at_committed_text_end() {
        let mut kernel = EditorKernel::with_text("你好".to_string(), 6).unwrap();
        let begin = kernel
            .apply(EditorCommand::BeginComposition {
                replace_range: Utf8ByteRange::point(6),
                expected_revision: EditorRevision::new(0),
            })
            .into_result();

        let (session_id, _base_rev, gen) = kernel.composition_session_info().unwrap();

        kernel
            .apply(EditorCommand::UpdateComposition {
                composition_session_id: EditorSessionId::new(session_id),
                composition_generation: EditorSessionGeneration::new(gen),
                new_preedit_text: "世界".to_string(),
                new_preedit_cursor_offset: Utf8ByteOffset::unchecked(2),
                expected_revision: begin.new_revision,
            })
            .into_result();

        let (_, _, new_gen) = kernel.composition_session_info().unwrap();

        let outcome = kernel.apply(EditorCommand::FinishComposition {
            composition_session_id: EditorSessionId::new(session_id),
            composition_generation: EditorSessionGeneration::new(new_gen),
            expected_revision: begin.new_revision,
        });
        assert!(matches!(outcome, EditorEditOutcome::Applied(_)));
        assert_eq!(kernel.text(), "你好世界");
        assert_eq!(kernel.cursor(), 12);
        assert_eq!(kernel.selection_anchor(), 12);
    }

    #[test]
    fn finish_composition_cursor_exceeds_preedit_length_returns_adjusted() {
        let mut kernel = EditorKernel::with_text("你好".to_string(), 6).unwrap();
        let begin = kernel
            .apply(EditorCommand::BeginComposition {
                replace_range: Utf8ByteRange::point(6),
                expected_revision: EditorRevision::new(0),
            })
            .into_result();

        let (session_id, _base_rev, gen) = kernel.composition_session_info().unwrap();

        kernel
            .apply(EditorCommand::UpdateComposition {
                composition_session_id: EditorSessionId::new(session_id),
                composition_generation: EditorSessionGeneration::new(gen),
                new_preedit_text: "世界".to_string(),
                new_preedit_cursor_offset: Utf8ByteOffset::unchecked(99),
                expected_revision: begin.new_revision,
            })
            .into_result();

        let (_, _, new_gen) = kernel.composition_session_info().unwrap();

        let outcome = kernel.apply(EditorCommand::FinishComposition {
            composition_session_id: EditorSessionId::new(session_id),
            composition_generation: EditorSessionGeneration::new(new_gen),
            expected_revision: begin.new_revision,
        });
        assert!(matches!(
            outcome,
            EditorEditOutcome::AppliedWithAdjustedSelection(_)
        ));
        assert_eq!(kernel.text(), "你好世界");
        assert_eq!(kernel.cursor(), 12);
        assert!(kernel.composition_session_info().is_none());
    }

    #[test]
    fn delete_surrounding_large_before_does_not_underflow() {
        let mut kernel = EditorKernel::with_text("ABCDE".to_string(), 3).unwrap();
        kernel
            .apply(EditorCommand::SetSelection {
                anchor: Utf8ByteOffset::unchecked(3),
                head: Utf8ByteOffset::unchecked(5),
                expected_revision: EditorRevision::new(0),
            })
            .into_result();

        let outcome = kernel.apply(EditorCommand::DeleteSurrounding {
            before_byte_range: Utf8ByteRange::from_ordered(0, 2),
            after_byte_range: Utf8ByteRange::zero(),
            cause: EditorTransactionCause::Delete,
            expected_revision: EditorRevision::new(0),
        });
        assert!(matches!(outcome, EditorEditOutcome::Applied(_)));
        assert_eq!(kernel.text(), "CDE");
        assert_eq!(kernel.selection_anchor(), 1);
        assert_eq!(kernel.cursor(), 3);
    }

    #[test]
    fn commit_text_without_composition_session_has_insert_operation_kind() {
        let mut kernel = EditorKernel::with_text("".to_string(), 0).unwrap();
        let outcome = kernel.apply(EditorCommand::CommitText {
            byte_range: Utf8ByteRange::point(0),
            replacement_text: "Hello".to_string(),
            resulting_selection_anchor: Utf8ByteOffset::unchecked(5),
            resulting_selection_head: Utf8ByteOffset::unchecked(5),
            composition_session_id: EditorSessionId::new(0),
            composition_base_revision: EditorRevision::new(0),
            composition_generation: EditorSessionGeneration::new(0),
            cause: EditorTransactionCause::Typing,
            expected_revision: EditorRevision::new(0),
        });
        let result = outcome.into_result();
        assert_eq!(
            result.visual_intent.operation_kind,
            EditorOperationKind::Insert
        );
        assert_eq!(kernel.text(), "Hello");
    }

    #[test]
    fn commit_text_without_composition_session_delete_has_delete_operation_kind() {
        let mut kernel = EditorKernel::with_text("ABCDE".to_string(), 3).unwrap();
        let outcome = kernel.apply(EditorCommand::CommitText {
            byte_range: Utf8ByteRange::from_ordered(1, 4),
            replacement_text: "".to_string(),
            resulting_selection_anchor: Utf8ByteOffset::unchecked(1),
            resulting_selection_head: Utf8ByteOffset::unchecked(1),
            composition_session_id: EditorSessionId::new(0),
            composition_base_revision: EditorRevision::new(0),
            composition_generation: EditorSessionGeneration::new(0),
            cause: EditorTransactionCause::Typing,
            expected_revision: EditorRevision::new(0),
        });
        let result = outcome.into_result();
        assert_eq!(
            result.visual_intent.operation_kind,
            EditorOperationKind::Delete
        );
        assert_eq!(kernel.text(), "AE");
    }

    #[test]
    fn commit_text_with_composition_session_has_composition_commit_operation_kind() {
        let mut kernel = EditorKernel::with_text("".to_string(), 0).unwrap();
        let begin = kernel
            .apply(EditorCommand::BeginComposition {
                replace_range: Utf8ByteRange::point(0),
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        let (session_id, base_rev, gen) = kernel.composition_session_info().unwrap();
        let outcome = kernel.apply(EditorCommand::CommitText {
            byte_range: Utf8ByteRange::point(0),
            replacement_text: "你好".to_string(),
            resulting_selection_anchor: Utf8ByteOffset::unchecked(6),
            resulting_selection_head: Utf8ByteOffset::unchecked(6),
            composition_session_id: EditorSessionId::new(session_id),
            composition_base_revision: EditorRevision::new(base_rev),
            composition_generation: EditorSessionGeneration::new(gen),
            cause: EditorTransactionCause::TypingCommit,
            expected_revision: begin.new_revision,
        });
        let result = outcome.into_result();
        assert_eq!(
            result.visual_intent.operation_kind,
            EditorOperationKind::CompositionCommit
        );
        assert_eq!(kernel.text(), "你好");
    }

    #[test]
    fn begin_composition_replaces_stale_session_instead_of_rejecting() {
        // Session-divergence recovery: when a composition session already exists (e.g.
        // the platform lost track of it after a soft reset), a new begin is authoritative
        // and replaces the stale session — otherwise every subsequent plain commit would
        // be rejected as StaleRevision and future compositions would be blocked.
        let mut kernel = EditorKernel::with_text("abc".to_string(), 1).unwrap();
        let first_begin = kernel.apply(EditorCommand::BeginComposition {
            replace_range: Utf8ByteRange::point(1),
            expected_revision: EditorRevision::new(0),
        });
        assert!(first_begin.is_applied());
        let first_begin_result = first_begin.into_result();
        let (first_session_id, first_base_rev, first_gen) =
            kernel.composition_session_info().unwrap();

        // A second begin (e.g. a fresh IME interaction after the platform lost the
        // session) must succeed and replace the stale session with a new one.
        let second_begin = kernel.apply(EditorCommand::BeginComposition {
            replace_range: Utf8ByteRange::point(2),
            expected_revision: first_begin_result.new_revision,
        });
        assert!(second_begin.is_applied());
        let second_begin_result = second_begin.into_result();
        let (second_session_id, _, _) = kernel.composition_session_info().unwrap();
        assert_ne!(second_session_id, first_session_id);

        // The stale session's id must no longer be accepted: a commit against it is stale.
        let stale_commit = kernel.apply(EditorCommand::CommitText {
            byte_range: Utf8ByteRange::point(2),
            replacement_text: "x".to_string(),
            resulting_selection_anchor: Utf8ByteOffset::unchecked(3),
            resulting_selection_head: Utf8ByteOffset::unchecked(3),
            composition_session_id: EditorSessionId::new(first_session_id),
            composition_base_revision: EditorRevision::new(first_base_rev),
            composition_generation: EditorSessionGeneration::new(first_gen),
            cause: EditorTransactionCause::TypingCommit,
            expected_revision: second_begin_result.new_revision,
        });
        assert!(matches!(stale_commit, EditorEditOutcome::StaleRevision(_)));

        // A commit through the *new* session must apply normally.
        let (_, second_base_rev, second_gen) = kernel.composition_session_info().unwrap();
        let commit = kernel.apply(EditorCommand::CommitText {
            byte_range: Utf8ByteRange::point(2),
            replacement_text: "y".to_string(),
            resulting_selection_anchor: Utf8ByteOffset::unchecked(3),
            resulting_selection_head: Utf8ByteOffset::unchecked(3),
            composition_session_id: EditorSessionId::new(second_session_id),
            composition_base_revision: EditorRevision::new(second_base_rev),
            composition_generation: EditorSessionGeneration::new(second_gen),
            cause: EditorTransactionCause::TypingCommit,
            expected_revision: second_begin_result.new_revision,
        });
        let result = commit.into_result();
        assert_eq!(
            result.visual_intent.operation_kind,
            EditorOperationKind::CompositionCommit
        );
        assert_eq!(kernel.text(), "abyc");
        // The session is consumed by the commit; the next begin starts clean again.
        assert!(kernel.composition_session_info().is_none());
    }

    #[test]
    fn replace_empty_range_has_insert_operation_kind() {
        let mut kernel = EditorKernel::with_text("ABCDE".to_string(), 3).unwrap();
        let result = kernel
            .apply(EditorCommand::Replace {
                byte_range: Utf8ByteRange::point(3),
                replacement_text: "X".to_string(),
                original_text: String::new(),
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(
            result.visual_intent.operation_kind,
            EditorOperationKind::Insert
        );
        assert_eq!(kernel.text(), "ABCXDE");
    }

    #[test]
    fn replace_with_empty_replacement_has_delete_operation_kind() {
        let mut kernel = EditorKernel::with_text("ABCDE".to_string(), 3).unwrap();
        let result = kernel
            .apply(EditorCommand::Replace {
                byte_range: Utf8ByteRange::from_ordered(1, 4),
                replacement_text: "".to_string(),
                original_text: "BCD".to_string(),
                cause: EditorTransactionCause::Delete,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(
            result.visual_intent.operation_kind,
            EditorOperationKind::Delete
        );
        assert_eq!(kernel.text(), "AE");
    }

    #[test]
    fn replace_non_empty_range_has_replace_operation_kind() {
        let mut kernel = EditorKernel::with_text("ABCDE".to_string(), 3).unwrap();
        let result = kernel
            .apply(EditorCommand::Replace {
                byte_range: Utf8ByteRange::from_ordered(1, 4),
                replacement_text: "XY".to_string(),
                original_text: "BCD".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(
            result.visual_intent.operation_kind,
            EditorOperationKind::Replace
        );
        assert_eq!(kernel.text(), "AXYE");
    }

    // #606: auto-indent 测试

    #[test]
    fn insert_line_break_with_auto_indent_copies_leading_whitespace() {
        let mut kernel = EditorKernel::with_text("    hello".to_string(), 9).unwrap();
        let result = kernel
            .apply(EditorCommand::InsertLineBreak {
                byte_offset: Utf8ByteOffset::unchecked(9),
                auto_indent_enabled: true,
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(kernel.text(), "    hello\n    ");
        assert_eq!(kernel.cursor(), 14);
        assert_eq!(
            result.visual_intent.operation_kind,
            EditorOperationKind::Insert
        );
    }

    #[test]
    fn insert_line_break_with_auto_indent_copies_tab_indent() {
        let mut kernel = EditorKernel::with_text("\t\thello".to_string(), 7).unwrap();
        let _result = kernel
            .apply(EditorCommand::InsertLineBreak {
                byte_offset: Utf8ByteOffset::unchecked(7),
                auto_indent_enabled: true,
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(kernel.text(), "\t\thello\n\t\t");
    }

    #[test]
    fn insert_line_break_without_auto_indent_inserts_only_newline() {
        let mut kernel = EditorKernel::with_text("    hello".to_string(), 9).unwrap();
        let _result = kernel
            .apply(EditorCommand::InsertLineBreak {
                byte_offset: Utf8ByteOffset::unchecked(9),
                auto_indent_enabled: false,
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(kernel.text(), "    hello\n");
    }

    #[test]
    fn insert_line_break_auto_indent_no_leading_whitespace() {
        let mut kernel = EditorKernel::with_text("hello".to_string(), 5).unwrap();
        let _result = kernel
            .apply(EditorCommand::InsertLineBreak {
                byte_offset: Utf8ByteOffset::unchecked(5),
                auto_indent_enabled: true,
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(kernel.text(), "hello\n");
    }

    #[test]
    fn insert_line_break_auto_indent_mid_line() {
        let mut kernel = EditorKernel::with_text("  hello world".to_string(), 7).unwrap();
        let _result = kernel
            .apply(EditorCommand::InsertLineBreak {
                byte_offset: Utf8ByteOffset::unchecked(7),
                auto_indent_enabled: true,
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(kernel.text(), "  hello\n   world");
    }

    #[test]
    fn insert_line_break_auto_indent_multiline() {
        let text = "first\n    second".to_string();
        let cursor = text.len();
        let mut kernel = EditorKernel::with_text(text, cursor).unwrap();
        let _result = kernel
            .apply(EditorCommand::InsertLineBreak {
                byte_offset: Utf8ByteOffset::unchecked(cursor),
                auto_indent_enabled: true,
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(kernel.text(), "first\n    second\n    ");
    }

    #[test]
    fn insert_line_break_auto_indent_utf8_safe() {
        let text = "  你好世界".to_string();
        let cursor = text.len();
        let mut kernel = EditorKernel::with_text(text, cursor).unwrap();
        let _result = kernel
            .apply(EditorCommand::InsertLineBreak {
                byte_offset: Utf8ByteOffset::unchecked(cursor),
                auto_indent_enabled: true,
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(kernel.text(), "  你好世界\n  ");
    }

    #[test]
    fn insert_line_break_auto_indent_utf8_no_indent_after_cjk() {
        let text = "你好  world".to_string();
        let cursor = text.len();
        let mut kernel = EditorKernel::with_text(text, cursor).unwrap();
        let _result = kernel
            .apply(EditorCommand::InsertLineBreak {
                byte_offset: Utf8ByteOffset::unchecked(cursor),
                auto_indent_enabled: true,
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(kernel.text(), "你好  world\n");
    }

    #[test]
    fn insert_line_break_auto_indent_mixed_space_and_tab() {
        let text = "  \t  hello".to_string();
        let cursor = text.len();
        let mut kernel = EditorKernel::with_text(text, cursor).unwrap();
        let _result = kernel
            .apply(EditorCommand::InsertLineBreak {
                byte_offset: Utf8ByteOffset::unchecked(cursor),
                auto_indent_enabled: true,
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(kernel.text(), "  \t  hello\n  \t  ");
    }

    #[test]
    fn insert_line_break_auto_indent_undo_restores_text() {
        let mut kernel = EditorKernel::with_text("    hello".to_string(), 9).unwrap();
        let _ = kernel
            .apply(EditorCommand::InsertLineBreak {
                byte_offset: Utf8ByteOffset::unchecked(9),
                auto_indent_enabled: true,
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(kernel.text(), "    hello\n    ");
        let _ = kernel
            .apply(EditorCommand::Undo {
                expected_revision: EditorRevision::new(1),
            })
            .into_result();
        assert_eq!(kernel.text(), "    hello");
    }

    // #606: composition 视觉分类通过共享逻辑确定

    #[test]
    fn composition_update_visual_intent_uses_shared_classification() {
        let mut kernel = EditorKernel::with_text("hello".to_string(), 5).unwrap();
        let _ = kernel.apply(EditorCommand::BeginComposition {
            replace_range: Utf8ByteRange::point(5),
            expected_revision: EditorRevision::new(0),
        });
        let (session_id, _, generation) = kernel.composition_session_info().unwrap();
        let result = kernel
            .apply(EditorCommand::UpdateComposition {
                composition_session_id: EditorSessionId::new(session_id),
                composition_generation: EditorSessionGeneration::new(generation),
                new_preedit_text: "n".to_string(),
                new_preedit_cursor_offset: Utf8ByteOffset::unchecked(1),
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(
            result.visual_intent.operation_kind,
            EditorOperationKind::CompositionUpdate
        );
        assert_eq!(
            result.visual_intent.animation_mode,
            AnimationMode::GlyphAnimation
        );
        assert!(!result.visual_intent.new_affected_byte_ranges.is_empty());
    }

    #[test]
    fn composition_cancel_visual_intent_uses_shared_classification() {
        let mut kernel = EditorKernel::with_text("hello".to_string(), 5).unwrap();
        let _ = kernel.apply(EditorCommand::BeginComposition {
            replace_range: Utf8ByteRange::point(5),
            expected_revision: EditorRevision::new(0),
        });
        let (session_id, _, generation) = kernel.composition_session_info().unwrap();
        let _ = kernel.apply(EditorCommand::UpdateComposition {
            composition_session_id: EditorSessionId::new(session_id),
            composition_generation: EditorSessionGeneration::new(generation),
            new_preedit_text: "nihao".to_string(),
            new_preedit_cursor_offset: Utf8ByteOffset::unchecked(5),
            expected_revision: EditorRevision::new(0),
        });
        let (_, _, gen2) = kernel.composition_session_info().unwrap();
        let result = kernel
            .apply(EditorCommand::CancelComposition {
                composition_session_id: EditorSessionId::new(session_id),
                composition_generation: EditorSessionGeneration::new(gen2),
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(
            result.visual_intent.operation_kind,
            EditorOperationKind::CompositionCancel
        );
        assert!(!result.visual_intent.old_affected_byte_ranges.is_empty());
        assert!(result.visual_intent.new_affected_byte_ranges.is_empty());
    }

    #[test]
    fn composition_finish_visual_intent_uses_shared_classification() {
        let mut kernel = EditorKernel::with_text("hello".to_string(), 5).unwrap();
        let _ = kernel.apply(EditorCommand::BeginComposition {
            replace_range: Utf8ByteRange::point(5),
            expected_revision: EditorRevision::new(0),
        });
        let (session_id, _, generation) = kernel.composition_session_info().unwrap();
        let _ = kernel.apply(EditorCommand::UpdateComposition {
            composition_session_id: EditorSessionId::new(session_id),
            composition_generation: EditorSessionGeneration::new(generation),
            new_preedit_text: " world".to_string(),
            new_preedit_cursor_offset: Utf8ByteOffset::unchecked(6),
            expected_revision: EditorRevision::new(0),
        });
        let (_, _, gen2) = kernel.composition_session_info().unwrap();
        let result = kernel
            .apply(EditorCommand::FinishComposition {
                composition_session_id: EditorSessionId::new(session_id),
                composition_generation: EditorSessionGeneration::new(gen2),
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert_eq!(
            result.visual_intent.operation_kind,
            EditorOperationKind::CompositionCommit
        );
        assert_eq!(kernel.text(), "hello world");
    }

    // ── #606: grapheme 边界 API 测试 ──

    #[test]
    fn grapheme_boundary_previous_ascii() {
        // 纯 ASCII：每个字节都是一个 grapheme cluster
        let kernel = EditorKernel::with_text("hello".to_string(), 5).unwrap();
        // offset=5 -> previous boundary=4
        assert_eq!(kernel.previous_grapheme_boundary(5), 4);
        // offset=3 -> previous boundary=2
        assert_eq!(kernel.previous_grapheme_boundary(3), 2);
        // offset=1 -> previous boundary=0
        assert_eq!(kernel.previous_grapheme_boundary(1), 0);
    }

    #[test]
    fn grapheme_boundary_previous_emoji() {
        // "a😀b"：😀 是 4 字节，grapheme 边界在 0, 1, 5, 6
        let text = "a😀b";
        let kernel = EditorKernel::with_text(text.to_string(), text.len()).unwrap();
        // offset=6 (末尾) -> previous=5
        assert_eq!(kernel.previous_grapheme_boundary(6), 5);
        // offset=5 (😀 之后) -> previous=1
        assert_eq!(kernel.previous_grapheme_boundary(5), 1);
        // offset=1 (a 之后) -> previous=0
        assert_eq!(kernel.previous_grapheme_boundary(1), 0);
    }

    #[test]
    fn grapheme_boundary_previous_combining_mark() {
        // "e\u{0301}" 是一个 grapheme cluster（e + 组合重音），边界在 0 和 3
        let text = "e\u{0301}";
        let kernel = EditorKernel::with_text(text.to_string(), text.len()).unwrap();
        // offset=3 (末尾) -> previous=0（整个 cluster 是一个 grapheme）
        assert_eq!(kernel.previous_grapheme_boundary(3), 0);
        // offset=1 (e 之后，但仍在 cluster 内) -> previous=0
        assert_eq!(kernel.previous_grapheme_boundary(1), 0);
    }

    #[test]
    fn grapheme_boundary_next_ascii() {
        let kernel = EditorKernel::with_text("hello".to_string(), 0).unwrap();
        // offset=0 -> next=1
        assert_eq!(kernel.next_grapheme_boundary(0), 1);
        // offset=3 -> next=4
        assert_eq!(kernel.next_grapheme_boundary(3), 4);
        // offset=4 -> next=5
        assert_eq!(kernel.next_grapheme_boundary(4), 5);
    }

    #[test]
    fn grapheme_boundary_next_emoji() {
        // "a😀b"：grapheme 边界在 0, 1, 5, 6
        let text = "a😀b";
        let kernel = EditorKernel::with_text(text.to_string(), 0).unwrap();
        // offset=0 -> next=1
        assert_eq!(kernel.next_grapheme_boundary(0), 1);
        // offset=1 -> next=5（跳过整个 emoji）
        assert_eq!(kernel.next_grapheme_boundary(1), 5);
        // offset=5 -> next=6
        assert_eq!(kernel.next_grapheme_boundary(5), 6);
    }

    #[test]
    fn grapheme_boundary_edge_cases() {
        let text = "abc";
        let kernel = EditorKernel::with_text(text.to_string(), 0).unwrap();
        // offset=0 -> previous=0（没有更早的边界）
        assert_eq!(kernel.previous_grapheme_boundary(0), 0);
        // offset=0 -> next=1
        assert_eq!(kernel.next_grapheme_boundary(0), 1);
        // offset=text.len() -> previous=len-1
        assert_eq!(kernel.previous_grapheme_boundary(3), 2);
        // offset=text.len() -> next=len（没有更晚的边界，返回 len）
        assert_eq!(kernel.next_grapheme_boundary(3), 3);
        // offset > text.len() -> previous 返回 len
        assert_eq!(kernel.previous_grapheme_boundary(100), 3);
        // offset > text.len() -> next 返回 len
        assert_eq!(kernel.next_grapheme_boundary(100), 3);
    }

    // ── #606: EditorVisualIntent.offset_map 测试 ──

    #[test]
    fn visual_intent_offset_map_insert() {
        // 插入 "X" 到 "ab" 的 offset=1，得到 "aXb"
        let mut kernel = EditorKernel::with_text("ab".to_string(), 2).unwrap();
        let result = kernel
            .apply(EditorCommand::Insert {
                byte_offset: Utf8ByteOffset::unchecked(1),
                text: "X".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        let map = result
            .visual_intent
            .offset_map
            .expect("insert 应产生 offset_map");
        // old="ab", new="aXb"：前缀 "a" identity (0->0, len=1)，后缀 "b" shifted (1->2, len=1)
        assert_eq!(map.entries.len(), 2);
        // 前缀 identity
        assert_eq!(map.entries[0].old_byte_offset.value(), 0);
        assert_eq!(map.entries[0].new_byte_offset.value(), 0);
        assert_eq!(map.entries[0].length, 1);
        assert_eq!(map.entries[0].kind, crate::editor::OffsetMapKind::Identity);
        // 后缀 shifted
        assert_eq!(map.entries[1].old_byte_offset.value(), 1);
        assert_eq!(map.entries[1].new_byte_offset.value(), 2);
        assert_eq!(map.entries[1].length, 1);
        assert_eq!(map.entries[1].kind, crate::editor::OffsetMapKind::Shifted);
        // 验证映射函数
        assert_eq!(map.map_old_to_new(0), Some(0));
        assert_eq!(map.map_old_to_new(1), Some(2));
    }

    #[test]
    fn visual_intent_offset_map_delete() {
        // 从 "aXb" 删除 "X"（offset 1..2），得到 "ab"
        let mut kernel = EditorKernel::with_text("aXb".to_string(), 3).unwrap();
        let result = kernel
            .apply(EditorCommand::Delete {
                byte_range: Utf8ByteRange::from_ordered(1, 2),
                deleted_text: "X".to_string(),
                cause: EditorTransactionCause::Delete,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        let map = result
            .visual_intent
            .offset_map
            .expect("delete 应产生 offset_map");
        // old="aXb", new="ab"：前缀 "a" identity (0->0, len=1)，后缀 "b" shifted (2->1, len=1)
        assert_eq!(map.entries.len(), 2);
        assert_eq!(map.entries[0].old_byte_offset.value(), 0);
        assert_eq!(map.entries[0].new_byte_offset.value(), 0);
        assert_eq!(map.entries[0].kind, crate::editor::OffsetMapKind::Identity);
        assert_eq!(map.entries[1].old_byte_offset.value(), 2);
        assert_eq!(map.entries[1].new_byte_offset.value(), 1);
        assert_eq!(map.entries[1].kind, crate::editor::OffsetMapKind::Shifted);
        // 验证映射
        assert_eq!(map.map_old_to_new(0), Some(0));
        assert_eq!(map.map_old_to_new(2), Some(1));
    }

    #[test]
    fn visual_intent_offset_map_replace() {
        // 从 "hello" 替换 "ll"（offset 2..4）为 "LL"，得到 "heLLo"
        let mut kernel = EditorKernel::with_text("hello".to_string(), 5).unwrap();
        let result = kernel
            .apply(EditorCommand::Replace {
                byte_range: Utf8ByteRange::from_ordered(2, 4),
                replacement_text: "LL".to_string(),
                original_text: "ll".to_string(),
                cause: EditorTransactionCause::Typing,
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        let map = result
            .visual_intent
            .offset_map
            .expect("replace 应产生 offset_map");
        // old="hello", new="heLLo"：前缀 "he" identity (0->0, len=2)，后缀 "o" shifted (4->4, len=1)
        assert_eq!(map.entries.len(), 2);
        assert_eq!(map.entries[0].old_byte_offset.value(), 0);
        assert_eq!(map.entries[0].new_byte_offset.value(), 0);
        assert_eq!(map.entries[0].length, 2);
        assert_eq!(map.entries[0].kind, crate::editor::OffsetMapKind::Identity);
        assert_eq!(map.entries[1].old_byte_offset.value(), 4);
        assert_eq!(map.entries[1].new_byte_offset.value(), 4);
        assert_eq!(map.entries[1].length, 1);
        assert_eq!(map.entries[1].kind, crate::editor::OffsetMapKind::Shifted);
    }

    #[test]
    fn visual_intent_offset_map_cursor_only() {
        // 选区操作不修改正文，offset_map 应为 None
        let mut kernel = EditorKernel::with_text("hello".to_string(), 5).unwrap();
        let result = kernel
            .apply(EditorCommand::SetSelection {
                anchor: Utf8ByteOffset::unchecked(0),
                head: Utf8ByteOffset::unchecked(3),
                expected_revision: EditorRevision::new(0),
            })
            .into_result();
        assert!(
            result.visual_intent.offset_map.is_none(),
            "选区操作 offset_map 应为 None"
        );
    }
}
