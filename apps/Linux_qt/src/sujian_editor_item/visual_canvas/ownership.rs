//! Issue #901：像素归属与互斥。
//!
//! `Ownership` 确保同一像素只被一个**需要互斥**的绘制命令拥有。当多个
//! `DrawCommand` 的目标矩形重叠时，按 `OwnershipRule` 优先级解决：高优先级
//! 互斥命令保留完整区域，低优先级命令的裁剪矩形减去高优先级命令覆盖的区域。
//!
//! ## 优先级规则
//!
//! - 光标（`Cursor`）在文字之上，优先级最高。
//! - 预输入（`Preedit`）在正文之上、光标之下。
//! - 选区背景（`Selection`）在正文之下，优先级最低。
//! - 静态正文（`StaticText`）和动画过渡（`AnimatedText`）**不互斥**：
//!   各自以自己的透明度绘制（alpha compositing），不做矩形裁剪。
//!
//! ## Issue #901 阻塞 2 修复
//!
//! 旧实现把 `StaticText` 标为高优先级、`AnimatedText` 标为低优先级，重叠时
//! 高优先级旧矩形裁掉低优先级新矩形——同一位置 A→B 时新 B 被完全裁掉，既
//! 不是吞字吐字也不是平滑 CrossFade。新实现让文字层之间用透明度叠加，只有
//! `Cursor`/`Preedit`/`Selection` 这些真正需要互斥的层才做矩形裁剪。
//!
//! ## Issue #901 问题4 复核
//!
//! 确认：静态、移动和新/旧图形的真实绘制互斥关系正确。
//! - `StaticText`（存活字/删除字）和 `AnimatedText`（新增字）**不互斥**：
//!   各自以自己的透明度绘制（alpha compositing），不做矩形裁剪。
//!   不能用"旧半透明矩形占据了就彻底禁止画新字"表达淡入淡出。
//! - 只有 `Cursor`/`Preedit`/`Selection` 做矩形裁剪（`is_exclusive = true`）。
//! - 文字层之间的叠加通过 `opacity` 字段实现，裁剪揭示/收回通过 `source_uv`
//!   和 `clip_rect` 的宽度收缩实现（见 `SceneTransition::command_for_inserting`
//!   和 `command_for_deleting`），不通过 ownership 互斥。
//!
//! ## 纯 Rust
//!
//! 本模块只做矩形几何运算，不依赖 Qt 类型。

use super::draw_plan::DrawCommand;
use super::presented_scene::CanvasRect;

/// 像素归属规则 —— 决定重叠时哪个命令优先拥有该像素。
///
/// 优先级从高到低：`Cursor` > `Preedit` > `StaticText` > `AnimatedText` > `Selection`。
///
/// Issue #901 阻塞 2：`StaticText` 和 `AnimatedText` **不互斥**（`is_exclusive`
/// 返回 `false`），各自以透明度绘制（alpha compositing）。只有 `Cursor`、
/// `Preedit`、`Selection` 是互斥层（`is_exclusive` 返回 `true`），占据区域
/// 并裁剪后续命令。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum OwnershipRule {
    /// 静态正文 —— 已稳定呈现、不参与过渡的文字。不互斥。
    StaticText,
    /// 动画过渡中的文字 —— 正在进入/退出的文字。不互斥。
    AnimatedText,
    /// 光标 —— 视觉最顶层。互斥。
    Cursor,
    /// 选区背景 —— 视觉最底层。互斥。
    Selection,
    /// 预输入（IME preedit）—— 正文之上、光标之下。互斥。
    Preedit,
}

impl OwnershipRule {
    /// 返回优先级数值，数值越大优先级越高。
    ///
    /// 优先级：`Cursor(100)` > `Preedit(80)` > `StaticText(60)` >
    /// `AnimatedText(40)` > `Selection(20)`。
    pub(crate) fn priority(self) -> u8 {
        match self {
            Self::Cursor => 100,
            Self::Preedit => 80,
            Self::StaticText => 60,
            Self::AnimatedText => 40,
            Self::Selection => 20,
        }
    }

    /// 该规则是否为**互斥层** —— 占据区域并裁剪后续命令。
    ///
    /// Issue #901 阻塞 2：`StaticText` 和 `AnimatedText` 返回 `false`，
    /// 它们各自以透明度绘制（alpha compositing），不做矩形裁剪互斥。
    /// `Cursor`、`Preedit`、`Selection` 返回 `true`，占据区域并裁剪后续命令。
    pub(crate) fn is_exclusive(self) -> bool {
        match self {
            Self::Cursor | Self::Preedit | Self::Selection => true,
            Self::StaticText | Self::AnimatedText => false,
        }
    }
}

/// 像素互斥管理器 —— 解决绘制命令间的重叠。
///
/// `resolve` 接收一组 `DrawCommand`，按优先级从高到低处理：
/// - **互斥层**（`Cursor`/`Preedit`/`Selection`）的目标矩形"占据"其覆盖的像素，
///   后续命令的裁剪矩形减去已被占据的区域。
/// - **文字层**（`StaticText`/`AnimatedText`）**不占据区域**，各自以自己的
///   透明度绘制（alpha compositing），不被其他文字层裁剪。
///
/// Issue #901 阻塞 2：旧实现让 `StaticText` 占据区域裁掉 `AnimatedText`，
/// 导致同一位置 A→B 时新 B 被完全裁掉。新实现让文字层之间用透明度叠加。
pub(crate) struct Ownership;

impl Ownership {
    /// 解决重叠：返回裁剪后的绘制命令列表。
    ///
    /// 算法：
    /// 1. 按优先级从高到低排序（稳定排序，同优先级保持原顺序）。
    /// 2. 维护一个"已占据区域"列表（仅互斥层命令的目标矩形加入）。
    /// 3. 对每条命令，将其 `clip_rect` 减去所有已占据区域，得到剩余裁剪。
    /// 4. 如果该命令是互斥层（`is_exclusive`），将其原始目标矩形加入已占据区域。
    /// 5. 按原始顺序返回结果。
    ///
    /// 透明度为 0 或裁剪面积为零的命令被过滤掉。
    ///
    /// Issue #901 阻塞 2：文字层（`StaticText`/`AnimatedText`）不占据区域，
    /// 不裁剪彼此，各自以透明度绘制。
    pub(crate) fn resolve(commands: &[DrawCommand]) -> Vec<DrawCommand> {
        if commands.is_empty() {
            return Vec::new();
        }

        // 带原始索引的排序视图：按优先级降序（稳定）。
        let mut indexed: Vec<(usize, &DrawCommand)> =
            commands.iter().enumerate().collect();
        indexed.sort_by(|a, b| {
            b.1.ownership
                .priority()
                .cmp(&a.1.ownership.priority())
                .then(a.0.cmp(&b.0))
        });

        // 已占据的矩形列表（仅互斥层命令的目标矩形）。
        let mut occupied: Vec<CanvasRect> = Vec::new();

        // 存放 (原始索引, 裁剪后的命令)。
        let mut resolved: Vec<(usize, DrawCommand)> = Vec::new();

        for (original_idx, cmd) in indexed {
            // 跳过完全透明的命令。
            if cmd.opacity <= 0.0 {
                continue;
            }

            // 将 clip_rect 减去所有已占据区域。
            let remaining_clips = subtract_rect(&cmd.clip_rect, &occupied);

            for rc in remaining_clips {
                if rc.w > 0.0 && rc.h > 0.0 {
                    let mut clipped = cmd.clone();
                    clipped.clip_rect = rc;
                    resolved.push((original_idx, clipped));
                }
            }

            // 只有互斥层命令的目标矩形加入已占据区域。
            // 文字层（StaticText/AnimatedText）不占据区域，不裁剪后续命令。
            if cmd.ownership.is_exclusive()
                && cmd.target_rect.w > 0.0
                && cmd.target_rect.h > 0.0
            {
                occupied.push(cmd.target_rect);
            }
        }

        // 按原始顺序返回。
        resolved.sort_by_key(|(idx, _)| *idx);
        resolved.into_iter().map(|(_, cmd)| cmd).collect()
    }
}

// ── 矩形差集运算 ──

/// 计算 `rect` 减去 `holes` 中所有矩形后的剩余区域列表。
///
/// 逐个用 `hole` 裁剪 `rect`：每一步把当前碎片列表中的每个矩形减去 `hole`，
/// 得到更小的碎片。最终返回所有非零面积碎片。
fn subtract_rect(rect: &CanvasRect, holes: &[CanvasRect]) -> Vec<CanvasRect> {
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return Vec::new();
    }

    let mut fragments = vec![*rect];

    for hole in holes {
        if hole.w <= 0.0 || hole.h <= 0.0 {
            continue;
        }
        let mut next = Vec::new();
        for frag in &fragments {
            next.extend(subtract_one(frag, hole));
        }
        fragments = next;
        if fragments.is_empty() {
            break;
        }
    }

    fragments
}

/// 计算 `a` 减去 `b` 后的剩余区域。
///
/// 如果 `a` 和 `b` 不相交，返回 `[a]`。如果 `b` 完全覆盖 `a`，返回空。
/// 否则返回最多 4 个矩形碎片（上、下、左、右）。
fn subtract_one(a: &CanvasRect, b: &CanvasRect) -> Vec<CanvasRect> {
    let a_right = a.x + a.w;
    let a_bottom = a.y + a.h;
    let b_right = b.x + b.w;
    let b_bottom = b.y + b.h;

    // 不相交。
    let inter_left = a.x.max(b.x);
    let inter_top = a.y.max(b.y);
    let inter_right = a_right.min(b_right);
    let inter_bottom = a_bottom.min(b_bottom);
    if inter_left >= inter_right || inter_top >= inter_bottom {
        return vec![*a];
    }

    let mut result = Vec::new();

    // 上方碎片：a.y .. inter_top
    if inter_top > a.y {
        result.push(CanvasRect::new(a.x, a.y, a.w, inter_top - a.y));
    }
    // 下方碎片：inter_bottom .. a_bottom
    if a_bottom > inter_bottom {
        result.push(CanvasRect::new(
            a.x,
            inter_bottom,
            a.w,
            a_bottom - inter_bottom,
        ));
    }
    // 左方碎片：a.x .. inter_left（高度为交集高度）
    if inter_left > a.x {
        result.push(CanvasRect::new(
            a.x,
            inter_top,
            inter_left - a.x,
            inter_bottom - inter_top,
        ));
    }
    // 右方碎片：inter_right .. a_right
    if a_right > inter_right {
        result.push(CanvasRect::new(
            inter_right,
            inter_top,
            a_right - inter_right,
            inter_bottom - inter_top,
        ));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::draw_plan::DrawCommandKind;
    use super::super::presented_scene::CanvasRect;
    use crate::sujian_editor_item::snapshot_id::LineSnapshotId;

    fn text_cmd(
        x: f64,
        y: f64,
        w: f64,
        h: f64,
        ownership: OwnershipRule,
    ) -> DrawCommand {
        DrawCommand {
            kind: DrawCommandKind::TextFragment,
            snapshot_id: LineSnapshotId::new(1, 0, 0),
            source_uv: CanvasRect::new(0.0, 0.0, w, h),
            target_rect: CanvasRect::new(x, y, w, h),
            clip_rect: CanvasRect::new(x, y, w, h),
            opacity: 1.0,
            ownership,
            byte_range: (0, 0),
        }
    }

    #[test]
    fn ownership_rule_priority_ordering() {
        assert!(OwnershipRule::Cursor.priority() > OwnershipRule::Preedit.priority());
        assert!(OwnershipRule::Preedit.priority() > OwnershipRule::StaticText.priority());
        assert!(OwnershipRule::StaticText.priority() > OwnershipRule::AnimatedText.priority());
        assert!(OwnershipRule::AnimatedText.priority() > OwnershipRule::Selection.priority());
    }

    #[test]
    fn resolve_empty_input() {
        let result = Ownership::resolve(&[]);
        assert!(result.is_empty());
    }

    #[test]
    fn resolve_no_overlap_preserves_all() {
        let cmds = vec![
            text_cmd(0.0, 0.0, 10.0, 10.0, OwnershipRule::StaticText),
            text_cmd(20.0, 0.0, 10.0, 10.0, OwnershipRule::StaticText),
        ];
        let result = Ownership::resolve(&cmds);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn resolve_static_text_and_animated_coexist() {
        // Issue #901 阻塞 2：静态正文和动画文字重叠时不再互斥裁剪，
        // 各自以自己的透明度绘制（alpha compositing）。
        let cmds = vec![
            text_cmd(0.0, 0.0, 10.0, 10.0, OwnershipRule::AnimatedText),
            text_cmd(0.0, 0.0, 10.0, 10.0, OwnershipRule::StaticText),
        ];
        let result = Ownership::resolve(&cmds);

        // 两者都保留完整区域。
        let static_cmd = result
            .iter()
            .find(|c| c.ownership == OwnershipRule::StaticText)
            .expect("static text must survive");
        assert_eq!(static_cmd.clip_rect, CanvasRect::new(0.0, 0.0, 10.0, 10.0));

        let animated_cmd = result
            .iter()
            .find(|c| c.ownership == OwnershipRule::AnimatedText)
            .expect("animated text must also survive");
        assert_eq!(
            animated_cmd.clip_rect,
            CanvasRect::new(0.0, 0.0, 10.0, 10.0),
            "animated text must NOT be clipped by static text (alpha compositing)"
        );
    }

    #[test]
    fn resolve_partial_overlap_text_coexist() {
        // Issue #901 阻塞 2：部分重叠的文字层也各自完整绘制。
        let cmds = vec![
            text_cmd(5.0, 0.0, 10.0, 10.0, OwnershipRule::AnimatedText),
            text_cmd(0.0, 0.0, 10.0, 10.0, OwnershipRule::StaticText),
        ];
        let result = Ownership::resolve(&cmds);

        // 两者都保留完整裁剪区域。
        let static_cmd = result
            .iter()
            .find(|c| c.ownership == OwnershipRule::StaticText)
            .expect("static text must survive");
        assert_eq!(static_cmd.clip_rect.w, 10.0);

        let animated_cmds: Vec<_> = result
            .iter()
            .filter(|c| c.ownership == OwnershipRule::AnimatedText)
            .collect();
        assert!(!animated_cmds.is_empty(), "animated text fully visible");
        // AnimatedText 不被 StaticText 裁剪，保留完整 10x10。
        let total_area: f64 = animated_cmds
            .iter()
            .map(|c| c.clip_rect.w * c.clip_rect.h)
            .sum();
        assert!(
            (total_area - 100.0).abs() < 1e-9,
            "animated text area should be 100 (not clipped by static text), got {total_area}"
        );
    }

    #[test]
    fn resolve_cursor_clips_text() {
        // Issue #901 阻塞 2：光标（互斥层）仍然裁剪文字。
        let cmds = vec![
            text_cmd(0.0, 0.0, 10.0, 10.0, OwnershipRule::StaticText),
            text_cmd(0.0, 0.0, 2.0, 10.0, OwnershipRule::Cursor),
        ];
        let result = Ownership::resolve(&cmds);

        // 光标保留完整。
        let cursor_cmd = result
            .iter()
            .find(|c| c.ownership == OwnershipRule::Cursor)
            .expect("cursor must survive");
        assert_eq!(cursor_cmd.clip_rect, CanvasRect::new(0.0, 0.0, 2.0, 10.0));

        // 文字被光标裁剪：剩余 [2,0,8,10]。
        let text_cmds: Vec<_> = result
            .iter()
            .filter(|c| c.ownership == OwnershipRule::StaticText)
            .collect();
        assert!(!text_cmds.is_empty(), "text partially visible after cursor");
        let total_area: f64 = text_cmds.iter().map(|c| c.clip_rect.w * c.clip_rect.h).sum();
        assert!(
            (total_area - 80.0).abs() < 1e-9,
            "text remaining area should be 80 (clipped by cursor), got {total_area}"
        );
    }

    #[test]
    fn resolve_filters_zero_opacity() {
        let mut cmd = text_cmd(0.0, 0.0, 10.0, 10.0, OwnershipRule::StaticText);
        cmd.opacity = 0.0;
        let result = Ownership::resolve(&[cmd]);
        assert!(result.is_empty(), "zero opacity command should be filtered");
    }

    #[test]
    fn resolve_preserves_original_order_for_equal_priority() {
        let cmds = vec![
            text_cmd(0.0, 0.0, 10.0, 10.0, OwnershipRule::StaticText),
            text_cmd(20.0, 0.0, 10.0, 10.0, OwnershipRule::StaticText),
        ];
        let result = Ownership::resolve(&cmds);
        assert_eq!(result[0].target_rect.x, 0.0);
        assert_eq!(result[1].target_rect.x, 20.0);
    }

    #[test]
    fn subtract_one_disjoint() {
        let a = CanvasRect::new(0.0, 0.0, 10.0, 10.0);
        let b = CanvasRect::new(20.0, 0.0, 10.0, 10.0);
        let result = subtract_one(&a, &b);
        assert_eq!(result, vec![a]);
    }

    #[test]
    fn subtract_one_full_cover() {
        let a = CanvasRect::new(0.0, 0.0, 10.0, 10.0);
        let b = CanvasRect::new(-1.0, -1.0, 20.0, 20.0);
        let result = subtract_one(&a, &b);
        assert!(result.is_empty());
    }
}
