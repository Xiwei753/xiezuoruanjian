//! #694 评论 5692161955：本地输入视觉计划纯计算 API —
//!
//! Core 对 `(old_text, new_text, old_affected, new_affected)` 做视觉分类，
//! 返回 `animation_mode` 和 `old/new animation units`（按 grapheme cluster / run / reflow 粒度）。
//!
//! Android `ComposeLocalVisualRebase` / `ComposeEditorVisualState.buildLocalInputPatch` 复用此 API，
//! 不再按 UTF-16 +1 硬切或硬编码 `CLUSTER_ANIMATION`。
//!
//! UDL 中已声明函数签名，此处只提供实现，不加 `#[::uniffi::export]` 宏
//! （UDL scaffolding 已生成绑定，加宏会重复定义符号）。

use crate::api::error::WriterError;
use crate::api::types::{AnimationModeDto, EditorByteRangeDto, LocalVisualPlanDto};
use crate::editor::strong_types::Utf8ByteRange;
use crate::editor::transaction::{
    choose_animation_mode, compute_animation_units_from_slices, AnimationTextSlice,
};
use crate::editor::{count_grapheme_clusters, text_contains_complex_grapheme};

/// #694 评论 5692161955：本地输入视觉计划纯计算入口。
///
/// 输入：old_text / new_text（UTF-8 字符串）、old_affected / new_affected（UTF-8 byte ranges）、
/// animation_enabled（是否启用动画）。
///
/// 输出：[`LocalVisualPlanDto`] — animation_mode + old/new animation units。
///
/// 算法：
/// 1. 取 changed_text = max(len(new_text), len(old_text)) 对应的文本（与 composition 分类一致）。
/// 2. 算 cluster_count / contains_newline / contains_complex_grapheme。
/// 3. `choose_animation_mode(...)` 选模式。
/// 4. `compute_animation_units_from_slices(...)` 按 mode 生成 old/new animation units。
///
/// 无 session/revision/事务副作用，纯函数。外部输入（byte range 越界）返回 `WriterError`，
/// 不用 `unwrap`/`expect`。
pub fn classify_local_visual_plan(
    old_text: String,
    new_text: String,
    old_affected_byte_ranges: Vec<EditorByteRangeDto>,
    new_affected_byte_ranges: Vec<EditorByteRangeDto>,
    animation_enabled: bool,
) -> std::result::Result<LocalVisualPlanDto, WriterError> {
    // 把 DTO byte ranges 转成 Core Utf8ByteRange，同时验证 char boundary。
    let old_affected = dto_ranges_to_core(&old_affected_byte_ranges, &old_text, "old_text")?;
    let new_affected = dto_ranges_to_core(&new_affected_byte_ranges, &new_text, "new_text")?;

    // 与 composition 分类一致：取较长的文本做 cluster/complex 判定。
    let changed_text = if new_text.len() >= old_text.len() {
        new_text.as_str()
    } else {
        old_text.as_str()
    };
    let cluster_count = count_grapheme_clusters(changed_text);
    let contains_newline = changed_text.contains('\n');
    let contains_complex = text_contains_complex_grapheme(changed_text);

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

    // 构建 AnimationTextSlice：对每个 affected range，从对应文本截取局部文本。
    // 与 visual_classification.rs::build_slices_from_text 同策略：
    // - range.start >= text.len()：局部文本模式，slice = (range.start, text)。
    // - range.end <= text.len() 且 char boundary：全文模式，slice = (range.start, &text[start..end])。
    // - 否则（mixed 或越界）：按局部文本处理，slice = (range.start, text)。
    let old_slices = build_slices_from_text(&old_text, &old_affected);
    let new_slices = build_slices_from_text(&new_text, &new_affected);

    let (old_units, new_units) = compute_animation_units_from_slices(
        animation_mode,
        &old_slices,
        &new_slices,
        &old_affected,
        &new_affected,
    );

    Ok(LocalVisualPlanDto {
        animation_mode: AnimationModeDto::from(animation_mode),
        old_animation_units: old_units.into_iter().map(EditorByteRangeDto::from).collect(),
        new_animation_units: new_units.into_iter().map(EditorByteRangeDto::from).collect(),
    })
}

/// 把 DTO byte ranges 转成 Core `Utf8ByteRange`，验证 char boundary 与越界。
fn dto_ranges_to_core(
    ranges: &[EditorByteRangeDto],
    text: &str,
    text_name: &str,
) -> std::result::Result<Vec<Utf8ByteRange>, WriterError> {
    let mut result = Vec::with_capacity(ranges.len());
    for (i, r) in ranges.iter().enumerate() {
        let start = r.start as usize;
        let end = r.end_exclusive as usize;
        if start > end {
            return Err(WriterError::Other(format!(
                "classify_local_visual_plan: {text_name} byte range [{i}] start ({start}) > end_exclusive ({end})"
            )));
        }
        if start > text.len() {
            return Err(WriterError::Other(format!(
                "classify_local_visual_plan: {text_name} byte range [{i}] start ({start}) > text.len() ({})",
                text.len()
            )));
        }
        if end > text.len() {
            return Err(WriterError::Other(format!(
                "classify_local_visual_plan: {text_name} byte range [{i}] end_exclusive ({end}) > text.len() ({})",
                text.len()
            )));
        }
        if !text.is_char_boundary(start) {
            return Err(WriterError::Other(format!(
                "classify_local_visual_plan: {text_name} byte range [{i}] start ({start}) is not a UTF-8 char boundary"
            )));
        }
        if !text.is_char_boundary(end) {
            return Err(WriterError::Other(format!(
                "classify_local_visual_plan: {text_name} byte range [{i}] end_exclusive ({end}) is not a UTF-8 char boundary"
            )));
        }
        result.push(Utf8ByteRange::from_ordered(start, end));
    }
    Ok(result)
}

/// 从 `text` + `affected` 构建 [`AnimationTextSlice`] 列表。
///
/// 与 `visual_classification.rs::build_slices_from_text` 同策略：
/// - `range.start >= text.len()`：局部文本模式，slice = `(range.start, text)`。
/// - `range.end <= text.len()` 且 char boundary：全文模式，slice = `(range.start, &text[start..end])`。
/// - 否则（mixed 或非 char boundary）：按局部文本处理，slice = `(range.start, text)`。
fn build_slices_from_text<'a>(
    text: &'a str,
    affected: &[Utf8ByteRange],
) -> Vec<AnimationTextSlice<'a>> {
    let mut slices = Vec::new();
    for range in affected {
        let start = range.start().value();
        let end = range.end().value();
        if start >= end {
            continue;
        }
        if start >= text.len() {
            // 局部文本模式：text 就是 affected range 内的文本
            slices.push(AnimationTextSlice {
                absolute_start: start,
                text,
            });
        } else if end <= text.len() && text.is_char_boundary(start) && text.is_char_boundary(end) {
            // 全文模式：从 text 截取 [start, end)
            slices.push(AnimationTextSlice {
                absolute_start: start,
                text: &text[start..end],
            });
        } else {
            // mixed 或非 char boundary：按局部文本处理，不丢失 units。
            slices.push(AnimationTextSlice {
                absolute_start: start,
                text,
            });
        }
    }
    slices
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

    #[test]
    fn classifies_empty_to_we_two_clusters() {
        // "" -> "我们"（2 个汉字 = 2 个 grapheme cluster）
        let plan = classify_local_visual_plan(
            "".to_string(),
            "我们".to_string(),
            vec![],
            vec![range(0, 6)], // UTF-8: 每个汉字 3 bytes
            true,
        )
        .expect("plan");
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
        let plan = classify_local_visual_plan(
            "".to_string(),
            "abc".to_string(),
            vec![],
            vec![range(0, 3)],
            true,
        )
        .expect("plan");
        assert_eq!(plan.animation_mode, AnimationModeDto::GlyphAnimation);
        assert_eq!(plan.new_animation_units.len(), 3);
    }

    #[test]
    fn classifies_empty_to_emoji_family_one_cluster() {
        // "" -> "👨‍👩‍👧‍👦"（emoji family 是 1 个 grapheme cluster）
        let emoji = "👨‍👩‍👧‍👦";
        let plan = classify_local_visual_plan(
            "".to_string(),
            emoji.to_string(),
            vec![],
            vec![range(0, emoji.len() as u32)],
            true,
        )
        .expect("plan");
        // �1 cluster，但含复杂 grapheme（ZWJ + emoji）-> ClusterAnimation
        assert_eq!(plan.animation_mode, AnimationModeDto::ClusterAnimation);
        // 1 个 unit（整个 emoji family）
        assert_eq!(plan.new_animation_units.len(), 1);
    }

    #[test]
    fn classifies_zero_clusters_to_system_suppressed() {
        // "" -> ""（0 cluster -> SystemSuppressed）
        let plan = classify_local_visual_plan(
            "".to_string(),
            "".to_string(),
            vec![],
            vec![],
            true,
        )
        .expect("plan");
        assert_eq!(plan.animation_mode, AnimationModeDto::SystemSuppressed);
        assert!(plan.new_animation_units.is_empty());
    }

    #[test]
    fn classifies_animation_disabled_to_system_suppressed() {
        let plan = classify_local_visual_plan(
            "".to_string(),
            "abc".to_string(),
            vec![],
            vec![range(0, 3)],
            false,
        )
        .expect("plan");
        assert_eq!(plan.animation_mode, AnimationModeDto::SystemSuppressed);
        assert!(plan.new_animation_units.is_empty());
    }

    #[test]
    fn rejects_non_char_boundary_range() {
        // "我们" 的 byte 1 不是 char boundary
        let result = classify_local_visual_plan(
            "".to_string(),
            "我们".to_string(),
            vec![],
            vec![range(0, 1)], // 1 不是 char boundary
            true,
        );
        assert!(result.is_err());
    }
}
