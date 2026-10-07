//! 将单一视觉过渡投影为 immutable RenderPlan。

use super::coordinator::LinuxEditorAnimationCoordinator;
use crate::sujian_editor_item::layout_snapshot::EditorLayoutSnapshot;
use crate::sujian_editor_item::render_plan::{
    CursorRenderState, CursorStyle, RenderPlan, SelectionPreeditPlan, SelectionPreeditStyle,
};

impl LinuxEditorAnimationCoordinator {
    pub(crate) fn build_render_plan_full(
        &self,
        cursor_render_state: CursorRenderState,
        selection_preedit: SelectionPreeditPlan,
        cursor_style: CursorStyle,
        selection_preedit_style: SelectionPreeditStyle,
        frame_now: std::time::Instant,
        canonical_snapshot: Option<&EditorLayoutSnapshot>,
    ) -> RenderPlan {
        let caret = (
            cursor_render_state.x,
            cursor_render_state.y,
            cursor_render_state.h,
        );
        RenderPlan {
            ownership: self.ownership_plan(frame_now, canonical_snapshot),
            selection_preedit,
            cursor: cursor_render_state,
            cursor_style,
            selection_preedit_style,
            drawn_caret_rect: Some(caret),
        }
    }
}
