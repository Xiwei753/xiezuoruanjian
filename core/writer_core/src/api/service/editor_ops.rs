//! Editor animation operations for WriterCoreApi.
//!
//! Delegates to `crate::editor::EditorEngine` for text-change semantic analysis
//! and `should_animate` decisions. Platform clients consume the resulting
//! `EditorAnimationEventDto` list to drive their own renderers.

use crate::api::service::{ApiResult, WriterCoreApi};
use crate::api::types::*;
use crate::editor::{EditorEngine, EditorSelection, EditorTransactionCause};

impl WriterCoreApi {
    /// Compute animation events for a text change.
    ///
    /// This is a **stateless** computation: the caller provides old/new text,
    /// cursor positions, and cause. Core decides `should_animate` and returns
    /// a list of animation events. The platform side is responsible for
    /// computing glyph coordinates from `range_start`/`range_len`/`text`
    /// using its own Layout, then submitting to its renderer.
    ///
    /// # Arguments
    ///
    /// * `old_text` - Text before the change
    /// * `new_text` - Text after the change
    /// * `old_cursor_index` - UTF-8 byte offset of cursor before the change
    /// * `new_cursor_index` - UTF-8 byte offset of cursor after the change
    /// * `cause` - What caused the change (Typing, Delete, Paste, etc.)
    /// * `max_animated_chars` - Maximum number of chars to animate (e.g., 8)
    /// * `animation_duration_ms` - Duration of each animation in milliseconds
    pub fn editor_animation_events(
        &self,
        old_text: &str,
        new_text: &str,
        old_cursor_index: u32,
        new_cursor_index: u32,
        cause: EditorTransactionCauseDto,
        max_animated_chars: u32,
        animation_duration_ms: u64,
    ) -> ApiResult<Vec<EditorAnimationEventDto>> {
        let old_sel = EditorSelection::collapsed(old_text, old_cursor_index as usize);
        let new_sel = EditorSelection::collapsed(new_text, new_cursor_index as usize);
        let core_cause: EditorTransactionCause = cause.into();

        let mut engine =
            EditorEngine::with_animation_limits(max_animated_chars as usize, animation_duration_ms);
        let transaction =
            engine.create_transaction(old_text, new_text, old_sel, new_sel, core_cause);
        let events = engine.animation_events(&transaction);

        Ok(events.into_iter().map(Into::into).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_single_char_emits_insert_and_cursor_events() {
        let api = WriterCoreApi::new("");
        let events = api
            .editor_animation_events("ab", "abc", 2, 3, EditorTransactionCauseDto::Typing, 8, 120)
            .unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, EditorAnimationKindDto::Insert);
        assert_eq!(events[0].text, "c");
        assert_eq!(events[0].range_start, 2);
        assert_eq!(events[1].kind, EditorAnimationKindDto::Cursor);
    }

    #[test]
    fn paste_does_not_emit_text_animation() {
        let api = WriterCoreApi::new("");
        let events = api
            .editor_animation_events(
                "a",
                "a long pasted text",
                1,
                19,
                EditorTransactionCauseDto::Paste,
                8,
                120,
            )
            .unwrap();

        // Only cursor event, no text animation
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].kind, EditorAnimationKindDto::Cursor);
    }

    #[test]
    fn load_does_not_emit_any_animation() {
        let api = WriterCoreApi::new("");
        let events = api
            .editor_animation_events("", "loaded", 0, 6, EditorTransactionCauseDto::Load, 8, 120)
            .unwrap();

        assert!(events.is_empty());
    }

    #[test]
    fn delete_single_char_emits_delete_event() {
        let api = WriterCoreApi::new("");
        // 删除 "c"：old="abc" new="ab"，光标从 3 移到 2
        let events = api
            .editor_animation_events("abc", "ab", 3, 2, EditorTransactionCauseDto::Delete, 8, 120)
            .unwrap();

        // Delete 事件 + Cursor 事件（光标从 3 移到 2）
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].kind, EditorAnimationKindDto::Delete);
        assert_eq!(events[0].text, "c");
        assert_eq!(events[1].kind, EditorAnimationKindDto::Cursor);
    }
}
