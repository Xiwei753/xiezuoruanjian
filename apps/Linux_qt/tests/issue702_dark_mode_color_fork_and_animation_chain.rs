//! Issue #702 修复验证 — 深色模式颜色分叉与编辑器动画绘制链失效。
//!
//! 本测试只保留确实只能做静态架构约束的检查。能通过对象行为测试的部分
//! 已迁移到 qt_runtime_theme_cursor.rs 和 qt_runtime_cursor_geometry.rs。

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
// 问题 1：深色模式颜色分叉 — 静态架构约束
// =========================================================================

#[test]
fn issue702_main_qml_binds_single_theme_state_json() {
    let src = read_src("qml/main.qml");
    assert!(
        src.contains("themeStateJson: themeController"),
        "main.qml 应只绑定 themeStateJson"
    );
    assert!(
        !src.contains("isDark: themeController"),
        "main.qml 不应再分开绑定 isDark"
    );
    assert!(
        !src.contains("resolvedSchemeJson: themeController"),
        "main.qml 不应再分开绑定 resolvedSchemeJson"
    );
}

#[test]
fn issue702_design_tokens_consumes_single_theme_state_json() {
    let src = read_src("qml/DesignTokens.qml");
    assert!(
        src.contains("themeStateJson"),
        "DesignTokens 应有 themeStateJson 属性"
    );
    assert!(
        src.contains("_themeState"),
        "DesignTokens 应从 _themeState 解析"
    );
    assert!(
        src.contains("_themeState.is_dark"),
        "isDark 应从 _themeState.is_dark 读取"
    );
    assert!(
        src.contains("_themeState.scheme"),
        "scheme 应从 _themeState.scheme 读取"
    );
    assert!(
        !src.contains("property string resolvedSchemeJson"),
        "DesignTokens 不应再有独立的 resolvedSchemeJson 属性"
    );
}

#[test]
fn issue702_theme_controller_publishes_unified_theme_state_json() {
    let src = read_src("src/backend/linux_theme_controller.rs");
    assert!(
        src.contains("theme_state_json: qt_property!"),
        "应发布 theme_state_json"
    );
    assert!(
        src.contains("\"is_dark\""),
        "theme_state_json 应包含 is_dark"
    );
    assert!(src.contains("\"scheme\""), "theme_state_json 应包含 scheme");
}

// =========================================================================
// 问题 2a：光标动画 — 静态架构约束
// =========================================================================

#[test]
fn issue702_text_visual_operation_kind_no_cursor_variant() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    assert!(src.contains("enum TextVisualOperationKind"), "枚举应存在");
    assert!(
        src.contains("Insert") && src.contains("Delete"),
        "应包含 Insert/Delete"
    );
    assert!(
        !src.contains("Cursor,\n"),
        "TextVisualOperationKind 不应再包含 Cursor 变体"
    );
}

#[test]
fn issue702_cursor_branch_returns_none() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    let cursor_marker = "EditorAnimationKind::Cursor =>";
    let window = function_window(&src, cursor_marker, 800);
    assert!(window.contains("return None"), "Cursor 分支应返回 None");
    assert!(
        !window.contains("TextVisualOperationKind::Cursor"),
        "Cursor 分支不应再创建 Cursor 事务"
    );
}

fn function_window(src: &str, fn_marker: &str, window_chars: usize) -> String {
    let pos = src
        .find(fn_marker)
        .unwrap_or_else(|| panic!("{} 必须存在", fn_marker));
    let target_end = pos + window_chars;
    let window_end = src
        .char_indices()
        .take_while(|(i, _)| *i < target_end)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(src.len())
        .min(src.len());
    src[pos..window_end].to_string()
}

#[test]
fn issue702_handle_cursor_only_deleted() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    assert!(
        !src.contains("pub fn handle_cursor_only"),
        "handle_cursor_only 应已彻底删除"
    );
    assert!(
        !src.contains("driver_key: Option<VisualTransactionKey>"),
        "build_cursor_plan 不应再接受 driver_key 参数"
    );
}

#[test]
fn issue702_cursor_animation_state_has_own_timeline() {
    let src = read_src("src/sujian_editor_item/rendering.rs");
    assert!(
        src.contains("pub struct CursorAnimationState"),
        "CursorAnimationState 应存在"
    );
    assert!(
        !src.contains("driver_key: VisualTransactionKey"),
        "CursorAnimationState 不应再有 driver_key"
    );
    assert!(
        src.contains("started_at: Option<Instant>"),
        "应有 started_at 字段"
    );
    assert!(src.contains("duration_ms: u64"), "应有 duration_ms 字段");
    assert!(
        src.contains("fn sample_progress"),
        "应有 sample_progress 方法"
    );
}

#[test]
fn issue702_cursor_transition_tween_has_duration_ms() {
    let src = read_src("src/sujian_editor_item/cursor_animation.rs");
    assert!(
        src.contains("duration_ms: u64"),
        "CursorTransition::Tween 应有 duration_ms 字段"
    );
}

// =========================================================================
// 问题 2b：静态正文与动画接管区域 — 静态架构约束
// =========================================================================

#[test]
fn issue702_render_plan_has_static_patches() {
    let src = read_src("src/sujian_editor_item/render_plan.rs");
    assert!(
        src.contains("static_patches: Vec<StaticLinePatch>"),
        "RenderPlan 应携带 static_patches"
    );
}

#[test]
fn issue702_scene_graph_rebuilds_on_animation_clip() {
    let src = read_src("src/sujian_editor_item/scene_graph_renderer.rs");
    assert!(
        src.contains("has_animation_clip"),
        "应有 has_animation_clip 判断"
    );
    assert!(
        src.contains("needs_relayout || has_animation_clip"),
        "需要在 needs_relayout 或 has_animation_clip 时重建"
    );
    assert!(
        src.contains("compute_clip_rects_from_patches"),
        "应有裁剪计算"
    );
}

#[test]
fn issue702_static_line_patch_has_doc_hidden_rects() {
    let src = read_src("src/sujian_editor_item/static_line_patch.rs");
    assert!(
        src.contains("doc_hidden_rects: Vec<SourceRect>"),
        "StaticLinePatch 应有 doc_hidden_rects 字段"
    );
}

// =========================================================================
// 问题 2c：删除错拍 — 静态架构约束
// =========================================================================

#[test]
fn issue702_delete_conceal_has_same_frame_progress() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    assert!(
        src.contains("AnimatedSliceKind::DeleteConceal"),
        "应存在 DeleteConceal"
    );
    assert!(src.contains("InsertReveal"), "应存在 InsertReveal");
    assert!(
        src.contains("delete_unit_progress"),
        "应有 delete_unit_progress"
    );
    assert!(
        src.contains("sampled_rect_at_progress"),
        "caret track 应用 sampled_rect_at_progress"
    );
}

#[test]
fn issue702_prepared_cursor_visual_track_has_sampled_rect() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    assert!(
        src.contains("fn sampled_rect_at_progress"),
        "PreparedCursorVisualTrack 应有 sampled_rect_at_progress 方法"
    );
}

#[test]
fn issue702_qquickitem_impl_starts_cursor_timeline_on_idle() {
    let src = read_src("src/sujian_editor_item/qquickitem_impl.rs");
    assert!(
        src.contains("CursorSampleOutcome::Idle"),
        "应处理 Idle 状态"
    );
    assert!(
        src.contains("started_at = Some(frame_now)"),
        "Idle 时应用 frame_now"
    );
    assert!(
        src.contains("cursor_ctrl.animation.is_some()"),
        "光标动画存在时应继续 request_frame_update"
    );
}

// =========================================================================
// Issue #709 评论 5728916561: 主题状态诊断字段 — resolved_source / resolved_scheme_kind
// =========================================================================

#[test]
fn issue709_theme_state_json_includes_resolved_source_and_scheme_kind() {
    let src = read_src("src/backend/linux_theme_controller.rs");
    assert!(
        src.contains("resolved_source"),
        "ResolvedThemeState 应包含 resolved_source 字段"
    );
    assert!(
        src.contains("resolved_scheme_kind"),
        "ResolvedThemeState 应包含 resolved_scheme_kind 字段"
    );
    assert!(
        src.contains("\"resolved_source\""),
        "theme_state_json 应输出 resolved_source"
    );
    assert!(
        src.contains("\"resolved_scheme_kind\""),
        "theme_state_json 应输出 resolved_scheme_kind"
    );
}

#[test]
fn issue709_resolved_state_tracks_scheme_origin() {
    let src = read_src("src/backend/linux_theme_controller.rs");
    // 确认 saved_palette 路径设置 resolved_source = "saved_palette"
    assert!(
        src.contains("\"saved_palette\""),
        "saved_palette 路径应设置 resolved_source"
    );
    // 确认 builtin 路径设置 resolved_source = "builtin"
    assert!(
        src.contains("\"builtin\""),
        "builtin 路径应设置 resolved_source"
    );
    // 确认 dark_scheme/light_scheme 选择逻辑
    assert!(
        src.contains("\"dark_scheme\""),
        "应设置 resolved_scheme_kind = dark_scheme"
    );
    assert!(
        src.contains("\"light_scheme\""),
        "应设置 resolved_scheme_kind = light_scheme"
    );
    // Issue #709 评论 5729368242: scheme == None 时 resolved_scheme_kind 应为 "none"
    assert!(
        src.contains("\"none\""),
        "scheme == None 时 resolved_scheme_kind 应为 none"
    );
    // 确认 resolved_scheme_kind 在 scheme 加载之后才计算（基于 scheme.is_some()）
    assert!(
        src.contains("scheme.is_some()"),
        "resolved_scheme_kind 应基于 scheme.is_some() 判断"
    );
}
