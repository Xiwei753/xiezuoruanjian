// =============================================================================
// settings_backend.rs — 应用配置与用户偏好领域 QObject 后端适配层
// =============================================================================
//
// 引用了什么：
// - super::*：引入 AppBackend 核心后端的全部方法与结构体。
// - crate::backend::SafeAppPtr：用于安全访问全局 AppBackend 指针以读取/更新本地/同步设置状态。
//
// 干什么的：
// - 实现 SettingsBackend 结构体，作为 QML 中 "settingsBackend" 对象的桥梁。
// - 维护并对外公开与设置 schema 强类型匹配的 Qt 属性，如字号、行距、自动保存延迟、首行缩进宽度、主题及 Monet 动态调色。
// - 支持对本地不随云端同步的本地设置（LocalSettingsDto，如侧边栏 sidebar 宽度、打字动画开关）以及自动随网络同步的全局设置（SyncableSettingsDto，如作品的字体偏好）进行强类型校验的加载（load_local_settings）与安全性持久化落盘（save_local_settings）。
//
// 被什么引用：
// - 被 apps/desktop/src/backend/mod.rs 引用，用于实例化设置后端并绑定为 QML 全局上下文属性。
// =============================================================================

use super::*;
use crate::backend::SafeAppPtr;

#[allow(non_snake_case)]
#[derive(QObject, Default)]
pub struct SettingsBackend {
    base: qt_base_class!(trait QObject),
    setting_font_size: qt_property!(f32; READ setting_font_size WRITE set_setting_font_size NOTIFY settings_changed),
    setting_line_spacing: qt_property!(f32; READ setting_line_spacing WRITE set_setting_line_spacing NOTIFY settings_changed),
    setting_auto_save_enabled: qt_property!(bool; READ setting_auto_save_enabled WRITE set_setting_auto_save_enabled NOTIFY settings_changed),
    setting_auto_save_delay_ms: qt_property!(u32; READ setting_auto_save_delay_ms WRITE set_setting_auto_save_delay_ms NOTIFY settings_changed),
    setting_auto_indent_enabled: qt_property!(bool; READ setting_auto_indent_enabled WRITE set_setting_auto_indent_enabled NOTIFY settings_changed),
    setting_auto_indent_width: qt_property!(f32; READ setting_auto_indent_width WRITE set_setting_auto_indent_width NOTIFY settings_changed),
    setting_theme_mode: qt_property!(QString; READ setting_theme_mode WRITE set_setting_theme_mode NOTIFY settings_changed),
    setting_monet_color: qt_property!(QString; READ setting_monet_color WRITE set_setting_monet_color NOTIFY settings_changed),
    setting_typing_animation_enabled: qt_property!(bool; READ setting_typing_animation_enabled WRITE set_setting_typing_animation_enabled NOTIFY settings_changed),
    setting_smooth_cursor_enabled: qt_property!(bool; READ setting_smooth_cursor_enabled WRITE set_setting_smooth_cursor_enabled NOTIFY settings_changed),
    setting_typing_animation_duration_ms: qt_property!(u32; READ setting_typing_animation_duration_ms WRITE set_setting_typing_animation_duration_ms NOTIFY settings_changed),
    setting_smooth_cursor_duration_ms: qt_property!(u32; READ setting_smooth_cursor_duration_ms WRITE set_setting_smooth_cursor_duration_ms NOTIFY settings_changed),
    ai_available: qt_property!(bool; READ ai_available NOTIFY ai_available_changed),
    ai_enabled: qt_property!(bool; READ ai_enabled WRITE set_ai_enabled NOTIFY ai_enabled_changed),
    setting_linux_sidebar_width: qt_property!(f64; READ setting_linux_sidebar_width WRITE set_setting_linux_sidebar_width NOTIFY settings_changed),
    setting_linux_editor_width: qt_property!(f64; READ setting_linux_editor_width WRITE set_setting_linux_editor_width NOTIFY settings_changed),
    settings_changed: qt_signal!(),
    ai_enabled_changed: qt_signal!(),
    ai_available_changed: qt_signal!(),
    load_local_settings: qt_method!(fn(&mut self)),
    save_local_settings: qt_method!(fn(&mut self) -> bool),
    app: SafeAppPtr,
}

impl SettingsBackend {
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
            crate::backend::app_backend::debug_error_static(
                "settings",
                "BACKEND_LINK_BROKEN",
                "app pointer is null",
            );
            default
        }
    }

    fn with_app_mut<R>(&mut self, default: R, f: impl FnOnce(&mut AppBackend) -> R) -> R {
        if let Some(app) = self.app.get() {
            unsafe { f(&mut *app) }
        } else {
            crate::backend::app_backend::debug_error_static(
                "settings",
                "BACKEND_LINK_BROKEN",
                "app pointer is null",
            );
            default
        }
    }

    fn setting_font_size(&self) -> f32 {
        self.with_app(16.0, |app| app.setting_font_size())
    }
    fn set_setting_font_size(&mut self, val: f32) {
        self.with_app_mut((), |app| app.set_setting_font_size(val));
        self.settings_changed();
    }
    fn setting_line_spacing(&self) -> f32 {
        self.with_app(1.5, |app| app.setting_line_spacing())
    }
    fn set_setting_line_spacing(&mut self, val: f32) {
        self.with_app_mut((), |app| app.set_setting_line_spacing(val));
        self.settings_changed();
    }
    fn setting_auto_save_enabled(&self) -> bool {
        self.with_app(true, |app| app.setting_auto_save_enabled())
    }
    fn set_setting_auto_save_enabled(&mut self, val: bool) {
        self.with_app_mut((), |app| app.set_setting_auto_save_enabled(val));
        self.settings_changed();
    }
    fn setting_auto_save_delay_ms(&self) -> u32 {
        self.with_app(1500, |app| app.setting_auto_save_delay_ms())
    }
    fn set_setting_auto_save_delay_ms(&mut self, val: u32) {
        self.with_app_mut((), |app| app.set_setting_auto_save_delay_ms(val));
        self.settings_changed();
    }
    fn setting_auto_indent_enabled(&self) -> bool {
        self.with_app(true, |app| app.setting_auto_indent_enabled())
    }
    fn set_setting_auto_indent_enabled(&mut self, val: bool) {
        self.with_app_mut((), |app| app.set_setting_auto_indent_enabled(val));
        self.settings_changed();
    }
    fn setting_auto_indent_width(&self) -> f32 {
        self.with_app(2.0, |app| app.setting_auto_indent_width())
    }
    fn set_setting_auto_indent_width(&mut self, val: f32) {
        self.with_app_mut((), |app| app.set_setting_auto_indent_width(val));
        self.settings_changed();
    }
    fn setting_theme_mode(&self) -> QString {
        self.with_app("system".into(), |app| app.setting_theme_mode())
    }
    fn set_setting_theme_mode(&mut self, val: QString) {
        self.with_app_mut((), |app| app.set_setting_theme_mode(val));
        self.settings_changed();
    }
    fn setting_monet_color(&self) -> QString {
        self.with_app("".into(), |app| app.setting_monet_color())
    }
    fn set_setting_monet_color(&mut self, val: QString) {
        self.with_app_mut((), |app| app.set_setting_monet_color(val));
        self.settings_changed();
    }
    fn setting_typing_animation_enabled(&self) -> bool {
        self.with_app(true, |app| app.setting_typing_animation_enabled())
    }
    fn set_setting_typing_animation_enabled(&mut self, val: bool) {
        self.with_app_mut((), |app| app.set_setting_typing_animation_enabled(val));
        self.settings_changed();
    }
    fn setting_smooth_cursor_enabled(&self) -> bool {
        self.with_app(true, |app| app.setting_smooth_cursor_enabled())
    }
    fn set_setting_smooth_cursor_enabled(&mut self, val: bool) {
        self.with_app_mut((), |app| app.set_setting_smooth_cursor_enabled(val));
        self.settings_changed();
    }
    fn setting_typing_animation_duration_ms(&self) -> u32 {
        self.with_app(100, |app| app.setting_typing_animation_duration_ms())
    }
    fn set_setting_typing_animation_duration_ms(&mut self, val: u32) {
        self.with_app_mut((), |app| app.set_setting_typing_animation_duration_ms(val));
        self.settings_changed();
    }
    fn setting_smooth_cursor_duration_ms(&self) -> u32 {
        self.with_app(80, |app| app.setting_smooth_cursor_duration_ms())
    }
    fn set_setting_smooth_cursor_duration_ms(&mut self, val: u32) {
        self.with_app_mut((), |app| app.set_setting_smooth_cursor_duration_ms(val));
        self.settings_changed();
    }
    fn ai_available(&self) -> bool {
        self.with_app(false, |app| app.ai_available())
    }
    fn ai_enabled(&self) -> bool {
        self.with_app(false, |app| app.ai_enabled())
    }
    fn set_ai_enabled(&mut self, val: bool) {
        self.with_app_mut((), |app| app.set_ai_enabled(val));
        self.ai_enabled_changed();
        self.settings_changed();
    }
    fn setting_linux_sidebar_width(&self) -> f64 {
        self.with_app(240.0, |app| app.setting_linux_sidebar_width())
    }
    fn set_setting_linux_sidebar_width(&mut self, val: f64) {
        self.with_app_mut((), |app| app.set_setting_linux_sidebar_width(val));
        self.settings_changed();
    }
    fn setting_linux_editor_width(&self) -> f64 {
        self.with_app(0.0, |app| app.setting_linux_editor_width())
    }
    fn set_setting_linux_editor_width(&mut self, val: f64) {
        self.with_app_mut((), |app| app.set_setting_linux_editor_width(val));
        self.settings_changed();
    }
    fn load_local_settings(&mut self) {
        self.with_app_mut((), |app| app.load_local_settings());
        self.settings_changed();
    }
    fn save_local_settings(&mut self) -> bool {
        self.with_app_mut(false, |app| app.save_local_settings())
    }
}

impl AppBackend {
    // Included inside impl AppBackend from app_backend.rs.
    // Deprecated compatibility methods for this Linux backend domain.

    // AppBackend::setting_font_size
    pub(crate) fn setting_font_size(&self) -> f32 {
        self.current_setting_font_size
    }

    // AppBackend::set_setting_font_size
    pub(crate) fn set_setting_font_size(&mut self, val: f32) {
        self.current_setting_font_size = val;
        self.settings_changed();
        self.save_local_settings();
    }

    // AppBackend::setting_line_spacing
    pub(crate) fn setting_line_spacing(&self) -> f32 {
        self.current_setting_line_spacing
    }

    // AppBackend::set_setting_line_spacing
    pub(crate) fn set_setting_line_spacing(&mut self, val: f32) {
        self.current_setting_line_spacing = val;
        self.settings_changed();
        self.save_local_settings();
    }

    // AppBackend::setting_auto_save_enabled
    pub(crate) fn setting_auto_save_enabled(&self) -> bool {
        self.current_setting_auto_save_enabled
    }

    // AppBackend::set_setting_auto_save_enabled
    pub(crate) fn set_setting_auto_save_enabled(&mut self, val: bool) {
        self.current_setting_auto_save_enabled = val;
        self.settings_changed();
        self.save_local_settings();
    }

    // AppBackend::setting_auto_save_delay_ms
    pub(crate) fn setting_auto_save_delay_ms(&self) -> u32 {
        self.current_setting_auto_save_delay_ms
    }

    // AppBackend::set_setting_auto_save_delay_ms
    pub(crate) fn set_setting_auto_save_delay_ms(&mut self, val: u32) {
        self.current_setting_auto_save_delay_ms = val;
        self.settings_changed();
        self.save_local_settings();
    }

    // AppBackend::setting_auto_indent_enabled
    pub(crate) fn setting_auto_indent_enabled(&self) -> bool {
        self.current_setting_auto_indent_enabled
    }

    // AppBackend::set_setting_auto_indent_enabled
    pub(crate) fn set_setting_auto_indent_enabled(&mut self, val: bool) {
        self.current_setting_auto_indent_enabled = val;
        self.settings_changed();
        self.save_local_settings();
    }

    // AppBackend::setting_auto_indent_width
    pub(crate) fn setting_auto_indent_width(&self) -> f32 {
        self.current_setting_auto_indent_width
    }

    // AppBackend::set_setting_auto_indent_width
    pub(crate) fn set_setting_auto_indent_width(&mut self, val: f32) {
        self.current_setting_auto_indent_width = val;
        self.settings_changed();
        self.save_local_settings();
    }

    // AppBackend::setting_theme_mode
    pub(crate) fn setting_theme_mode(&self) -> QString {
        if self.current_setting_theme_mode.is_empty() {
            "system".into()
        } else {
            self.current_setting_theme_mode.clone().into()
        }
    }

    // AppBackend::set_setting_theme_mode
    pub(crate) fn set_setting_theme_mode(&mut self, val: QString) {
        self.current_setting_theme_mode = val.to_string();
        self.settings_changed();
    }

    // AppBackend::setting_monet_color
    pub(crate) fn setting_monet_color(&self) -> QString {
        self.current_setting_monet_color.clone().into()
    }

    // AppBackend::set_setting_monet_color
    pub(crate) fn set_setting_monet_color(&mut self, val: QString) {
        self.current_setting_monet_color = val.to_string();
        self.settings_changed();
    }

    // AppBackend::setting_typing_animation_enabled
    pub(crate) fn setting_typing_animation_enabled(&self) -> bool {
        self.current_setting_typing_animation_enabled
    }

    // AppBackend::set_setting_typing_animation_enabled
    pub(crate) fn set_setting_typing_animation_enabled(&mut self, val: bool) {
        self.current_setting_typing_animation_enabled = val;
        self.settings_changed();
        self.save_local_settings();
    }

    // AppBackend::setting_smooth_cursor_enabled
    pub(crate) fn setting_smooth_cursor_enabled(&self) -> bool {
        self.current_setting_smooth_cursor_enabled
    }

    // AppBackend::set_setting_smooth_cursor_enabled
    pub(crate) fn set_setting_smooth_cursor_enabled(&mut self, val: bool) {
        self.current_setting_smooth_cursor_enabled = val;
        self.settings_changed();
        self.save_local_settings();
    }

    // AppBackend::setting_typing_animation_duration_ms
    pub(crate) fn setting_typing_animation_duration_ms(&self) -> u32 {
        self.current_setting_typing_animation_duration_ms
    }

    // AppBackend::set_setting_typing_animation_duration_ms
    pub(crate) fn set_setting_typing_animation_duration_ms(&mut self, val: u32) {
        self.current_setting_typing_animation_duration_ms = val;
        self.settings_changed();
        self.save_local_settings();
    }

    // AppBackend::setting_smooth_cursor_duration_ms
    pub(crate) fn setting_smooth_cursor_duration_ms(&self) -> u32 {
        self.current_setting_smooth_cursor_duration_ms
    }

    // AppBackend::set_setting_smooth_cursor_duration_ms
    pub(crate) fn set_setting_smooth_cursor_duration_ms(&mut self, val: u32) {
        self.current_setting_smooth_cursor_duration_ms = val;
        self.settings_changed();
        self.save_local_settings();
    }

    // AppBackend::setting_linux_sidebar_width
    pub(crate) fn setting_linux_sidebar_width(&self) -> f64 {
        self.current_setting_linux_sidebar_width
    }

    // AppBackend::set_setting_linux_sidebar_width
    pub(crate) fn set_setting_linux_sidebar_width(&mut self, val: f64) {
        self.current_setting_linux_sidebar_width = val;
        self.settings_changed();
        self.save_local_settings();
    }

    // AppBackend::setting_linux_editor_width
    pub(crate) fn setting_linux_editor_width(&self) -> f64 {
        self.current_setting_linux_editor_width
    }

    // AppBackend::set_setting_linux_editor_width
    pub(crate) fn set_setting_linux_editor_width(&mut self, val: f64) {
        self.current_setting_linux_editor_width = val;
        self.settings_changed();
        self.save_local_settings();
    }

    // AppBackend::load_app_theme_mode
    pub(crate) fn load_app_theme_mode(&mut self) {
        // Load theme mode from app_config (when no workspace is open)
        // Default to "system"
        self.current_setting_theme_mode = "system".to_string();
        self.current_setting_monet_color = "".to_string();
        self.settings_changed();
    }

    // AppBackend::load_local_settings
    pub(crate) fn load_local_settings(&mut self) {
        self.debug_log("settings", "load_local_settings_start", "");
        if let Some(core) = self.core_api() {
            let local_load = core.load_local_settings();
            self.debug_log(
                "settings",
                "load_local_settings_result",
                &format!("success={}", local_load.is_ok()),
            );
            if let Ok(settings) = local_load {
                self.current_setting_line_spacing = settings.editor_line_spacing_multiplier;
                self.current_setting_auto_save_enabled = settings.auto_save_enabled;
                self.current_setting_auto_save_delay_ms = settings.auto_save_delay_ms as u32;
                self.current_setting_auto_indent_enabled = settings.auto_indent_enabled;
                self.current_setting_auto_indent_width = settings.auto_indent_width;
                self.current_setting_typing_animation_enabled =
                    settings.editor_typing_animation_enabled;
                self.current_setting_smooth_cursor_enabled = settings.editor_smooth_cursor_enabled;
                self.current_setting_typing_animation_duration_ms =
                    settings.editor_typing_animation_duration_ms as u32;
                self.current_setting_smooth_cursor_duration_ms =
                    settings.editor_smooth_cursor_duration_ms as u32;
                self.current_ai_enabled = settings.ai_enabled;
                if let Some(ref device_id) = settings.stats_device_id {
                    if !device_id.is_empty() {
                        self.stats_device_id = device_id.clone();
                    }
                }
                self.current_setting_linux_sidebar_width = settings.linux_sidebar_width;
                self.current_setting_linux_editor_width = settings.linux_editor_width;
            }

            let syncable_load = core.load_syncable_settings();
            self.debug_log(
                "settings",
                "load_syncable_settings_result",
                &format!("success={}", syncable_load.is_ok()),
            );
            if let Ok(sync_settings) = syncable_load {
                self.current_setting_font_size = sync_settings.font_size as f32;
                if self.current_setting_font_size <= 0.0 {
                    if let Ok(local) = core.load_local_settings() {
                        self.current_setting_font_size = local.editor_font_size;
                    }
                    if self.current_setting_font_size <= 0.0 {
                        self.current_setting_font_size = 16.0;
                    }
                }
                self.current_setting_theme_mode = sync_settings.theme_mode.clone();
                self.current_setting_monet_color = sync_settings.monet_color.clone();
            } else {
                self.current_setting_monet_color = "".to_string();
                if let Ok(local) = core.load_local_settings() {
                    self.current_setting_font_size = local.editor_font_size;
                }
                if self.current_setting_font_size <= 0.0 {
                    self.current_setting_font_size = 16.0;
                }
                self.current_setting_theme_mode = "system".to_string();
            }

            self.settings_changed();
            self.debug_log(
                "settings",
                "load_local_settings_success",
                &format!(
                    "fontSize={}, themeMode={}",
                    self.current_setting_font_size, self.current_setting_theme_mode
                ),
            );
        } else {
            self.debug_warn(
                "settings",
                "load_local_settings_failed",
                "core_not_initialized",
            );
        }
    }

    // AppBackend::save_local_settings
    pub(crate) fn save_local_settings(&mut self) -> bool {
        self.debug_log("settings", "save_local_settings_start", "");
        let mut error_msg: Option<String> = None;
        if let Some(core) = self.core_api() {
            let mut local = core.load_local_settings().unwrap_or_else(|_| {
                writer_core::api::types::LocalSettingsDto::from(
                    writer_core::settings::LocalSettings::default(),
                )
            });
            local.editor_font_size = self.current_setting_font_size;
            local.editor_line_spacing_multiplier = self.current_setting_line_spacing;
            local.auto_save_enabled = self.current_setting_auto_save_enabled;
            local.auto_save_delay_ms = self.current_setting_auto_save_delay_ms as u64;
            local.auto_indent_enabled = self.current_setting_auto_indent_enabled;
            local.auto_indent_width = self.current_setting_auto_indent_width;
            local.editor_typing_animation_enabled = self.current_setting_typing_animation_enabled;
            local.editor_smooth_cursor_enabled = self.current_setting_smooth_cursor_enabled;
            local.editor_typing_animation_duration_ms =
                self.current_setting_typing_animation_duration_ms as u64;
            local.editor_smooth_cursor_duration_ms =
                self.current_setting_smooth_cursor_duration_ms as u64;
            local.ai_enabled = self.current_ai_enabled;
            local.linux_sidebar_width = self.current_setting_linux_sidebar_width;
            local.linux_editor_width = self.current_setting_linux_editor_width;

            let local_save = core.save_local_settings(local.clone());
            self.debug_log(
                "settings",
                "save_local_settings_result",
                &format!("success={}", local_save.is_ok()),
            );
            if let Err(e) = local_save {
                error_msg = Some(format!("保存本地设置失败: {}", e));
            }

            let mut syncable = core.load_syncable_settings().unwrap_or_else(|_| {
                writer_core::api::types::SyncableSettingsDto::from(
                    writer_core::settings::SyncableSettings::default(),
                )
            });
            syncable.font_size = self.current_setting_font_size as f64;
            syncable.theme_mode = self.current_setting_theme_mode.clone();
            syncable.monet_color = self.current_setting_monet_color.clone();

            let syncable_save = core.save_syncable_settings(syncable.clone());
            self.debug_log(
                "settings",
                "save_syncable_settings_result",
                &format!("success={}", syncable_save.is_ok()),
            );
            if let Err(e) = syncable_save {
                error_msg = Some(format!("保存同步设置失败: {}", e));
            }
        } else {
            error_msg = Some("Core 未初始化".to_string());
        }

        if let Some(msg) = error_msg {
            self.set_error(&msg);
            self.debug_error("settings", "save_local_settings_failed", &msg);
            false
        } else {
            self.debug_log("settings", "save_local_settings_success", "");
            true
        }
    }
}
