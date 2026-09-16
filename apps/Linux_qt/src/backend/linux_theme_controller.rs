use super::*;
use crate::backend::AppRef;
use crate::backend::DomainSnapshot;

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

    /// Issue #677 评论 5653315696: 输出的主题 JSON 字段名与 Core DTO 一致，统一使用 snake_case。
    ///
    /// `serde_json::to_string(&s)` 序列化的是 Core 的 `ThemeColorScheme` DTO，其字段名
    /// （`on_surface`、`on_surface_variant`、`surface_container_low` 等）由 serde 派生为
    /// snake_case。本方法不做任何 camelCase 转换，QML 侧（DesignTokens.qml）必须按
    /// snake_case key 读取。这是 Linux_Qt 与 Core 之间唯一的主题 JSON 字段名协议。
    fn resolved_scheme_json(&self) -> QString {
        self.with_app(|app| {
            let color_source = app.setting_color_source().to_string();
            let appearance_mode = app.setting_appearance_mode().to_string();
            let is_dark = Self::compute_is_dark(&appearance_mode, self.system_is_dark());

            let scheme = if color_source == "saved_palette" {
                let palette_id = app.setting_selected_palette_id().to_string();
                if !palette_id.is_empty() {
                    let parts: Vec<&str> = palette_id.splitn(2, ':').collect();
                    if parts.len() == 2 {
                        if let Some(core) = app.core_api() {
                            if let Ok(record) = core.load_palette_record(parts[0], parts[1]) {
                                let dto: writer_core::api::types::ThemePaletteRecordDto = record;
                                if is_dark {
                                    Some(dto.dark_scheme)
                                } else {
                                    Some(dto.light_scheme)
                                }
                            } else {
                                None
                            }
                        } else {
                            None
                        }
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
                let theme_id = app.setting_selected_builtin_theme_id().to_string();
                if let Some(core) = app.core_api() {
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
                } else {
                    None
                }
            });

            match scheme {
                Some(s) => {
                    let json = serde_json::to_string(&s).unwrap_or_else(|_| "{}".to_string());
                    QString::from(json)
                }
                None => "{}".into(),
            }
        })
        .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }

    fn is_dark(&self) -> bool {
        let mode = self.snap().appearance_mode.clone();
        Self::compute_is_dark(&mode, self.system_is_dark())
    }

    fn color_source(&self) -> QString {
        self.with_app(|app| app.setting_color_source())
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }

    fn selected_builtin_theme_id(&self) -> QString {
        self.with_app(|app| app.setting_selected_builtin_theme_id())
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }

    fn selected_palette_id(&self) -> QString {
        self.with_app(|app| app.setting_selected_palette_id())
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }

    fn appearance_mode(&self) -> QString {
        self.with_app(|app| app.setting_appearance_mode())
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }

    fn system_is_dark(&self) -> bool {
        self.snap().system_is_dark
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
        let appearance_mode = self.snap().appearance_mode.clone();
        let is_dark = self.is_dark();
        let is_dark_str = if is_dark { "true" } else { "false" };
        crate::backend::app_backend::record_struct_event(
            writer_diagnostics::DiagnosticOrigin::App,
            "theme.resolve",
            "theme",
            &[
                ("appearanceMode", &appearance_mode),
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
