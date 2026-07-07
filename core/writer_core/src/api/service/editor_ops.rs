//! Editor animation operations for WriterCoreApi.
//!
//! Delegates to `crate::editor::EditorEngine` for text-change semantic analysis
//! and `should_animate` decisions. Platform clients consume the resulting
//! `EditorAnimationEventDto` list or `EditorVisualTransactionDto` to drive
//! their own renderers.

use crate::api::service::{ApiResult, WriterCoreApi};
use crate::api::types::*;
use crate::editor::{EditorEngine, EditorSelection, EditorTransactionCause};

impl WriterCoreApi {
    /// Compute a visual transaction for a text change.
    ///
    /// This is a **stateless** computation: the caller provides old/new text,
    /// cursor positions, and cause. Core decides `should_animate` and returns
    /// an `EditorVisualTransactionDto` if animation is warranted, or `None`
    /// otherwise. The platform side is responsible for computing glyph
    /// coordinates using its own Layout, then submitting to its renderer.
    ///
    /// # Arguments
    ///
    /// * `old_text` - Text before the change
    /// * `new_text` - Text after the change
    /// * `old_cursor_index` - UTF-8 byte offset of cursor before the change
    /// * `new_cursor_index` - UTF-8 byte offset of cursor after the change
    /// * `cause` - What caused the change (Typing, Delete, Paste, etc.)
    /// * `max_animated_chars` - Maximum number of chars to animate (e.g., 8)
    /// * `animation_duration_ms` - Duration of the animation in milliseconds
    pub fn editor_visual_transaction(
        &self,
        old_text: &str,
        new_text: &str,
        old_cursor_index: u32,
        new_cursor_index: u32,
        cause: EditorTransactionCauseDto,
        max_animated_chars: u32,
        animation_duration_ms: u64,
    ) -> ApiResult<Option<EditorVisualTransactionDto>> {
        let old_sel = EditorSelection::collapsed(old_text, old_cursor_index as usize);
        let new_sel = EditorSelection::collapsed(new_text, new_cursor_index as usize);
        let core_cause: EditorTransactionCause = cause.into();

        let mut engine =
            EditorEngine::with_animation_limits(max_animated_chars as usize, animation_duration_ms);
        let transaction =
            engine.create_transaction(old_text, new_text, old_sel, new_sel, core_cause);
        let vt = engine.visual_transaction(&transaction);

        Ok(vt.map(Into::into))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── editor_visual_transaction tests ──

    #[test]
    fn typing_single_char_returns_visual_transaction() {
        let api = WriterCoreApi::new("");
        let vt = api
            .editor_visual_transaction("ab", "abc", 2, 3, EditorTransactionCauseDto::Typing, 8, 120)
            .unwrap()
            .unwrap();

        assert_eq!(vt.kind, EditorAnimationKindDto::Insert);
        assert_eq!(vt.cause, EditorTransactionCauseDto::Typing);
        assert_eq!(vt.old_text, "ab");
        assert_eq!(vt.new_text, "abc");
        assert_eq!(vt.old_selection_anchor, 2);
        assert_eq!(vt.old_selection_head, 2);
        assert_eq!(vt.new_selection_anchor, 3);
        assert_eq!(vt.new_selection_head, 3);
        assert_eq!(vt.inserted_range_start, 2);
        assert_eq!(vt.inserted_range_end, 3);
        assert_eq!(vt.duration_ms, 120);
        assert_eq!(vt.coordinate_mode, VisualCoordinateModeDto::Baseline);
    }

    #[test]
    fn delete_single_char_returns_visual_transaction() {
        let api = WriterCoreApi::new("");
        let vt = api
            .editor_visual_transaction("abc", "ab", 3, 2, EditorTransactionCauseDto::Delete, 8, 120)
            .unwrap()
            .unwrap();

        assert_eq!(vt.kind, EditorAnimationKindDto::Delete);
        assert_eq!(vt.cause, EditorTransactionCauseDto::Delete);
        assert_eq!(vt.old_text, "abc");
        assert_eq!(vt.new_text, "ab");
        // Delete has no inserted_range → (0, 0)
        assert_eq!(vt.inserted_range_start, 0);
        assert_eq!(vt.inserted_range_end, 0);
    }

    #[test]
    fn paste_returns_none_visual_transaction() {
        let api = WriterCoreApi::new("");
        let vt = api
            .editor_visual_transaction(
                "a",
                "a long pasted text",
                1,
                19,
                EditorTransactionCauseDto::Paste,
                8,
                120,
            )
            .unwrap();

        // Paste 现在进入 visual transaction
        assert!(vt.is_some());
        let vt = vt.unwrap();
        assert_eq!(vt.kind, EditorAnimationKindDto::Insert);
        assert_eq!(vt.cause, EditorTransactionCauseDto::Paste);
    }

    #[test]
    fn load_returns_none_visual_transaction() {
        let api = WriterCoreApi::new("");
        let vt = api
            .editor_visual_transaction("", "loaded", 0, 6, EditorTransactionCauseDto::Load, 8, 120)
            .unwrap();

        assert!(vt.is_none());
    }

    #[test]
    fn visual_transaction_insert_has_inserted_range() {
        let api = WriterCoreApi::new("");
        let vt = api
            .editor_visual_transaction("ab", "abc", 2, 3, EditorTransactionCauseDto::Typing, 8, 160)
            .unwrap()
            .unwrap();

        assert_eq!(vt.inserted_range_start, 2);
        assert_eq!(vt.inserted_range_end, 3);
        assert_eq!(vt.duration_ms, 160);
    }

    #[test]
    fn visual_transaction_delete_has_zero_inserted_range() {
        let api = WriterCoreApi::new("");
        let vt = api
            .editor_visual_transaction("abc", "ab", 3, 2, EditorTransactionCauseDto::Delete, 8, 160)
            .unwrap()
            .unwrap();

        assert_eq!(vt.inserted_range_start, 0);
        assert_eq!(vt.inserted_range_end, 0);
    }

    #[test]
    fn visual_transaction_utf8_insert() {
        let api = WriterCoreApi::new("");
        // Insert "世" after "你好" → byte offset 6
        let vt = api
            .editor_visual_transaction(
                "你好",
                "你好世",
                "你好".len() as u32,
                "你好世".len() as u32,
                EditorTransactionCauseDto::Typing,
                8,
                120,
            )
            .unwrap()
            .unwrap();

        assert_eq!(vt.kind, EditorAnimationKindDto::Insert);
        assert_eq!(vt.inserted_range_start, "你好".len() as u32);
        assert_eq!(vt.inserted_range_end, "你好世".len() as u32);
    }
}
