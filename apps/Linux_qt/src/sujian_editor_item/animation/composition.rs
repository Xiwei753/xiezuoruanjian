//! Issue #826: IME preedit 独立临时显示层。
//!
//! composition **update**（预输入未上屏）：只更新 preedit 临时显示，不创建正文过渡，
//! 也不 carry/rebase preedit glyph。正文此时还没变，Core 提交的就是最新真实内容。
//!
//! composition **commit / cancel**（上屏或取消）：
//! 1. 去掉 preedit 临时层；
//! 2. 只读 Core `EditorEditResult.display_patches`；
//! 3. 从最后已绘制的 VisualFrame 到 committed new canonical 创建一份过渡；
//! 4. 过渡直接消费 Core 提交的 offset map，不保留 composition 专属状态机。

use std::time::Instant;

use crate::sujian_editor_item::animation::{LinuxEditorAnimationCoordinator, VisualEditRequest};
use crate::sujian_editor_item::edit_motion::PreparedEditMotion;
use crate::sujian_editor_item::editor_animation_debug_log;
use crate::sujian_editor_item::layout_snapshot::EditorLayoutSnapshot;
use writer_core::editor::OffsetMap;

/// composition commit 时构造视觉过渡所需的 canonical 快照和精确 offset map。
///
/// `motion` 里的 `inserted_ranges` / `deleted_ranges` 就是 Core
/// `display_patches` 的派生产物（`edit_motion::ranges_from_display_patches`），
/// 正文动画分类只认这一份，不另建 composition 专属分类。
pub(crate) struct CompositionVisualEditInput {
    pub motion: PreparedEditMotion,
    pub old_snapshot: EditorLayoutSnapshot,
    pub new_snapshot: EditorLayoutSnapshot,
    pub now: Instant,
}

impl LinuxEditorAnimationCoordinator {
    /// composition **update** 只更新 preedit 临时层，不影响正文过渡。
    pub(crate) fn handle_composition_update(&mut self) {
        editor_animation_debug_log("composition_update: preedit 临时层更新，正文无动画");
    }

    /// composition **commit / cancel** 用提交后的 canonical layout 替换当前过渡。
    ///
    /// 分类只看 patch 事实：纯 Insert 就是 Insert；同时有删除和插入才是 Replace。
    pub(crate) fn handle_composition_commit_or_cancel(
        &mut self,
        input: CompositionVisualEditInput,
    ) {
        let CompositionVisualEditInput {
            motion,
            old_snapshot,
            new_snapshot,
            now,
        } = input;

        // preedit glyph 不进入正文视觉状态。commit 的新 transition 从最后一帧实际
        // 绘制结果重定向到新的 canonical layout，多个快速编辑也不会积累动画历史。
        self.begin_visual_edit(VisualEditRequest {
            base_snapshot: old_snapshot,
            target_snapshot: new_snapshot,
            offset_map: motion
                .offset_map
                .clone()
                .unwrap_or_else(|| OffsetMap::build(&motion.old_text, &motion.new_text)),
            deleted_range_edges: motion.deleted_range_edges,
            caret_motion: motion.old_cursor_rect.zip(motion.new_cursor_rect),
            animate: true,
            now,
        });
    }
}
