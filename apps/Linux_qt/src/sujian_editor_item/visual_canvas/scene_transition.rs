//! Issue #901：场景过渡 —— 从 `PresentedScene` + `TargetLayout` 生成 `CanvasDrawPlan`。
//!
//! ## 核心语义
//!
//! - `build_draw_plan` 根据当前 `VisualPose` 的 `progress` 插值 presented 和 target。
//! - `progress = 0`：完全显示 presented（已提交画面）。
//! - `progress = 1`：完全显示 target（最新目标布局）。
//! - 中间状态：旧帧已呈现部分保持原位（淡出），新内容从前沿逐步展开（淡入）。
//!
//! ## 连续性
//!
//! 连续性从"已提交画面"出发：source 永远来自最新 `PresentedScene`。
//! 新编辑不从中途未显示状态凭空生成 glyph，只从 presented source 通过
//! `RevisionChain` 精确映射到最新 revision。
//!
//! ## 纯 Rust
//!
//! 本模块不依赖 Qt 类型。`TargetLayoutDescriptor` 由调用方从 Qt 排版结果转换而来。

use super::draw_plan::{CanvasDrawPlan, DrawCommand};
use super::frame_clock::VisualPose;
use super::ownership::Ownership;
use super::presented_scene::{
    CanvasAffine2, CanvasRect, CanvasVec2, PixelContribution, PresentedScene,
};
use crate::sujian_editor_item::layout_revision::LayoutRevision;

// ── 目标布局描述 ──

/// 目标布局描述 —— 最新正文经过排版后的目标几何（纯 Rust）。
///
/// 由调用方从 Qt `QTextLayout` 排版结果转换而来。包含 layout revision、
/// 段落几何、光标位置和滚动位置。不携带动画状态——动画由 `VisualPose` 统一管理。
#[derive(Clone, Debug)]
pub(crate) struct TargetLayoutDescriptor {
    /// 该布局对应的 layout revision（标识排版版本）。
    pub layout_revision: LayoutRevision,
    /// 段落几何列表（每段一个或多个 `TargetGlyphRun`）。
    pub glyph_runs: Vec<TargetGlyphRun>,
    /// 目标光标位置（光标不可见时为 `None`）。
    pub caret: Option<TargetCaret>,
    /// 目标滚动位置。
    pub scroll: TargetScroll,
    /// 整体裁剪区域（视口可见矩形）。
    pub clip: CanvasRect,
}

/// 目标布局中的单个 glyph run —— 一段连续的文字片段及其纹理。
#[derive(Clone, Debug)]
pub(crate) struct TargetGlyphRun {
    /// 源纹理 id。
    pub texture_id: u64,
    /// 源纹理 UV 区域。
    pub source_uv: CanvasRect,
    /// 文档坐标系中的目标矩形。
    pub document_rect: CanvasRect,
    /// 该 glyph run 对应的 byte range（用于前沿裁剪）。
    pub byte_range: (usize, usize),
}

/// 目标光标位置和状态。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TargetCaret {
    /// 光标左上角位置（视口坐标）。
    pub position: CanvasVec2,
    /// 光标是否可见。
    pub visible: bool,
    /// 光标宽度（像素）。
    pub width: f64,
    /// 光标高度（像素）。
    pub height: f64,
}

/// 目标滚动位置。
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TargetScroll {
    /// 水平滚动偏移。
    pub x: f64,
    /// 垂直滚动偏移。
    pub y: f64,
}

impl Default for TargetScroll {
    fn default() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
}

// ── 场景过渡 ──

/// 场景过渡 —— 从 `PresentedScene` + `TargetLayoutDescriptor` 生成 `CanvasDrawPlan`。
///
/// 核心方法 `build_draw_plan` 根据 `VisualPose.progress` 插值 presented 和 target，
/// 生成不可变的 `CanvasDrawPlan`，通过 `Ownership::resolve` 解决重叠。
pub(crate) struct SceneTransition;

impl SceneTransition {
    /// 从已提交画面和目标布局生成绘制计划。
    ///
    /// ## 插值规则
    ///
    /// - `progress = 0`：完全显示 presented，target 透明度为 0。
    /// - `progress = 1`：完全显示 target，presented 透明度为 0。
    /// - 中间状态：presented 以 `(1 - progress)` 淡出，target 以 `progress` 淡入。
    ///   旧帧已呈现部分保持原位（不移动），新内容从前沿逐步展开。
    ///
    /// ## 重叠解决
    ///
    /// 生成的命令通过 `Ownership::resolve` 解决重叠：
    /// - presented 的文字标记为 `StaticText`（已呈现部分优先）。
    /// - target 的文字标记为 `AnimatedText`（新内容）。
    /// - 光标标记为 `Cursor`，选区标记为 `Selection`，预输入标记为 `Preedit`。
    pub(crate) fn build_draw_plan(
        presented: &PresentedScene,
        target: &TargetLayoutDescriptor,
        pose: &VisualPose,
        now_ms: f64,
    ) -> CanvasDrawPlan {
        let progress = pose.progress.clamp(0.0, 1.0);
        let fade_out = 1.0 - progress; // presented 淡出系数
        let fade_in = progress; // target 淡入系数

        let mut commands = Vec::new();

        // 1. 从 presented 生成文字命令（淡出）。
        if fade_out > 0.0 {
            for contrib in &presented.pixel_contributions {
                if let Some(cmd) = Self::command_from_contribution(contrib, fade_out, presented) {
                    commands.push(cmd);
                }
            }
        }

        // 2. 从 target 生成文字命令（淡入）。
        if fade_in > 0.0 {
            for run in &target.glyph_runs {
                if let Some(cmd) = Self::command_from_glyph_run(run, fade_in, target) {
                    commands.push(cmd);
                }
            }
        }

        // 3. 光标命令：插值 presented caret 和 target caret。
        Self::push_caret_commands(
            &mut commands,
            presented,
            target,
            pose,
            fade_out,
            fade_in,
        );

        // 4. 通过 Ownership 解决重叠。
        let resolved = Ownership::resolve(&commands);

        let caret_sample = pose.caret_position;
        CanvasDrawPlan::from_commands(now_ms, caret_sample, resolved)
    }

    /// 只从 `PresentedScene` 生成绘制计划（无目标布局时使用）。
    ///
    /// 当尚无 `TargetLayoutDescriptor` 但已有已提交画面时，直接从 presented
    /// 的像素贡献生成绘制命令。`progress` 不影响输出——没有目标可过渡。
    pub(crate) fn build_draw_plan_presented_only(
        presented: &PresentedScene,
        pose: &VisualPose,
        now_ms: f64,
    ) -> CanvasDrawPlan {
        let mut commands = Vec::new();

        // 从 presented 生成文字命令（完整透明度）。
        for contrib in &presented.pixel_contributions {
            if let Some(cmd) = Self::command_from_contribution(contrib, 1.0, presented) {
                commands.push(cmd);
            }
        }

        // 光标命令。
        if let Some(pc) = &presented.caret {
            if pc.visible {
                let rect = CanvasRect::new(
                    pose.caret_position.x,
                    pose.caret_position.y,
                    pc.width,
                    pc.height,
                );
                commands.push(DrawCommand::caret(rect, presented.clip, 1.0));
            }
        }

        let resolved = Ownership::resolve(&commands);
        CanvasDrawPlan::from_commands(now_ms, pose.caret_position, resolved)
    }

    /// 从 `PixelContribution` 生成一条文字绘制命令。
    ///
    /// 透明度乘以 `fade`（淡出系数）和 presented 整体透明度。
    fn command_from_contribution(
        contrib: &PixelContribution,
        fade: f64,
        presented: &PresentedScene,
    ) -> Option<DrawCommand> {
        if !contrib.has_visible_area() {
            return None;
        }
        let opacity = contrib.opacity * presented.opacity * fade;
        if opacity <= 0.0 {
            return None;
        }
        // 将 document_rect 通过 presented 的 transform 映射到画布坐标。
        let target_rect = Self::transform_rect(contrib.document_rect, presented.transform);
        let target_rect = CanvasRect::new(
            target_rect.x + presented.position.x,
            target_rect.y + presented.position.y,
            target_rect.w,
            target_rect.h,
        );
        Some(DrawCommand::text_fragment(
            contrib.texture_id,
            contrib.source_uv,
            target_rect,
            contrib.visible_clip,
            opacity,
            super::ownership::OwnershipRule::StaticText,
        ))
    }

    /// 从 `TargetGlyphRun` 生成一条文字绘制命令。
    ///
    /// 透明度乘以 `fade`（淡入系数）。目标矩形减去滚动偏移转为视口坐标。
    fn command_from_glyph_run(
        run: &TargetGlyphRun,
        fade: f64,
        target: &TargetLayoutDescriptor,
    ) -> Option<DrawCommand> {
        if run.document_rect.w <= 0.0 || run.document_rect.h <= 0.0 {
            return None;
        }
        let opacity = fade;
        if opacity <= 0.0 {
            return None;
        }
        // 文档坐标减去滚动偏移 → 视口坐标。
        let viewport_rect = CanvasRect::new(
            run.document_rect.x - target.scroll.x,
            run.document_rect.y - target.scroll.y,
            run.document_rect.w,
            run.document_rect.h,
        );
        Some(DrawCommand::text_fragment(
            run.texture_id,
            run.source_uv,
            viewport_rect,
            target.clip,
            opacity,
            super::ownership::OwnershipRule::AnimatedText,
        ))
    }

    /// 生成光标命令：插值 presented caret 和 target caret。
    ///
    /// - `progress < 0.5`：显示 presented caret（淡出）。
    /// - `progress >= 0.5`：显示 target caret（淡入）。
    /// - 光标位置使用 `VisualPose.caret_position`（已由 FrameClock 插值）。
    fn push_caret_commands(
        commands: &mut Vec<DrawCommand>,
        presented: &PresentedScene,
        target: &TargetLayoutDescriptor,
        pose: &VisualPose,
        fade_out: f64,
        fade_in: f64,
    ) {
        // 使用 pose 的光标位置（FrameClock 已插值）。
        let caret_pos = pose.caret_position;

        // presented caret（淡出阶段）。
        if fade_out > 0.0 {
            if let Some(pc) = &presented.caret {
                if pc.visible {
                    let rect = CanvasRect::new(caret_pos.x, caret_pos.y, pc.width, pc.height);
                    commands.push(DrawCommand::caret(rect, presented.clip, fade_out));
                }
            }
        }

        // target caret（淡入阶段）。
        if fade_in > 0.0 {
            if let Some(tc) = &target.caret {
                if tc.visible {
                    let rect = CanvasRect::new(caret_pos.x, caret_pos.y, tc.width, tc.height);
                    commands.push(DrawCommand::caret(rect, target.clip, fade_in));
                }
            }
        }
    }

    /// 将 `CanvasRect` 通过仿射变换映射。
    ///
    /// 对矩形的四个角点做变换后取 AABB（轴对齐包围盒）。
    fn transform_rect(rect: CanvasRect, transform: CanvasAffine2) -> CanvasRect {
        let p0 = transform.transform_point(CanvasVec2::new(rect.x, rect.y));
        let p1 = transform.transform_point(CanvasVec2::new(rect.x + rect.w, rect.y));
        let p2 = transform.transform_point(CanvasVec2::new(rect.x, rect.y + rect.h));
        let p3 = transform.transform_point(CanvasVec2::new(rect.x + rect.w, rect.y + rect.h));

        let min_x = p0.x.min(p1.x).min(p2.x).min(p3.x);
        let max_x = p0.x.max(p1.x).max(p2.x).max(p3.x);
        let min_y = p0.y.min(p1.y).min(p2.y).min(p3.y);
        let max_y = p0.y.max(p1.y).max(p2.y).max(p3.y);

        CanvasRect::new(min_x, min_y, max_x - min_x, max_y - min_y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::ownership::OwnershipRule;
    use super::super::presented_scene::{PresentedCaret, PresentedScroll};
    use writer_core::editor::EditorRevision;

    fn sample_presented(opacity: f64) -> PresentedScene {
        PresentedScene {
            source_revision: EditorRevision::initial(),
            document_session: 1,
            frame_id: 1,
            geometry_id: 1,
            clip: CanvasRect::new(0.0, 0.0, 800.0, 600.0),
            position: CanvasVec2::new(0.0, 0.0),
            opacity,
            transform: CanvasAffine2::identity(),
            pixel_contributions: vec![PixelContribution {
                texture_id: 1,
                source_uv: CanvasRect::new(0.0, 0.0, 100.0, 20.0),
                document_rect: CanvasRect::new(0.0, 0.0, 100.0, 20.0),
                visible_clip: CanvasRect::new(0.0, 0.0, 100.0, 20.0),
                opacity: 1.0,
            }],
            caret: None,
            scroll: PresentedScroll::default(),
        }
    }

    fn sample_target() -> TargetLayoutDescriptor {
        TargetLayoutDescriptor {
            layout_revision: LayoutRevision::initial(),
            glyph_runs: vec![TargetGlyphRun {
                texture_id: 2,
                source_uv: CanvasRect::new(0.0, 0.0, 100.0, 20.0),
                document_rect: CanvasRect::new(0.0, 0.0, 100.0, 20.0),
                byte_range: (0, 5),
            }],
            caret: None,
            scroll: TargetScroll::default(),
            clip: CanvasRect::new(0.0, 0.0, 800.0, 600.0),
        }
    }

    fn pose(progress: f64) -> VisualPose {
        VisualPose {
            progress,
            caret_position: CanvasVec2::new(0.0, 0.0),
            scroll_position: CanvasVec2::new(0.0, 0.0),
        }
    }

    #[test]
    fn build_draw_plan_progress_zero_shows_presented() {
        let presented = sample_presented(1.0);
        let target = sample_target();
        let plan = SceneTransition::build_draw_plan(&presented, &target, &pose(0.0), 16.0);

        // progress=0 → 完全显示 presented，target 透明度为 0。
        assert!(!plan.empty);
        // presented 的文字命令应该存在（StaticText）。
        let static_cmds: Vec<_> = plan
            .commands
            .iter()
            .filter(|c| c.ownership == OwnershipRule::StaticText)
            .collect();
        assert!(!static_cmds.is_empty(), "presented text must be visible at progress=0");
        // target 的文字命令不应该存在（AnimatedText 透明度为 0 被过滤）。
        let animated_cmds: Vec<_> = plan
            .commands
            .iter()
            .filter(|c| c.ownership == OwnershipRule::AnimatedText)
            .collect();
        assert!(animated_cmds.is_empty(), "target text must be invisible at progress=0");
    }

    #[test]
    fn build_draw_plan_progress_one_shows_target() {
        let presented = sample_presented(1.0);
        let target = sample_target();
        let plan = SceneTransition::build_draw_plan(&presented, &target, &pose(1.0), 16.0);

        // progress=1 → 完全显示 target，presented 透明度为 0。
        assert!(!plan.empty);
        let animated_cmds: Vec<_> = plan
            .commands
            .iter()
            .filter(|c| c.ownership == OwnershipRule::AnimatedText)
            .collect();
        assert!(!animated_cmds.is_empty(), "target text must be visible at progress=1");
        let static_cmds: Vec<_> = plan
            .commands
            .iter()
            .filter(|c| c.ownership == OwnershipRule::StaticText)
            .collect();
        assert!(static_cmds.is_empty(), "presented text must be invisible at progress=1");
    }

    #[test]
    fn build_draw_plan_progress_half_shows_both() {
        let presented = sample_presented(1.0);
        let target = sample_target();
        let plan = SceneTransition::build_draw_plan(&presented, &target, &pose(0.5), 16.0);

        // 中间状态：两者都有（各自 0.5 透明度）。
        assert!(!plan.empty);
        // 由于 presented 和 target 在同一位置重叠，Ownership::resolve 会让
        // StaticText 优先，AnimatedText 被裁剪。但两者都应该在 resolve 前生成。
        // resolve 后 StaticText 保留，AnimatedText 可能被裁剪。
        let has_content = !plan.commands.is_empty();
        assert!(has_content, "mid-progress must have visible content");
    }

    #[test]
    fn build_draw_plan_empty_presented_and_target() {
        let presented = PresentedScene {
            source_revision: EditorRevision::initial(),
            document_session: 1,
            frame_id: 1,
            geometry_id: 1,
            clip: CanvasRect::new(0.0, 0.0, 800.0, 600.0),
            position: CanvasVec2::new(0.0, 0.0),
            opacity: 1.0,
            transform: CanvasAffine2::identity(),
            pixel_contributions: vec![],
            caret: None,
            scroll: PresentedScroll::default(),
        };
        let target = TargetLayoutDescriptor {
            layout_revision: LayoutRevision::initial(),
            glyph_runs: vec![],
            caret: None,
            scroll: TargetScroll::default(),
            clip: CanvasRect::new(0.0, 0.0, 800.0, 600.0),
        };
        let plan = SceneTransition::build_draw_plan(&presented, &target, &pose(0.5), 16.0);
        assert!(plan.empty);
    }

    #[test]
    fn build_draw_plan_includes_caret() {
        let mut presented = sample_presented(1.0);
        presented.caret = Some(PresentedCaret {
            position: CanvasVec2::new(50.0, 10.0),
            visible: true,
            width: 2.0,
            height: 20.0,
        });
        let target = sample_target();
        let plan = SceneTransition::build_draw_plan(&presented, &target, &pose(0.0), 16.0);

        let has_caret = plan
            .commands
            .iter()
            .any(|c| c.kind == super::super::draw_plan::DrawCommandKind::Caret);
        assert!(has_caret, "caret must be included in draw plan");
    }

    #[test]
    fn transform_rect_identity() {
        let rect = CanvasRect::new(10.0, 20.0, 100.0, 50.0);
        let result = SceneTransition::transform_rect(rect, CanvasAffine2::identity());
        assert_eq!(result, rect);
    }

    #[test]
    fn target_scroll_offsets_glyph_run() {
        let target = TargetLayoutDescriptor {
            layout_revision: LayoutRevision::initial(),
            glyph_runs: vec![TargetGlyphRun {
                texture_id: 1,
                source_uv: CanvasRect::new(0.0, 0.0, 100.0, 20.0),
                document_rect: CanvasRect::new(100.0, 200.0, 100.0, 20.0),
                byte_range: (0, 5),
            }],
            caret: None,
            scroll: TargetScroll { x: 10.0, y: 50.0 },
            clip: CanvasRect::new(0.0, 0.0, 800.0, 600.0),
        };
        let presented = sample_presented(0.0); // 透明，不产生命令
        let plan = SceneTransition::build_draw_plan(&presented, &target, &pose(1.0), 16.0);

        let cmd = plan
            .commands
            .iter()
            .find(|c| c.ownership == OwnershipRule::AnimatedText)
            .expect("target glyph run must produce a command");
        // 文档坐标 (100, 200) 减去滚动 (10, 50) → 视口 (90, 150)。
        assert!((cmd.target_rect.x - 90.0).abs() < 1e-9);
        assert!((cmd.target_rect.y - 150.0).abs() < 1e-9);
    }
}
