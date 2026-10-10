//! 将单一视觉过渡投影为 immutable RenderPlan。

use super::coordinator::LinuxEditorAnimationCoordinator;
use crate::sujian_editor_item::cursor_controller::CursorMoveSource;
use crate::sujian_editor_item::layout_snapshot::EditorLayoutSnapshot;
use crate::sujian_editor_item::render_plan::{
    CursorRenderState, CursorStyle, RenderPlan, SelectionPreeditPlan, SelectionPreeditStyle,
    VisualCaretGeometry,
};

impl LinuxEditorAnimationCoordinator {
    pub(crate) fn build_render_plan_full(
        &self,
        cursor_render_state: CursorRenderState,
        selection_preedit: SelectionPreeditPlan,
        cursor_style: CursorStyle,
        selection_preedit_style: SelectionPreeditStyle,
        coordinated_animation_enabled: bool,
        frame_now: std::time::Instant,
        canonical_snapshot: Option<&EditorLayoutSnapshot>,
    ) -> RenderPlan {
        let edit_sample = self.sample_edit_timeline(frame_now);
        let mut cursor_render_state = cursor_render_state;
        if coordinated_animation_enabled
            && cursor_render_state.visible
            && cursor_render_state.movement_source == Some(CursorMoveSource::TextTransaction)
        {
            if let Some(sample) = edit_sample.filter(|sample| {
                sample.transition_id == cursor_render_state.driver_transition_id
                    && sample.document_session == cursor_render_state.document_session
                    && Some(sample.target_revision) == cursor_render_state.driver_revision
            }) {
                if let Some(caret) = sample.caret_rect {
                    cursor_render_state.x = caret.x;
                    cursor_render_state.y = caret.top;
                    cursor_render_state.h = (caret.bottom - caret.top).max(0.0);
                    cursor_render_state.baseline_y = caret.baseline_y;
                    if let Some(from) = sample.caret_from {
                        cursor_render_state.path_start_x = from.x;
                        cursor_render_state.path_start_y = from.top;
                    }
                    if let Some(target) = sample.caret_target {
                        cursor_render_state.target_x = target.x;
                        cursor_render_state.target_y = target.top;
                    }
                }
            }
        }
        let caret = (
            cursor_render_state.x,
            cursor_render_state.y,
            cursor_render_state.h,
        );
        let visual_caret = if coordinated_animation_enabled && cursor_render_state.visible {
            cursor_render_state
                .movement_source
                .map(|movement_source| VisualCaretGeometry {
                    x: cursor_render_state.x,
                    y: cursor_render_state.y,
                    h: cursor_render_state.h,
                    movement_source,
                    transition_id: cursor_render_state.driver_transition_id,
                    layout_revision: cursor_render_state.driver_revision,
                    target_x: cursor_render_state.target_x,
                    target_y: cursor_render_state.target_y,
                    path_start_x: cursor_render_state.path_start_x,
                    path_start_y: cursor_render_state.path_start_y,
                    document_session: cursor_render_state.document_session,
                })
        } else {
            None
        };
        RenderPlan {
            ownership: self.ownership_plan(edit_sample, canonical_snapshot, visual_caret),
            selection_preedit,
            cursor: cursor_render_state,
            cursor_style,
            selection_preedit_style,
            drawn_caret_rect: Some(caret),
        }
    }
}
