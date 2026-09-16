//! #694 评论 5693077441：本地输入视觉计划纯计算 API — affected-slice 版本。
//!
//! Core 对 `(old_slices, new_slices)` 做视觉分类，返回 `animation_mode` 和
//! `old/n_animation units`（按 grapheme cluster / run / reflow 粒度）。
//!
//! 每个 slice 只携带本次输入的 affected substring + 它在正文中的绝对 UTF-8 byte 起点，
//! 不再把整章正文送过 FFI 做视觉分类。这样长章节里每笔普通输入不会因为整章含换行
//! 或整章超过 8 个 grapheme cluster 而被误判成 `LineReflowAnimation` / `RunAnimation`。
//!
//! Android `ComposeLocalVisualRebase` / `ComposeEditorVisualState.buildLocalInputPatch` 复用此 API，
//! 不再按 UTF-16 +1 硬切或硬编码 `CLUSTER_ANIMATION`。
//!
//! UDL 中已声明函数签名，此处只提供实现，不加 `#[::uniffi::export]` 宏
//! （UDL scaffolding 已生成绑定，加宏会重复定义符号）。

use crate::api::types::{
    AnimationModeDto, EditorByteRangeDto, LocalVisualPlanDto, LocalVisualSliceDto,
};
use crate::editor::strong_types::Utf8ByteRange;
use crate::editor::transaction::{
    choose_animation_mode, compute_animation_units_from_slices, AnimationTextSlice,
};
use crate::editor::{count_grapheme_clusters, text_contains_complex_grapheme};

/// #694 评论 5693077441：本地输入视觉计划纯计算入口 — affected-slice 版本。
///
/// 输入：`old_slices` / `new_slices`（每个 slice = affected substring + 绝对 UTF-8 byte 起点）、
/// `animation_enabled`（是否启用动画）。
///
/// 输出：[`LocalVisualPlanDto`] — animation_mode + old/new animation units。
///
/// 算法：
/// 1. 选 classify_slices：优先 `new_slices`（insert/replace），为空才看 `old_slices`（delete）。
/// 2. 对 classify_slices 内每个 slice 的 `text` 算 cluster_count / contains_newline /
///    contains_complex_grapheme，聚合（sum / any / any）。
/// 3. `choose_animation_mode(...)` 选模式。
/// 4. 把 DTO slice 转成 Core `AnimationTextSlice`，调 `compute_animation_units_from_slices`
///    按 mode 生成 old/n_animation units。affected ranges 从 slice 的
///    `absolute_start + text.len()` 构造（给 LineReflow/Snapshot 用）。
///
/// 无 session/revision/事务副作用，纯函数。不再消费整章正文，
/// 也不会因为整章含换行或整章 > 8 cluster 而误判。
pub fn classify_local_visual_plan(
    old_slices: Vec<LocalVisualSliceDto>,
    new_slices: Vec<LocalVisualSliceDto>,
    animation_enabled: bool,
) -> LocalVisualPlanDto {
    // #694 评论 5693077441：用 affected slice 做视觉分类，不再用整章正文。
    // 与现有 apply_insert/apply_delete/apply_replace 语义一致：
    // 优先用 new_slices（insert/replace），为空才看 old_slices（delete）。
    let classify_slices = if !new_slices.is_empty() {
        &new_slices
    } else {
        &old_slices
    };

    let cluster_count = classify_slices
        .iter()
        .map(|slice| count_grapheme_clusters(&slice.text))
        .sum();

    let contains_newline = classify_slices
        .iter()
        .any(|slice| slice.text.contains('\n'));

    let contains_complex = classify_slices
        .iter()
        .any(|slice| text_contains_complex_grapheme(&slice.text));

    let animation_mode = choose_animation_mode(
        cluster_count,
        contains_newline,
        contains_complex,
        false,
        false,
        false,
        false,
        animation_enabled,
    );

    // 把 DTO slice 转成 Core AnimationTextSlice，调现有 compute_animation_units_from_slices。
    let old_core_slices: Vec<AnimationTextSlice<'_>> = old_slices
        .iter()
        .map(|s| AnimationTextSlice {
            absolute_start: s.absolute_start as usize,
            text: s.text.as_str(),
        })
        .collect();
    let new_core_slices: Vec<AnimationTextSlice<'_>> = new_slices
        .iter()
        .map(|s| AnimationTextSlice {
            absolute_start: s.absolute_start as usize,
            text: s.text.as_str(),
        })
        .collect();

    // affected ranges 从 slice 的 absolute_start + text.len() 构造（给 LineReflow/Snapshot 用）。
    let old_affected: Vec<Utf8ByteRange> = old_slices
        .iter()
        .map(|s| {
            Utf8ByteRange::from_ordered(
                s.absolute_start as usize,
                s.absolute_start as usize + s.text.len(),
            )
        })
        .collect();
    let new_affected: Vec<Utf8ByteRange> = new_slices
        .iter()
        .map(|s| {
            Utf8ByteRange::from_ordered(
                s.absolute_start as usize,
                s.absolute_start as usize + s.text.len(),
            )
        })
        .collect();

    let (old_units, new_units) = compute_animation_units_from_slices(
        animation_mode,
        &old_core_slices,
        &new_core_slices,
        &old_affected,
        &new_affected,
    );

    LocalVisualPlanDto {
        animation_mode: AnimationModeDto::from(animation_mode),
        old_animation_units: old_units
            .into_iter()
            .map(EditorByteRangeDto::from)
            .collect(),
        new_animation_units: new_units
            .into_iter()
            .map(EditorByteRangeDto::from)
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::AnimationModeDto;

    fn range(start: u32, end: u32) -> EditorByteRangeDto {
        EditorByteRangeDto {
            start,
            end_exclusive: end,
        }
    }

    fn slice(absolute_start: u32, text: &str) -> LocalVisualSliceDto {
        LocalVisualSliceDto {
            absolute_start,
            text: text.to_string(),
        }
    }

    #[test]
    fn classifies_empty_to_we_two_clusters() {
        // "" -> "我们"（2 个汉字 = 2 个 grapheme cluster）
        let plan = classify_local_visual_plan(vec![], vec![slice(0, "我们")], true);
        // 2 cluster <= 8 -> GlyphAnimation
        assert_eq!(plan.animation_mode, AnimationModeDto::GlyphAnimation);
        // 2 个 unit: [0,3) 和 [3,6)
        assert_eq!(plan.new_animation_units.len(), 2);
        assert_eq!(plan.new_animation_units[0], range(0, 3));
        assert_eq!(plan.new_animation_units[1], range(3, 6));
    }

    #[test]
    fn classifies_empty_to_abc_three_clusters_glyph_animation() {
        // "" -> "abc"（3 cluster <= 8 -> GlyphAnimation）
        let plan = classify_local_visual_plan(vec![], vec![slice(0, "abc")], true);
        assert_eq!(plan.animation_mode, AnimationModeDto::GlyphAnimation);
        assert_eq!(plan.new_animation_units.len(), 3);
    }

    #[test]
    fn classifies_empty_to_emoji_family_one_cluster() {
        // "" -> "👨‍👩‍👧‍👦"（emoji family 是 1 个 grapheme cluster）
        let emoji = "👨‍👩‍👧‍👦";
        let plan = classify_local_visual_plan(vec![], vec![slice(0, emoji)], true);
        // 1 cluster，但含复杂 grapheme（ZWJ + emoji）-> ClusterAnimation
        assert_eq!(plan.animation_mode, AnimationModeDto::ClusterAnimation);
        // 1 个 unit（整个 emoji family）
        assert_eq!(plan.new_animation_units.len(), 1);
    }

    #[test]
    fn classifies_zero_clusters_to_system_suppressed() {
        // "" -> ""（0 cluster -> SystemSuppressed）
        let plan = classify_local_visual_plan(vec![], vec![], true);
        assert_eq!(plan.animation_mode, AnimationModeDto::SystemSuppressed);
        assert!(plan.new_animation_units.is_empty());
    }

    #[test]
    fn classifies_animation_disabled_to_system_suppressed() {
        let plan = classify_local_visual_plan(vec![], vec![slice(0, "abc")], false);
        assert_eq!(plan.animation_mode, AnimationModeDto::SystemSuppressed);
        assert!(plan.new_animation_units.is_empty());
    }

    #[test]
    // SAFETY: long_text 是测试常量，长度远小于 u32::MAX，截断安全。
    #[allow(clippy::cast_possible_truncation)]
    fn classifies_long_text_with_newline_append_we_two_clusters() {
        // #694 评论 5693077441 问题1 回归：
        // 已有几十字且包含换行的正文，末尾一次输入"我们"，
        // 应按本次 affected text（"我们"）判定，得 GlyphAnimation + 2 个 cluster，
        // 而非因整章含换行被判成 LineReflowAnimation 或因整章 > 8 cluster 被判成 RunAnimation。
        let long_text = "第一章\n\n这是正文的第一段，已经有不少字了。\n\n第二段也有些内容。";
        let append_text = "我们";
        let append_start = long_text.len() as u32; // UTF-8 byte offset
        let plan = classify_local_visual_plan(
            vec![], // old_slices：本次是纯插入，old 无 affected
            vec![slice(append_start, append_text)],
            true,
        );
        // "我们" = 2 cluster <= 8，不含换行，不含复杂 grapheme -> GlyphAnimation
        assert_eq!(
            plan.animation_mode,
            AnimationModeDto::GlyphAnimation,
            "长正文末尾输入\"我们\"应按 affected text 判定为 GlyphAnimation，\
             不应因整章含换行被判成 LineReflowAnimation 或因整章 > 8 cluster 被判成 RunAnimation"
        );
        // 2 个 cluster unit
        assert_eq!(
            plan.new_animation_units.len(),
            2,
            "应按\"我们\"的 2 个 grapheme cluster 拆成 2 个 unit"
        );
        // unit 的绝对 byte offset 应在 append_start 之后
        let first_unit_start = plan.new_animation_units[0].start as usize;
        assert!(
            first_unit_start >= append_start as usize,
            "第一个 unit 应在 append_start ({}) 之后，实际 start={first_unit_start}",
            append_start
        );
    }

    #[test]
    // SAFETY: long_text 是测试常量，长度远小于 u32::MAX，截断安全。
    #[allow(clippy::cast_possible_truncation)]
    fn classifies_long_text_with_newline_append_single_char_glyph_animation() {
        // 正文含换行，末尾输入单个"我"，应按 affected text "我" 判定为 GlyphAnimation，
        // 不应因整章含换行被判成 LineReflowAnimation。
        let long_text = "标题\n\n正文内容已经有很多字了，超过八个 grapheme cluster。";
        let plan =
            classify_local_visual_plan(vec![], vec![slice(long_text.len() as u32, "我")], true);
        assert_eq!(plan.animation_mode, AnimationModeDto::GlyphAnimation);
        assert_eq!(plan.new_animation_units.len(), 1);
    }

    #[test]
    fn classifies_delete_newline_slice_line_reflow() {
        // 删除一段含换行的文字，应按 deleted slice 判定为 LineReflowAnimation。
        let plan = classify_local_visual_plan(vec![slice(0, "abc\ndef")], vec![], true);
        assert_eq!(plan.animation_mode, AnimationModeDto::LineReflowAnimation);
    }
}
