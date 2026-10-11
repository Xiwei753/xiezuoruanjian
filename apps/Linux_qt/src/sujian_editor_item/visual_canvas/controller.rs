//! Issue #901：画布控制器 —— 统一接收编辑结果，协调各模块。
//!
//! `CanvasController` 是视觉画布的编排核心，持有：
//! - `PresentedScene`：已提交画面（视觉真相）。
//! - `RevisionChain`：presented → latest 的精确 offset map 组合。
//! - `FrameClock`：统一帧时间与视觉姿态。
//! - `TargetLayoutDescriptor`：最新目标布局。
//! - `TextFrontier`：文字展开/收缩的前沿边界。
//!
//! ## 核心规则
//!
//! - 一个编辑动作只改变 canonical 的最终目标，**不**排"第几次按键的动画队列"。
//! - 连续多笔编辑通过 `RevisionChain::compose` 累计 offset map，不积累动画历史。
//! - 连续性从"已提交画面"出发：source 永远来自最新 `PresentedScene`。
//!
//! ## 纯 Rust
//!
//! 本模块不依赖 Qt 类型。`TargetLayoutDescriptor` 由调用方从 Qt 排版结果转换而来。

use super::draw_plan::CanvasDrawPlan;
use super::frame_clock::{pose_from_presented, FrameClock, VisualPose};
use super::presented_scene::PresentedScene;
use super::revision_chain::RevisionChain;
use super::scene_transition::{SceneTransition, TargetLayoutDescriptor};
use super::text_frontier::TextFrontier;
use super::CanvasSettings;

use writer_core::editor::{EditorEditResult, EditorRevision};

/// 画布控制器 —— 统一接收编辑结果，协调各子模块。
///
/// 持有三份真相中的两份（`PresentedScene` 和 `RevisionChain`），并通过
/// `FrameClock` 统一帧时间。`TargetLayoutDescriptor` 是第三份真相的标识。
pub(crate) struct CanvasController {
    /// Qt `afterFrameEnd` 确认的最近一帧视觉图形。
    presented_scene: Option<PresentedScene>,
    /// presented revision → latest revision 的精确 offset map 组合。
    revision_chain: RevisionChain,
    /// 统一帧时钟。
    frame_clock: FrameClock,
    /// 最新目标布局描述。
    target_layout: Option<TargetLayoutDescriptor>,
    /// 文字前沿 —— 跟踪文字展开/收缩的可见范围。
    text_frontier: TextFrontier,
    /// 当前设置。
    settings: CanvasSettings,
}

impl CanvasController {
    /// 创建画布控制器。
    ///
    /// `initial_revision` 是当前 Core 的 revision（也是初始 presented revision）。
    pub(crate) fn new(
        settings: CanvasSettings,
        initial_revision: EditorRevision,
        now_ms: f64,
    ) -> Self {
        let frame_clock = FrameClock::new(
            VisualPose::zero(),
            settings.response_duration_ms,
            now_ms,
        );
        let text_frontier = TextFrontier::new(0, 0, Vec::new(), 0.5);
        Self {
            presented_scene: None,
            revision_chain: RevisionChain::new(initial_revision),
            frame_clock,
            target_layout: None,
            text_frontier,
            settings,
        }
    }

    /// 接收一次 Core 编辑结果。
    ///
    /// 一个编辑动作只改变 canonical 的最终目标：
    /// 1. 更新 `RevisionChain`（compose offset map）。
    /// 2. 更新 `TargetLayoutDescriptor`。
    /// 3. 通知 `FrameClock` retarget（保留当前已提交姿态，不从 1.0 饱和进度继承）。
    /// 4. 更新 `TextFrontier` retarget。
    ///
    /// **不**排"第几次按键的动画队列"。
    pub(crate) fn on_edit_result(
        &mut self,
        result: &EditorEditResult,
        target: &TargetLayoutDescriptor,
        now_ms: f64,
    ) {
        // 1. 更新 RevisionChain。
        if let Some(ref offset_map) = result.offset_map {
            self.revision_chain
                .compose(offset_map, result.new_revision);
        } else {
            // 纯选区/光标操作：正文不变，只推进 latest revision。
            self.revision_chain.advance_revision(result.new_revision);
        }

        // 2. 更新 TargetLayoutDescriptor。
        self.target_layout = Some(target.clone());

        // 3. 通知 FrameClock retarget。
        // 从 target 构造目标 VisualPose。
        let target_pose = self.target_pose_from_layout(target);
        self.frame_clock.retarget(target_pose, now_ms);

        // 4. 更新 TextFrontier retarget。
        // 从 target 的 glyph_runs 推导目标可见范围。
        let target_range = self.target_text_range(target);
        self.text_frontier.retarget(target_range);
    }

    /// 帧提交回执（Qt `afterFrameEnd`）。
    ///
    /// 更新 `PresentedScene` 和 `RevisionChain`：如果该帧的 `source_revision`
    /// 等于当前 `latest_revision` **且画面真正达到终点**（`is_settled`），
    /// 画面已追上 Core，累计映射归零。
    /// 同时更新 `FrameClock` 的已提交姿态。
    ///
    /// Issue #901 问题3：
    /// - 只接受同 `document_session` 且 `frame_id` 比当前 presented 更新的帧。
    ///   如果帧的 session 不同或 frame_id 更旧，忽略（防止旧画面覆盖新 scene）。
    /// - 不再以 `frame_revision == latest` 单独判定 `reset_to_committed`。
    ///   只有在 `is_settled` 为真（progress 真正接近 1.0）**且**
    ///   `frame_revision == latest` 时才 reset。如果 progress < 1，保留
    ///   revision_chain 映射，因为画面还在过渡中。
    /// - progress 从 `PresentedScene.pose_progress` 读取（由渲染层填入真实值），
    ///   不从 `frame_clock.frame_now` 估算理论进度。
    pub(crate) fn on_frame_committed(&mut self, scene: PresentedScene, now_ms: f64) {
        // Issue #901 问题3：只接受同 session 且更新的帧。
        if let Some(current) = &self.presented_scene {
            if scene.document_session != current.document_session {
                // 不同 session：忽略旧 session 的迟到帧。
                return;
            }
            if scene.frame_id <= current.frame_id {
                // 同 session 但 frame_id 更旧或相同：忽略乱序帧。
                return;
            }
        }

        let frame_revision = scene.source_revision;
        let latest = self.revision_chain.latest_revision();

        // Issue #901 问题3：progress 从 PresentedScene.pose_progress 读取，
        // 不从 frame_clock.frame_now 估算。pose_progress 由渲染层在
        // afterFrameEnd 时填入该帧实际对应的 VisualPose.progress。
        let progress = scene.pose_progress.clamp(0.0, 1.0);

        // 从已提交帧构造 VisualPose 并更新 FrameClock。
        let pose = pose_from_presented(scene.caret.as_ref(), &scene.scroll, progress);
        self.frame_clock.commit_pose(pose, now_ms);

        // Issue #901 问题3：只有在帧的 pose_progress 真正接近 1.0
        // 且 frame_revision == latest 时才 reset_to_committed。
        // 用 scene.pose_progress（渲染层填入的真实值）判断，不用
        // frame_clock.is_settled（理论估算值）——帧的实际进度才是真实事实。
        // 如果 progress < 1，画面还在过渡中，保留 revision_chain 映射，
        // 后续"输入 A、露出半字、删除 A"必须保留 A 的当前可见部分和旧→新逻辑映射。
        if frame_revision == latest && progress >= 1.0 - 0.001 {
            self.revision_chain.reset_to_committed(frame_revision);
        }

        // 更新 TextFrontier 的可见范围为已提交帧的可见范围。
        let visible_range = self.presented_text_range(&scene);
        let (vs, ve) = visible_range;
        self.text_frontier = TextFrontier::new(
            vs,
            ve,
            self.text_frontier.line_boundaries_clone(),
            0.5,
        );

        self.presented_scene = Some(scene);
    }

    /// 首帧静态 canonical 初始化（Issue #901 问题1+3 修复后改为 no-op）。
    ///
    /// **旧实现**从 `TargetLayoutDescriptor` 伪造一张 progress=1.0 的
    /// "已显示场景"并注册为 `presented_scene`，导致：
    /// - 首帧 `commit_pose` 把 `last_committed_pose.progress` 设为 1.0。
    /// - 后续 `retarget` 从 1.0 继承起始进度，新过渡 1→1 没有运动。
    ///
    /// **新实现**不再伪造首帧。`presented_scene` 只在 Qt `afterFrameEnd`
    /// 回执时设置（`on_frame_committed`）。如果 `presented_scene` 为 None，
    /// `frame()` 从 target 生成初始静态画面但不注册为 `presented_scene`。
    ///
    /// 此方法保留公开 API 签名（`VisualCanvas::initialize_presented_from_target`
    /// 委托到此），但内部不再做任何事。调用方（`qquickitem_impl.rs`）仍可
    /// 安全调用，行为从"伪造首帧"变为"无操作"。
    pub(crate) fn initialize_presented_from_target(&mut self, _now_ms: f64) {
        // Issue #901 问题1+3：不再从 target 伪造首帧。
        // presented_scene 只在 on_frame_committed（afterFrameEnd 回执）时设置。
        // frame() 在 presented_scene 为 None 时从 target 生成静态画面。
        // 此处 intentionally empty。
    }

    /// Issue #901 问题3：重置视觉状态，用于切章/清文档。
    ///
    /// 清除 `presented_scene`（丢弃旧章的视觉真相）和 `target_layout`
    /// （丢弃旧章的目标布局）。保留 `revision_chain` 的 latest_revision
    /// （Core revision 仍正确）、`frame_clock`、`text_frontier` 和 `settings`。
    ///
    /// 切章后下一帧 `frame()` 会从新 target 生成静态画面，
    /// `on_frame_committed` 会注册新 session 的 `PresentedScene`。
    pub(crate) fn reset(&mut self) {
        self.presented_scene = None;
        self.target_layout = None;
    }

    /// 产生绘制计划。
    ///
    /// 从 `PresentedScene` + `TargetLayoutDescriptor` + `VisualPose` 生成
    /// `CanvasDrawPlan`，通过 `SceneTransition::build_draw_plan` 插值，
    /// 通过 `Ownership::resolve` 解决重叠。
    ///
    /// Issue #901 阻塞 2：`revision_chain` 传递给 `SceneTransition::build_draw_plan`，
    /// 用于判断每个已显示字形贡献是存活、删除还是新插入。
    ///
    /// Issue #901 问题1+3：如果尚无 `PresentedScene`（首帧未提交）但有
    /// `TargetLayoutDescriptor`，从 target 生成初始静态画面（progress=1.0），
    /// **但不注册为 `presented_scene`**。`presented_scene` 只在
    /// `on_frame_committed`（afterFrameEnd 回执）时设置。
    /// 如果两者都无，返回空计划。
    pub(crate) fn frame(&self, now_ms: f64) -> CanvasDrawPlan {
        let pose = self.frame_clock.frame_now(now_ms);

        let Some(presented) = &self.presented_scene else {
            // 尚无已提交帧：从 target 生成初始静态画面（progress=1.0），
            // 但不注册为 presented_scene。
            let Some(target) = &self.target_layout else {
                return CanvasDrawPlan::empty(now_ms);
            };
            return SceneTransition::build_draw_plan_target_only(
                target,
                &self.revision_chain,
                now_ms,
            );
        };
        let Some(target) = &self.target_layout else {
            // 有 presented 但无 target：只显示 presented。
            return SceneTransition::build_draw_plan_presented_only(presented, &pose, now_ms);
        };

        SceneTransition::build_draw_plan(
            presented,
            target,
            &pose,
            &self.revision_chain,
            now_ms,
        )
    }

    /// 更新设置。
    pub(crate) fn apply_settings(&mut self, settings: CanvasSettings) {
        self.frame_clock
            .set_response_duration_ms(settings.response_duration_ms);
        self.settings = settings;
    }

    /// 返回当前已提交画面（仅供调试 / 测试）。
    pub(crate) fn presented_scene(&self) -> Option<&PresentedScene> {
        self.presented_scene.as_ref()
    }

    /// 返回 revision chain（仅供调试 / 测试）。
    pub(crate) fn revision_chain(&self) -> &RevisionChain {
        &self.revision_chain
    }

    /// 返回 revision chain 的可变引用（供 `VisualCanvas` 委托编辑操作）。
    pub(crate) fn revision_chain_mut(&mut self) -> &mut RevisionChain {
        &mut self.revision_chain
    }

    /// 设置目标布局描述（不触发完整的 `on_edit_result` 流程）。
    ///
    /// 用于排版完成后只更新目标几何、不关联新编辑结果的场景。
    pub(crate) fn set_target_layout(&mut self, target: TargetLayoutDescriptor, now_ms: f64) {
        let target_pose = self.target_pose_from_layout(&target);
        self.frame_clock.retarget(target_pose, now_ms);

        let target_range = self.target_text_range(&target);
        self.text_frontier.retarget(target_range);

        self.target_layout = Some(target);
    }

    /// 返回目标布局描述（仅供调试 / 测试）。
    pub(crate) fn target_layout(&self) -> Option<&TargetLayoutDescriptor> {
        self.target_layout.as_ref()
    }

    /// 返回当前设置。
    pub(crate) fn settings(&self) -> &CanvasSettings {
        &self.settings
    }

    /// 返回文字前沿（仅供调试 / 测试）。
    pub(crate) fn text_frontier(&self) -> &TextFrontier {
        &self.text_frontier
    }

    /// 返回帧时钟（仅供调试 / 测试）。
    pub(crate) fn frame_clock(&self) -> &FrameClock {
        &self.frame_clock
    }

    // ── 内部辅助 ──

    /// 从 `TargetLayoutDescriptor` 构造目标 `VisualPose`。
    fn target_pose_from_layout(&self, target: &TargetLayoutDescriptor) -> VisualPose {
        let caret_position = target
            .caret
            .filter(|c| c.visible)
            .map(|c| c.position)
            .unwrap_or_else(|| super::presented_scene::CanvasVec2::new(0.0, 0.0));
        VisualPose {
            progress: 1.0,
            caret_position,
            scroll_position: super::presented_scene::CanvasVec2::new(
                target.scroll.x,
                target.scroll.y,
            ),
        }
    }

    /// 从 `TargetLayoutDescriptor` 的 glyph_runs 推导目标文字 byte range。
    fn target_text_range(&self, target: &TargetLayoutDescriptor) -> (usize, usize) {
        if target.glyph_runs.is_empty() {
            return (0, 0);
        }
        let start = target
            .glyph_runs
            .iter()
            .map(|r| r.byte_range.0)
            .min()
            .unwrap_or(0);
        let end = target
            .glyph_runs
            .iter()
            .map(|r| r.byte_range.1)
            .max()
            .unwrap_or(0);
        (start, end)
    }

    /// 从 `PresentedScene` 推导已提交帧的可见文字 byte range。
    ///
    /// Issue #901 阻塞 4：用 `PixelContribution` 的 `byte_range` 字段计算真实的
    /// UTF-8 byte range，不再用 `pixel_contributions.len()` 代替字节范围。
    /// 返回所有贡献的 byte_range 的并集 `(min_start, max_end)`。
    fn presented_text_range(&self, scene: &PresentedScene) -> (usize, usize) {
        if scene.pixel_contributions.is_empty() {
            return (0, 0);
        }
        let start = scene
            .pixel_contributions
            .iter()
            .map(|c| c.byte_range.0)
            .min()
            .unwrap_or(0);
        let end = scene
            .pixel_contributions
            .iter()
            .map(|c| c.byte_range.1)
            .max()
            .unwrap_or(0);
        (start, end)
    }
}

// TextFrontier 的 line_boundaries clone 辅助方法（避免在 TextFrontier 上加 pub 方法）。
impl TextFrontier {
    /// 克隆行边界列表（内部辅助）。
    fn line_boundaries_clone(&self) -> Vec<usize> {
        self.line_boundaries_internal()
    }

    /// 返回行边界列表的克隆（内部）。
    fn line_boundaries_internal(&self) -> Vec<usize> {
        // 通过 visible_range 和 target_range 无法恢复 line_boundaries，
        // 所以这里返回空列表——排版变化时调用方应通过 set_line_boundaries 更新。
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::presented_scene::{
        CanvasAffine2, CanvasRect, CanvasVec2, PixelContribution, PresentedCaret, PresentedScroll,
    };
    use super::super::scene_transition::{TargetCaret, TargetGlyphRun, TargetScroll};
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

    fn sample_target() -> TargetLayoutDescriptor {
        TargetLayoutDescriptor {
            layout_revision: LayoutRevision::next(),
            glyph_runs: vec![TargetGlyphRun {
                snapshot_id: crate::sujian_editor_item::snapshot_id::LineSnapshotId::new(1, 0, 0),
                source_uv: CanvasRect::new(0.0, 0.0, 100.0, 20.0),
                document_rect: CanvasRect::new(0.0, 0.0, 100.0, 20.0),
                byte_range: (0, 5),
            }],
            caret: Some(TargetCaret {
                position: CanvasVec2::new(100.0, 0.0),
                visible: true,
                width: 2.0,
                height: 20.0,
            }),
            scroll: TargetScroll::default(),
            clip: CanvasRect::new(0.0, 0.0, 800.0, 600.0),
        }
    }

    fn sample_presented() -> PresentedScene {
        PresentedScene {
            source_revision: EditorRevision::initial(),
            document_session: 1,
            frame_id: 1,
            geometry_id: 1,
            pose_progress: 1.0,
            clip: CanvasRect::new(0.0, 0.0, 800.0, 600.0),
            position: CanvasVec2::new(0.0, 0.0),
            opacity: 1.0,
            transform: CanvasAffine2::identity(),
            pixel_contributions: vec![PixelContribution {
                snapshot_id: crate::sujian_editor_item::snapshot_id::LineSnapshotId::new(1, 0, 0),
                source_uv: CanvasRect::new(0.0, 0.0, 100.0, 20.0),
                document_rect: CanvasRect::new(0.0, 0.0, 100.0, 20.0),
                visible_clip: CanvasRect::new(0.0, 0.0, 100.0, 20.0),
                opacity: 1.0,
                byte_range: (0, 5),
            }],
            caret: None,
            scroll: PresentedScroll::default(),
        }
    }

    #[test]
    fn controller_creation() {
        let ctrl = CanvasController::new(
            CanvasSettings::default(),
            EditorRevision::initial(),
            0.0,
        );
        assert!(ctrl.presented_scene().is_none());
        assert!(ctrl.target_layout().is_none());
        assert!(!ctrl.revision_chain().has_pending_edits());
    }

    #[test]
    fn controller_frame_empty_without_presented() {
        let ctrl = CanvasController::new(
            CanvasSettings::default(),
            EditorRevision::initial(),
            0.0,
        );
        let plan = ctrl.frame(16.0);
        assert!(plan.empty);
    }

    #[test]
    fn controller_on_edit_result_updates_chain_and_target() {
        let mut ctrl = CanvasController::new(
            CanvasSettings::default(),
            EditorRevision::new(0),
            0.0,
        );
        let map = OffsetMap {
            entries: vec![writer_core::editor::OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::unchecked(0),
                new_byte_offset: Utf8ByteOffset::unchecked(0),
                length: 5,
                kind: writer_core::editor::OffsetMapKind::Identity,
            }],
        };
        let result = dummy_edit_result(0, 1, Some(map));
        let target = sample_target();
        ctrl.on_edit_result(&result, &target, 16.0);

        assert!(ctrl.revision_chain().has_pending_edits());
        assert!(ctrl.target_layout().is_some());
    }

    #[test]
    fn controller_on_edit_result_pure_selection_preserves_map() {
        let mut ctrl = CanvasController::new(
            CanvasSettings::default(),
            EditorRevision::new(0),
            0.0,
        );
        // 第一笔：有 offset_map。
        let map = OffsetMap {
            entries: vec![writer_core::editor::OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::unchecked(0),
                new_byte_offset: Utf8ByteOffset::unchecked(0),
                length: 3,
                kind: writer_core::editor::OffsetMapKind::Identity,
            }],
        };
        ctrl.on_edit_result(&dummy_edit_result(0, 1, Some(map)), &sample_target(), 16.0);
        assert_eq!(ctrl.revision_chain().map_presented_to_latest(1), Some(1));

        // 第二笔：纯选区操作。
        ctrl.on_edit_result(&dummy_edit_result(1, 2, None), &sample_target(), 32.0);
        assert_eq!(
            ctrl.revision_chain().map_presented_to_latest(1),
            Some(1),
            "pure selection must preserve accumulated offset map"
        );
        assert_eq!(
            ctrl.revision_chain().latest_revision(),
            EditorRevision::new(2)
        );
    }

    #[test]
    fn controller_on_frame_committed_clears_pending() {
        let mut ctrl = CanvasController::new(
            CanvasSettings::default(),
            EditorRevision::new(0),
            0.0,
        );
        let map = OffsetMap {
            entries: vec![writer_core::editor::OffsetMapEntry {
                old_byte_offset: Utf8ByteOffset::unchecked(0),
                new_byte_offset: Utf8ByteOffset::unchecked(0),
                length: 5,
                kind: writer_core::editor::OffsetMapKind::Identity,
            }],
        };
        ctrl.on_edit_result(&dummy_edit_result(0, 1, Some(map)), &sample_target(), 16.0);
        assert!(ctrl.revision_chain().has_pending_edits());

        // 提交追上 Core 的帧。
        let mut scene = sample_presented();
        scene.source_revision = EditorRevision::new(1);
        ctrl.on_frame_committed(scene, 32.0);
        assert!(!ctrl.revision_chain().has_pending_edits());

        // frame 现在应该有内容。
        let plan = ctrl.frame(48.0);
        assert!(!plan.empty);
    }

    #[test]
    fn controller_frame_with_presented_and_target() {
        let mut ctrl = CanvasController::new(
            CanvasSettings::default(),
            EditorRevision::new(0),
            0.0,
        );
        ctrl.on_frame_committed(sample_presented(), 0.0);
        ctrl.on_edit_result(&dummy_edit_result(0, 1, None), &sample_target(), 16.0);

        let plan = ctrl.frame(32.0);
        // 应该有绘制命令。
        assert!(!plan.empty || !plan.commands.is_empty());
    }

    #[test]
    fn controller_apply_settings_updates_response_time() {
        let mut ctrl = CanvasController::new(
            CanvasSettings::default(),
            EditorRevision::initial(),
            0.0,
        );
        let mut settings = CanvasSettings::default();
        settings.response_duration_ms = 300.0;
        ctrl.apply_settings(settings);
        assert!((ctrl.settings().response_duration_ms - 300.0).abs() < 1e-9);
        assert!((ctrl.frame_clock().response_duration_ms() - 300.0).abs() < 1e-9);
    }

    #[test]
    fn controller_continuous_edits_no_animation_queue() {
        // 连续多笔编辑不积累动画历史。
        let mut ctrl = CanvasController::new(
            CanvasSettings::default(),
            EditorRevision::new(0),
            0.0,
        );
        ctrl.on_frame_committed(sample_presented(), 0.0);

        // 连续 3 笔编辑。
        for i in 0..3 {
            let result = dummy_edit_result(i, i + 1, None);
            ctrl.on_edit_result(&result, &sample_target(), (i + 1) as f64 * 16.0);
        }

        // latest revision 应该是 3，但只有一份 target（不积累动画队列）。
        assert_eq!(
            ctrl.revision_chain().latest_revision(),
            EditorRevision::new(3)
        );
        assert!(ctrl.target_layout().is_some());
    }

    #[test]
    fn controller_text_frontier_updated_on_edit_and_commit() {
        let mut ctrl = CanvasController::new(
            CanvasSettings::default(),
            EditorRevision::new(0),
            0.0,
        );
        let target = sample_target();
        ctrl.on_edit_result(&dummy_edit_result(0, 1, None), &target, 16.0);

        // TextFrontier 的目标应该更新为 target 的 byte range。
        let (ts, te) = ctrl.text_frontier().target_range();
        assert_eq!(ts, 0);
        assert_eq!(te, 5);
    }
}
