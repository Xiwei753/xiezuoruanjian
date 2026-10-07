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
    // Issue #724 评论 5751573705 问题3: 改为 applyThemeState 函数原子替换 resolvedTheme，
    // 不再用 _themeState 中间变量逐个写属性。
    assert!(
        src.contains("applyThemeState"),
        "DesignTokens 应用 applyThemeState 函数原子替换 resolvedTheme"
    );
    // Issue #702: isDark 从 resolvedTheme.is_dark 读取，
    // resolvedTheme 从 themeStateJson 解析（包含 is_dark 和 scheme），
    // 不再分开绑定 is_dark 和 resolved_scheme_json 两个可能不同步的属性。
    assert!(
        src.contains("property bool isDark: resolvedTheme.is_dark"),
        "isDark 应从 resolvedTheme.is_dark 读取（themeStateJson 包含 is_dark 和 scheme）"
    );
    assert!(
        !src.contains("property string resolvedSchemeJson"),
        "DesignTokens 不应再有独立的 resolvedSchemeJson 属性"
    );
    // resolvedTheme 整体替换后，所有派生 token 只读 resolvedTheme
    assert!(
        src.contains("resolvedTheme"),
        "resolvedTheme 应作为单一事实源"
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
    // Issue #826: 旧 animation/transaction/ 模块（含 TextVisualOperationKind 枚举）
    // 已整体删除，正文视觉操作不再有任何 Cursor 变体 —— 光标与正文彻底解耦。
    let tx_dir = linux_qt_root().join("src/sujian_editor_item/animation/transaction");
    assert!(
        !tx_dir.exists(),
        "animation/transaction 已删除，TextVisualOperationKind 不复存在"
    );
    for rel in [
        "src/sujian_editor_item/animation/coordinator.rs",
        "src/sujian_editor_item/animation/composition.rs",
        "src/sujian_editor_item/edit_motion.rs",
    ] {
        let src = read_src(rel);
        assert!(
            !src.contains("TextVisualOperationKind"),
            "{}: 不得引用已删除的 TextVisualOperationKind（尤其不得有 Cursor 变体）",
            rel
        );
    }
}

#[test]
fn issue702_cursor_branch_returns_none() {
    // Issue #826: 旧 transaction_builder.rs（`EditorAnimationKind::CursorOnly =>` 分支）
    // 已删除；CursorOnly 不再生成任何正文事务，只能在 coordinator 里直接 return。
    assert!(
        !linux_qt_root()
            .join("src/sujian_editor_item/animation/transaction_builder.rs")
            .exists(),
        "transaction_builder.rs 已删除，纯光标移动不得再进正文事务链"
    );
    let src = read_src("src/sujian_editor_item/animation/coordinator.rs");
    let cursor_marker = "EditorAnimationKind::CursorOnly => return,";
    let window = function_window(&src, cursor_marker, 400);
    assert!(
        window.contains("return,"),
        "CursorOnly 分支应直接 return（不建正文前沿、不建正文事务）"
    );
    assert!(
        !src.contains("TextVisualOperationKind::Cursor"),
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
    // Issue #747: animation_coordinator.rs 已拆分为多个子模块，检查所有子模块
    let animation_files = [
        "src/sujian_editor_item/animation/coordinator.rs",
        "src/sujian_editor_item/animation/composition.rs",
        "src/sujian_editor_item/animation/cursor_motion.rs",
        "src/sujian_editor_item/animation/rebase.rs",
        "src/sujian_editor_item/animation/render_plan_builder.rs",
        "src/sujian_editor_item/animation/transaction_builder.rs",
        "src/sujian_editor_item/animation/transaction/types.rs",
        "src/sujian_editor_item/animation/transaction/timeline.rs",
        "src/sujian_editor_item/animation/transaction/rebind.rs",
        "src/sujian_editor_item/animation/transaction/queue.rs",
    ];
    for file in &animation_files {
        // Issue #826 评论 36: 旧 transaction/ / cursor_motion.rs / rebase.rs /
        // transaction_builder.rs 已删除，只扫描仍然存在的文件。
        if !linux_qt_root().join(file).exists() {
            continue;
        }
        let src = read_src(file);
        assert!(
            !src.contains("pub fn handle_cursor_only"),
            "{}: handle_cursor_only 应已彻底删除",
            file
        );
        assert!(
            !src.contains("driver_key: Option<VisualTransactionKey>"),
            "{}: build_cursor_plan 不应再接受 driver_key 参数",
            file
        );
    }
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
fn issue702_render_plan_has_clip_rects() {
    let src = read_src("src/sujian_editor_item/render_plan.rs");
    assert!(
        src.contains("clip_rects: Vec<AnimationClipRect>"),
        "RenderPlan 应携带 clip_rects"
    );
}

#[test]
fn issue702_scene_graph_rebuilds_on_animation_clip() {
    let src = read_src("src/sujian_editor_item/scene_graph_renderer.rs");
    // Issue #714 评论 5740007764: 静态层不再因 static_patches 非空而在每个动画帧
    // 重建，只在 needs_relayout=true（正文/layout/颜色变化或活动事务集合变化）时
    // 重建一次。动画 progress 变化帧走轻量 update_scroll_transform，避免 100ms
    // 动画期间每帧销毁/重建静态 QSGTextNode 产生闪烁。
    assert!(
        !src.contains("has_animation_clip"),
        "Issue #714 评论 5740007764: 应删除 has_animation_clip 每帧重建条件"
    );
    assert!(
        !src.contains("needs_relayout || has_animation_clip"),
        "Issue #714 评论 5740007764: 不应再保留 needs_relayout || has_animation_clip 旧条件"
    );
    // Issue #736 评论 5786531280: 静态层重建条件从单纯的 needs_relayout 改为
    // should_rebuild_static = needs_relayout || has_unavailable_clip_texture。
    // texture miss 时也必须同帧重建，使 canonical 正文同帧恢复。
    assert!(
        src.contains("should_rebuild_static"),
        "应使用 should_rebuild_static 作为重建条件"
    );
    assert!(
        src.contains("has_unavailable_clip_texture"),
        "应检查 has_unavailable_clip_texture 以保证 texture miss 同帧恢复 canonical"
    );
    assert!(
        src.contains("plan.clip_rects"),
        "应直接从 plan.clip_rects 读取裁剪区域"
    );
}

// =========================================================================
// 问题 2c：删除错拍 — 静态架构约束
// =========================================================================

#[test]
fn issue702_delete_conceal_has_same_frame_progress() {
    // Issue #826: 旧 transaction_builder.rs / cursor_motion.rs / transaction/types.rs
    // 已删除；DeleteConceal/InsertReveal 现在由 composition 层描述（不进 EditFrontier、
    // 不 carry preedit glyph），光标层不再有 sampled_rect 插值 —— 光标是独立 timeline。
    let root = linux_qt_root();
    for rel in [
        "src/sujian_editor_item/animation/transaction_builder.rs",
        "src/sujian_editor_item/animation/cursor_motion.rs",
        "src/sujian_editor_item/animation/transaction/types.rs",
    ] {
        assert!(
            !root.join(rel).exists(),
            "{} 已删除，旧同帧采样链不得复活",
            rel
        );
    }
    let comp = read_src("src/sujian_editor_item/animation/composition.rs");
    assert!(
        comp.contains("DeleteConceal"),
        "composition 层应存在 DeleteConceal"
    );
    assert!(
        comp.contains("InsertReveal"),
        "composition 层应存在 InsertReveal"
    );
}

#[test]
fn issue702_prepared_cursor_visual_track_has_sampled_rect() {
    // Issue #826: PreparedCursorVisualTrack（sampled_rect_at_progress）随协同光标删除。
    assert!(
        !linux_qt_root()
            .join("src/sujian_editor_item/animation/transaction/types.rs")
            .exists(),
        "transaction/types.rs 已删除，PreparedCursorVisualTrack 不复存在"
    );
    let rendering = read_src("src/sujian_editor_item/rendering.rs");
    assert!(
        !rendering.contains("sampled_rect_at_progress"),
        "光标插值不再走 sampled_rect_at_progress"
    );
    assert!(
        rendering.contains("fn current_position(&self)"),
        "光标插值唯一位置是 CursorAnimationState::current_position"
    );
}

#[test]
fn issue702_qquickitem_impl_starts_cursor_timeline_on_idle() {
    // Issue #826 评论 36: 旧 `CursorSampleOutcome::Idle` 首帧启动已删除；
    // 光标 timeline 只由每帧 `tick_animation(frame_now)` 推进，started_at 的
    // 初始化只允许发生在 cursor_controller 内。
    let src = read_src("src/sujian_editor_item/qquickitem_impl.rs");
    assert!(
        !src.contains("CursorSampleOutcome"),
        "旧 CursorSampleOutcome（正文事务驱动 caret progress）不得复活"
    );
    assert!(
        src.contains("self.cursor_ctrl.tick_animation(frame_now);"),
        "每帧用本帧 frame_now 推进光标 timeline"
    );
    assert!(
        !src.contains("started_at = Some(frame_now)"),
        "started_at 不在 item 层初始化"
    );
    assert!(
        src.contains("cursor_ctrl.animation.is_some()"),
        "光标动画存在时应继续 request_frame_update"
    );
    let ctrl = read_src("src/sujian_editor_item/cursor_controller.rs");
    assert_eq!(
        ctrl.matches("started_at = Some(frame_now)").count(),
        1,
        "started_at 只在 tick_animation(frame_now) 一处初始化"
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
    // Issue #727 评论 5757225958 问题4: theme_state_json 改为 serde_json::to_string(&snapshot)，
    // JSON 键由 serde 从 ResolvedThemeUiSnapshot 结构体字段名自动生成。
    // 检查 resolved_theme_snapshot.rs 中字段定义确保诊断字段存在。
    let snapshot = read_src("src/backend/resolved_theme_snapshot.rs");
    assert!(
        snapshot.contains("resolved_source"),
        "ResolvedThemeUiSnapshot 应包含 resolved_source 字段定义（serde 序列化输出该键）"
    );
    assert!(
        snapshot.contains("resolved_scheme_kind"),
        "ResolvedThemeUiSnapshot 应包含 resolved_scheme_kind 字段定义（serde 序列化输出该键）"
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
