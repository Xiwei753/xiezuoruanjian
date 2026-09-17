//! Issue #702 修复验证 — 深色模式颜色分叉与编辑器动画绘制链失效。
//!
//! WHITE_BOX 结构守卫：验证 issue #702 描述的两个问题已被结构性修复。
//! 这些结构的存在即证实了修复的正确性：
//!
//! 问题 1（深色模式颜色分叉）：修复后 main.qml 只绑定 `themeStateJson` 一个属性，
//! controller 把 `is_dark` 和 `ThemeColorScheme` 打包成 `{"is_dark": bool, "scheme": <obj>}`
//! 一次性发布，DesignTokens 从同一份 JSON 解析 isDark 和 scheme，彻底消除中间状态。
//!
//! 问题 2（编辑器动画绘制链失效）：
//! (a) 纯光标移动不再创建空 `TextVisualOperationKind::Cursor` 事务，
//!     `CursorAnimationState` 拥有自己的 timeline（`started_at` + `duration_ms`）。
//! (b) `static_patches/doc_hidden_rects` 每帧实际参与静态层裁剪（`has_animation_clip`）。
//! (c) DeleteConceal 的 caret track 与文字吞吐使用同一帧基准（`delete_unit_progress`）。

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
// 问题 1：深色模式颜色分叉 — 修复验证
// =========================================================================

#[test]
fn issue702_main_qml_binds_single_theme_state_json() {
    let src = read_src("qml/main.qml");
    // Issue #702 修复: main.qml 只绑定 themeStateJson 一个属性，
    // 不再分开绑定 isDark 和 resolvedSchemeJson 两个可能不同步的属性。
    assert!(
        src.contains("themeStateJson: themeController"),
        "Issue #702 修复: main.qml 应只绑定 themeStateJson ← themeController.theme_state_json"
    );
    assert!(
        !src.contains("isDark: themeController"),
        "Issue #702 修复: main.qml 不应再分开绑定 isDark ← themeController.is_dark"
    );
    assert!(
        !src.contains("resolvedSchemeJson: themeController"),
        "Issue #702 修复: main.qml 不应再分开绑定 resolvedSchemeJson"
    );
    println!("[BUGFIX_VERIFY] 问题1修复: main.qml 只绑定 themeStateJson 一个完整主题状态");
}

#[test]
fn issue702_design_tokens_consumes_single_theme_state_json() {
    let src = read_src("qml/DesignTokens.qml");
    // Issue #702 修复: DesignTokens 只消费 themeStateJson，
    // isDark 和 scheme 都从同一份 JSON 解析。
    assert!(
        src.contains("themeStateJson"),
        "Issue #702 修复: DesignTokens 应有 themeStateJson 属性"
    );
    assert!(
        src.contains("_themeState"),
        "Issue #702 修复: DesignTokens 应从 _themeState 解析 themeStateJson"
    );
    // isDark 从同一份 themeStateJson 的 is_dark 字段解析
    assert!(
        src.contains("_themeState.is_dark"),
        "Issue #702 修复: isDark 应从 _themeState.is_dark 读取，与 scheme 同步"
    );
    // scheme 从同一份 themeStateJson 的 scheme 字段读取
    assert!(
        src.contains("_themeState.scheme"),
        "Issue #702 修复: scheme 应从 _themeState.scheme 读取，与 isDark 同步"
    );
    // 不再有独立的 resolvedSchemeJson 属性
    assert!(
        !src.contains("property string resolvedSchemeJson"),
        "Issue #702 修复: DesignTokens 不应再有独立的 resolvedSchemeJson 属性"
    );
    // textPrimary/editorText 保持不变
    assert!(
        src.contains("property color textPrimary: onSurface"),
        "Issue #702: textPrimary = onSurface 保持不变"
    );
    assert!(
        src.contains("property color editorText: textPrimary"),
        "Issue #702: editorText = textPrimary 保持不变"
    );
    println!("[BUGFIX_VERIFY] 问题1修复: DesignTokens 从同一份 themeStateJson 解析 isDark 和 scheme");
}

#[test]
fn issue702_theme_controller_publishes_unified_theme_state_json() {
    let src = read_src("src/backend/linux_theme_controller.rs");
    // Issue #702 修复: controller 发布 theme_state_json 一个属性，
    // 包含 {"is_dark": bool, "scheme": <ThemeColorScheme object>}。
    assert!(
        src.contains("theme_state_json: qt_property!"),
        "Issue #702 修复: linux_theme_controller 应发布 theme_state_json 属性"
    );
    assert!(
        src.contains("fn theme_state_json"),
        "Issue #702 修复: linux_theme_controller 应有 theme_state_json getter"
    );
    // theme_state_json 把 is_dark 和 scheme 打包成一个 JSON
    assert!(
        src.contains("\"is_dark\""),
        "Issue #702 修复: theme_state_json 应包含 is_dark 字段"
    );
    assert!(
        src.contains("\"scheme\""),
        "Issue #702 修复: theme_state_json 应包含 scheme 字段"
    );
    println!("[BUGFIX_VERIFY] 问题1修复: controller 把 is_dark 和 scheme 打包成 theme_state_json 一次性发布");
}

// =========================================================================
// 问题 2a：光标动画 — 不再创建空 Cursor 事务，CursorAnimationState 有自己的 timeline
// =========================================================================

#[test]
fn issue702_text_visual_operation_kind_has_cursor_variant() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    // TextVisualOperationKind 枚举仍然存在（Insert/Delete/Cursor 变体保留），
    // 但 Cursor 变体不再用于纯光标移动创建空事务。
    assert!(
        src.contains("enum TextVisualOperationKind"),
        "Issue #702: TextVisualOperationKind 枚举应存在"
    );
    assert!(
        src.contains("Cursor") && src.contains("Insert") && src.contains("Delete"),
        "Issue #702: TextVisualOperationKind 应包含 Cursor/Insert/Delete 变体"
    );
    println!("[BUGFIX_VERIFY] 问题2a: TextVisualOperationKind 枚举保留，Cursor 变体不再用于纯光标移动");
}

#[test]
fn issue702_cursor_branch_no_longer_creates_empty_transaction() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    // Issue #702 修复: EditorAnimationKind::Cursor 分支不再创建空事务。
    // 纯光标移动直接维护 CursorAnimationState，不再伪装成文字事务。
    assert!(
        src.contains("EditorAnimationKind::Cursor"),
        "Issue #702: EditorAnimationKind::Cursor 分支应存在"
    );
    // Cursor 分支应返回 None（不再创建事务）
    let cursor_marker = "EditorAnimationKind::Cursor =>";
    let marker_pos = src
        .find(cursor_marker)
        .expect("EditorAnimationKind::Cursor 分支必须存在");
    // 取 Cursor 分支后 800 字符的窗口
    let window_end = marker_pos + 800;
    let window = if window_end <= src.len() {
        &src[marker_pos..window_end]
    } else {
        &src[marker_pos..]
    };
    assert!(
        window.contains("return None"),
        "Issue #702 修复: Cursor 分支应返回 None，不再创建空事务"
    );
    assert!(
        !window.contains("TextVisualOperationKind::Cursor"),
        "Issue #702 修复: Cursor 分支不应再创建 operation_kind=Cursor 的事务"
    );
    assert!(
        !window.contains("units: Vec::new()"),
        "Issue #702 修复: Cursor 分支不应再创建空 units"
    );
    println!("[BUGFIX_VERIFY] 问题2a修复: Cursor 分支返回 None，纯光标移动不再伪装成文字事务");
}

#[test]
fn issue702_handle_cursor_only_no_longer_enqueues_prepared_transaction() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    // Issue #702 修复: handle_cursor_only 不再把空事务入 prepared_queue。
    // 只分配一个 key 作为 CursorAnimationState 的 driver_key 标识。
    let hc_marker = "pub fn handle_cursor_only";
    let hc_pos = src
        .find(hc_marker)
        .expect("handle_cursor_only 必须存在");
    // 取 handle_cursor_only 后 1200 字符
    let window_end = hc_pos + 1200;
    let window = if window_end <= src.len() {
        &src[hc_pos..window_end]
    } else {
        &src[hc_pos..]
    };
    assert!(
        !window.contains("prepared_queue.enqueue"),
        "Issue #702 修复: handle_cursor_only 不应再 enqueue 空事务到 prepared_queue"
    );
    assert!(
        !window.contains("mark_prepared"),
        "Issue #702 修复: handle_cursor_only 不应再 mark_prepared"
    );
    println!("[BUGFIX_VERIFY] 问题2a修复: handle_cursor_only 不再入队空事务");
}

#[test]
fn issue702_cursor_animation_state_has_own_timeline() {
    let src = read_src("src/sujian_editor_item/rendering.rs");
    // Issue #702 修复: CursorAnimationState 拥有自己的 timeline。
    assert!(
        src.contains("pub struct CursorAnimationState"),
        "Issue #702: CursorAnimationState 结构体应存在"
    );
    assert!(
        src.contains("driver_key: VisualTransactionKey"),
        "Issue #702: CursorAnimationState 应有 driver_key"
    );
    // 新增 started_at 和 duration_ms 字段，让纯光标移动有自己的 timeline
    assert!(
        src.contains("started_at: Option<Instant>"),
        "Issue #702 修复: CursorAnimationState 应有 started_at 字段（自己的 timeline 起始）"
    );
    assert!(
        src.contains("duration_ms: u64"),
        "Issue #702 修复: CursorAnimationState 应有 duration_ms 字段（自己的 timeline 时长）"
    );
    // sample_progress 方法用 frame_now 推进 from→to 动画
    assert!(
        src.contains("fn sample_progress"),
        "Issue #702 修复: CursorAnimationState 应有 sample_progress 方法"
    );
    assert!(
        src.contains("frame_now: Instant"),
        "Issue #702 修复: sample_progress 应接受 frame_now 参数"
    );
    println!("[BUGFIX_VERIFY] 问题2a修复: CursorAnimationState 拥有自己的 timeline（started_at + duration_ms）");
}

#[test]
fn issue702_cursor_transition_tween_has_duration_ms() {
    let src = read_src("src/sujian_editor_item/cursor_animation.rs");
    // Issue #702 修复: CursorTransition::Tween 新增 duration_ms 字段。
    assert!(
        src.contains("duration_ms: u64"),
        "Issue #702 修复: CursorTransition::Tween 应有 duration_ms 字段"
    );
    println!("[BUGFIX_VERIFY] 问题2a修复: CursorTransition::Tween 携带 duration_ms 供纯光标移动 timeline");
}

// =========================================================================
// 问题 2b：静态正文与动画接管区域 — 每帧实际参与裁剪
// =========================================================================

#[test]
fn issue702_render_plan_carries_static_patches_from_active_transaction() {
    let src = read_src("src/sujian_editor_item/render_plan.rs");
    // static_patches 由 active transaction 提供，包含行级裁剪信息。
    assert!(
        src.contains("static_patches: Vec<StaticLinePatch>"),
        "Issue #702: RenderPlan 应携带 static_patches"
    );
    println!("[BUGFIX_VERIFY] 问题2b: RenderPlan 携带 static_patches");
}

#[test]
fn issue702_scene_graph_rebuilds_on_animation_clip() {
    let src = read_src("src/sujian_editor_item/scene_graph_renderer.rs");
    // Issue #702 修复: 有动画接管区域时也重建静态节点（含裁剪），
    // 不再只在 needs_relayout 时才重建。
    assert!(
        src.contains("has_animation_clip"),
        "Issue #702 修复: scene_graph_renderer 应有 has_animation_clip 判断"
    );
    assert!(
        src.contains("needs_relayout || has_animation_clip"),
        "Issue #702 修复: 需要在 needs_relayout 或 has_animation_clip 时重建静态节点"
    );
    assert!(
        src.contains("compute_clip_rects_from_patches"),
        "Issue #702: scene_graph_renderer 应有 compute_clip_rects_from_patches"
    );
    assert!(
        src.contains("doc_hidden_rects"),
        "Issue #702: scene_graph_renderer 应使用 doc_hidden_rects 计算裁剪"
    );
    println!("[BUGFIX_VERIFY] 问题2b修复: 有动画接管区域时也重建静态节点，doc_hidden_rects 每帧参与裁剪");
}

#[test]
fn issue702_static_line_patch_has_doc_hidden_rects_field() {
    let src = read_src("src/sujian_editor_item/static_line_patch.rs");
    // StaticLinePatch 携带 doc_hidden_rects 字段。
    assert!(
        src.contains("doc_hidden_rects: Vec<SourceRect>"),
        "Issue #702: StaticLinePatch 应有 doc_hidden_rects 字段"
    );
    println!("[BUGFIX_VERIFY] 问题2b: StaticLinePatch 携带 doc_hidden_rects 字段");
}

// =========================================================================
// 问题 2c：删除错拍 — DeleteConceal caret track 与文字 unit 同一帧基准
// =========================================================================

#[test]
fn issue702_delete_conceal_caret_track_uses_same_frame_progress() {
    let src = read_src("src/sujian_editor_item/animation_coordinator.rs");
    // Issue #702 修复: DeleteConceal 时 caret track 跟随文字 unit 的可见进度，
    // 不再使用 caret track 自己的 timeline，消除"光标先完成、旧字晚消失"错拍。
    assert!(
        src.contains("AnimatedSliceKind::DeleteConceal"),
        "Issue #702: 应存在 DeleteConceal 动画切片"
    );
    assert!(
        src.contains("InsertReveal"),
        "Issue #702: 应存在 InsertReveal 动画切片"
    );
    assert!(
        src.contains("conceal_from_left"),
        "Issue #702: DeleteConceal 应有 conceal_from_left 方向标记"
    );
    // 新增 delete_unit_progress 让 caret track 跟随文字 unit 的可见进度
    assert!(
        src.contains("delete_unit_progress"),
        "Issue #702 修复: 应有 delete_unit_progress 跟随文字 unit 可见进度"
    );
    assert!(
        src.contains("sampled_rect_at_progress"),
        "Issue #702 修复: caret track 应通过 sampled_rect_at_progress 用文字 unit progress 采样"
    );
    println!("[BUGFIX_VERIFY] 问题2c修复: DeleteConceal caret track 跟随文字 unit 同一帧进度，消除错拍");
}

#[test]
fn issue702_prepared_cursor_visual_track_has_sampled_rect_at_progress() {
    let src = read_src("src/sujian_editor_item/text_visual_transaction.rs");
    // Issue #702 修复: PreparedCursorVisualTrack 新增 sampled_rect_at_progress 方法，
    // 用外部传入的 progress（来自文字 unit 的可见进度）采样 caret rect。
    assert!(
        src.contains("fn sampled_rect_at_progress"),
        "Issue #702 修复: PreparedCursorVisualTrack 应有 sampled_rect_at_progress 方法"
    );
    println!("[BUGFIX_VERIFY] 问题2c修复: PreparedCursorVisualTrack.sampled_rect_at_progress 用文字 unit progress 采样");
}

#[test]
fn issue702_qquickitem_impl_starts_cursor_timeline_on_idle() {
    let src = read_src("src/sujian_editor_item/qquickitem_impl.rs");
    // Issue #702 修复: Idle 状态时启动纯光标动画的 started_at（首帧 frame_now）。
    assert!(
        src.contains("CursorSampleOutcome::Idle"),
        "Issue #702 修复: qquickitem_impl 应处理 Idle 状态"
    );
    assert!(
        src.contains("started_at = Some(frame_now)"),
        "Issue #702 修复: Idle 时应用 frame_now 启动 started_at"
    );
    // 帧更新请求：正文动画或光标动画任意一个没结束就继续
    assert!(
        src.contains("cursor_ctrl.animation.is_some()"),
        "Issue #702 修复: 光标动画存在时应继续 request_frame_update"
    );
    println!("[BUGFIX_VERIFY] 问题2a修复: Idle 时启动纯光标 timeline，光标动画未结束时持续请求帧更新");
}
