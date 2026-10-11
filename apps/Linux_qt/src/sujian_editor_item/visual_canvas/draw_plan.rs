//! Issue #901：不可变绘制计划。
//!
//! `CanvasDrawPlan` 是 `frame(now)` 的输出，由 `SceneTransition` 生成。
//! 一旦生成不再修改，渲染层直接消费。
//!
//! ## 设计
//!
//! - `CanvasDrawPlan` 包含一组 `DrawCommand`，每条命令描述一个绘制操作。
//! - 每条 `DrawCommand` 引用源纹理 + 源 UV + 目标 rect + 裁剪 + 透明度。
//! - `DrawCommand` 是不可变值类型：一旦创建不再修改。`Ownership::resolve`
//!   通过 clone + 修改 `clip_rect` 产生裁剪后的新命令，不原地修改。
//!
//! ## 坐标契约（Issue #901 问题2）
//!
//! `DrawCommand.target_rect` 和 `DrawCommand.clip_rect` 始终使用**文档坐标**
//! （不含滚动偏移）。C++ 动画层（`AnimationLayerNode`）和静态层
//! （`QSGTransformNode`）各自统一施加一次 `translate(0, -scroll_y)` 变换。
//! Rust 侧不在 `target_rect` 中预减 `scroll_y`，避免滚动时被减两次。
//!
//! ## 纯 Rust
//!
//! 本模块不依赖 Qt C++ 类型。所有几何都是纯 Rust f64。

use super::ownership::OwnershipRule;
use super::presented_scene::{CanvasRect, CanvasVec2};
use crate::sujian_editor_item::snapshot_id::LineSnapshotId;
use writer_core::editor::EditorRevision;

/// 绘制命令类型 —— 决定渲染层如何绘制该命令。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DrawCommandKind {
    /// 绘制单个文字片段（一个 glyph 或一段连续 glyph）。
    TextFragment,
    /// 绘制光标。
    Caret,
    /// 绘制选区背景。
    SelectionBackground,
    /// 绘制预输入（IME preedit）。
    Preedit,
}

/// Issue #901 问题3：静态层排除矩形 —— 动画画布接管的**完整目标范围**（文档坐标）。
///
/// 整个过渡期间与 `progress` 无关。静态正文层在这些区域挖空，由动画层绘制。
/// 动画层的动态 clip 才按 `progress` 更新（在 `DrawCommand.target_rect` /
/// `DrawCommand.clip_rect` 中）。
///
/// 与 `DrawCommand.target_rect` 的区别：
/// - `DrawCommand.target_rect`：当前帧实际绘制的目标矩形（可能按 progress 缩短）。
/// - `StaticExclusionRect.rect`：整个过渡期间动画画布接管的完整目标范围。
///
/// 只在接管集合改变时重建静态 QSGTextNode，不在每帧改变挖空矩形。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StaticExclusionRect {
    /// 文档坐标系中的完整目标矩形（与 progress 无关）。
    pub rect: CanvasRect,
    /// 源纹理的行快照 id（用于纹理依赖检查）。
    pub snapshot_id: LineSnapshotId,
}

/// 不可变单条绘制命令。
///
/// 每条命令引用一个源纹理区域（`snapshot_id` + `source_uv`），将其绘制到
/// **文档坐标系**的 `target_rect`，受 `clip_rect` 裁剪，整体透明度乘以 `opacity`。
///
/// `snapshot_id` 为 `LineSnapshotId::none()` 表示纯色填充（用于光标、选区背景等
/// 无纹理的绘制）。
///
/// **不可变**：一旦创建不再修改。`Ownership::resolve` 通过 clone 产生新命令。
///
/// Issue #901 问题2：`target_rect` 和 `clip_rect` 始终使用**文档坐标**（不含滚动
/// 偏移）。C++ 动画层和静态层各自统一施加一次 `translate(0, -scroll_y)`。
///
/// Issue #901 问题2：`snapshot_id` 携带完整 `LineSnapshotId`（layout_revision +
/// paragraph_id + visual_line_ordinal），而非 u64 hash。u64 hash 只在渲染器中
/// 通过 `to_cache_key()` 生成 GPU 缓存 key，绝不反解成 `LineSnapshotId`。
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DrawCommand {
    /// 命令类型。
    pub kind: DrawCommandKind,
    /// 源纹理的完整行快照 id。`LineSnapshotId::none()` 表示纯色填充。
    pub snapshot_id: LineSnapshotId,
    /// 源纹理中的 UV 区域（纹理局部坐标）。
    pub source_uv: CanvasRect,
    /// 目标矩形（画布/视口坐标）。
    pub target_rect: CanvasRect,
    /// 裁剪矩形（画布/视口坐标）。渲染时只绘制 `target_rect ∩ clip_rect`。
    pub clip_rect: CanvasRect,
    /// 透明度（0.0 完全透明 – 1.0 完全不透明）。
    pub opacity: f64,
    /// 像素归属规则（用于 `Ownership::resolve` 解决重叠）。
    pub ownership: OwnershipRule,
    /// 该命令对应的 Core 逻辑 UTF-8 byte range `(start, end)`（半开区间）。
    ///
    /// Issue #901 问题3：从 `PixelContribution.byte_range` 或
    /// `TargetGlyphRun.byte_range` 传递，供 `build_presented_scene_from_frame`
    /// 构造 `PixelContribution` 时使用真实字节范围，不硬编码 `(0, 0)`。
    /// 非文字命令（光标/选区/preedit）使用 `(0, 0)`。
    pub byte_range: (usize, usize),
}

impl DrawCommand {
    /// 创建一条文字片段绘制命令。
    pub(crate) fn text_fragment(
        snapshot_id: LineSnapshotId,
        source_uv: CanvasRect,
        target_rect: CanvasRect,
        clip_rect: CanvasRect,
        opacity: f64,
        ownership: OwnershipRule,
        byte_range: (usize, usize),
    ) -> Self {
        Self {
            kind: DrawCommandKind::TextFragment,
            snapshot_id,
            source_uv,
            target_rect,
            clip_rect,
            opacity,
            ownership,
            byte_range,
        }
    }

    /// 创建一条光标绘制命令（纯色填充，`snapshot_id = none()`）。
    pub(crate) fn caret(target_rect: CanvasRect, clip_rect: CanvasRect, opacity: f64) -> Self {
        Self {
            kind: DrawCommandKind::Caret,
            snapshot_id: LineSnapshotId::none(),
            source_uv: CanvasRect::zero(),
            target_rect,
            clip_rect,
            opacity,
            ownership: OwnershipRule::Cursor,
            byte_range: (0, 0),
        }
    }

    /// 创建一条选区背景绘制命令（纯色填充，`snapshot_id = none()`）。
    pub(crate) fn selection_background(
        target_rect: CanvasRect,
        clip_rect: CanvasRect,
        opacity: f64,
    ) -> Self {
        Self {
            kind: DrawCommandKind::SelectionBackground,
            snapshot_id: LineSnapshotId::none(),
            source_uv: CanvasRect::zero(),
            target_rect,
            clip_rect,
            opacity,
            ownership: OwnershipRule::Selection,
            byte_range: (0, 0),
        }
    }

    /// 创建一条预输入绘制命令。
    pub(crate) fn preedit(
        snapshot_id: LineSnapshotId,
        source_uv: CanvasRect,
        target_rect: CanvasRect,
        clip_rect: CanvasRect,
        opacity: f64,
    ) -> Self {
        Self {
            kind: DrawCommandKind::Preedit,
            snapshot_id,
            source_uv,
            target_rect,
            clip_rect,
            opacity,
            ownership: OwnershipRule::Preedit,
            byte_range: (0, 0),
        }
    }

    /// 该命令是否有非零可见面积（裁剪矩形非零且透明度 > 0）。
    pub(crate) fn has_visible_area(&self) -> bool {
        self.clip_rect.w > 0.0
            && self.clip_rect.h > 0.0
            && self.opacity > 0.0
            && self.target_rect.w > 0.0
            && self.target_rect.h > 0.0
    }
}

/// 不可变绘制计划 —— `frame(now)` 的输出。
///
/// 由 `SceneTransition::build_draw_plan` 生成，包含一组已解决重叠的
/// `DrawCommand`。渲染层直接消费，不再做任何状态决策。
///
/// **不可变**：一旦生成不再修改。
#[derive(Clone, Debug)]
pub(crate) struct CanvasDrawPlan {
    /// 该计划对应的时间戳（毫秒）。
    pub now_ms: f64,
    /// 该计划的整体姿态采样点（光标位置等）。
    pub caret_sample: CanvasVec2,
    /// 该计划是否为空（无可见内容）。
    pub empty: bool,
    /// 已解决重叠的绘制命令列表。
    pub commands: Vec<DrawCommand>,
    /// Issue #901 问题3：该计划对应的 render frame id。
    ///
    /// 由 `SceneTransition::build_draw_plan` 从 `PresentedScene.frame_id` 拷贝，
    /// 供 `build_presented_scene_from_frame` 原样回填到下一张 `PresentedScene`。
    pub frame_id: u64,
    /// Issue #901 问题3：该计划对应的 document session generation。
    pub document_session: u64,
    /// Issue #901 问题3：产生该计划时绑定的 Core source revision。
    pub source_revision: EditorRevision,
    /// Issue #901 问题3：该计划对应的最新 Core target revision。
    pub target_revision: EditorRevision,
    /// Issue #901 问题1：本次采样的真实 `VisualPose.progress`（0.0 – 1.0）。
    ///
    /// 由 `SceneTransition::build_draw_plan` 从 `VisualPose.progress` 拷贝，
    /// 供 `build_presented_scene_from_frame` 原样回填到 `PresentedScene.pose_progress`，
    /// 不再硬编码 1.0。`controller.rs` 据此判断画面是否真正到达终点。
    pub pose_progress: f64,
    /// Issue #901 问题3：静态层排除矩形列表 —— 动画画布接管的完整目标范围。
    ///
    /// 整个过渡期间与 `progress` 无关。`scene_graph_renderer` 据此让静态正文层
    /// 在这些区域挖空，由动画层绘制。动画层的动态 clip 按 `progress` 更新
    /// （在 `commands` 中）。只在接管集合改变时重建静态 QSGTextNode。
    pub static_exclusion_rects: Vec<StaticExclusionRect>,
}

impl CanvasDrawPlan {
    /// 创建一个空绘制计划（无可见内容）。
    pub(crate) fn empty(now_ms: f64) -> Self {
        Self {
            now_ms,
            caret_sample: CanvasVec2::new(0.0, 0.0),
            empty: true,
            commands: Vec::new(),
            frame_id: 0,
            document_session: 0,
            source_revision: EditorRevision::initial(),
            target_revision: EditorRevision::initial(),
            pose_progress: 0.0,
            static_exclusion_rects: Vec::new(),
        }
    }

    /// 从绘制命令列表构造绘制计划。
    ///
    /// `empty` 由命令列表是否含可见内容自动推导。
    /// 元数据字段使用默认值；需要真实元数据时用 `from_commands_with_metadata`。
    pub(crate) fn from_commands(
        now_ms: f64,
        caret_sample: CanvasVec2,
        commands: Vec<DrawCommand>,
    ) -> Self {
        let has_visible = commands.iter().any(DrawCommand::has_visible_area);
        Self {
            now_ms,
            caret_sample,
            empty: !has_visible,
            commands,
            frame_id: 0,
            document_session: 0,
            source_revision: EditorRevision::initial(),
            target_revision: EditorRevision::initial(),
            pose_progress: 0.0,
            static_exclusion_rects: Vec::new(),
        }
    }

    /// Issue #901 问题3：从绘制命令列表构造绘制计划，携带完整元数据。
    ///
    /// `build_presented_scene_from_frame` 可从返回的 `CanvasDrawPlan` 原样
    /// 拷贝 `frame_id` / `document_session` / `source_revision` /
    /// `target_revision` / `pose_progress` 到下一张 `PresentedScene`，不依赖
    /// 外部额外传参。
    ///
    /// Issue #901 问题1：`pose_progress` 携带本次采样的真实 `VisualPose.progress`，
    /// 不再硬编码 1.0。
    ///
    /// Issue #901 问题3：`static_exclusion_rects` 携带动画画布接管的完整目标范围，
    /// 与 `progress` 无关，供静态层挖空。
    pub(crate) fn from_commands_with_metadata(
        now_ms: f64,
        caret_sample: CanvasVec2,
        commands: Vec<DrawCommand>,
        frame_id: u64,
        document_session: u64,
        source_revision: EditorRevision,
        target_revision: EditorRevision,
        pose_progress: f64,
        static_exclusion_rects: Vec<StaticExclusionRect>,
    ) -> Self {
        let has_visible = commands.iter().any(DrawCommand::has_visible_area);
        Self {
            now_ms,
            caret_sample,
            empty: !has_visible,
            commands,
            frame_id,
            document_session,
            source_revision,
            target_revision,
            pose_progress,
            static_exclusion_rects,
        }
    }

    /// 可见绘制命令的数量。
    pub(crate) fn visible_command_count(&self) -> usize {
        self.commands
            .iter()
            .filter(|c| c.has_visible_area())
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_rect(x: f64, y: f64, w: f64, h: f64) -> CanvasRect {
        CanvasRect::new(x, y, w, h)
    }

    fn sample_snapshot_id() -> LineSnapshotId {
        LineSnapshotId::new(1, 0, 0)
    }

    #[test]
    fn draw_command_text_fragment_construction() {
        let cmd = DrawCommand::text_fragment(
            sample_snapshot_id(),
            sample_rect(0.0, 0.0, 10.0, 10.0),
            sample_rect(0.0, 0.0, 10.0, 10.0),
            sample_rect(0.0, 0.0, 10.0, 10.0),
            1.0,
            OwnershipRule::StaticText,
            (0, 5),
        );
        assert_eq!(cmd.kind, DrawCommandKind::TextFragment);
        assert_eq!(cmd.snapshot_id, sample_snapshot_id());
        assert!(cmd.has_visible_area());
    }

    #[test]
    fn draw_command_caret_is_pure_color() {
        let cmd = DrawCommand::caret(
            sample_rect(5.0, 0.0, 2.0, 20.0),
            sample_rect(0.0, 0.0, 800.0, 600.0),
            1.0,
        );
        assert_eq!(cmd.kind, DrawCommandKind::Caret);
        assert!(cmd.snapshot_id.is_none());
        assert_eq!(cmd.ownership, OwnershipRule::Cursor);
        assert!(cmd.has_visible_area());
    }

    #[test]
    fn draw_command_selection_background() {
        let cmd = DrawCommand::selection_background(
            sample_rect(0.0, 0.0, 50.0, 20.0),
            sample_rect(0.0, 0.0, 800.0, 600.0),
            0.5,
        );
        assert_eq!(cmd.kind, DrawCommandKind::SelectionBackground);
        assert_eq!(cmd.ownership, OwnershipRule::Selection);
        assert!(cmd.has_visible_area());
    }

    #[test]
    fn draw_command_preedit() {
        let cmd = DrawCommand::preedit(
            LineSnapshotId::new(2, 0, 0),
            sample_rect(0.0, 0.0, 30.0, 20.0),
            sample_rect(10.0, 0.0, 30.0, 20.0),
            sample_rect(0.0, 0.0, 800.0, 600.0),
            0.8,
        );
        assert_eq!(cmd.kind, DrawCommandKind::Preedit);
        assert_eq!(cmd.ownership, OwnershipRule::Preedit);
        assert!(cmd.has_visible_area());
    }

    #[test]
    fn draw_command_zero_clip_not_visible() {
        let cmd = DrawCommand::text_fragment(
            sample_snapshot_id(),
            sample_rect(0.0, 0.0, 10.0, 10.0),
            sample_rect(0.0, 0.0, 10.0, 10.0),
            CanvasRect::zero(),
            1.0,
            OwnershipRule::StaticText,
            (0, 5),
        );
        assert!(!cmd.has_visible_area());
    }

    #[test]
    fn draw_command_zero_opacity_not_visible() {
        let cmd = DrawCommand::text_fragment(
            sample_snapshot_id(),
            sample_rect(0.0, 0.0, 10.0, 10.0),
            sample_rect(0.0, 0.0, 10.0, 10.0),
            sample_rect(0.0, 0.0, 10.0, 10.0),
            0.0,
            OwnershipRule::StaticText,
            (0, 5),
        );
        assert!(!cmd.has_visible_area());
    }

    #[test]
    fn canvas_draw_plan_empty() {
        let plan = CanvasDrawPlan::empty(16.0);
        assert!(plan.empty);
        assert!(plan.commands.is_empty());
        assert_eq!(plan.visible_command_count(), 0);
    }

    #[test]
    fn canvas_draw_plan_from_commands() {
        let cmd = DrawCommand::text_fragment(
            sample_snapshot_id(),
            sample_rect(0.0, 0.0, 10.0, 10.0),
            sample_rect(0.0, 0.0, 10.0, 10.0),
            sample_rect(0.0, 0.0, 10.0, 10.0),
            1.0,
            OwnershipRule::StaticText,
            (0, 5),
        );
        let plan = CanvasDrawPlan::from_commands(
            16.0,
            CanvasVec2::new(5.0, 10.0),
            vec![cmd],
        );
        assert!(!plan.empty);
        assert_eq!(plan.visible_command_count(), 1);
        assert_eq!(plan.commands.len(), 1);
    }

    #[test]
    fn canvas_draw_plan_from_empty_commands_is_empty() {
        let plan = CanvasDrawPlan::from_commands(16.0, CanvasVec2::new(0.0, 0.0), vec![]);
        assert!(plan.empty);
    }
}
