//! Issue #707 评论 5724685300 — 深色模式真实 Qt 行为测试。
//!
//! 本测试直接构造 `LinuxThemeController` 生产对象，调用其 pub 方法
//! （`set_appearance_mode`、`set_system_is_dark`、`theme_state_json`、
//! `rebuild_resolved_state`、`compute_is_dark`），验证运行时行为不变量。
//!
//! Issue #707 评论 5724685300 关键修改:
//! - 不再用空的 `AppRef::default()` 做颜色测试。空 AppBackend 没有 data root，
//!   `core_api()` 返回 `None`，`rebuild_resolved_state()` 拿不到 builtin theme，
//!   scheme 退成 None，只证明 `is_dark` 会变，没证明 scheme 切到深色。
//! - 现在用 `tempfile` 创建临时目录，通过 `open_data_root_for_tests` 初始化
//!   真实 Core/AppBackend 状态，使 `core_api()` 返回 `Some`，从而加载
//!   builtin theme 的真实深色/浅色 scheme。
//! - dark 必须断言 `scheme.on_surface` / `scheme.on_surface_variant` 非空
//!   且是深色方案值（浅色文字，如 "#DFE3E7"）。
//! - light 必须断言对应浅色 scheme（深色文字，如 "#171C1F"）。
//! - system 从 false -> true 后断言 `scheme` 内容确实变化。
//! - 不再接受 `scheme == {}` 作为颜色测试通过。
//!
//! Issue #709 评论 issue-body-709: `ResolvedThemeState.scheme_json: String` 改为
//! `scheme: Option<ThemeColorSchemeDto>`。行为守卫 7 直接断言 `state.scheme`
//! 是 Some 且序列化后非空对象。

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "common/qt_runtime.rs"]
mod qt_runtime;

use qt_runtime::run_on_qt_thread;
use sujian_linux_qt::backend::linux_theme_controller::ResolvedThemeState;
use sujian_linux_qt::backend::{AppRef, LinuxThemeController};

/// 构造一个已打开真实 data root 的 `AppRef`，使 `core_api()` 返回 `Some`。
///
/// 在 `run_on_qt_thread` 闭包内调用，`TempDir` 生命周期与闭包相同。
fn make_app_with_real_data_root() -> (AppRef, tempfile::TempDir) {
    let temp_dir = tempfile::tempdir().expect("failed to create temp dir");
    let temp_path = temp_dir.path().to_str().expect("temp path is valid utf-8");
    let app = AppRef::default();
    app.with_app_mut(|backend| backend.open_data_root_for_tests(temp_path))
        .expect("open_data_root_for_tests should succeed");
    (app, temp_dir)
}

/// 判断 on_surface 颜色是否为浅色文字（深色方案）。
///
/// 深色方案的 `on_surface` 是浅色文字（如 "#DFE3E7"），RGB 分量都较大。
/// 浅色方案的 `on_surface` 是深色文字（如 "#171C1F"），RGB 分量都较小。
fn is_light_color(hex: &str) -> bool {
    // 解析 #RRGGBB
    if hex.len() < 7 || !hex.starts_with('#') {
        return false;
    }
    let r = u8::from_str_radix(&hex[1..3], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[3..5], 16).unwrap_or(0);
    let b = u8::from_str_radix(&hex[5..7], 16).unwrap_or(0);
    // 浅色文字: RGB 分量平均值 > 128
    (r as u32 + g as u32 + b as u32) / 3 > 128
}

/// 判断 on_surface 颜色是否为深色文字（浅色方案）。
fn is_dark_color(hex: &str) -> bool {
    !is_light_color(hex)
}

// =========================================================================
// 行为守卫 1: dark 模式 → is_dark=true，scheme 是真实深色方案
// =========================================================================

#[test]
fn qt_theme_dark_mode_is_dark_true_and_real_dark_scheme() {
    run_on_qt_thread(|| {
        let (app, _temp) = make_app_with_real_data_root();
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
            "dark 模式下 scheme 必须是合法 JSON 对象"
        );
        let scheme_obj = scheme.as_object().expect("scheme is object");
        assert!(
            !scheme_obj.is_empty(),
            "dark 模式下 scheme 不能是空对象 {{}} — core_api 必须已初始化并加载 builtin theme"
        );
        let on_surface = scheme["on_surface"]
            .as_str()
            .expect("dark scheme.on_surface 必须存在且为字符串");
        assert!(!on_surface.is_empty(), "dark scheme.on_surface 不能为空");
        assert!(
            is_light_color(on_surface),
            "dark scheme.on_surface 必须是浅色文字（深色方案），实际: {}",
            on_surface
        );
        let on_surface_variant = scheme["on_surface_variant"]
            .as_str()
            .expect("dark scheme.on_surface_variant 必须存在且为字符串");
        assert!(
            !on_surface_variant.is_empty(),
            "dark scheme.on_surface_variant 不能为空"
        );
        assert!(
            is_light_color(on_surface_variant),
            "dark scheme.on_surface_variant 必须是浅色文字（深色方案），实际: {}",
            on_surface_variant
        );
        println!(
            "[BEHAVIOR_VERIFY] dark: is_dark=true, on_surface={}, on_surface_variant={}",
            on_surface, on_surface_variant
        );
    });
}

// =========================================================================
// 行为守卫 2: light 模式 → is_dark=false，scheme 是真实浅色方案
// =========================================================================

#[test]
fn qt_theme_light_mode_is_dark_false_and_real_light_scheme() {
    run_on_qt_thread(|| {
        let (app, _temp) = make_app_with_real_data_root();
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
        let scheme_obj = scheme.as_object().expect("scheme is object");
        assert!(
            !scheme_obj.is_empty(),
            "light 模式下 scheme 不能是空对象 {{}} — core_api 必须已初始化并加载 builtin theme"
        );
        let on_surface = scheme["on_surface"]
            .as_str()
            .expect("light scheme.on_surface 必须存在且为字符串");
        assert!(!on_surface.is_empty(), "light scheme.on_surface 不能为空");
        assert!(
            is_dark_color(on_surface),
            "light scheme.on_surface 必须是深色文字（浅色方案），实际: {}",
            on_surface
        );
        let on_surface_variant = scheme["on_surface_variant"]
            .as_str()
            .expect("light scheme.on_surface_variant 必须存在且为字符串");
        assert!(
            !on_surface_variant.is_empty(),
            "light scheme.on_surface_variant 不能为空"
        );
        assert!(
            is_dark_color(on_surface_variant),
            "light scheme.on_surface_variant 必须是深色文字（浅色方案），实际: {}",
            on_surface_variant
        );
        println!(
            "[BEHAVIOR_VERIFY] light: is_dark=false, on_surface={}, on_surface_variant={}",
            on_surface, on_surface_variant
        );
    });
}

// =========================================================================
// 行为守卫 3: system 模式 + set_system_is_dark → payload 整体切换（scheme 内容变化）
// =========================================================================

#[test]
fn qt_theme_system_mode_payload_switches_with_system_is_dark() {
    run_on_qt_thread(|| {
        let (app, _temp) = make_app_with_real_data_root();
        let mut ctrl = LinuxThemeController::new(app);
        ctrl.reload();
        ctrl.set_appearance_mode("system".into());

        // system + sys_dark=false → is_dark=false, 浅色 scheme
        ctrl.set_system_is_dark(false);
        let json1: String = ctrl.theme_state_json().into();
        let v1: serde_json::Value = serde_json::from_str(&json1).expect("valid json");
        assert_eq!(
            v1["is_dark"].as_bool(),
            Some(false),
            "system + sys_dark=false → is_dark=false"
        );
        let scheme1 = &v1["scheme"];
        assert!(scheme1.is_object(), "system+false scheme 必须是对象");
        let scheme1_obj = scheme1.as_object().expect("scheme1 is object");
        assert!(!scheme1_obj.is_empty(), "system+false scheme 不能是空对象");
        let on_surface1 = scheme1["on_surface"]
            .as_str()
            .expect("system+false scheme.on_surface 必须存在");
        assert!(
            is_dark_color(on_surface1),
            "system+false (浅色) scheme.on_surface 必须是深色文字，实际: {}",
            on_surface1
        );

        // system + sys_dark=true → is_dark=true, 深色 scheme
        ctrl.set_system_is_dark(true);
        let json2: String = ctrl.theme_state_json().into();
        let v2: serde_json::Value = serde_json::from_str(&json2).expect("valid json");
        assert_eq!(
            v2["is_dark"].as_bool(),
            Some(true),
            "system + sys_dark=true → is_dark=true"
        );
        let scheme2 = &v2["scheme"];
        assert!(scheme2.is_object(), "system+true scheme 必须是对象");
        let scheme2_obj = scheme2.as_object().expect("scheme2 is object");
        assert!(!scheme2_obj.is_empty(), "system+true scheme 不能是空对象");
        let on_surface2 = scheme2["on_surface"]
            .as_str()
            .expect("system+true scheme.on_surface 必须存在");
        assert!(
            is_light_color(on_surface2),
            "system+true (深色) scheme.on_surface 必须是浅色文字，实际: {}",
            on_surface2
        );

        // scheme 内容必须随 is_dark 一起变化（不是同一个 scheme）
        assert_ne!(
            scheme1, scheme2,
            "system 模式下 is_dark 切换后 scheme 内容必须变化"
        );
        assert_ne!(
            on_surface1, on_surface2,
            "system 模式下 on_surface 必须随 is_dark 切换而变化"
        );
        println!(
            "[BEHAVIOR_VERIFY] system: payload switches is_dark + scheme together (on_surface: {} -> {})",
            on_surface1, on_surface2
        );
    });
}

// =========================================================================
// 行为守卫 4: compute_is_dark 三值匹配
// =========================================================================

#[test]
fn qt_theme_compute_is_dark_three_way_match() {
    run_on_qt_thread(|| {
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
    });
}

// =========================================================================
// 行为守卫 5: theme_state_json 始终是合法 JSON，包含 is_dark 和 scheme
// =========================================================================

#[test]
fn qt_theme_state_json_always_valid_with_is_dark_and_scheme() {
    run_on_qt_thread(|| {
        let (app, _temp) = make_app_with_real_data_root();
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
            assert!(
                !v["scheme"].as_object().map_or(true, |m| m.is_empty()),
                "{} 模式下 scheme 不能是空对象 {{}} — core_api 必须已初始化",
                mode
            );
        }
        println!(
            "[BEHAVIOR_VERIFY] theme_state_json: always valid JSON with is_dark + non-empty scheme"
        );
    });
}

// =========================================================================
// 行为守卫 6: appearance_mode 非法值回退到 system
// =========================================================================

#[test]
fn qt_theme_appearance_mode_invalid_falls_back_to_system() {
    run_on_qt_thread(|| {
        let (app, _temp) = make_app_with_real_data_root();
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
    });
}

// =========================================================================
// 行为守卫 7: rebuild_resolved_state 返回七字段完整的 ResolvedThemeState
// =========================================================================

#[test]
fn qt_theme_rebuild_resolved_state_has_all_seven_fields() {
    run_on_qt_thread(|| {
        let (app, _temp) = make_app_with_real_data_root();
        let ctrl = LinuxThemeController::new(app);
        let state: ResolvedThemeState = ctrl.rebuild_resolved_state();
        // 七字段完整 — 通过访问每个字段验证存在性
        let _appearance_mode: &String = &state.appearance_mode;
        let _system_is_dark: &bool = &state.system_is_dark;
        let _is_dark: &bool = &state.is_dark;
        let _color_source: &String = &state.color_source;
        let _selected_palette_id: &String = &state.selected_palette_id;
        let _selected_builtin_theme_id: &String = &state.selected_builtin_theme_id;
        // Issue #709 评论 issue-body-709: scheme 现在是 Option<ThemeColorSchemeDto>，
        // 不再是 scheme_json: String。直接断言 Some 且非空对象。
        let scheme = state
            .scheme
            .as_ref()
            .expect("scheme 必须是 Some — core_api 必须已初始化并加载 builtin theme");
        // scheme 必须能序列化为合法 JSON 对象且非空
        let parsed: serde_json::Value =
            serde_json::to_value(scheme).expect("scheme 必须能序列化为 JSON");
        assert!(parsed.is_object(), "scheme 必须是 JSON 对象");
        assert!(
            !parsed.as_object().map_or(true, |m| m.is_empty()),
            "scheme 不能是空对象 {{}} — core_api 必须已初始化并加载 builtin theme"
        );
        println!(
            "[BEHAVIOR_VERIFY] ResolvedThemeState 七字段完整: appearance_mode={} is_dark={} scheme_is_some={}",
            state.appearance_mode, state.is_dark, state.scheme.is_some()
        );
    });
}

// =========================================================================
// 行为守卫 8: dark/light 切换后 is_dark 真实变化且 scheme 真实切换
// =========================================================================

#[test]
fn qt_theme_dark_light_switch_is_dark_and_scheme_actually_change() {
    run_on_qt_thread(|| {
        let (app, _temp) = make_app_with_real_data_root();
        let mut ctrl = LinuxThemeController::new(app);
        ctrl.reload();

        ctrl.set_appearance_mode("dark".into());
        let dark_is_dark = ctrl.is_dark();
        assert!(dark_is_dark, "dark → is_dark=true");
        let dark_json: String = ctrl.theme_state_json().into();
        let dark_v: serde_json::Value = serde_json::from_str(&dark_json).expect("valid json");
        let dark_scheme = &dark_v["scheme"];
        let dark_on_surface = dark_scheme["on_surface"]
            .as_str()
            .expect("dark scheme.on_surface 必须存在");
        assert!(
            is_light_color(dark_on_surface),
            "dark scheme.on_surface 必须是浅色文字，实际: {}",
            dark_on_surface
        );

        ctrl.set_appearance_mode("light".into());
        let light_is_dark = ctrl.is_dark();
        assert!(!light_is_dark, "light → is_dark=false");
        let light_json: String = ctrl.theme_state_json().into();
        let light_v: serde_json::Value = serde_json::from_str(&light_json).expect("valid json");
        let light_scheme = &light_v["scheme"];
        let light_on_surface = light_scheme["on_surface"]
            .as_str()
            .expect("light scheme.on_surface 必须存在");
        assert!(
            is_dark_color(light_on_surface),
            "light scheme.on_surface 必须是深色文字，实际: {}",
            light_on_surface
        );

        assert_ne!(
            dark_is_dark, light_is_dark,
            "dark/light 切换后 is_dark 必须真实变化"
        );
        assert_ne!(
            dark_scheme, light_scheme,
            "dark/light 切换后 scheme 内容必须真实变化"
        );
        assert_ne!(
            dark_on_surface, light_on_surface,
            "dark/light 切换后 on_surface 必须真实变化"
        );
        println!(
            "[BEHAVIOR_VERIFY] dark/light switch: is_dark + scheme actually change (on_surface: {} -> {})",
            dark_on_surface, light_on_surface
        );
    });
}

// =========================================================================
// 行为守卫 9: set_system_is_dark 在非 system 模式下不发 scheme_changed
//              （通过 is_dark 不变验证 — 用户选了 dark/light 后系统变化不重新解释）
// =========================================================================

#[test]
fn qt_theme_set_system_is_dark_does_not_override_user_choice() {
    run_on_qt_thread(|| {
        let (app, _temp) = make_app_with_real_data_root();
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
    });
}
