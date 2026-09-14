//! Issue #684 评论 5669048233 复现测试 — 两个 animation units 丢失 bug。
//!
//! ## Bug 1（Core 端，可单元测试复现）
//! `resolve_subtext()` 靠 `range.start` 和 `text.len()` 猜"传进来的是全文还是局部文本"：
//! - 全文模式：`start < text.len() && end <= text.len()`
//! - 局部文本模式：`start >= text.len()`
//! - mixed（start < text.len() 但 end > text.len()）：返回 None，丢失 animation units
//!
//! 复现条件：正文 offset=1 的位置插入两个 ASCII 字符：
//! - text.len() = 2（局部 changed text "ab"）
//! - range = [1, 3)（正文绝对 offset）
//! - 此时 start=1 < text.len()=2，end=3 > text.len()=2，落入 mixed 分支返回 None
//! - `compute_animation_units()` 对该 range 的 cluster/run units 全部丢失
//!
//! 这些测试断言**期望（修复后）的正确行为**。在当前缺陷存在时它们会**失败**，
//! 失败的 panic 信息即为复现证据。修复后所有测试应通过。
//!
//! ## Bug 2（Android 端，逻辑测试复现）
//! `ComposeVisualFrameCoordinator.kt` 第 420-430 行：当 chain 有多笔 intent 时，
//! animation units 被退化成整块 mergedRanges，丢失了单笔的 unit 边界。
//! 通过静态分析确认退化逻辑存在。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use writer_core::editor::{compute_animation_units, AnimationMode, Utf8ByteRange};

// ===========================================================================
// Bug 1 复现 1：GlyphAnimation 模式下，局部文本 "ab" + affected range [1,3)
// 落入 resolve_subtext 的 mixed 分支，new_animation_units 为空。
//
// resolve_subtext("ab", start=1, end=3):
//   - start=1 < text.len()=2 → true
//   - end=3 <= text.len()=2 → false
//   - 不是全文模式
//   - start=1 >= text.len()=2 → false
//   - 不是局部文本模式
//   - mixed 情况 → 返回 None
//   - collect_cluster_units 跳过该 range → new_animation_units 为空
//
// 期望：new_animation_units 应包含 [1,3) 范围内的 cluster units（非空）。
// 实际：为空 Vec → bug 存在。
// ===========================================================================
#[test]
fn repro_bug1_glyph_animation_mixed_branch_loses_units() {
    let old_text = "";
    let new_text = "ab"; // 局部 changed text，len=2
    let old_affected: Vec<Utf8ByteRange> = vec![];
    let new_affected = vec![Utf8ByteRange::from_values(1, 3).unwrap()]; // 正文绝对 offset [1,3)

    let (old_animation_units, new_animation_units) = compute_animation_units(
        AnimationMode::GlyphAnimation,
        old_text,
        new_text,
        &old_affected,
        &new_affected,
    );

    eprintln!(
        "[BUGFIX_REPRO_TRACE] Bug1 GlyphAnimation: old_units={:?}, new_units={:?}",
        old_animation_units, new_animation_units
    );

    // 期望：new_animation_units 应包含 [1,3) 范围内的 cluster units（非空）。
    // "ab" 有两个 grapheme cluster 'a' 和 'b'，在 base_offset=1 下应生成
    // [1,2) 和 [2,3) 两个 cluster unit（或至少一个覆盖 [1,3) 的 unit）。
    assert!(
        !new_animation_units.is_empty(),
        "FAIL [Bug1]: GlyphAnimation 模式下，new_affected=[1,3) 的局部文本 \"ab\" \
         应生成非空的 new_animation_units（期望包含 [1,3) 范围内的 cluster units）；\
         实际为空 Vec。原因：resolve_subtext(text=\"ab\", start=1, end=3) 落入 \
         mixed 分支（start=1 < text.len()=2 但 end=3 > text.len()=2）返回 None，\
         collect_cluster_units 跳过该 range，animation units 全部丢失。"
    );
}

// ===========================================================================
// Bug 1 复现 2：ClusterAnimation 模式同样受 resolve_subtext mixed 分支影响。
// ===========================================================================
#[test]
fn repro_bug1_cluster_animation_mixed_branch_loses_units() {
    let old_text = "";
    let new_text = "ab";
    let old_affected: Vec<Utf8ByteRange> = vec![];
    let new_affected = vec![Utf8ByteRange::from_values(1, 3).unwrap()];

    let (old_animation_units, new_animation_units) = compute_animation_units(
        AnimationMode::ClusterAnimation,
        old_text,
        new_text,
        &old_affected,
        &new_affected,
    );

    eprintln!(
        "[BUGFIX_REPRO_TRACE] Bug1 ClusterAnimation: old_units={:?}, new_units={:?}",
        old_animation_units, new_animation_units
    );

    assert!(
        !new_animation_units.is_empty(),
        "FAIL [Bug1]: ClusterAnimation 模式下，new_affected=[1,3) 的局部文本 \"ab\" \
         应生成非空的 new_animation_units；实际为空 Vec。同 Bug1 复现 1 根因。"
    );
}

// ===========================================================================
// Bug 1 复现 3：RunAnimation 模式同样受 resolve_subtext mixed 分支影响。
// collect_run_units 也调用 resolve_subtext，mixed 分支同样返回 None。
// ===========================================================================
#[test]
fn repro_bug1_run_animation_mixed_branch_loses_units() {
    let old_text = "";
    let new_text = "ab";
    let old_affected: Vec<Utf8ByteRange> = vec![];
    let new_affected = vec![Utf8ByteRange::from_values(1, 3).unwrap()];

    let (old_animation_units, new_animation_units) = compute_animation_units(
        AnimationMode::RunAnimation,
        old_text,
        new_text,
        &old_affected,
        &new_affected,
    );

    eprintln!(
        "[BUGFIX_REPRO_TRACE] Bug1 RunAnimation: old_units={:?}, new_units={:?}",
        old_animation_units, new_animation_units
    );

    assert!(
        !new_animation_units.is_empty(),
        "FAIL [Bug1]: RunAnimation 模式下，new_affected=[1,3) 的局部文本 \"ab\" \
         应生成非空的 new_animation_units；实际为空 Vec。\
         collect_run_units 也调用 resolve_subtext，mixed 分支同样返回 None。"
    );
}

// ===========================================================================
// Bug 1 复现 4：对照测试 — 全文模式下不丢失 units（确认 bug 仅在 mixed 分支）。
// text="xab"（全文），range=[1,3)（在 text 内），start=1 < len=3, end=3 <= len=3 → 全文模式。
// ===========================================================================
#[test]
fn repro_bug1_full_text_mode_does_not_lose_units() {
    let old_text = "x";
    let new_text = "xab"; // 全文，len=3
    let old_affected: Vec<Utf8ByteRange> = vec![];
    let new_affected = vec![Utf8ByteRange::from_values(1, 3).unwrap()]; // [1,3) 在 text 内

    let (_old_animation_units, new_animation_units) = compute_animation_units(
        AnimationMode::GlyphAnimation,
        old_text,
        new_text,
        &old_affected,
        &new_affected,
    );

    eprintln!(
        "[BUGFIX_REPRO_TRACE] Bug1 对照全文模式: new_units={:?}",
        new_animation_units
    );

    // 全文模式下 resolve_subtext 正常返回，units 不丢失。
    assert!(
        !new_animation_units.is_empty(),
        "FAIL [Bug1 对照]: 全文模式下 new_animation_units 不应为空"
    );
}

// ===========================================================================
// Bug 1 复现 5：对照测试 — 局部文本模式下不丢失 units（确认 bug 仅在 mixed 分支）。
// text="ab"（局部），range=[5,7)（start=5 >= text.len()=2 → 局部文本模式）。
// ===========================================================================
#[test]
fn repro_bug1_local_text_mode_does_not_lose_units() {
    let old_text = "";
    let new_text = "ab"; // 局部文本，len=2
    let old_affected: Vec<Utf8ByteRange> = vec![];
    let new_affected = vec![Utf8ByteRange::from_values(5, 7).unwrap()]; // start=5 >= len=2

    let (_old_animation_units, new_animation_units) = compute_animation_units(
        AnimationMode::GlyphAnimation,
        old_text,
        new_text,
        &old_affected,
        &new_affected,
    );

    eprintln!(
        "[BUGFIX_REPRO_TRACE] Bug1 对照局部文本模式: new_units={:?}",
        new_animation_units
    );

    // 局部文本模式下 resolve_subtext 正常返回，units 不丢失。
    assert!(
        !new_animation_units.is_empty(),
        "FAIL [Bug1 对照]: 局部文本模式下 new_animation_units 不应为空"
    );
}

// ===========================================================================
// Bug 2 复现：ComposeVisualFrameCoordinator.kt 第 420-430 行的多笔 intent 退化逻辑。
//
// 当 chain.size > 1 时，oldAnimationUnits=mergedOldRanges、
// newAnimationUnits=mergedNewRanges（整块），丢失了单笔 intent 的 unit 边界。
//
// 修复后：退化逻辑应被删除，改用 ComposeVisualRebase.composeNewAnimationUnitsToFinal
// 和 composeOldAnimationUnitsToBase 把每笔 intent 的 units 沿 offsetMap chain
// 映射到统一坐标系。本测试断言修复后的正确行为（退化逻辑不存在 + 新 compose 函数存在）。
// ===========================================================================
#[test]
fn repro_bug2_multi_intent_animation_units_degrade_to_merged_ranges() {
    use std::fs;
    use std::path::PathBuf;

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let manifest_path = PathBuf::from(manifest_dir);
    // core/writer_core -> core -> repo root
    let repo_root = manifest_path
        .parent()
        .and_then(|p| p.parent())
        .expect("repo root");
    let coordinator_path = repo_root.join(
        "apps/android/app/src/main/kotlin/com/xiwei/sujian/feature/editor/visual/ComposeVisualFrameCoordinator.kt",
    );
    let source = fs::read_to_string(&coordinator_path)
        .unwrap_or_else(|e| panic!("读取 {:?} 失败: {e}", coordinator_path));

    let rebase_path = repo_root.join(
        "apps/android/app/src/main/kotlin/com/xiwei/sujian/feature/editor/visual/ComposeVisualRebase.kt",
    );
    let rebase_source = fs::read_to_string(&rebase_path)
        .unwrap_or_else(|e| panic!("读取 {:?} 失败: {e}", rebase_path));

    eprintln!(
        "[BUGFIX_REPRO_TRACE] Bug2 读取 ComposeVisualFrameCoordinator.kt，总行数={}",
        source.lines().count()
    );

    // 修复后：退化逻辑应被删除。
    let has_chain_size_one_branch = source.contains("chain.size == 1");
    let has_else_merged_old = source.contains("oldAnimationUnits = mergedOldRanges");
    let has_else_merged_new = source.contains("newAnimationUnits = mergedNewRanges");

    assert!(
        !has_chain_size_one_branch,
        "FAIL [Bug2 修复后]: 不应再存在 `chain.size == 1` 退化分支判断，\
         应改用 ComposeVisualRebase.composeOldAnimationUnitsToBase/composeNewAnimationUnitsToFinal"
    );
    assert!(
        !(has_else_merged_old && has_else_merged_new),
        "FAIL [Bug2 修复后]: 不应再将 oldAnimationUnits/newAnimationUnits 设为 \
         mergedOldRanges/mergedNewRanges（整块退化），应改用 compose 函数保留 unit 边界。"
    );

    // 修复后：应调用新的 compose 函数。
    let coordinator_uses_compose_old =
        source.contains("ComposeVisualRebase.composeOldAnimationUnitsToBase(chain)");
    let coordinator_uses_compose_new =
        source.contains("ComposeVisualRebase.composeNewAnimationUnitsToFinal(chain)");
    assert!(
        coordinator_uses_compose_old && coordinator_uses_compose_new,
        "FAIL [Bug2 修复后]: ComposeVisualFrameCoordinator.kt 应调用 \
         ComposeVisualRebase.composeOldAnimationUnitsToBase(chain) 和 \
         composeNewAnimationUnitsToFinal(chain)"
    );

    // 修复后：ComposeVisualRebase.kt 应定义这两个 compose 函数。
    let rebase_has_compose_new = rebase_source.contains("fun composeNewAnimationUnitsToFinal(");
    let rebase_has_compose_old = rebase_source.contains("fun composeOldAnimationUnitsToBase(");
    assert!(
        rebase_has_compose_new && rebase_has_compose_old,
        "FAIL [Bug2 修复后]: ComposeVisualRebase.kt 应定义 \
         composeNewAnimationUnitsToFinal 和 composeOldAnimationUnitsToBase 函数"
    );

    eprintln!(
        "[BUGFIX_REPRO_TRACE] Bug2 确认修复: 退化分支已删除={}, \
         else mergedRanges 已删除={}, \
         coordinator 调用 compose 函数={}, \
         rebase 定义 compose 函数={}",
        !has_chain_size_one_branch,
        !(has_else_merged_old && has_else_merged_new),
        coordinator_uses_compose_old && coordinator_uses_compose_new,
        rebase_has_compose_new && rebase_has_compose_old,
    );
}
