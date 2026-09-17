//! Issue #705 复现测试 — 主题状态与当前帧光标几何的事实源未收口。
//!
//! 本测试为 WHITE_BOX 结构守卫复现:验证当前实现中仍存在 Issue #705 描述的
//! "重复事实源",从而确定性复现两个子问题:
//!
//! 问题 1(主题文字不可见):Core/Qt 设置链同时存在 `theme_mode` 和
//! `appearance_mode` 两套主题状态事实源。运行时一边认为当前是 dark,
//! 另一边仍按 system 重新解析,即使 `isDark=true`,`on_surface/editorText`
//! 仍可能来自错误的一套 scheme,导致深色背景上文字不可见。
//!
//! 问题 2(光标几何错位):光标 visual position 与当前 render generation 的
//! `QTextLine.cursorToX()` 不严格同源,鼠标点击命中可能用临时排版而非
//! 当前 generation 的 `xToCursor()`,`RenderPlan` 缺少 `drawn_caret_rect`
//! 字段,下一次事务起点与屏幕这一帧真正画出的光标位置不是严格同一个值。
//!
//! 复现语义:每个测试断言 Issue #705 期望的正确结构。当前(未修复)代码
//! 违反这些断言 → 测试 FAIL → 缺陷复现成功。测试输出携带
//! `[BUGFIX_REPRO_TRACE]` 诊断行,记录观察到的重复来源/缺失字段。

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

/// 在源码中统计某个模式的出现次数(非重叠)。
fn count_occurrences(src: &str, needle: &str) -> usize {
    src.matches(needle).count()
}

// =========================================================================
// 问题 1:主题状态第二套事实源 — 深色模式下文字不可见
// =========================================================================

/// 复现 1a:Core `LocalSettings` 同时存在 `theme_mode` 和 `appearance_mode`
/// 两个字段。Issue #705 要求运行时只认 `appearance_mode`,`theme_mode`
/// 只能用于加载旧设置时的一次性迁移,不应继续作为运行时第二套事实源。
///
/// 当前代码:`core/writer_core/src/settings/mod.rs` 的 `LocalSettings`
/// 同时有 `pub theme_mode: Option<String>` 和 `pub appearance_mode: String`。
/// 断言"不应有运行时 theme_mode 字段"在当前代码上 FAIL → 复现成功。
#[test]
fn issue705_repro_1a_local_settings_has_runtime_theme_mode_duplicate() {
    let src = read_src("../../core/writer_core/src/settings/mod.rs");
    // Issue #705 期望:LocalSettings 运行时只认 appearance_mode,
    // theme_mode 不应作为运行时字段继续存在(只能做一次性迁移)。
    let has_theme_mode_field = src.contains("pub theme_mode: Option<String>");
    let has_appearance_mode_field = src.contains("pub appearance_mode: String");
    println!(
        "[BUGFIX_REPRO_TRACE] 1a local_settings: theme_mode_field={} appearance_mode_field={}",
        has_theme_mode_field, has_appearance_mode_field
    );
    assert!(
        has_appearance_mode_field,
        "前提:LocalSettings 必须有 appearance_mode 字段"
    );
    // 复现断言:theme_mode 不应作为运行时字段存在。当前代码有 → FAIL → 复现。
    assert!(
        !has_theme_mode_field,
        "Issue #705 复现 1a: LocalSettings 仍同时持有运行时 theme_mode 字段, \
         与 appearance_mode 形成两套主题状态事实源。运行时一边按 theme_mode \
         判断 dark,另一边按 appearance_mode 重新解析,即使 isDark=true, \
         on_surface/editorText 仍可能来自错误的一套 scheme,导致深色背景上 \
         文字不可见。"
    );
}

/// 复现 1b:`SettingsBackend` 仍把 `setting_theme_mode` 暴露成 QML 属性。
/// Issue #705 要求把现在还能形成第二套主题判断的读取点删掉,不要只是把
/// 字段留着"暂时不用"。
///
/// 当前代码:`apps/Linux_qt/src/backend/settings_backend.rs` 有
/// `setting_theme_mode: qt_property!(...)`。断言"不应暴露"在当前代码上
/// FAIL → 复现成功。
#[test]
fn issue705_repro_1b_settings_backend_exposes_theme_mode_qml_property() {
    let src = read_src("src/backend/settings_backend.rs");
    let exposes_theme_mode = src.contains("setting_theme_mode: qt_property!");
    println!(
        "[BUGFIX_REPRO_TRACE] 1b settings_backend exposes setting_theme_mode qml property: {}",
        exposes_theme_mode
    );
    assert!(
        !exposes_theme_mode,
        "Issue #705 复现 1b: SettingsBackend 仍把 setting_theme_mode 暴露成 QML 属性, \
         形成运行时第二套主题判断出口。Issue 要求删掉这类读取点,不要留着\"暂时不用\"。"
    );
}

/// 复现 1c:`AppBackend` 持有独立的 `current_setting_theme_mode` 字段,且
/// `set_setting_appearance_mode` 只更新 `current_setting_appearance_mode`,
/// 不同步更新 `current_setting_theme_mode`。两套字段可不同步,是主题状态
/// 分叉的运行时根源。
///
/// 当前代码:`app_backend.rs` 有 `current_setting_theme_mode: String` 和
/// `current_setting_appearance_mode: String` 两个独立字段。
#[test]
fn issue705_repro_1c_app_backend_has_independent_theme_mode_field() {
    let src = read_src("src/backend/app_backend.rs");
    let has_theme_mode = src.contains("current_setting_theme_mode: String");
    let has_appearance_mode = src.contains("current_setting_appearance_mode: String");
    println!(
        "[BUGFIX_REPRO_TRACE] 1c app_backend: current_setting_theme_mode={} current_setting_appearance_mode={}",
        has_theme_mode, has_appearance_mode
    );
    assert!(
        has_appearance_mode,
        "前提:AppBackend 必须有 current_setting_appearance_mode"
    );
    assert!(
        !has_theme_mode,
        "Issue #705 复现 1c: AppBackend 仍持有独立的 current_setting_theme_mode 字段, \
         与 current_setting_appearance_mode 并存。set_setting_appearance_mode 只更新 \
         appearance_mode 侧,theme_mode 侧可保留旧值,两套字段不同步即主题状态分叉。"
    );
}

/// 复现 1d:`set_setting_appearance_mode` 不同步更新
/// `current_setting_theme_mode`。即使两套字段并存,只要 setter 只写一份,
/// 就存在分叉窗口。Issue #705 要求运行时只认一份。
///
/// 当前代码:`app_backend.rs` 有 `current_setting_theme_mode` 字段,且
/// `set_setting_appearance_mode` 不同步更新它 → 断言 FAIL → 复现。
/// 修复后:`current_setting_theme_mode` 字段被删除 → 前提不满足 → 跳过(PASS)。
#[test]
fn issue705_repro_1d_set_appearance_mode_does_not_sync_theme_mode() {
    let src = read_src("src/backend/app_backend.rs");
    // 前提:AppBackend 持有 current_setting_theme_mode 字段
    let has_theme_mode_field = src.contains("current_setting_theme_mode: String");
    if !has_theme_mode_field {
        // 修复后字段已删除,前提不满足,跳过
        println!(
            "[BUGFIX_REPRO_TRACE] 1d app_backend current_setting_theme_mode field removed -> skip"
        );
        return;
    }
    // 字段存在时,set_setting_appearance_mode 应同步更新它(否则两套字段分叉)
    let setter_src = read_src("src/backend/settings_backend.rs");
    let setter_marker = "fn set_setting_appearance_mode";
    let marker_pos = setter_src
        .find(setter_marker)
        .expect("set_setting_appearance_mode 必须存在");
    let window_end = marker_pos + 400;
    let window = if window_end <= setter_src.len() {
        &setter_src[marker_pos..window_end]
    } else {
        &setter_src[marker_pos..]
    };
    let syncs_theme_mode = window.contains("current_setting_theme_mode");
    println!(
        "[BUGFIX_REPRO_TRACE] 1d set_setting_appearance_mode syncs current_setting_theme_mode: {}",
        syncs_theme_mode
    );
    assert!(
        syncs_theme_mode,
        "Issue #705 复现 1d: AppBackend 持有 current_setting_theme_mode 字段,但 \
         set_setting_appearance_mode 不同步更新它。两套字段在 setter 路径上分叉, \
         是深色模式下 isDark 与 on_surface/editorText 不同源的运行时根源。"
    );
}

// =========================================================================
// 问题 2:光标几何不唯一源 — 光标错位 / 鼠标点击动画时好时坏
// =========================================================================

/// 复现 2a:`RenderPlan` 缺少 `drawn_caret_rect` 字段。Issue #705 要求
/// `RenderPlan` 增加明确的"本帧真正绘制出去的 caret rect",例如
/// `drawn_caret_rect`。`qquickitem_impl.rs` 每帧生成 RenderPlan 后,把
/// `cursor_ctrl.visual_x/visual_y/visual_h` 同步成 `drawn_caret_rect`。
/// 下一次输入、删除、鼠标点击创建新事务时,只允许从这个"上一帧真正
/// 画出来的位置" rebase。
///
/// 当前代码:`render_plan.rs` 的 `RenderPlan` 没有 `drawn_caret_rect` 字段。
#[test]
fn issue705_repro_2a_render_plan_missing_drawn_caret_rect() {
    let src = read_src("src/sujian_editor_item/render_plan.rs");
    let has_drawn_caret_rect = src.contains("drawn_caret_rect");
    println!(
        "[BUGFIX_REPRO_TRACE] 2a render_plan has drawn_caret_rect field: {}",
        has_drawn_caret_rect
    );
    assert!(
        has_drawn_caret_rect,
        "Issue #705 复现 2a: RenderPlan 缺少 drawn_caret_rect 字段。 \
         当前 RenderPlan 只有 cursor: CursorRenderState (从 cursor_ctrl.visual_x/y/h 构造) \
         和 cursor_sample_outcome,没有明确的\"本帧真正绘制出去的 caret rect\"。 \
         下一次事务起点无法从上一帧真正画出的位置 rebase,导致光标错位。"
    );
}

/// 复现 2b:`hit_test`(鼠标点击命中)调用 `self.layout_snapshot(width)`,
/// 该方法在 cache 失效时会 `clear_layout_generation` + `begin_layout_generation`
/// 重新排版,而非使用当前 render generation 对应的同一份已排版
/// `QTextLayout/QTextLine`。Issue #705 要求鼠标点击命中必须使用当前
/// render generation 的同一份已排版 layout,不允许临时排一遍文字。
///
/// 当前代码:`layout_ops.rs` 的 `hit_test` 调用 `self.layout_snapshot(width)`。
#[test]
fn issue705_repro_2b_hit_test_uses_layout_snapshot_not_current_render_generation() {
    let src = read_src("src/sujian_editor_item/layout_ops.rs");
    // hit_test 实现体
    let marker = "fn hit_test(&mut self, x: f64, y: f64)";
    let marker_pos = src.find(marker).expect("hit_test 必须存在");
    let window_end = marker_pos + 400;
    let window = if window_end <= src.len() {
        &src[marker_pos..window_end]
    } else {
        &src[marker_pos..]
    };
    let calls_layout_snapshot = window.contains("self.layout_snapshot(");
    let uses_prepared_frame = window.contains("prepared_frame");
    println!(
        "[BUGFIX_REPRO_TRACE] 2b hit_test calls layout_snapshot={} uses_prepared_frame={}",
        calls_layout_snapshot, uses_prepared_frame
    );
    assert!(
        calls_layout_snapshot,
        "前提:hit_test 当前调用 self.layout_snapshot(width)"
    );
    assert!(
        !calls_layout_snapshot || uses_prepared_frame,
        "Issue #705 复现 2b: hit_test 调用 self.layout_snapshot(width) 临时排版, \
         而非使用当前 render generation 对应的 prepared_frame.layout_snapshot。 \
         layout_snapshot 在 cache 失效时会 clear_layout_generation + \
         begin_layout_generation 重新排版,鼠标点击命中用的 generation 与 \
         屏幕这一帧真正画出的 generation 可能不同,导致点击命中错位。"
    );
}

/// 复现 2c:`build_editor_layout_snapshot` 调用 `begin_layout_generation()`
/// 分配独立 generation 临时排版。Issue #705 要求正文静态绘制、cursorToX()、
/// xToCursor()、鼠标点击命中都必须使用当前 render generation 对应的同一份
/// 已排版 layout,不允许为了算光标位置再临时排一遍文字,也不要拿旧
/// generation 的行数据和新 generation 的正文混用。
///
/// 当前代码:`layout_ops.rs` 的 `build_editor_layout_snapshot` 调用
/// `begin_layout_generation()`。
#[test]
fn issue705_repro_2c_build_editor_layout_snapshot_allocs_new_generation() {
    let src = read_src("src/sujian_editor_item/layout_ops.rs");
    let marker = "fn build_editor_layout_snapshot";
    let marker_pos = src
        .find(marker)
        .expect("build_editor_layout_snapshot 必须存在");
    let window_end = marker_pos + 2000;
    let window = if window_end <= src.len() {
        &src[marker_pos..window_end]
    } else {
        &src[marker_pos..]
    };
    let allocs_new_gen = window.contains("begin_layout_generation()");
    let new_gen_count = count_occurrences(window, "begin_layout_generation()");
    println!(
        "[BUGFIX_REPRO_TRACE] 2c build_editor_layout_snapshot allocs new generation: {} (count={})",
        allocs_new_gen, new_gen_count
    );
    assert!(
        !allocs_new_gen,
        "Issue #705 复现 2c: build_editor_layout_snapshot 调用 begin_layout_generation() \
         分配独立 generation 临时排版。编辑事务算光标位置用的 generation 与 \
         当前 render generation 不同,下一次事务的起点和屏幕这一帧真正画出来 \
         的光标位置不是严格同一个值,导致快速输入、不同宽度字符时光标错位。"
    );
}

/// 复现 2d:`qquickitem_impl.rs` 每帧生成 RenderPlan 后,应把
/// `cursor_ctrl.visual_x/visual_y/visual_h` 同步成 `drawn_caret_rect`
/// (本帧真正绘制出去的 caret rect)。当前代码只从 `cursor_sample_outcome`
/// 的 `Coordinated { x, y, h }` 同步,没有从 `drawn_caret_rect` 同步,
/// 也没有把 `drawn_caret_rect` 作为下一次事务的 rebase 基准。
///
/// 当前代码:`qquickitem_impl.rs` 没有 `drawn_caret_rect` 相关同步逻辑。
#[test]
fn issue705_repro_2d_qquickitem_impl_does_not_sync_from_drawn_caret_rect() {
    let src = read_src("src/sujian_editor_item/qquickitem_impl.rs");
    let references_drawn_caret_rect = src.contains("drawn_caret_rect");
    println!(
        "[BUGFIX_REPRO_TRACE] 2d qquickitem_impl references drawn_caret_rect: {}",
        references_drawn_caret_rect
    );
    assert!(
        references_drawn_caret_rect,
        "Issue #705 复现 2d: qquickitem_impl.rs 没有从 drawn_caret_rect 同步 \
         cursor_ctrl.visual_x/visual_y/visual_h 的逻辑。当前只从 \
         cursor_sample_outcome::Coordinated 同步,没有明确的\"本帧真正绘制出去 \
         的 caret rect\"作为下一次事务 rebase 基准,target_x/target_y 仍可能被 \
         拿来当当前屏幕位置,导致光标错位。"
    );
}

/// 复现 2e:`editing.rs` 的鼠标点击路径 `click_at` 在普通单击时不强制
/// `force_snap_next`,但 `drag_select_at`/`long_press_at`/`select_word_at`
/// 仍各自设置 `force_snap_next = true`。Issue #705 要求鼠标点击路径里
/// 不要自己单独决定光标动画模式,是否 Tween 由统一的光标移动规则决定。
///
/// 当前代码:`editing.rs` 的 `drag_select_at`/`long_press_at`/`select_word_at`
/// 各自设置 `force_snap_next = true`,形成点击路径里的特殊分支。
#[test]
fn issue705_repro_2e_click_path_has_per_method_force_snap_special_branches() {
    let src = read_src("src/sujian_editor_item/editing.rs");
    // 统计 click 相关方法里 force_snap_next = true 的出现次数
    let force_snap_count = count_occurrences(&src, "self.cursor_ctrl.force_snap_next = true;");
    println!(
        "[BUGFIX_REPRO_TRACE] 2e editing.rs force_snap_next=true count in click paths: {}",
        force_snap_count
    );
    // Issue #705 要求鼠标点击路径里不要自己单独决定光标动画模式。
    // 当前 drag_select_at/long_press_at/select_word_at 各自 force_snap_next=true,
    // 形成多个特殊分支。期望:点击路径不应有多个 per-method force_snap 分支。
    assert!(
        force_snap_count <= 1,
        "Issue #705 复现 2e: editing.rs 鼠标点击路径有 {} 处 \
         self.cursor_ctrl.force_snap_next = true,形成多个 per-method 特殊分支 \
         (drag_select_at/long_press_at/select_word_at 各自决定光标动画模式)。 \
         Issue 要求是否 Tween 由统一的光标移动规则决定,不要在点击代码里自己 \
         强制 Snap。多个特殊分支导致鼠标点击后的光标动画时好时坏。",
        force_snap_count
    );
}
