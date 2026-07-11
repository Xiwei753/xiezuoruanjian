use super::*;
use crate::backend::SafeAppPtr;

#[allow(non_snake_case, dead_code)]
#[derive(QObject, Default)]
pub struct LinuxThemeController {
    base: qt_base_class!(trait QObject),
    resolved_scheme_json: qt_property!(QString; READ resolved_scheme_json NOTIFY scheme_changed),
    is_dark: qt_property!(bool; READ is_dark NOTIFY scheme_changed),
    color_source: qt_property!(QString; READ color_source NOTIFY scheme_changed),
    selected_builtin_theme_id: qt_property!(QString; READ selected_builtin_theme_id NOTIFY scheme_changed),
    selected_palette_id: qt_property!(QString; READ selected_palette_id NOTIFY scheme_changed),
    appearance_mode: qt_property!(QString; READ appearance_mode NOTIFY scheme_changed),
    system_is_dark: qt_property!(bool; READ system_is_dark NOTIFY scheme_changed),
    scheme_changed: qt_signal!(),
    reload: qt_method!(fn(&mut self)),
    set_color_source: qt_method!(fn(&mut self, val: QString)),
    set_appearance_mode: qt_method!(fn(&mut self, val: QString)),
    set_selected_builtin_theme_id: qt_method!(fn(&mut self, val: QString)),
    set_selected_palette_id: qt_method!(fn(&mut self, val: QString)),
    set_system_is_dark: qt_method!(fn(&mut self, val: bool)),
    app: SafeAppPtr,
}

impl LinuxThemeController {
    pub fn new(app: SafeAppPtr) -> Self {
        Self {
            app,
            ..Default::default()
        }
    }

    fn with_app<R>(&self, default: R, f: impl FnOnce(&AppBackend) -> R) -> R {
        if let Some(app) = self.app.get() {
            unsafe { f(&*app) }
        } else {
            default
        }
    }

    fn with_app_mut<R>(&mut self, default: R, f: impl FnOnce(&mut AppBackend) -> R) -> R {
        if let Some(app) = self.app.get() {
            unsafe { f(&mut *app) }
        } else {
            default
        }
    }

    fn resolved_scheme_json(&self) -> QString {
        self.with_app("{}".into(), |app| {
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
                                let dto: writer_core::api::types::ThemePaletteRecordDto = record.into();
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
                            t.dark_scheme.clone().into()
                        } else {
                            t.light_scheme.clone().into()
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
    }

    fn is_dark(&self) -> bool {
        let mode = self.with_app("system".into(), |app| app.setting_appearance_mode().to_string());
        Self::compute_is_dark(&mode, self.system_is_dark())
    }

    fn color_source(&self) -> QString {
        self.with_app("built_in".into(), |app| app.setting_color_source())
    }

    fn selected_builtin_theme_id(&self) -> QString {
        self.with_app("".into(), |app| app.setting_selected_builtin_theme_id())
    }

    fn selected_palette_id(&self) -> QString {
        self.with_app("".into(), |app| app.setting_selected_palette_id())
    }

    fn appearance_mode(&self) -> QString {
        self.with_app("system".into(), |app| app.setting_appearance_mode())
    }

    fn system_is_dark(&self) -> bool {
        self.with_app(false, |app| app.current_system_is_dark)
    }

    fn compute_is_dark(mode: &str, sys_dark: bool) -> bool {
        match mode {
            "dark" => true,
            "light" => false,
            _ => sys_dark,
        }
    }

    fn reload(&mut self) {
        self.scheme_changed();
    }

    fn set_color_source(&mut self, val: QString) {
        let source = val.to_string();
        if source == "saved_palette" {
            let palette_id = self.with_app("".into(), |app| app.setting_selected_palette_id().to_string());
            if palette_id.is_empty() {
                return;
            }
        }
        self.with_app_mut((), |app| app.set_setting_color_source(val));
        self.scheme_changed();
    }

    fn set_appearance_mode(&mut self, val: QString) {
        self.with_app_mut((), |app| app.set_setting_appearance_mode(val));
        self.scheme_changed();
    }

    fn set_selected_builtin_theme_id(&mut self, val: QString) {
        self.with_app_mut((), |app| app.set_setting_selected_builtin_theme_id(val));
        self.with_app_mut((), |app| app.set_setting_color_source("built_in".into()));
        self.scheme_changed();
    }

    fn set_selected_palette_id(&mut self, val: QString) {
        let palette_id = val.to_string();
        if !palette_id.is_empty() {
            self.with_app_mut((), |app| app.set_setting_selected_palette_id(val));
            self.with_app_mut((), |app| app.set_setting_color_source("saved_palette".into()));
        }
        self.scheme_changed();
    }

    fn set_system_is_dark(&mut self, val: bool) {
        self.with_app_mut((), |app| {
            app.current_system_is_dark = val;
        });
        self.scheme_changed();
    }
}
