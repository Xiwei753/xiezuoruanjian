//! Issue #901：场景过渡 —— 从 `PresentedScene` + `TargetLayout` 生成 `CanvasDrawPlan`。
//!
//! ## 核心语义
//!
//! - `build_draw_plan` 根据 `RevisionChain` 判断每个已显示字形贡献在最终正文里
//!   的存活/删除/新插入关系，分别做不同处理：
//!   - **存活字**：只做老→新矩形位移（FLIP），不淡出淡入。
//!   - **新增字**：做 UV/裁剪揭示（从前沿逐步展开），透明度由 `progress` 控制。
//!   - **删除字**：从其已显示裁剪收回，透明度由 `(1 - progress)` 控制。
//!   - **shaping 替换**：字形已改变可用同一次 `progress` 下的旧→新贴图替换。
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
use super::revision_chain::RevisionChain;
use crate::sujian_editor_item::layout_revision::LayoutRevision;
use crate::sujian_editor_item::snapshot_id::LineSnapshotId;

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
///
/// Issue #901 问题2：`snapshot_id` 携带完整 `LineSnapshotId`，而非 u64 hash。
/// 渲染层通过完整 `LineSnapshotId` 查询 `TextureCache`，不再从 u64 反解。
#[derive(Clone, Debug)]
pub(crate) struct TargetGlyphRun {
    /// 源纹理的完整行快照 id。
    pub snapshot_id: LineSnapshotId,
    /// 源纹理 UV 区域。
    pub source_uv: CanvasRect,
    /// 文档坐标系中的目标矩形。
    pub document_rect: CanvasRect,
    /// 该 glyph run 对应的 byte range（用于前沿裁剪和存活判断）。
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
    /// ## 存活/删除/新插入分类（Issue #901 阻塞 2）
    ///
    /// 通过 `RevisionChain` 判断每个已显示字形贡献在最终正文里的关系：
    /// - **存活字**：`map_presented_to_latest(byte_range)` 返回 `Some`，
    ///   只做老→新矩形位移（FLIP），不淡出淡入。透明度保持 1.0。
    ///   clip 从 target 坐标系计算（`target.clip ∩ interpolated_rect`）。
    /// - **删除字**：`map_presented_to_latest(byte_range)` 返回 `None`，
    ///   沿相反方向从已显示宽度→0 裁剪收回（`source_uv` 宽度按
    ///   `displayed_width * (1-progress)` 收缩），不是 opacity=(1-progress)。
    /// - **新增字**：target glyph run 的 byte_range 在 presented revision 中
    ///   不存在，沿文字方向从 0→实际字形可见宽度裁剪揭示（`source_uv` 宽度
    ///   按 `full_width * progress` 展开），不是 opacity=progress。
    ///
    /// ## 重叠解决
    ///
    /// 生成的命令通过 `Ownership::resolve` 解决重叠。文字层之间用透明度叠加
    /// （alpha compositing），只有 `Cursor`/`Preedit`/`Selection` 做矩形裁剪。
    pub(crate) fn build_draw_plan(
        presented: &PresentedScene,
        target: &TargetLayoutDescriptor,
        pose: &VisualPose,
        revision_chain: &RevisionChain,
        now_ms: f64,
    ) -> CanvasDrawPlan {
        let progress = pose.progress.clamp(0.0, 1.0);
        let fade_out = 1.0 - progress; // 删除字裁剪收回系数
        let fade_in = progress; // 新增字裁剪揭示系数

        let mut commands = Vec::new();

        // 记算每个 target glyph run 是否是存活字（在 presented revision 中存在）。
        // 存活字的 target 索引集合 —— 这些 run 由 presented 侧的 FLIP 处理。
        let mut surviving_target_indices: Vec<bool> = vec![false; target.glyph_runs.len()];

        // 1. 从 presented 生成文字命令。
        for contrib in &presented.pixel_contributions {
            if !contrib.has_visible_area() {
                continue;
            }
            // 贡献的实际可见透明度（contrib.opacity * presented.opacity）。
            // 如果为 0，该贡献不可见，不标记存活 —— 让 target 侧作为新增字处理。
            let contrib_visible_opacity = contrib.opacity * presented.opacity;
            // 用 RevisionChain 判断该贡献是存活还是删除。
            let survived_offset = revision_chain.map_presented_to_latest(contrib.byte_range.0);
            if let Some(latest_offset) = survived_offset {
                // 存活字：尝试匹配 target glyph run 做 FLIP 位移。
                let matched = Self::find_matching_target_run(
                    target,
                    latest_offset,
                    contrib.byte_range,
                    revision_chain,
                );
                if let Some((run_idx, run)) = matched {
                    // 只有当贡献实际可见时才标记存活，否则让 target 侧作为新增字。
                    if contrib_visible_opacity > 0.0 {
                        surviving_target_indices[run_idx] = true;
                    }
                    // FLIP：在 old_rect 和 new_rect 之间插值，透明度保持 1.0。
                    if let Some(cmd) = Self::command_for_surviving(
                        contrib,
                        run,
                        progress,
                        presented,
                        target,
                    ) {
                        commands.push(cmd);
                    }
                } else {
                    // 未找到匹配 target run：按存活字原地保持（无位移）。
                    if let Some(cmd) = Self::command_from_contribution(
                        contrib,
                        1.0,
                        presented,
                        super::ownership::OwnershipRule::StaticText,
                    ) {
                        commands.push(cmd);
                    }
                }
            } else {
                // 删除字：沿相反方向从已显示宽度→0 裁剪收回。
                // Issue #901 问题4：不是 opacity=(1-progress)，
                // 而是 source_uv/target_rect 宽度按 (1-progress) 收缩。
                if let Some(cmd) = Self::command_for_deleting(contrib, progress, presented) {
                    commands.push(cmd);
                }
            }
        }

        // 2. 从 target 生成新增字命令（非存活的 glyph run）。
        for (idx, run) in target.glyph_runs.iter().enumerate() {
            if surviving_target_indices[idx] {
                // 存活字已由 presented 侧 FLIP 处理，跳过。
                continue;
            }
            // 新增字：沿文字方向从 0→full_width 裁剪揭示。
            // Issue #901 问题4：不是 opacity=progress，
            // 而是 source_uv/target_rect 宽度按 progress 展开。
            if let Some(cmd) = Self::command_for_inserting(run, progress, target) {
                commands.push(cmd);
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
        CanvasDrawPlan::from_commands_with_metadata(
            now_ms,
            caret_sample,
            resolved,
            presented.frame_id,
            presented.document_session,
            presented.source_revision,
            revision_chain.latest_revision(),
        )
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
            if let Some(cmd) = Self::command_from_contribution(
                contrib,
                1.0,
                presented,
                super::ownership::OwnershipRule::StaticText,
            ) {
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
        CanvasDrawPlan::from_commands_with_metadata(
            now_ms,
            pose.caret_position,
            resolved,
            presented.frame_id,
            presented.document_session,
            presented.source_revision,
            presented.source_revision, // 无 target，target_revision = source
        )
    }

    /// Issue #901 问题1+3：尚无 `PresentedScene` 时从 target 生成初始静态画面。
    ///
    /// 首帧（`presented_scene` 为 None）但有 `TargetLayoutDescriptor` 时，
    /// 从 target 的 glyph_runs 生成完整静态画面（progress=1.0）。
    /// **不注册为 `presented_scene`**——`presented_scene` 只在
    /// `on_frame_committed`（afterFrameEnd 回执）时设置。
    pub(crate) fn build_draw_plan_target_only(
        target: &TargetLayoutDescriptor,
        revision_chain: &RevisionChain,
        now_ms: f64,
    ) -> CanvasDrawPlan {
        let mut commands = Vec::new();

        // 从 target 生成静态画面（progress=1.0，完整显示）。
        for run in &target.glyph_runs {
            if let Some(cmd) = Self::command_from_glyph_run(run, 1.0, target) {
                commands.push(cmd);
            }
        }

        // 光标命令。
        if let Some(tc) = &target.caret {
            if tc.visible {
                let rect = CanvasRect::new(tc.position.x, tc.position.y, tc.width, tc.height);
                commands.push(DrawCommand::caret(rect, target.clip, 1.0));
            }
        }

        let resolved = Ownership::resolve(&commands);
        let caret_sample = super::presented_scene::CanvasVec2::new(
            target.caret.filter(|c| c.visible).map(|c| c.position.x).unwrap_or(0.0),
            target.caret.filter(|c| c.visible).map(|c| c.position.y).unwrap_or(0.0),
        );
        CanvasDrawPlan::from_commands_with_metadata(
            now_ms,
            caret_sample,
            resolved,
            0, // frame_id: 尚无已提交帧
            0, // document_session: 尚无
            revision_chain.presented_revision(),
            revision_chain.latest_revision(),
        )
    }

    /// 在 target glyph_runs 中查找与存活贡献匹配的 run。
    ///
    /// `latest_offset` 是 presented byte_range 起始通过 `RevisionChain` 映射到
    /// latest revision 的 offset。匹配条件：target run 的 byte_range 包含
    /// 映射后的整个 range。
    ///
    /// Issue #901 问题4：使用 `map_presented_range_to_latest` 精确判断整个区间
    /// 身份，而非仅凭 `map_presented_to_latest(old_end)` 的起点是 `Some` 就宣告
    /// 整条 run 存活。`OffsetMap` 是半开范围，映射条目末端本来就可能返回 `None`。
    fn find_matching_target_run<'a>(
        target: &'a TargetLayoutDescriptor,
        latest_offset: usize,
        presented_byte_range: (usize, usize),
        revision_chain: &RevisionChain,
    ) -> Option<(usize, &'a TargetGlyphRun)> {
        // 精确映射整个 byte_range 到 latest 坐标。
        // 仅当整个 range 落在同一个映射条目内时才视为存活。
        let (latest_start, latest_end) = revision_chain
            .map_presented_range_to_latest(
                presented_byte_range.0,
                presented_byte_range.1,
            )
            .unwrap_or((latest_offset, latest_offset));

        for (idx, run) in target.glyph_runs.iter().enumerate() {
            // target run 的 byte_range 包含映射后的 range。
            if run.byte_range.0 <= latest_start && run.byte_range.1 >= latest_end {
                return Some((idx, run));
            }
        }
        None
    }

    /// 为存活字生成 FLIP 位移命令。
    ///
    /// 在 old_rect（presented 的画布坐标）和 new_rect（target 的视口坐标）之间
    /// 按 `progress` 插值。透明度保持 1.0（不淡出淡入）。
    fn command_for_surviving(
        contrib: &PixelContribution,
        run: &TargetGlyphRun,
        progress: f64,
        presented: &PresentedScene,
        target: &TargetLayoutDescriptor,
    ) -> Option<DrawCommand> {
        if !contrib.has_visible_area() {
            return None;
        }
        if run.document_rect.w <= 0.0 || run.document_rect.h <= 0.0 {
            return None;
        }

        // old_rect：presented 的 document_rect 经过 transform 映射到画布坐标。
        let old_transformed = Self::transform_rect(contrib.document_rect, presented.transform);
        let old_rect = CanvasRect::new(
            old_transformed.x + presented.position.x,
            old_transformed.y + presented.position.y,
            old_transformed.w,
            old_transformed.h,
        );

        // new_rect：target 的 document_rect 减去滚动偏移 → 视口坐标。
        let new_rect = CanvasRect::new(
            run.document_rect.x - target.scroll.x,
            run.document_rect.y - target.scroll.y,
            run.document_rect.w,
            run.document_rect.h,
        );

        // FLIP：在 old_rect 和 new_rect 之间插值。
        let interpolated = CanvasRect::new(
            old_rect.x + (new_rect.x - old_rect.x) * progress,
            old_rect.y + (new_rect.y - old_rect.y) * progress,
            old_rect.w + (new_rect.w - old_rect.w) * progress,
            old_rect.h + (new_rect.h - old_rect.h) * progress,
        );

        let opacity = contrib.opacity * presented.opacity;
        if opacity <= 0.0 {
            return None;
        }

        // 存活字用 StaticText 标记（不参与淡入淡出，保持完整透明度）。
        // Issue #901 问题4：clip 从 target 坐标系计算（字形已移动到新位置），
        // 用 target.clip 与 interpolated_rect 求交，不是拿旧 visible_clip 与
        // 新 viewport 求交。旧裁剪是旧帧屏幕坐标，字形已移动后这块裁剪
        // 不能原地不变。
        let clip = Self::intersect_clip(target.clip, interpolated);
        Some(DrawCommand::text_fragment(
            run.snapshot_id,
            run.source_uv,
            interpolated,
            clip,
            opacity,
            super::ownership::OwnershipRule::StaticText,
            // Issue #901 问题3：传递真实 byte_range，不硬编码 (0,0)。
            // 存活字用 target run 的 byte_range（映射后的最新坐标）。
            run.byte_range,
        ))
    }

    /// 计算两个裁剪矩形的交集（取交集，保证不扩大可见区域）。
    fn intersect_clip(a: CanvasRect, b: CanvasRect) -> CanvasRect {
        let left = a.x.max(b.x);
        let top = a.y.max(b.y);
        let right = (a.x + a.w).min(b.x + b.w);
        let bottom = (a.y + a.h).min(b.y + b.h);
        if right <= left || bottom <= top {
            CanvasRect::zero()
        } else {
            CanvasRect::new(left, top, right - left, bottom - top)
        }
    }

    /// 从 `PixelContribution` 生成一条文字绘制命令。
    ///
    /// 透明度乘以 `fade` 和 presented 整体透明度。
    fn command_from_contribution(
        contrib: &PixelContribution,
        fade: f64,
        presented: &PresentedScene,
        ownership: super::ownership::OwnershipRule,
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
            contrib.snapshot_id,
            contrib.source_uv,
            target_rect,
            contrib.visible_clip,
            opacity,
            ownership,
            // Issue #901 问题3：传递真实 byte_range，不硬编码 (0,0)。
            contrib.byte_range,
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
            run.snapshot_id,
            run.source_uv,
            viewport_rect,
            target.clip,
            opacity,
            super::ownership::OwnershipRule::AnimatedText,
            // Issue #901 问题3：传递真实 byte_range，不硬编码 (0,0)。
            run.byte_range,
        ))
    }

    /// Issue #901 问题4：为新增字生成裁剪揭示命令。
    ///
    /// 沿文字视觉方向从 0→实际字形可见宽度逐步揭示。
    /// `source_uv` 宽度从 `full_width * progress` 计算，
    /// `target_rect` 宽度也按 `progress` 展开。
    /// **不是 opacity=progress**——用裁剪揭示而非透明度淡入。
    fn command_for_inserting(
        run: &TargetGlyphRun,
        progress: f64,
        target: &TargetLayoutDescriptor,
    ) -> Option<DrawCommand> {
        if run.document_rect.w <= 0.0 || run.document_rect.h <= 0.0 {
            return None;
        }
        if progress <= 0.0 {
            return None;
        }
        // 沿文字方向从 0→full_width 揭示。
        let revealed_uv_w = run.source_uv.w * progress;
        let revealed_doc_w = run.document_rect.w * progress;
        if revealed_uv_w <= 0.0 || revealed_doc_w <= 0.0 {
            return None;
        }
        let source_uv = CanvasRect::new(
            run.source_uv.x,
            run.source_uv.y,
            revealed_uv_w,
            run.source_uv.h,
        );
        let viewport_rect = CanvasRect::new(
            run.document_rect.x - target.scroll.x,
            run.document_rect.y - target.scroll.y,
            revealed_doc_w,
            run.document_rect.h,
        );
        Some(DrawCommand::text_fragment(
            run.snapshot_id,
            source_uv,
            viewport_rect,
            target.clip,
            1.0, // 用裁剪揭示，不用透明度
            super::ownership::OwnershipRule::AnimatedText,
            run.byte_range,
        ))
    }

    /// Issue #901 问题4：为删除字生成裁剪收回命令。
    ///
    /// 沿相反方向从已显示宽度→0 逐步收回。
    /// `source_uv` 宽度从 `displayed_width * (1-progress)` 计算，
    /// `target_rect` 宽度也按 `(1-progress)` 收缩。
    /// **不是 opacity=(1-progress)**——用裁剪收回而非透明度淡出。
    fn command_for_deleting(
        contrib: &PixelContribution,
        progress: f64,
        presented: &PresentedScene,
    ) -> Option<DrawCommand> {
        if !contrib.has_visible_area() {
            return None;
        }
        let fade_out = 1.0 - progress;
        if fade_out <= 0.0 {
            return None;
        }
        // 沿相反方向从已显示宽度→0 收回。
        let remaining_uv_w = contrib.source_uv.w * fade_out;
        let remaining_doc_w = contrib.document_rect.w * fade_out;
        if remaining_uv_w <= 0.0 || remaining_doc_w <= 0.0 {
            return None;
        }
        let source_uv = CanvasRect::new(
            contrib.source_uv.x,
            contrib.source_uv.y,
            remaining_uv_w,
            contrib.source_uv.h,
        );
        let transformed = Self::transform_rect(contrib.document_rect, presented.transform);
        let target_rect = CanvasRect::new(
            transformed.x + presented.position.x,
            transformed.y + presented.position.y,
            remaining_doc_w,
            transformed.h,
        );
        let opacity = contrib.opacity * presented.opacity;
        if opacity <= 0.0 {
            return None;
        }
        Some(DrawCommand::text_fragment(
            contrib.snapshot_id,
            source_uv,
            target_rect,
            contrib.visible_clip,
            opacity,
            super::ownership::OwnershipRule::StaticText,
            contrib.byte_range,
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

    fn identity_chain() -> RevisionChain {
        RevisionChain::new(EditorRevision::initial())
    }

    fn sample_presented(opacity: f64) -> PresentedScene {
        PresentedScene {
            source_revision: EditorRevision::initial(),
            document_session: 1,
            frame_id: 1,
            geometry_id: 1,
            pose_progress: 1.0,
            clip: CanvasRect::new(0.0, 0.0, 800.0, 600.0),
            position: CanvasVec2::new(0.0, 0.0),
            opacity,
            transform: CanvasAffine2::identity(),
            pixel_contributions: vec![PixelContribution {
                snapshot_id: LineSnapshotId::new(1, 0, 0),
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

    fn sample_target() -> TargetLayoutDescriptor {
        TargetLayoutDescriptor {
            layout_revision: LayoutRevision::initial(),
            glyph_runs: vec![TargetGlyphRun {
                snapshot_id: LineSnapshotId::new(2, 0, 0),
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
    fn build_draw_plan_surviving_text_uses_flip() {
        // identity chain → presented byte_range (0,5) 存活 → FLIP 位移。
        let presented = sample_presented(1.0);
        let target = sample_target();
        let chain = identity_chain();
        let plan = SceneTransition::build_draw_plan(
            &presented,
            &target,
            &pose(0.0),
            &chain,
            16.0,
        );

        // progress=0 → 存活字显示 old_rect（presented 位置），标记为 StaticText。
        assert!(!plan.empty);
        let static_cmds: Vec<_> = plan
            .commands
            .iter()
            .filter(|c| c.ownership == OwnershipRule::StaticText)
            .collect();
        assert!(
            !static_cmds.is_empty(),
            "surviving text must be visible as StaticText at progress=0"
        );
        // 存活字不产生 AnimatedText（不淡入淡出）。
        let animated_cmds: Vec<_> = plan
            .commands
            .iter()
            .filter(|c| c.ownership == OwnershipRule::AnimatedText)
            .collect();
        assert!(
            animated_cmds.is_empty(),
            "surviving text must NOT produce AnimatedText (FLIP, not fade)"
        );
    }

    #[test]
    fn build_draw_plan_surviving_text_at_progress_one() {
        // identity chain → 存活字，progress=1 → 显示 new_rect（target 位置）。
        let presented = sample_presented(1.0);
        let target = sample_target();
        let chain = identity_chain();
        let plan = SceneTransition::build_draw_plan(
            &presented,
            &target,
            &pose(1.0),
            &chain,
            16.0,
        );

        assert!(!plan.empty);
        // 存活字始终是 StaticText（FLIP），不变成 AnimatedText。
        let static_cmds: Vec<_> = plan
            .commands
            .iter()
            .filter(|c| c.ownership == OwnershipRule::StaticText)
            .collect();
        assert!(
            !static_cmds.is_empty(),
            "surviving text must be visible as StaticText at progress=1"
        );
    }

    #[test]
    fn build_draw_plan_surviving_text_flip_interpolates_position() {
        // 存活字在 progress=0.5 时位置应在 old_rect 和 new_rect 中间。
        let presented = sample_presented(1.0);
        // target 在不同位置 (200, 100)。
        let target = TargetLayoutDescriptor {
            layout_revision: LayoutRevision::initial(),
            glyph_runs: vec![TargetGlyphRun {
                snapshot_id: LineSnapshotId::new(2, 0, 0),
                source_uv: CanvasRect::new(0.0, 0.0, 100.0, 20.0),
                document_rect: CanvasRect::new(200.0, 100.0, 100.0, 20.0),
                byte_range: (0, 5),
            }],
            caret: None,
            scroll: TargetScroll::default(),
            clip: CanvasRect::new(0.0, 0.0, 800.0, 600.0),
        };
        let chain = identity_chain();
        let plan = SceneTransition::build_draw_plan(
            &presented,
            &target,
            &pose(0.5),
            &chain,
            16.0,
        );

        let static_cmd = plan
            .commands
            .iter()
            .find(|c| c.ownership == OwnershipRule::StaticText)
            .expect("surviving text must be present");
        // old_rect.x=0, new_rect.x=200 → progress=0.5 → x=100。
        assert!(
            (static_cmd.target_rect.x - 100.0).abs() < 1e-9,
            "FLIP must interpolate position: expected x=100, got {}",
            static_cmd.target_rect.x
        );
        // old_rect.y=0, new_rect.y=100 → progress=0.5 → y=50。
        assert!(
            (static_cmd.target_rect.y - 50.0).abs() < 1e-9,
            "FLIP must interpolate position: expected y=50, got {}",
            static_cmd.target_rect.y
        );
    }

    #[test]
    fn build_draw_plan_deleted_text_fades_out() {
        // 删除字：RevisionChain 将 byte_range 映射到 None。
        // Issue #901 问题4：删除字用裁剪收回而非透明度淡出。
        let presented = sample_presented(1.0);
        let target = TargetLayoutDescriptor {
            layout_revision: LayoutRevision::initial(),
            glyph_runs: vec![],
            caret: None,
            scroll: TargetScroll::default(),
            clip: CanvasRect::new(0.0, 0.0, 800.0, 600.0),
        };
        // 构造一个删除映射：空 entries → 所有 offset 映射到 None。
        let mut chain = identity_chain();
        let empty_map = writer_core::editor::OffsetMap {
            entries: Vec::new(),
        };
        chain.compose(&empty_map, EditorRevision::new(1));

        let plan = SceneTransition::build_draw_plan(
            &presented,
            &target,
            &pose(0.5),
            &chain,
            16.0,
        );

        // 删除字在 progress=0.5 时用裁剪收回：
        // source_uv 宽度 = 100 * (1 - 0.5) = 50，opacity 保持 1.0。
        let static_cmd = plan
            .commands
            .iter()
            .find(|c| c.ownership == OwnershipRule::StaticText)
            .expect("deleted text must be present as StaticText clipping out");
        assert!(
            (static_cmd.opacity - 1.0).abs() < 1e-9,
            "deleted text opacity must be 1.0 (clip-based, not fade), got {}",
            static_cmd.opacity
        );
        assert!(
            (static_cmd.source_uv.w - 50.0).abs() < 1e-9,
            "deleted text source_uv width must be 50 (100 * (1-0.5)), got {}",
            static_cmd.source_uv.w
        );
    }

    #[test]
    fn build_draw_plan_new_text_fades_in() {
        // 新增字：target 有 glyph run，presented 无对应贡献。
        // Issue #901 问题4：新增字用裁剪揭示而非透明度淡入。
        let presented = PresentedScene {
            source_revision: EditorRevision::initial(),
            document_session: 1,
            frame_id: 1,
            geometry_id: 1,
            pose_progress: 1.0,
            clip: CanvasRect::new(0.0, 0.0, 800.0, 600.0),
            position: CanvasVec2::new(0.0, 0.0),
            opacity: 1.0,
            transform: CanvasAffine2::identity(),
            pixel_contributions: vec![],
            caret: None,
            scroll: PresentedScroll::default(),
        };
        let target = sample_target();
        let chain = identity_chain();
        let plan = SceneTransition::build_draw_plan(
            &presented,
            &target,
            &pose(0.5),
            &chain,
            16.0,
        );

        // 新增字在 progress=0.5 时用裁剪揭示：
        // source_uv 宽度 = 100 * 0.5 = 50，opacity 保持 1.0。
        let animated_cmd = plan
            .commands
            .iter()
            .find(|c| c.ownership == OwnershipRule::AnimatedText)
            .expect("new text must be present as AnimatedText clipping in");
        assert!(
            (animated_cmd.opacity - 1.0).abs() < 1e-9,
            "new text opacity must be 1.0 (clip-based, not fade), got {}",
            animated_cmd.opacity
        );
        assert!(
            (animated_cmd.source_uv.w - 50.0).abs() < 1e-9,
            "new text source_uv width must be 50 (100 * 0.5), got {}",
            animated_cmd.source_uv.w
        );
    }

    #[test]
    fn build_draw_plan_empty_presented_and_target() {
        let presented = PresentedScene {
            source_revision: EditorRevision::initial(),
            document_session: 1,
            frame_id: 1,
            geometry_id: 1,
            pose_progress: 1.0,
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
        let chain = identity_chain();
        let plan = SceneTransition::build_draw_plan(
            &presented,
            &target,
            &pose(0.5),
            &chain,
            16.0,
        );
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
        let chain = identity_chain();
        let plan = SceneTransition::build_draw_plan(
            &presented,
            &target,
            &pose(0.0),
            &chain,
            16.0,
        );

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
                snapshot_id: LineSnapshotId::new(1, 0, 0),
                source_uv: CanvasRect::new(0.0, 0.0, 100.0, 20.0),
                document_rect: CanvasRect::new(100.0, 200.0, 100.0, 20.0),
                byte_range: (0, 5),
            }],
            caret: None,
            scroll: TargetScroll { x: 10.0, y: 50.0 },
            clip: CanvasRect::new(0.0, 0.0, 800.0, 600.0),
        };
        let presented = sample_presented(0.0); // 透明，不产生命令
        let chain = identity_chain();
        let plan = SceneTransition::build_draw_plan(
            &presented,
            &target,
            &pose(1.0),
            &chain,
            16.0,
        );

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
