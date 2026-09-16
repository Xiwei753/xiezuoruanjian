use super::*;
use crate::backend::AppRef;
use crate::backend::DomainSnapshot;

/// Issue #701 评论 5699565102: 运行时主题状态的唯一事实来源。
///
/// `appearance_mode`、`system_is_dark`、`is_dark`、`color_source`、当前
/// builtin/palette id 以及最终 `ThemeColorScheme`（`resolved_scheme_json`）
/// 全部从同一份 `DomainSnapshot` 读取，不再一个读 snapshot、另一个临时去
/// `AppBackend` 再算一次。
///
/// 切换主题模式、颜色来源、内置主题、已保存 palette 后，只在 setter 里写
/// AppBackend 设置并发一次 `scheme_changed`；`resolved_scheme_json` 在 QML
/// 侧通过该信号重新求值。`with_app` 仅用于调用 `core_api()` 加载 palette
/// record / builtin theme，不再用于读取主题设置本身。
#[allow(non_snake_case)] // Qt QML naming convention
#[derive(QObject, Default)]
pub struct LinuxThemeController {
    #[allow(dead_code)]
    base: qt_base_class!(trait QObject),
    #[allow(dead_code)]
    resolved_scheme_json: qt_property!(QString; READ resolved_scheme_json NOTIFY scheme_changed),
    #[allow(dead_code)]
    is_dark: qt_property!(bool; READ is_dark NOTIFY scheme_changed),
    #[allow(dead_code)]
    color_source: qt_property!(QString; READ color_source NOTIFY scheme_changed),
    #[allow(dead_code)]
    selected_builtin_theme_id: qt_property!(QString; READ selected_builtin_theme_id NOTIFY scheme_changed),
    #[allow(dead_code)]
    selected_palette_id: qt_property!(QString; READ selected_palette_id NOTIFY scheme_changed),
    #[allow(dead_code)]
    appearance_mode: qt_property!(QString; READ appearance_mode NOTIFY scheme_changed),
    #[allow(dead_code)]
    system_is_dark: qt_property!(bool; READ system_is_dark NOTIFY scheme_changed),
    #[allow(dead_code)]
    scheme_changed: qt_signal!(),
    #[allow(dead_code)]
    reload: qt_method!(fn(&mut self)),
    #[allow(dead_code)]
    set_color_source: qt_method!(fn(&mut self, val: QString)),
    #[allow(dead_code)]
    set_appearance_mode: qt_method!(fn(&mut self, val: QString)),
    #[allow(dead_code)]
    set_selected_builtin_theme_id: qt_method!(fn(&mut self, val: QString)),
    #[allow(dead_code)]
    set_selected_palette_id: qt_method!(fn(&mut self, val: QString)),
    #[allow(dead_code)]
    set_system_is_dark: qt_method!(fn(&mut self, val: bool)),
    app: AppRef,
}

impl LinuxThemeController {
    pub fn new(app: AppRef) -> Self {
        Self {
            app,
            ..Default::default()
        }
    }

    fn with_app<R>(&self, f: impl FnOnce(&AppBackend) -> R) -> Result<R, super::AppBorrowError> {
        self.app.with_app(f)
    }

    fn with_app_mut<R>(
        &self,
        f: impl FnOnce(&mut AppBackend) -> R,
    ) -> Result<R, super::AppBorrowError> {
        self.app.with_app_mut(f)
    }

    fn snap(&self) -> std::cell::Ref<'_, DomainSnapshot> {
        self.app.snapshot().borrow()
    }

    /// 从同一份 `DomainSnapshot` 一次性解析当前运行时主题状态。
    ///
    /// 返回 `(appearance_mode, system_is_dark, is_dark, color_source,
    /// selected_palette_id, selected_builtin_theme_id)`。所有 QML 只读属性
    /// （`appearance_mode`、`is_dark`、`color_source` 等）和
    /// `resolved_scheme_json` 都基于这份快照，避免一个属性读 snapshot、
    /// 另一个属性临时再 borrow `AppBackend` 造成的双状态机。
    fn resolve_state(&self) -> ResolvedThemeState {
        let s = self.snap();
        let appearance_mode = s.appearance_mode.clone();
        let system_is_dark = s.system_is_dark;
        let is_dark = Self::compute_is_dark(&appearance_mode, system_is_dark);
        let color_source = s.color_source.clone();
        let selected_palette_id = s.selected_palette_id.clone();
        let selected_builtin_theme_id = s.selected_builtin_theme_id.clone();
        ResolvedThemeState {
            appearance_mode,
            system_is_dark,
            is_dark,
            color_source,
            selected_palette_id,
            selected_builtin_theme_id,
        }
    }

    /// Issue #677 评论 5653315696: 输出的主题 JSON 字段名与 Core DTO 一致，统一使用 snake_case。
    ///
    /// `serde_json::to_string(&s)` 序列化的是 Core 的 `ThemeColorScheme` DTO，其字段名
    /// （`on_surface`、`on_surface_variant`、`surface_container_low` 等）由 serde 派生为
    /// snake_case。本方法不做任何 camelCase 转换，QML 侧（DesignTokens.qml）必须按
    /// snake_case key 读取。这是 Linux_Qt 与 Core 之间唯一的主题 JSON 字段名协议。
    ///
    /// Issue #701 评论 5699565102: 主题设置（appearance_mode、color_source、
    /// selected_palette_id、selected_builtin_theme_id、system_is_dark）全部从
    /// `resolve_state()` 的同一份快照读取。`with_app` 仅用于调用 `core_api()`
    /// 加载 palette record / builtin theme 数据。
    fn resolved_scheme_json(&self) -> QString {
        let state = self.resolve_state();
        let is_dark = state.is_dark;

        let scheme = if state.color_source == "saved_palette" {
            if !state.selected_palette_id.is_empty() {
                let parts: Vec<&str> = state.selected_palette_id.splitn(2, ':').collect();
                if parts.len() == 2 {
                    self.with_app(|app| {
                        app.core_api().and_then(|core| {
                            core.load_palette_record(parts[0], parts[1])
                                .ok()
                                .map(|dto| {
                                    let dto: writer_core::api::types::ThemePaletteRecordDto = dto;
                                    if is_dark {
                                        dto.dark_scheme
                                    } else {
                                        dto.light_scheme
                                    }
                                })
                        })
                    })
                    .unwrap_or(None)
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let scheme = scheme.or_else(|| {
            let theme_id = state.selected_builtin_theme_id.clone();
            self.with_app(|app| {
                app.core_api().and_then(|core| {
                    let themes = core.list_builtin_themes();
                    let theme = if theme_id.is_empty() {
                        themes.first()
                    } else {
                        themes.iter().find(|t| t.theme_id == theme_id)
                    };
                    theme.map(|t| {
                        if is_dark {
                            t.dark_scheme.clone()
                        } else {
                            t.light_scheme.clone()
                        }
                    })
                })
            })
            .unwrap_or(None)
        });

        match scheme {
            Some(s) => {
                let json = serde_json::to_string(&s).unwrap_or_else(|_| "{}".to_string());
                QString::from(json)
            }
            None => "{}".into(),
        }
    }

    fn is_dark(&self) -> bool {
        self.resolve_state().is_dark
    }

    fn color_source(&self) -> QString {
        QString::from(self.resolve_state().color_source)
    }

    fn selected_builtin_theme_id(&self) -> QString {
        QString::from(self.resolve_state().selected_builtin_theme_id)
    }

    fn selected_palette_id(&self) -> QString {
        QString::from(self.resolve_state().selected_palette_id)
    }

    fn appearance_mode(&self) -> QString {
        QString::from(self.resolve_state().appearance_mode)
    }

    fn system_is_dark(&self) -> bool {
        self.resolve_state().system_is_dark
    }

    fn compute_is_dark(mode: &str, sys_dark: bool) -> bool {
        match mode {
            "dark" => true,
            "light" => false,
            _ => sys_dark,
        }
    }

    fn reload(&mut self) {
        // 主题解析（应用内部逻辑）→ origin=App。
        let state = self.resolve_state();
        let is_dark_str = if state.is_dark { "true" } else { "false" };
        crate::backend::app_backend::record_struct_event(
            writer_diagnostics::DiagnosticOrigin::App,
            "theme.resolve",
            "theme",
            &[
                ("appearanceMode", &state.appearance_mode),
                ("isDark", is_dark_str),
            ],
        );
        self.scheme_changed();
    }

    fn set_color_source(&mut self, val: QString) {
        let source = val.to_string();
        if source == "saved_palette" {
            let pid = self.snap().selected_palette_id.clone();
            if pid.is_empty() {
                return;
            }
        }
        if self
            .with_app_mut(|app| app.set_setting_color_source(val))
            .is_ok()
        {
            self.scheme_changed();
        }
    }

    fn set_appearance_mode(&mut self, val: QString) {
        // 用户选择外观模式（点击 dark/light/system）→ origin=User。
        let mode_str = val.to_string();
        crate::backend::app_backend::record_struct_event(
            writer_diagnostics::DiagnosticOrigin::User,
            "theme.appearance_select",
            "theme",
            &[("requested", &mode_str)],
        );
        if self
            .with_app_mut(|app| app.set_setting_appearance_mode(val))
            .is_ok()
        {
            self.scheme_changed();
        }
    }

    fn set_selected_builtin_theme_id(&mut self, val: QString) {
        if self
            .with_app_mut(|app| app.set_setting_selected_builtin_theme_id(val))
            .is_ok()
        {
            if self
                .with_app_mut(|app| app.set_setting_color_source("built_in".into()))
                .is_err()
            {
                crate::backend::app_backend::debug_error_static(
                    "theme_controller",
                    "BORROW_CONFLICT",
                    "set_setting_color_source skipped due to borrow conflict",
                );
            }
            self.scheme_changed();
        }
    }

    fn set_selected_palette_id(&mut self, val: QString) {
        let palette_id = val.to_string();
        if !palette_id.is_empty() {
            if self
                .with_app_mut(|app| app.set_setting_selected_palette_id(val))
                .is_err()
            {
                crate::backend::app_backend::debug_error_static(
                    "theme_controller",
                    "BORROW_CONFLICT",
                    "set_setting_selected_palette_id skipped due to borrow conflict",
                );
            }
            if self
                .with_app_mut(|app| app.set_setting_color_source("saved_palette".into()))
                .is_err()
            {
                crate::backend::app_backend::debug_error_static(
                    "theme_controller",
                    "BORROW_CONFLICT",
                    "set_setting_color_source skipped due to borrow conflict",
                );
            }
        }
        self.scheme_changed();
    }

    fn set_system_is_dark(&mut self, val: bool) {
        // 系统主题变化回调 → origin=System。
        let is_dark_str = if val { "true" } else { "false" };
        crate::backend::app_backend::record_struct_event(
            writer_diagnostics::DiagnosticOrigin::System,
            "theme.system_color_scheme",
            "theme",
            &[("isDark", is_dark_str)],
        );
        // Issue #696 评论 5696993601: 只有 appearance_mode == "system" 时，
        // 系统深浅变化才改变有效 scheme。用户明确选 light/dark 时，系统色彩
        // 变化不能重新解释用户偏好，也不发 scheme_changed。current_system_is_dark
        // 仍然更新，以便用户切回 system 模式时用到最新值。
        let mode = self.snap().appearance_mode.clone();
        let should_emit = mode == "system";
        if self
            .with_app_mut(|app| {
                app.current_system_is_dark = val;
            })
            .is_ok()
        {
            if should_emit {
                self.scheme_changed();
            }
        }
    }
}

/// 从 `DomainSnapshot` 一次性解析出的运行时主题状态。
///
/// `LinuxThemeController` 的所有 QML 只读属性和 `resolved_scheme_json` 都
/// 基于这同一份状态，避免一个属性读 `DomainSnapshot`、另一个属性临时再
/// borrow `AppBackend` 造成的双状态机。
struct ResolvedThemeState {
    appearance_mode: String,
    system_is_dark: bool,
    is_dark: bool,
    color_source: String,
    selected_palette_id: String,
    selected_builtin_theme_id: String,
}
