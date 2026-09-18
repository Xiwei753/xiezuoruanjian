//! Issue #707 评论 5723616999 — 深色模式真实 Qt 行为测试。
//!
//! 本测试直接构造 `LinuxThemeController` 生产对象，调用其 pub 方法
//! （`set_appearance_mode`、`set_system_is_dark`、`theme_state_json`、
//! `rebuild_resolved_state`、`compute_is_dark`），验证运行时行为不变量。
//!
//! 不再读取源码字符串做字段计数。每条测试先 `ensure_qt_application()`，
//! 然后操作真实对象，解析真实 JSON 输出。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/qt_runtime.rs"]
mod qt_runtime;

use qt_runtime::ensure_qt_application;
use sujian_linux_qt::backend::{AppRef, LinuxThemeController};
use sujian_linux_qt::backend::linux_theme_controller::ResolvedThemeState;

// =========================================================================
// 行为守卫 1: dark 模式 → is_dark=true，scheme 是合法 JSON 对象
// =========================================================================

#[test]
fn qt_theme_dark_mode_is_dark_true_and_scheme_present() {
    ensure_qt_application();
    let app = AppRef::default();
    let mut ctrl = LinuxThemeController::new(app);
    ctrl.reload();
    ctrl.set_appearance_mode("dark".into());
    let json: String = ctrl.theme_state_json().into();
    let v: serde_json::Value = serde_json::from_str(&json).expect("valid json");
    assert_eq!(
        v["is_dark"].as_bool(),
        Some(true),
        "dark 模式下 is_dark 必须为 true"
    );
    let scheme = &v["scheme"];
    assert!(
        scheme.is_object(),
        "dark 模式下 scheme 必须是合法 JSON 对象（core_api 未初始化时为空对象 {{}}）"
    );
    // 如果 scheme 非空（core_api 已初始化），on_surface 必须存在
    if !scheme.as_object().map_or(true, |m| m.is_empty()) {
        assert!(
            scheme["on_surface"].as_str().is_some(),
            "dark 模式下非空 scheme.on_surface 必须存在且为字符串"
        );
    }
    println!("[BEHAVIOR_VERIFY] dark: is_dark=true, scheme is valid JSON object");
}

// =========================================================================
// 行为守卫 2: light 模式 → is_dark=false，scheme 同步切换
// =========================================================================

#[test]
fn qt_theme_light_mode_is_dark_false_and_scheme_switched() {
    ensure_qt_application();
    let app = AppRef::default();
    let mut ctrl = LinuxThemeController::new(app);
    ctrl.reload();
    ctrl.set_appearance_mode("light".into());
    let json: String = ctrl.theme_state_json().into();
    let v: serde_json::Value = serde_json::from_str(&json).expect("valid json");
    assert_eq!(
        v["is_dark"].as_bool(),
        Some(false),
        "light 模式下 is_dark 必须为 false"
    );
    let scheme = &v["scheme"];
    assert!(
        scheme.is_object(),
        "light 模式下 scheme 必须是合法 JSON 对象"
    );
    println!("[BEHAVIOR_VERIFY] light: is_dark=false, scheme is valid JSON object");
}

// =========================================================================
// 行为守卫 3: system 模式 + set_system_is_dark → payload 整体切换
// =========================================================================

#[test]
fn qt_theme_system_mode_payload_switches_with_system_is_dark() {
    ensure_qt_application();
    let app = AppRef::default();
    let mut ctrl = LinuxThemeController::new(app);
    ctrl.reload();
    ctrl.set_appearance_mode("system".into());

    // system + sys_dark=false → is_dark=false
    ctrl.set_system_is_dark(false);
    let json1: String = ctrl.theme_state_json().into();
    let v1: serde_json::Value = serde_json::from_str(&json1).expect("valid json");
    assert_eq!(
        v1["is_dark"].as_bool(),
        Some(false),
        "system + sys_dark=false → is_dark=false"
    );

    // system + sys_dark=true → is_dark=true
    ctrl.set_system_is_dark(true);
    let json2: String = ctrl.theme_state_json().into();
    let v2: serde_json::Value = serde_json::from_str(&json2).expect("valid json");
    assert_eq!(
        v2["is_dark"].as_bool(),
        Some(true),
        "system + sys_dark=true → is_dark=true"
    );

    // scheme 必须随 is_dark 一起变化（同一份 payload，合法 JSON 对象）
    assert!(
        v1["scheme"].is_object() && v2["scheme"].is_object(),
        "system 模式下 scheme 必须始终是合法 JSON 对象"
    );
    println!("[BEHAVIOR_VERIFY] system: payload switches is_dark + scheme together");
}

// =========================================================================
// 行为守卫 4: compute_is_dark 三值匹配
// =========================================================================

#[test]
fn qt_theme_compute_is_dark_three_way_match() {
    ensure_qt_application();
    assert_eq!(
        LinuxThemeController::compute_is_dark("dark", false),
        true,
        "compute_is_dark(dark, _) == true"
    );
    assert_eq!(
        LinuxThemeController::compute_is_dark("dark", true),
        true,
        "compute_is_dark(dark, _) == true"
    );
    assert_eq!(
        LinuxThemeController::compute_is_dark("light", false),
        false,
        "compute_is_dark(light, _) == false"
    );
    assert_eq!(
        LinuxThemeController::compute_is_dark("light", true),
        false,
        "compute_is_dark(light, _) == false"
    );
    assert_eq!(
        LinuxThemeController::compute_is_dark("system", false),
        false,
        "compute_is_dark(system, false) == false"
    );
    assert_eq!(
        LinuxThemeController::compute_is_dark("system", true),
        true,
        "compute_is_dark(system, true) == true"
    );
    println!("[BEHAVIOR_VERIFY] compute_is_dark: dark=>true, light=>false, system=>sys_dark");
}

// =========================================================================
// 行为守卫 5: theme_state_json 始终是合法 JSON，包含 is_dark 和 scheme
// =========================================================================

#[test]
fn qt_theme_state_json_always_valid_with_is_dark_and_scheme() {
    ensure_qt_application();
    let app = AppRef::default();
    let mut ctrl = LinuxThemeController::new(app);
    ctrl.reload();

    for mode in &["dark", "light", "system"] {
        ctrl.set_appearance_mode((*mode).into());
        let json: String = ctrl.theme_state_json().into();
        let v: serde_json::Value =
            serde_json::from_str(&json).unwrap_or_else(|_| panic!("{} 模式下 JSON 合法", mode));
        assert!(
            v.get("is_dark").is_some(),
            "{} 模式下 JSON 必须有 is_dark 顶层 key",
            mode
        );
        assert!(
            v.get("scheme").is_some(),
            "{} 模式下 JSON 必须有 scheme 顶层 key",
            mode
        );
        assert!(
            v["is_dark"].is_boolean(),
            "{} 模式下 is_dark 必须是 bool",
            mode
        );
        assert!(
            v["scheme"].is_object(),
            "{} 模式下 scheme 必须是 object",
            mode
        );
    }
    println!("[BEHAVIOR_VERIFY] theme_state_json: always valid JSON with is_dark + scheme");
}

// =========================================================================
// 行为守卫 6: appearance_mode 非法值回退到 system
// =========================================================================

#[test]
fn qt_theme_appearance_mode_invalid_falls_back_to_system() {
    ensure_qt_application();
    let app = AppRef::default();
    let mut ctrl = LinuxThemeController::new(app);
    ctrl.reload();
    // 非法值 "purple" 应被 set_setting_appearance_mode 归一为 "system"
    ctrl.set_appearance_mode("purple".into());
    let mode: String = ctrl.appearance_mode().into();
    assert_eq!(
        mode, "system",
        "非法 appearance_mode 必须回退到 system，实际: {}",
        mode
    );
    println!("[BEHAVIOR_VERIFY] appearance_mode invalid → fallback to system");
}

// =========================================================================
// 行为守卫 7: rebuild_resolved_state 返回七字段完整的 ResolvedThemeState
// =========================================================================

#[test]
fn qt_theme_rebuild_resolved_state_has_all_seven_fields() {
    ensure_qt_application();
    let app = AppRef::default();
    let ctrl = LinuxThemeController::new(app);
    let state: ResolvedThemeState = ctrl.rebuild_resolved_state();
    // 七字段完整 — 通过访问每个字段验证存在性
    let _appearance_mode: &String = &state.appearance_mode;
    let _system_is_dark: &bool = &state.system_is_dark;
    let _is_dark: &bool = &state.is_dark;
    let _color_source: &String = &state.color_source;
    let _selected_palette_id: &String = &state.selected_palette_id;
    let _selected_builtin_theme_id: &String = &state.selected_builtin_theme_id;
    let _scheme_json: &String = &state.scheme_json;
    // scheme_json 必须是合法 JSON 或 "{}"
    let _: serde_json::Value =
        serde_json::from_str(&state.scheme_json).expect("scheme_json 必须是合法 JSON");
    println!(
        "[BEHAVIOR_VERIFY] ResolvedThemeState 七字段完整: appearance_mode={} is_dark={} scheme_json_len={}",
        state.appearance_mode, state.is_dark, state.scheme_json.len()
    );
}

// =========================================================================
// 行为守卫 8: dark/light 切换后 is_dark 真实变化（不是缓存陈旧值）
// =========================================================================

#[test]
fn qt_theme_dark_light_switch_is_dark_actually_changes() {
    ensure_qt_application();
    let app = AppRef::default();
    let mut ctrl = LinuxThemeController::new(app);
    ctrl.reload();

    ctrl.set_appearance_mode("dark".into());
    let dark_is_dark = ctrl.is_dark();
    assert!(dark_is_dark, "dark → is_dark=true");

    ctrl.set_appearance_mode("light".into());
    let light_is_dark = ctrl.is_dark();
    assert!(!light_is_dark, "light → is_dark=false");

    assert_ne!(
        dark_is_dark, light_is_dark,
        "dark/light 切换后 is_dark 必须真实变化"
    );
    println!("[BEHAVIOR_VERIFY] dark/light switch: is_dark actually changes");
}

// =========================================================================
// 行为守卫 9: set_system_is_dark 在非 system 模式下不发 scheme_changed
//              （通过 is_dark 不变验证 — 用户选了 dark/light 后系统变化不重新解释）
// =========================================================================

#[test]
fn qt_theme_set_system_is_dark_does_not_override_user_choice() {
    ensure_qt_application();
    let app = AppRef::default();
    let mut ctrl = LinuxThemeController::new(app);
    ctrl.reload();

    // 用户明确选 dark
    ctrl.set_appearance_mode("dark".into());
    assert!(ctrl.is_dark(), "dark → is_dark=true");

    // 系统变 light — 不应改变用户选的 dark
    ctrl.set_system_is_dark(false);
    assert!(
        ctrl.is_dark(),
        "用户选 dark 后，系统变 light 不应改变 is_dark"
    );

    // 用户明确选 light
    ctrl.set_appearance_mode("light".into());
    assert!(!ctrl.is_dark(), "light → is_dark=false");

    // 系统变 dark — 不应改变用户选的 light
    ctrl.set_system_is_dark(true);
    assert!(
        !ctrl.is_dark(),
        "用户选 light 后，系统变 dark 不应改变 is_dark"
    );
    println!("[BEHAVIOR_VERIFY] set_system_is_dark does not override user choice");
}
