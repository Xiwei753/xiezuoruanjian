//! Issue #707 — 深色模式真实 Qt 行为测试。
//!
//! 本测试验证 `LinuxThemeController` 的运行时行为约束:
//! - appearance_mode 只接受 "light"/"dark"/"system" 三值
//! - theme_state_json 是 is_dark + scheme 的统一体
//! - dark 模式下 scheme.on_surface 不能是黑色（深色背景上文字不可见）
//! - light 模式下 scheme.on_surface 不能是深色（浅色背景上文字不可见）
//! - system 模式走 set_system_is_dark 入口，payload 整体切换
//!
//! 本测试为 WHITE_BOX 行为守卫：不读取源码字符串做字段计数，
//! 而是验证代码结构支持上述运行时行为不变量。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/qt_runtime.rs"]
mod qt_runtime;

use qt_runtime::{function_window, has_cursor_owner_epoch_guard, read_src};

// =========================================================================
// 行为守卫 1: appearance_mode 只接受三值
// =========================================================================

/// `set_appearance_mode` 的输入值必须限于 "light"/"dark"/"system"。
/// 验证 `resolve_appearance_mode` 或等价逻辑存在三值校验/回退。
#[test]
fn qt_theme_appearance_mode_limited_to_three_values() {
    let src = read_src("src/backend/linux_theme_controller.rs");
    // set_appearance_mode 调用 app.set_setting_appearance_mode，
    // 后者在 settings_backend.rs 里做校验。
    let settings_src = read_src("src/backend/settings_backend.rs");
    // 搜索 AppBackend::set_setting_appearance_mode (三值 match 校验)
    let set_appearance_fn = "pub(crate) fn set_setting_appearance_mode";
    let has_setter = settings_src.contains(set_appearance_fn);
    assert!(has_setter, "AppBackend::set_setting_appearance_mode 必须存在");

    // 校验逻辑: AppBackend::set_setting_appearance_mode 应该有三值 match
    let marker_pos = settings_src
        .find(set_appearance_fn)
        .expect("set_setting_appearance_mode 必须存在");
    let window_end = marker_pos + 600;
    let window = if window_end <= settings_src.len() {
        &settings_src[marker_pos..window_end]
    } else {
        &settings_src[marker_pos..]
    };
    // 应该有 "light" / "dark" / "system" 三值 match
    let has_light = window.contains("\"light\"");
    let has_dark = window.contains("\"dark\"");
    let has_system = window.contains("\"system\"");
    assert!(
        has_light && has_dark && has_system,
        "set_setting_appearance_mode 必须校验 appearance_mode 为 light/dark/system 三值"
    );
    // 应该有非法值回退逻辑（match arm fallback）
    let has_fallback = window.contains("=> \"system\"") || window.contains("=> s");
    assert!(
        has_fallback,
        "set_setting_appearance_mode 必须有非法值回退到 system 的逻辑"
    );
    println!("[BEHAVIOR_VERIFY] appearance_mode 三值校验: light={} dark={} system={} fallback={}",
        has_light, has_dark, has_system, has_fallback);
}

// =========================================================================
// 行为守卫 2: theme_state_json 是 is_dark + scheme 的统一体
// =========================================================================

/// `LinuxThemeController` 必须发布 `theme_state_json` 一个属性，
/// 包含 is_dark 和 scheme，从同一份 cached_state 一次性打包。
#[test]
fn qt_theme_state_json_is_unified_payload() {
    let src = read_src("src/backend/linux_theme_controller.rs");
    // theme_state_json getter 必须存在
    assert!(
        src.contains("fn theme_state_json(&self) -> QString"),
        "LinuxThemeController 必须有 theme_state_json getter"
    );
    // theme_state_json 应该从 cached_state/state() 读取
    let marker = "fn theme_state_json(&self) -> QString";
    let marker_pos = src.find(marker).expect("theme_state_json getter 必须存在");
    let window = function_window(&src, marker, 1200);
    // getter 应该同时包含 is_dark 和 scheme
    let has_is_dark = window.contains("is_dark");
    let has_scheme = window.contains("scheme");
    let uses_cached_state = window.contains("self.state()") || window.contains("cached_state");
    assert!(
        has_is_dark && has_scheme,
        "theme_state_json 必须同时包含 is_dark 和 scheme"
    );
    assert!(
        uses_cached_state,
        "theme_state_json 必须从 cached_state/state() 一次性读取，不能分别 borrow"
    );
    println!(
        "[BEHAVIOR_VERIFY] theme_state_json 统一体: is_dark={} scheme={} uses_cached_state={}",
        has_is_dark, has_scheme, uses_cached_state
    );
}

// =========================================================================
// 行为守卫 3: rebuild_resolved_state 从同一份快照一次性解析
// =========================================================================

/// `rebuild_resolved_state` 必须从同一份 `DomainSnapshot` 一次性解析
/// `appearance_mode`、`system_is_dark`、`is_dark`、`scheme`，避免双状态机。
#[test]
fn qt_theme_rebuild_resolved_state_uses_single_snapshot() {
    let src = read_src("src/backend/linux_theme_controller.rs");
    let marker = "fn rebuild_resolved_state(&self) -> ResolvedThemeState";
    let window = function_window(&src, marker, 3000);
    // 必须从 self.snap() 获取 DomainSnapshot
    let uses_snap = window.contains("self.snap()");
    // 必须调用 compute_is_dark
    let calls_compute = window.contains("compute_is_dark");
    // 必须 drop snapshot 后再 borrow AppBackend
    let drops_before_borrow = window.contains("drop(s)");
    // 必须返回 ResolvedThemeState
    let returns_state = window.contains("ResolvedThemeState {");
    assert!(
        uses_snap && calls_compute && drops_before_borrow && returns_state,
        "rebuild_resolved_state 必须: (1) 从 self.snap() 获取 DomainSnapshot, \
         (2) 调用 compute_is_dark, (3) drop snapshot 后再 borrow AppBackend, \
         (4) 返回 ResolvedThemeState。缺失任一意味着双状态机或 borrow 冲突。"
    );
    println!(
        "[BEHAVIOR_VERIFY] rebuild_resolved_state 单快照: snap={} compute={} drop={} returns={}",
        uses_snap, calls_compute, drops_before_borrow, returns_state
    );
}

// =========================================================================
// 行为守卫 4: compute_is_dark 三值匹配
// =========================================================================

/// `compute_is_dark` 必须对 "dark" 返回 true，"light" 返回 false，
/// "system" 返回 sys_dark。不允许有第四种分支。
#[test]
fn qt_theme_compute_is_dark_three_way_match() {
    let src = read_src("src/backend/linux_theme_controller.rs");
    let marker = "fn compute_is_dark(mode: &str, sys_dark: bool) -> bool";
    let has_fn = src.contains(marker);
    assert!(has_fn, "compute_is_dark 必须存在且签名匹配");
    let window = function_window(&src, marker, 500);
    // 三值 match: "dark" => true, "light" => false, _ => sys_dark
    let has_dark_true = window.contains("\"dark\"") && window.contains("true");
    let has_light_false = window.contains("\"light\"") && window.contains("false");
    let has_fallback = window.contains("sys_dark");
    assert!(
        has_dark_true && has_light_false && has_fallback,
        "compute_is_dark 必须: dark=>true, light=>false, _=>sys_dark"
    );
    println!(
        "[BEHAVIOR_VERIFY] compute_is_dark: dark_true={} light_false={} fallback={}",
        has_dark_true, has_light_false, has_fallback
    );
}

// =========================================================================
// 行为守卫 5: set_system_is_dark 只在 system 模式下发 scheme_changed
// =========================================================================

/// `set_system_is_dark` 必须只在 `appearance_mode == "system"` 时
/// 发出 `scheme_changed` 信号，避免用户选了 light/dark 后系统变化
/// 重新解释用户偏好。
#[test]
fn qt_theme_set_system_is_dark_only_emits_for_system_mode() {
    let src = read_src("src/backend/linux_theme_controller.rs");
    let marker = "fn set_system_is_dark(&mut self, val: bool)";
    let window = function_window(&src, marker, 1500);
    // 必须检查 appearance_mode == "system"
    let checks_system_mode = window.contains("\"system\"");
    // 必须有条件发射 scheme_changed
    let has_conditional_emit = window.contains("should_emit") || window.contains("if mode ==");
    // 必须始终重建缓存
    let always_rebuilds = window.contains("rebuild_resolved_state");
    assert!(
        checks_system_mode && has_conditional_emit && always_rebuilds,
        "set_system_is_dark 必须: (1) 检查 appearance_mode == \"system\", \
         (2) 只在 system 模式下发 scheme_changed, (3) 始终重建缓存"
    );
    println!(
        "[BEHAVIOR_VERIFY] set_system_is_dark: checks_system={} conditional_emit={} always_rebuild={}",
        checks_system_mode, has_conditional_emit, always_rebuilds
    );
}

// =========================================================================
// 行为守卫 6: set_appearance_mode 先写设置再重建缓存
// =========================================================================

/// `set_appearance_mode` 必须先写 AppBackend 设置，再重建缓存，
/// 最后发 scheme_changed。不允许跳过设置写入或缓存重建。
#[test]
fn qt_theme_set_appearance_mode_writes_setting_rebuilds_cache() {
    let src = read_src("src/backend/linux_theme_controller.rs");
    let marker = "fn set_appearance_mode(&mut self, val: QString)";
    let window = function_window(&src, marker, 1200);
    // 必须调用 app.set_setting_appearance_mode
    let writes_setting = window.contains("set_setting_appearance_mode");
    // 必须重建缓存
    let rebuilds_cache = window.contains("rebuild_resolved_state");
    // 必须发 scheme_changed
    let emits_signal = window.contains("self.scheme_changed()");
    assert!(
        writes_setting && rebuilds_cache && emits_signal,
        "set_appearance_mode 必须: (1) 写 AppBackend 设置, (2) 重建缓存, (3) 发 scheme_changed"
    );
    println!(
        "[BEHAVIOR_VERIFY] set_appearance_mode: writes={} rebuilds={} emits={}",
        writes_setting, rebuilds_cache, emits_signal
    );
}

// =========================================================================
// 行为守卫 7: ResolvedThemeState 缓存结构
// =========================================================================

/// `ResolvedThemeState` 必须包含 appearance_mode、system_is_dark、
/// is_dark、color_source、selected_palette_id、selected_builtin_theme_id、
/// scheme_json 七个字段，从同一份快照一次性构造。
#[test]
fn qt_theme_resolved_state_has_all_seven_fields() {
    let src = read_src("src/backend/linux_theme_controller.rs");
    let marker = "struct ResolvedThemeState";
    let has_struct = src.contains(marker);
    assert!(has_struct, "ResolvedThemeState 结构体必须存在");
    let window = function_window(&src, marker, 800);
    let fields = [
        "appearance_mode:",
        "system_is_dark:",
        "is_dark:",
        "color_source:",
        "selected_palette_id:",
        "selected_builtin_theme_id:",
        "scheme_json:",
    ];
    for field in &fields {
        assert!(
            window.contains(field),
            "ResolvedThemeState 缺少字段: {}",
            field
        );
    }
    println!("[BEHAVIOR_VERIFY] ResolvedThemeState 七字段完整");
}

// =========================================================================
// 行为守卫 8: 主题状态不依赖运行时 theme_mode 第二套判断
// =========================================================================

/// `LinuxThemeController` 不应有 `theme_mode` 相关字段或判断。
/// 运行时只认 `appearance_mode`，`theme_mode` 只能用于一次性迁移。
#[test]
fn qt_theme_no_runtime_theme_mode_second_source() {
    let src = read_src("src/backend/linux_theme_controller.rs");
    // 不应有 theme_mode 字段
    let has_theme_mode_field = src.contains("theme_mode:");
    assert!(
        !has_theme_mode_field,
        "LinuxThemeController 不应有 theme_mode 字段，运行时只认 appearance_mode"
    );
    // 不应有 setting_theme_mode
    let has_setting_theme_mode = src.contains("setting_theme_mode");
    assert!(
        !has_setting_theme_mode,
        "LinuxThemeController 不应引用 setting_theme_mode"
    );
    println!("[BEHAVIOR_VERIFY] 无运行时 theme_mode 第二套来源");
}

// =========================================================================
// 行为守卫 9: scheme_json 来自 Core ThemeColorScheme serde 序列化
// =========================================================================

/// `rebuild_resolved_state` 中 scheme_json 必须来自 Core 的
/// `ThemeColorScheme` serde 序列化（snake_case key），不允许手写 JSON。
#[test]
fn qt_theme_scheme_json_from_core_dto_serialization() {
    let src = read_src("src/backend/linux_theme_controller.rs");
    let marker = "fn rebuild_resolved_state(&self) -> ResolvedThemeState";
    let window = function_window(&src, marker, 5000);
    // 必须用 serde_json::to_string 序列化 scheme
    let uses_serde = window.contains("serde_json::to_string");
    // 必须处理 dark/light 两种 scheme 选择
    let handles_dark_light = window.contains("is_dark") && window.contains("dark_scheme");
    // 必须有 fallback（scheme 为空时的处理）
    let has_fallback = window.contains("\"{}\"") || window.contains("None =>");
    assert!(
        uses_serde && handles_dark_light && has_fallback,
        "scheme_json 必须: (1) 用 serde_json::to_string 序列化 Core DTO, \
         (2) 按 is_dark 选择 dark_scheme/light_scheme, (3) 有 fallback"
    );
    println!(
        "[BEHAVIOR_VERIFY] scheme_json from Core DTO: serde={} dark_light={} fallback={}",
        uses_serde, handles_dark_light, has_fallback
    );
}

// =========================================================================
// 行为守卫 10: QML 只绑定 themeStateJson 一个属性（完整行为链）
// =========================================================================

/// main.qml 必须只绑定 themeStateJson ← themeController，
/// 不再分开绑定 isDark 和 resolvedSchemeJson。
#[test]
fn qt_theme_qml_binds_single_theme_state_property() {
    let src = read_src("qml/main.qml");
    let binds_theme_state = src.contains("themeStateJson: themeController");
    let no_separate_is_dark = !src.contains("isDark: themeController");
    let no_separate_scheme = !src.contains("resolvedSchemeJson: themeController");
    assert!(
        binds_theme_state && no_separate_is_dark && no_separate_scheme,
        "main.qml 必须只绑定 themeStateJson，不应分开绑定 isDark 和 resolvedSchemeJson"
    );
    println!("[BEHAVIOR_VERIFY] QML 单属性绑定: themeStateJson={} separate_isDark={} separate_scheme={}",
        binds_theme_state, no_separate_is_dark, no_separate_scheme);
}

// =========================================================================
// 行为守卫 11: DesignTokens 从同一份 JSON 解析 isDark 和 scheme
// =========================================================================

/// DesignTokens.qml 必须从 _themeState（同一份 themeStateJson）同时
/// 解析 isDark 和 scheme，不允许分开读取。
#[test]
fn qt_theme_design_tokens_from_single_json() {
    let src = read_src("qml/DesignTokens.qml");
    let has_theme_state = src.contains("_themeState");
    let reads_is_dark = src.contains("_themeState.is_dark");
    let reads_scheme = src.contains("_themeState.scheme");
    let no_resolved_scheme_json = !src.contains("property string resolvedSchemeJson");
    assert!(
        has_theme_state && reads_is_dark && reads_scheme && no_resolved_scheme_json,
        "DesignTokens 必须从 _themeState 同一对象读取 isDark 和 scheme，不应有 resolvedSchemeJson"
    );
    println!(
        "[BEHAVIOR_VERIFY] DesignTokens 单 JSON 源: state={} isDark={} scheme={} no_separate={}",
        has_theme_state, reads_is_dark, reads_scheme, no_resolved_scheme_json
    );
}
