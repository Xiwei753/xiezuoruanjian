//! Issue #727 评论 5757225958 复现测试 — 上一轮分支 auto-issue-727-comment-5755858583
//! 部分修复后残留的 4 个主链错误 + 1 个 ownership 收严问题。本测试为 WHITE_BOX
//! 结构守卫复现：验证当前实现仍违反评论 5757225958 指出的 5 个核心语义。
//! 每个子测试断言评论期望的正确结构，当前（未修复）代码违反这些断言
//! → 测试 FAIL → 缺陷复现成功。
//!
//! ## 评论 5757225958 指出的 5 个问题
//!
//! 1. cursor layer 坐标混用（doc/viewport 混用）：rendering.rs 仍调
//!    editor_layout_cursor_rect(..., scroll_y) 得到 viewport y，进入
//!    cursor_ctrl.visual_y → CursorRenderState.y，render_cursor_layer 当
//!    document y 传给 update_cursor_node 再减 scroll_y。
//! 2. QML 调用已删除的 Rust 方法：WritingWorkspace.qml 仍调
//!    set_auto_follow_anchor_with_target，但 properties.rs/mod.rs 已无此方法。
//! 3. 事务完成帧被 clip 掏空：build_text_animation_plan_with_sample 完成时
//!    keys_to_complete.push + continue 不生成 glyph，但 build_render_plan_full
//!    clip_rects 收集时事务状态仍为 Rendering/Prepared 仍收集裁剪 → 一帧闪烁。
//! 4. 主题颜色链未真正换成 ResolvedThemeUiSnapshot -> Qt.color：
//!    linux_theme_controller.rs 未引用 resolved_theme_snapshot 模块，
//!    DesignTokens.qml 直接把 resolvedTheme.on_surface 赋给 property color
//!    无 Qt.color() 包裹，*_hex getter 仍保留。
//! 5. CoordinatedMotionFrame 不带 owner transaction key：只带 caret，
//!    同一份 caret frame 喂给所有 active transaction。

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

fn linux_qt_root() -> PathBuf {
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR must be set by cargo");
    PathBuf::from(manifest_dir)
}

fn read_src(rel: &str) -> String {
    let path = linux_qt_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

// =========================================================================
// 问题1: cursor layer 坐标混用（doc/viewport 混用）
// =========================================================================

/// 问题1 守卫1: scene_graph.rs::update_cursor_node 已改为 document transform
/// （cursorMatrix.translate(0, -scroll_y)），假设 cursor_y 是文档坐标。
/// 这是上一轮已修复的部分，应保持。
#[test]
fn issue1_scene_graph_cursor_layer_uses_document_transform() {
    let src = read_src("src/editor/scene_graph.rs");
    let has_doc_transform =
        src.contains("cursorMatrix.translate(0, -static_cast<qreal>(scroll_y))");
    assert!(
        has_doc_transform,
        "scene_graph.rs::update_cursor_node 应使用 document transform translate(0, -scroll_y)"
    );
}

/// 问题1 守卫2: rendering.rs::update_cursor_visual_position 仍调用
/// editor_layout_cursor_rect(..., scroll_y)，返回 viewport y（减过 scroll_y）。
/// 这个 viewport y 进入 cursor_ctrl.target_y/visual_y → CursorRenderState.y。
/// 修复后应传 0（或不传 scroll_y）使 caret_rect 返回文档坐标，或改用 caret_rect_doc。
#[test]
fn issue1_rendering_passes_scroll_y_to_cursor_rect() {
    let src = read_src("src/sujian_editor_item/rendering.rs");
    // 定位 update_cursor_visual_position 中的 editor_layout_cursor_rect 调用
    let marker = "fn update_cursor_visual_position";
    let pos = src
        .find(marker)
        .expect("update_cursor_visual_position 必须存在");
    let window = &src[pos..pos.saturating_add(2000)];
    // 当前缺陷：传 scroll_y 给 editor_layout_cursor_rect
    let passes_scroll_y = window.contains(
        "editor_layout_cursor_rect(self.buffer.cursor, self.cursor_ctrl.affinity, scroll_y)",
    );
    assert!(
        !passes_scroll_y,
        "rendering.rs::update_cursor_visual_position 仍传 scroll_y 给 editor_layout_cursor_rect，\
         返回 viewport y 进入 CursorRenderState.y，与 scene_graph 的 document transform 坐标混用。\
         修复应改用 caret_rect_doc 或传 0 使返回文档坐标。"
    );
}

/// 问题1 守卫3: layout_ops.rs::editor_layout_cursor_rect 调用 caret_rect（带 scroll_y）
/// 返回 viewport y，而非 caret_rect_doc（返回文档坐标）。
#[test]
fn issue1_layout_ops_calls_caret_rect_with_scroll_y() {
    let src = read_src("src/sujian_editor_item/layout_ops.rs");
    let marker = "fn editor_layout_cursor_rect";
    let pos = src
        .find(marker)
        .expect("editor_layout_cursor_rect 必须存在");
    let window = &src[pos..pos.saturating_add(500)];
    // 当前缺陷：调用 caret_rect（带 scroll_y），返回 viewport y
    let calls_caret_rect_with_scroll =
        window.contains("self.editor_layout.caret_rect(") && window.contains("scroll_y");
    assert!(
        !calls_caret_rect_with_scroll,
        "layout_ops.rs::editor_layout_cursor_rect 调用 caret_rect(..., scroll_y, ...) 返回 viewport y，\
         应改用 caret_rect_doc 返回文档坐标以与 scene_graph document transform 一致。"
    );
}

// =========================================================================
// 问题2: QML 调用已删除的 Rust 方法（断接口）
// =========================================================================

/// 问题2 守卫1: WritingWorkspace.qml 仍调用 set_auto_follow_anchor_with_target。
/// 修复后应删除此 QML 调用（或恢复 Rust 方法）。
#[test]
fn issue2_qml_calls_deleted_rust_method() {
    let src = read_src("qml/WritingWorkspace.qml");
    let qml_calls = src.contains("set_auto_follow_anchor_with_target(");
    assert!(
        !qml_calls,
        "WritingWorkspace.qml 仍调用 sujianEditor.set_auto_follow_anchor_with_target(...)，\
         但 Rust 侧 properties.rs/mod.rs 已无此方法定义，auto-follow 触发时运行时断接口。"
    );
}

/// 问题2 守卫2: properties.rs 和 mod.rs 中不应有 set_auto_follow_anchor_with_target 定义。
/// 这是上一轮已删除的部分，应保持删除状态。
#[test]
fn issue2_rust_method_already_deleted() {
    let properties = read_src("src/sujian_editor_item/properties.rs");
    let mod_rs = read_src("src/sujian_editor_item/mod.rs");
    let rust_has_method = properties.contains("fn set_auto_follow_anchor_with_target")
        || mod_rs.contains("fn set_auto_follow_anchor_with_target");
    assert!(
        !rust_has_method,
        "Rust 侧不应有 set_auto_follow_anchor_with_target 方法定义（已删除），\
         但 QML 仍调用它 → 断接口。"
    );
}

// =========================================================================
// 问题3: 事务完成帧被 clip 掏空（正文闪一帧）
// =========================================================================

/// 问题3 守卫1: 完成帧（keys_to_complete）的事务在 build_text_animation_plan_with_sample
/// 里 keys_to_complete.push + continue 不生成动画 glyph。为避免"glyph 无、clip 有"的
/// 一帧文字消失/闪烁，clip_rects 收集必须同帧排除 keys_to_complete。
/// 上一轮（评论 5757225958）选择"调整 clip 收集时序"方式：clip 收集时用
/// keys_to_complete_set 排除即将完成的事务。本守卫验证该结构特征存在。
#[test]
fn issue3_complete_frame_continue_skips_glyph_generation() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    // clip_rects 收集必须排除 keys_to_complete，否则完成帧"glyph 无、clip 有"→ 文字闪烁。
    let clip_marker = "let mut clip_rects";
    let clip_pos = src.find(clip_marker).expect("clip_rects 收集必须存在");
    let clip_window = &src[clip_pos..clip_pos.saturating_add(1200)];
    assert!(
        clip_window.contains("keys_to_complete_set.contains"),
        "build_render_plan_full clip_rects 收集必须用 keys_to_complete_set 排除即将完成的事务，\
         否则完成帧 keys_to_complete.push + continue 不画 glyph 但 clip 仍藏 canonical → 文字闪烁。"
    );
}

/// 问题3 守卫2: build_render_plan_full 收集 clip_rects 时条件包含 Rendering 状态，
/// 完成帧事务尚未 finish_by_key()，状态仍为 Rendering，仍收集 static_hidden_document_rects。
/// 修复后应在收集 clip_rects 时排除即将完成的事务（keys_to_complete），或先 finish 再收集。
#[test]
fn issue3_clip_rects_collects_rendering_state() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    // 定位 build_render_plan_full 中 clip_rects 收集
    let marker = "let mut clip_rects";
    let pos = src.find(marker).expect("clip_rects 收集必须存在");
    let window = &src[pos..pos.saturating_add(800)];
    // 当前缺陷：clip 收集条件包含 Rendering 状态
    let collects_rendering = window.contains("TextVisualTransactionState::Rendering");
    assert!(
        !collects_rendering,
        "build_render_plan_full clip_rects 收集条件包含 Rendering 状态，\
         完成帧事务（keys_to_complete）尚未 finish_by_key() 状态仍为 Rendering，\
         仍从 static_hidden_document_rects 收集裁剪，与 continue 跳过 glyph 生成 \
         叠加产生一帧文字消失。"
    );
}

// =========================================================================
// 问题4: 主题颜色链未真正换成 ResolvedThemeUiSnapshot -> Qt.color
// =========================================================================

/// 问题4 守卫1: linux_theme_controller.rs 应 use resolved_theme_snapshot 模块并构造
/// ResolvedThemeUiSnapshot。当前只有注释提及，未实际引用。
#[test]
fn issue4_theme_controller_uses_resolved_snapshot() {
    let src = read_src("src/backend/linux_theme_controller.rs");
    // 修复后应实际 use 或构造 ResolvedThemeUiSnapshot
    let uses_snapshot = src.contains("use super::resolved_theme_snapshot")
        || src.contains("use crate::backend::resolved_theme_snapshot")
        || src.contains("ResolvedThemeUiSnapshot {")
        || src.contains("ResolvedThemeUiSnapshot::");
    assert!(
        uses_snapshot,
        "linux_theme_controller.rs 未引用 resolved_theme_snapshot 模块也未构造 \
         ResolvedThemeUiSnapshot，theme_state_json() 仍手工拼 serde_json::Map，\
         颜色链未真正统一。"
    );
}

/// 问题4 守卫2: DesignTokens.qml 应在唯一边界用 Qt.color(value) 包裹 JSON 字符串，
/// 而非直接把 resolvedTheme.on_surface 赋给 property color。
#[test]
fn issue4_design_tokens_uses_qt_color_boundary() {
    let src = read_src("qml/DesignTokens.qml");
    // 当前缺陷：直接赋值 resolvedTheme.xxx 给 property color，无 Qt.color() 包裹
    let direct_assign = src.contains("property color onSurface: resolvedTheme.on_surface")
        || src.contains("property color on_background: resolvedTheme.on_background")
        || src.contains("property color primary: resolvedTheme.primary");
    assert!(
        !direct_assign,
        "DesignTokens.qml 直接把 resolvedTheme.on_surface 等 JSON 字符串赋给 property color，\
         未经 Qt.color() 包裹，Rust 侧 #DFE3E7 到 QML 变成 #000000。\
         修复应在唯一边界调用 Qt.color(value)。"
    );
}

/// 问题4 守卫3: linux_theme_controller.rs 不应保留 scheme_hex_or_fallback 和全套 *_hex getter
/// （第三条颜色通道）。修复后应删除这些。
#[test]
fn issue4_no_hex_getter_channel() {
    let src = read_src("src/backend/linux_theme_controller.rs");
    let has_hex_channel =
        src.contains("fn scheme_hex_or_fallback") && src.contains("fn on_surface_hex");
    assert!(
        !has_hex_channel,
        "linux_theme_controller.rs 仍保留 scheme_hex_or_fallback 和 *_hex getter，\
         第三条颜色通道（hex）仍存在，与 ResolvedThemeUiSnapshot 统一链冲突。"
    );
}

// =========================================================================
// 问题5（ownership 收严）: CoordinatedMotionFrame 不带 owner transaction key
// =========================================================================

/// 问题5 守卫1: CoordinatedMotionFrame 应携带 owner transaction key 字段，
/// 不只带 caret。当前只有 caret: Option<SampledCaretFrame>。
#[test]
fn issue5_coordinated_motion_frame_has_owner_key() {
    let src = read_src("src/sujian_editor_item/render_plan.rs");
    // 定位 CoordinatedMotionFrame 结构体
    let marker = "struct CoordinatedMotionFrame";
    let pos = src.find(marker).expect("CoordinatedMotionFrame 必须存在");
    let window = &src[pos..pos.saturating_add(300)];
    // 修复后应有 owner transaction key 字段
    let has_owner_key = window.contains("owner_key")
        || window.contains("owner_transaction_key")
        || window.contains("tx_key")
        || window.contains("transaction_key");
    assert!(
        has_owner_key,
        "CoordinatedMotionFrame 只带 caret 不带 owner transaction key，\
         sample_coordinated_motion_frame 只采最新同 epoch 事务，\
         build_text_animation_plan_with_sample 把同一份 caret frame 喂给所有 \
         active transaction，editor.anim.keep 保留旧事务时旧 CaretDriven unit \
         可能消费新事务的 caret。"
    );
}

/// 问题5 守卫2: build_text_animation_plan_with_sample 把同一份 coordinated_motion_frame
/// 喂给所有 active transaction（在 for tx 循环中）。修复后应按 owner key 过滤。
#[test]
fn issue5_caret_frame_shared_across_all_transactions() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    // 定位 build_text_animation_plan_with_sample 中使用 coordinated_motion_frame 的地方
    let marker = "coordinated_motion_frame.caret";
    let has_shared_caret = src.contains(marker);
    // 当前缺陷：在 for tx 循环中直接用 coordinated_motion_frame.caret，未按 owner key 过滤
    assert!(
        !has_shared_caret
            || src.contains("coordinated_motion_frame.owner_key")
            || src.contains("coordinated_motion_frame.tx_key"),
        "build_text_animation_plan_with_sample 把同一份 coordinated_motion_frame.caret \
         喂给所有 active transaction，未按 owner transaction key 过滤，\
         旧事务可能消费新事务的 caret。"
    );
}
