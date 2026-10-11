//! Issue #901：视觉画布基础模块。
//!
//! `VisualCanvas` 统一接收一次 Core 编辑结果、新 Qt LayoutSnapshot（以
//! `LayoutRevision` 标识）和当前设置，产出一份整体绘制计划 `CanvasDrawPlan`。
//!
//! ## 三份真相
//!
//! - `CoreDocument`：最新正文和 selection（Core 持有）。
//! - `TargetLayout`：最新正文经过 Qt QTextLayout 后的目标几何（以
//!   `target_layout_revision` 标识）。
//! - `PresentedScene`：Qt `afterFrameEnd` 确认的最近一帧视觉图形。
//!
//! 新编辑的 source 永远来自最新 `PresentedScene`；尚没有新帧回执时，维持
//! 同一份 presented source，用 `RevisionChain` 精确映射 presented revision
//! 到最新 revision。
//!
//! ## 编辑动作语义
//!
//! 一个编辑动作只改变 canonical 的最终目标；**不**排"第几次按键的动画队列"。
//! 连续多笔编辑通过 `RevisionChain::compose` 累计 offset map，不积累动画历史。
//!
//! ## 模块结构
//!
//! - `CanvasController`：编排核心，持有各子模块，统一接收编辑结果。
//! - `SceneTransition`：从 `PresentedScene` + `TargetLayoutDescriptor` 生成 `CanvasDrawPlan`。
//! - `CanvasDrawPlan` / `DrawCommand`：不可变绘制计划。
//! - `Ownership`：像素互斥，解决绘制命令重叠。
//! - `TextFrontier`：二维文字前沿，跟踪吞字/吐字。
//! - `VisualCanvas`：`CanvasController` 的薄包装，保留旧接口兼容。

pub(crate) mod controller;
pub(crate) mod draw_plan;
pub(crate) mod frame_clock;
pub(crate) mod ownership;
pub(crate) mod presented_scene;
pub(crate) mod revision_chain;
pub(crate) mod scene_transition;
pub(crate) mod text_frontier;

pub(crate) use controller::CanvasController;
pub(crate) use draw_plan::{CanvasDrawPlan, DrawCommand, DrawCommandKind};
pub(crate) use frame_clock::{pose_from_presented, FrameClock, VisualPose};
pub(crate) use ownership::{Ownership, OwnershipRule};
pub(crate) use presented_scene::{
    CanvasAffine2, CanvasRect, CanvasVec2, PixelContribution, PresentedCaret, PresentedScroll,
    PresentedScene,
};
pub(crate) use revision_chain::RevisionChain;
pub(crate) use scene_transition::{
    SceneTransition, TargetCaret, TargetGlyphRun, TargetLayoutDescriptor, TargetScroll,
};
pub(crate) use text_frontier::TextFrontier;

use crate::sujian_editor_item::layout_revision::LayoutRevision;
use writer_core::editor::{EditorEditResult, EditorRevision};

/// 画布设置 —— 纯 Rust，不依赖 Qt 类型。
///
/// 控制整体动画行为。一次设置变更影响后续所有 `frame` 采样，
/// 不排"第几次设置的队列"。
#[derive(Clone, Debug)]
pub(crate) struct CanvasSettings {
    /// 文字动画是否启用。
    pub text_animation_enabled: bool,
    /// 光标动画是否启用。
    pub cursor_animation_enabled: bool,
    /// 整体响应时间（毫秒）—— "当前显示姿态追赶最新目标"的时间。
    pub response_duration_ms: f64,
}

impl Default for CanvasSettings {
    fn default() -> Self {
        Self {
            text_animation_enabled: true,
            cursor_animation_enabled: true,
            response_duration_ms: 120.0,
        }
    }
}

/// 视觉画布 —— `CanvasController` 的薄包装，保留旧接口兼容。
///
/// 内部委托给 `CanvasController`。`frame` 方法通过 `CanvasController` 产生
/// `CanvasDrawPlan`（经 `SceneTransition::build_draw_plan` 插值 +
/// `Ownership::resolve` 解决重叠）。
pub(crate) struct VisualCanvas {
    /// 内部控制器。
    controller: CanvasController,
    /// 最新目标 layout revision（`TargetLayout` 的标识）。
    target_layout_revision: Option<LayoutRevision>,
}

impl VisualCanvas {
    /// 创建视觉画布。
    ///
    /// `initial_revision` 是当前 Core 的 revision（也是初始 presented revision）。
    pub(crate) fn new(settings: CanvasSettings, initial_revision: EditorRevision, now_ms: f64) -> Self {
        Self {
            controller: CanvasController::new(settings, initial_revision, now_ms),
            target_layout_revision: None,
        }
    }

    /// 接收一次 Core 编辑结果。
    ///
    /// 一个编辑动作只改变 canonical 的最终目标。如果 `offset_map` 存在，
    /// 用 `RevisionChain::compose` 精确组合（保留中间 unchanged islands）；
    /// 纯选区操作（`offset_map` 为 `None`）只推进 revision，不改变映射。
    pub(crate) fn apply_edit_result(&mut self, result: &EditorEditResult) {
        if let Some(ref offset_map) = result.offset_map {
            self.controller
                .revision_chain_mut()
                .compose(offset_map, result.new_revision);
        } else {
            // 纯选区/光标操作：正文不变，只推进 latest revision。
            self.controller
                .revision_chain_mut()
                .advance_revision(result.new_revision);
        }
    }

    /// 接收新 Qt LayoutSnapshot 的 revision（`TargetLayout` 标识）。
    ///
    /// 实际的 `EditorLayoutSnapshot` 几何由外部持有（避免在纯 Rust 模块中
    /// 引入 Qt `QImage` 类型）。本方法只记录目标 layout 版本。
    pub(crate) fn apply_layout_revision(&mut self, revision: LayoutRevision) {
        self.target_layout_revision = Some(revision);
    }

    /// 接收完整的目标布局描述（含几何）。
    ///
    /// 这是 `CanvasController::on_edit_result` 的补充：只更新目标布局，
    /// 不触发编辑结果处理。用于排版完成后更新目标几何。
    pub(crate) fn apply_target_layout(&mut self, target: TargetLayoutDescriptor, now_ms: f64) {
        self.target_layout_revision = Some(target.layout_revision);
        self.controller.set_target_layout(target, now_ms);
    }

    /// 更新设置。
    pub(crate) fn apply_settings(&mut self, settings: CanvasSettings) {
        self.controller.apply_settings(settings);
    }

    /// 提交已呈现画面（Qt `afterFrameEnd` 回执）。
    ///
    /// 更新 `PresentedScene` 和 `RevisionChain`：如果该帧的 `source_revision`
    /// 等于当前 `latest_revision`，画面已追上 Core，累计映射归零。
    pub(crate) fn commit_presented_scene(&mut self, scene: PresentedScene, now_ms: f64) {
        self.controller.on_frame_committed(scene, now_ms);
    }

    /// 产生绘制计划。
    ///
    /// 一次 `frame` 只产生一份整体 `CanvasDrawPlan`。通过 `CanvasController`
    /// 经 `SceneTransition::build_draw_plan` 插值 + `Ownership::resolve` 解决重叠。
    /// 一个编辑动作只改变 canonical 的最终目标，不排"第几次按键的动画队列"。
    pub(crate) fn frame(&self, now_ms: f64) -> CanvasDrawPlan {
        self.controller.frame(now_ms)
    }

    /// 返回当前已提交画面（仅供调试 / 测试）。
    pub(crate) fn presented_scene(&self) -> Option<&PresentedScene> {
        self.controller.presented_scene()
    }

    /// 返回 revision chain（仅供调试 / 测试）。
    pub(crate) fn revision_chain(&self) -> &RevisionChain {
        self.controller.revision_chain()
    }

    /// 返回当前设置。
    pub(crate) fn settings(&self) -> &CanvasSettings {
        self.controller.settings()
    }

    /// 返回目标 layout revision。
    pub(crate) fn target_layout_revision(&self) -> Option<LayoutRevision> {
        self.target_layout_revision
    }

    /// 返回内部控制器（仅供调试 / 测试）。
    pub(crate) fn controller(&self) -> &CanvasController {
        &self.controller
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sujian_editor_item::layout_revision::LayoutRevision;
    use writer_core::editor::{
        EditorContentDelta, EditorCursor, EditorEditResult, EditorOperationKind,
        EditorRevision, EditorSelection, EditorTransactionCause, OffsetMap,
    };
    use writer_core::editor::strong_types::{Utf8ByteOffset, Utf8ByteRange};

    fn dummy_edit_result(
        base_rev: u64,
        new_rev: u64,
        offset_map: Option<OffsetMap>,
    ) -> EditorEditResult {
        let cursor = EditorCursor::from_offset(Utf8ByteOffset::unchecked(0));
        EditorEditResult {
            transaction_id: 1,
            base_revision: EditorRevision::new(base_rev),
            new_revision: EditorRevision::new(new_rev),
            display_patches: vec![],
            old_selection: EditorSelection {
                anchor: cursor,
                head: cursor,
            },
            new_selection: EditorSelection {
                anchor: cursor,
                head: cursor,
            },
            cause: EditorTransactionCause::Typing,
            operation_kind: EditorOperationKind::Insert,
            offset_map,
            content_delta: EditorContentDelta::default(),
        }
    }

    #[test]
    fn canvas_creation() {
        let settings = CanvasSettings::default();
        let canvas = VisualCanvas::new(settings, EditorRevision::initial(), 0.0);
        assert!(canvas.presented_scene().is_none());
        assert!(!canvas.revision_chain().has_pending_edits());
        assert!(canvas.target_layout_revision().is_none());
    }

    #[test]
    fn canvas_frame_empty_without_presented_scene() {
        let canvas = VisualCanvas::new(
            CanvasSettings::default(),
            EditorRevision::initial(),
            0.0,
        );
        let plan = canvas.frame(16.0);
        assert!(plan.empty);
    }

    #[test]
    fn canvas_apply_layout_revision() {
        let mut canvas = VisualCanvas::new(
            CanvasSettings::default(),
            EditorRevision::initial(),
            0.0,
        );
        let rev = LayoutRevision::next();
        canvas.apply_layout_revision(rev);
        assert_eq!(canvas.target_layout_revision(), Some(rev));
    }

    #[test]
    fn canvas_apply_settings_updates_response_time() {
        let mut canvas = VisualCanvas::new(
            CanvasSettings::default(),
            EditorRevision::initial(),
            0.0,
        );
        let mut settings = CanvasSettings::default();
        settings.response_duration_ms = 250.0;
        canvas.apply_settings(settings);
        assert!((canvas.settings().response_duration_ms - 250.0).abs() < 1e-9);
    }

    #[test]
    fn canvas_apply_edit_result_with_offset_map() {
        let mut canvas = VisualCanvas::new(
            CanvasSettings::default(),
            EditorRevision::new(0),
            0.0,
        );
        // 有 offset_map 的编辑
        let map = OffsetMap {
            entries: vec![writer_core::editor::OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::unchecked(0),
                new_byte_offset: Utf8ByteOffset::unchecked(0),
                length: 5,
                kind: writer_core::editor::OffsetMapKind::Identity,
            }],
        };
        let result = dummy_edit_result(0, 1, Some(map));
        canvas.apply_edit_result(&result);
        assert!(canvas.revision_chain().has_pending_edits());
    }

    #[test]
    fn canvas_commit_presented_scene_clears_pending() {
        let mut canvas = VisualCanvas::new(
            CanvasSettings::default(),
            EditorRevision::new(0),
            0.0,
        );

        // 编辑推进 revision
        let map = OffsetMap {
            entries: vec![writer_core::editor::OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::unchecked(0),
                new_byte_offset: Utf8ByteOffset::unchecked(0),
                length: 5,
                kind: writer_core::editor::OffsetMapKind::Identity,
            }],
        };
        let result = dummy_edit_result(0, 1, Some(map));
        canvas.apply_edit_result(&result);
        assert!(canvas.revision_chain().has_pending_edits());

        // 提交追上 Core 的帧
        let scene = PresentedScene {
            source_revision: EditorRevision::new(1),
            document_session: 1,
            frame_id: 1,
            geometry_id: 1,
            clip: CanvasRect::new(0.0, 0.0, 800.0, 600.0),
            position: CanvasVec2::new(0.0, 0.0),
            opacity: 1.0,
            transform: CanvasAffine2::identity(),
            pixel_contributions: vec![PixelContribution {
                texture_id: 1,
                source_uv: CanvasRect::new(0.0, 0.0, 100.0, 20.0),
                document_rect: CanvasRect::new(0.0, 0.0, 100.0, 20.0),
                visible_clip: CanvasRect::new(0.0, 0.0, 100.0, 20.0),
                opacity: 1.0,
                byte_range: (0, 100),
            }],
            caret: None,
            scroll: PresentedScroll::default(),
        };
        canvas.commit_presented_scene(scene, 16.0);
        assert!(!canvas.revision_chain().has_pending_edits());

        // frame 现在应该有内容
        let plan = canvas.frame(32.0);
        assert!(!plan.empty);
    }

    #[test]
    fn canvas_pure_selection_preserves_accumulated_map() {
        let mut canvas = VisualCanvas::new(
            CanvasSettings::default(),
            EditorRevision::new(0),
            0.0,
        );

        // 第一笔：有 offset_map 的编辑（插入字符）
        let map = OffsetMap {
            entries: vec![writer_core::editor::OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::unchecked(0),
                new_byte_offset: Utf8ByteOffset::unchecked(0),
                length: 3,
                kind: writer_core::editor::OffsetMapKind::Identity,
            }],
        };
        canvas.apply_edit_result(&dummy_edit_result(0, 1, Some(map)));
        // offset 1 应该映射到 1
        assert_eq!(canvas.revision_chain().map_presented_to_latest(1), Some(1));

        // 第二笔：纯选区操作（offset_map 为 None）—— 正文不变
        canvas.apply_edit_result(&dummy_edit_result(1, 2, None));
        // 累计映射应该保留：offset 1 仍然映射到 1
        assert_eq!(
            canvas.revision_chain().map_presented_to_latest(1),
            Some(1),
            "pure selection must preserve accumulated offset map"
        );
        // latest revision 应该前进到 2
        assert_eq!(
            canvas.revision_chain().latest_revision(),
            EditorRevision::new(2)
        );
        // presented revision 不变（还是 0）
        assert_eq!(
            canvas.revision_chain().presented_revision(),
            EditorRevision::new(0)
        );
    }
}
