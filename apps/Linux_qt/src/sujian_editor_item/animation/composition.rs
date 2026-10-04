//! Issue #826: IME preedit 独立临时显示层。
//!
//! composition **update**（预输入未上屏）：只更新 preedit 临时显示，不创建
//! InsertReveal / DeleteConceal、不进 EditFrontier、不 carry/rebase preedit glyph。
//! 正文此时还没变，Core 提交的就是最新真实内容。
//!
//! composition **commit / cancel**（上屏或取消）：
//! 1. 去掉 preedit 临时层；
//! 2. 只读 Core `EditorEditResult.display_patches`；
//! 3. 用 committed old canonical → committed new canonical 创建**一次** EditFrontier；
//! 4. 纯 Insert 就是 Insert；真正同时有删除和插入才是 Replace。

use std::time::Instant;

use crate::sujian_editor_item::animation::coordinator::EditFrontierRequest;
use crate::sujian_editor_item::animation::LinuxEditorAnimationCoordinator;
use crate::sujian_editor_item::edit_motion::{CursorRect, PreparedEditMotion};
use crate::sujian_editor_item::editor_animation_debug_log;
use crate::sujian_editor_item::layout_snapshot::EditorLayoutSnapshot;
use writer_core::editor::OffsetMap;

/// composition commit 时构造遮罩前沿所需的全部事实。
///
/// `motion` 里的 `inserted_ranges` / `deleted_ranges` 就是 Core
/// `display_patches` 的派生产物（`edit_motion::ranges_from_display_patches`），
/// 正文动画分类只认这一份，不另建 composition 专属分类。
pub(crate) struct CompositionCommitFrontierInput {
    pub motion: PreparedEditMotion,
    pub old_snapshot: EditorLayoutSnapshot,
    pub new_snapshot: EditorLayoutSnapshot,
    pub now: Instant,
}

impl LinuxEditorAnimationCoordinator {
    /// Issue #826: composition **update** —— 只更新 preedit 临时层。
    ///
    /// 正文还没提交变更，本层不产生任何正文动画；先把上一轮正文前沿收成
    /// canonical 终态，避免 preedit 期间挂着遮罩。
    pub(crate) fn handle_composition_update(&mut self) {
        self.finish_edit_frontier_to_canonical();
        editor_animation_debug_log("composition_update: preedit 临时层更新，正文无动画");
    }

    /// Issue #826: composition **commit / cancel** —— 用 display_patches 造一次前沿。
    ///
    /// 分类只看 patch 事实：纯 Insert 就是 Insert；同时有删除和插入才是 Replace。
    pub(crate) fn handle_composition_commit_or_cancel(
        &mut self,
        input: CompositionCommitFrontierInput,
    ) {
        let CompositionCommitFrontierInput {
            motion,
            old_snapshot,
            new_snapshot,
            now,
        } = input;

        // 1. 去掉 preedit 临时层：preedit glyph 不进前沿、不 carry/rebase。
        //    收口后这次 commit 一定是**新**的前沿，不会继承 preedit 期间的遮罩。
        self.finish_edit_frontier_to_canonical();

        // 2 + 3 + 4. 只用 Core display_patches 派生的 inserted / deleted 事实，
        //    用 committed old → committed new canonical 创建唯一一次前沿。
        //    纯 Insert 就是 Insert；同时有删除和插入才是 Replace。
        self.begin_or_extend_edit_frontier(EditFrontierRequest {
            kind: motion.kind,
            base_snapshot: old_snapshot,
            target_snapshot: new_snapshot,
            deleted_ranges: motion.deleted_ranges.clone(),
            inserted_ranges: motion.inserted_ranges.clone(),
            start_frontier: caret_or_zero(motion.old_cursor_rect),
            target_frontier: caret_or_zero(motion.new_cursor_rect),
            offset_map: OffsetMap::build(&motion.old_text, &motion.new_text),
            now,
        });
    }
}

fn caret_or_zero(rect: Option<CursorRect>) -> CursorRect {
    rect.unwrap_or(CursorRect {
        x: 0.0,
        top: 0.0,
        bottom: 0.0,
        baseline_y: 0.0,
    })
}
