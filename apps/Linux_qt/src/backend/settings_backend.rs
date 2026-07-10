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
// - 被 apps/Linux_qt/src/backend/mod.rs 引用，用于实例化设置后端并绑定为 QML 全局上下文属性。
// =============================================================================

use super::*;
use crate::backend::SafeAppPtr;

#[allow(non_snake_case, dead_code)]
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
    setting_theme_palette_json: qt_property!(QString; READ setting_theme_palette_json WRITE set_setting_theme_palette_json NOTIFY settings_changed),
    setting_color_source: qt_property!(QString; READ setting_color_source WRITE set_setting_color_source NOTIFY settings_changed),
    setting_appearance_mode: qt_property!(QString; READ setting_appearance_mode WRITE set_setting_appearance_mode NOTIFY settings_changed),
    setting_dynamic_color_enabled: qt_property!(bool; READ setting_dynamic_color_enabled WRITE set_setting_dynamic_color_enabled NOTIFY settings_changed),
    setting_selected_palette_id: qt_property!(QString; READ setting_selected_palette_id WRITE set_setting_selected_palette_id NOTIFY settings_changed),
    setting_selected_builtin_theme_id: qt_property!(QString; READ setting_selected_builtin_theme_id WRITE set_setting_selected_builtin_theme_id NOTIFY settings_changed),
    setting_typing_animation_enabled: qt_property!(bool; READ setting_typing_animation_enabled WRITE set_setting_typing_animation_enabled NOTIFY settings_changed),
    setting_smooth_cursor_enabled: qt_property!(bool; READ setting_smooth_cursor_enabled WRITE set_setting_smooth_cursor_enabled NOTIFY settings_changed),
    setting_typing_animation_duration_ms: qt_property!(u32; READ setting_typing_animation_duration_ms WRITE set_setting_typing_animation_duration_ms NOTIFY settings_changed),
    setting_smooth_cursor_duration_ms: qt_property!(u32; READ setting_smooth_cursor_duration_ms WRITE set_setting_smooth_cursor_duration_ms NOTIFY settings_changed),
    setting_coordinated_text_cursor_animation_enabled: qt_property!(bool; READ setting_coordinated_text_cursor_animation_enabled WRITE set_setting_coordinated_text_cursor_animation_enabled NOTIFY settings_changed),
    ai_available: qt_property!(bool; READ ai_available NOTIFY ai_available_changed),
    ai_enabled: qt_property!(bool; READ ai_enabled WRITE set_ai_enabled NOTIFY ai_enabled_changed),
    setting_linux_qt_sidebar_width: qt_property!(f64; READ setting_linux_qt_sidebar_width WRITE set_setting_linux_qt_sidebar_width NOTIFY settings_changed),
    setting_linux_qt_editor_width: qt_property!(f64; READ setting_linux_qt_editor_width WRITE set_setting_linux_qt_editor_width NOTIFY settings_changed),
    setting_diagnostics_enabled: qt_property!(bool; READ setting_diagnostics_enabled WRITE set_setting_diagnostics_enabled NOTIFY settings_changed),
    setting_diagnostics_verbose: qt_property!(bool; READ setting_diagnostics_verbose WRITE set_setting_diagnostics_verbose NOTIFY settings_changed),
    settings_changed: qt_signal!(),
    /// 设置变更后发出，QML 层用 Timer 延迟调用 do_save_local_settings()
    save_requested: qt_signal!(),
    ai_enabled_changed: qt_signal!(),
    ai_available_changed: qt_signal!(),
    load_local_settings: qt_method!(fn(&mut self)),
    save_local_settings: qt_method!(fn(&mut self) -> bool),
    /// 标记设置已变更，需要保存。QML 层用 Timer 延迟调用 do_save_local_settings()
    debounced_save_local_settings: qt_method!(fn(&mut self)),
    /// 实际执行保存（由 QML Timer 触发或应用关闭时调用）
    do_save_local_settings: qt_method!(fn(&mut self) -> bool),
    /// 应用关闭前 flush pending save
    flush_pending_settings_save: qt_method!(fn(&mut self) -> bool),
    export_diagnostics_pack: qt_method!(fn(&self) -> QString),
    clear_logs: qt_method!(fn(&self) -> QString),
    copy_device_info: qt_method!(fn(&self) -> QString),
    open_log_directory: qt_method!(fn(&self) -> QString),
    copy_text_to_clipboard: qt_method!(fn(&mut self, text: QString) -> QString),
    list_palette_records_json: qt_method!(fn(&self) -> QString),
    list_builtin_themes_json: qt_method!(fn(&self) -> QString),
    load_selected_palette_json: qt_method!(fn(&self) -> QString),
    /// 标记是否有待保存的设置
    pending_save: bool,
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
            // SAFETY: pointer was set from QObjectBox-pinned AppBackend in
            // BackendRuntime::new; null-guarded above; single-threaded (Rc).
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
            // SAFETY: same as with_app; &mut is safe because callers hold
            // &mut self, preventing aliasing within this backend.
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
    fn setting_theme_palette_json(&self) -> QString {
        self.with_app("".into(), |app| app.setting_theme_palette_json())
    }
    fn set_setting_theme_palette_json(&mut self, val: QString) {
        self.with_app_mut((), |app| app.set_setting_theme_palette_json(val));
        self.settings_changed();
    }
    fn setting_color_source(&self) -> QString {
        self.with_app("built_in".into(), |app| app.setting_color_source())
    }
    fn set_setting_color_source(&mut self, val: QString) {
        self.with_app_mut((), |app| app.set_setting_color_source(val));
        self.settings_changed();
    }
    fn setting_appearance_mode(&self) -> QString {
        self.with_app("system".into(), |app| app.setting_appearance_mode())
    }
    fn set_setting_appearance_mode(&mut self, val: QString) {
        self.with_app_mut((), |app| app.set_setting_appearance_mode(val));
        self.settings_changed();
    }
    fn setting_dynamic_color_enabled(&self) -> bool {
        self.with_app(false, |app| app.setting_dynamic_color_enabled())
    }
    fn set_setting_dynamic_color_enabled(&mut self, val: bool) {
        self.with_app_mut((), |app| app.set_setting_dynamic_color_enabled(val));
        self.settings_changed();
    }
    fn setting_selected_palette_id(&self) -> QString {
        self.with_app("".into(), |app| app.setting_selected_palette_id())
    }
    fn set_setting_selected_palette_id(&mut self, val: QString) {
        self.with_app_mut((), |app| app.set_setting_selected_palette_id(val));
        self.settings_changed();
    }
    fn setting_selected_builtin_theme_id(&self) -> QString {
        self.with_app("".into(), |app| app.setting_selected_builtin_theme_id())
    }
    fn set_setting_selected_builtin_theme_id(&mut self, val: QString) {
        self.with_app_mut((), |app| app.set_setting_selected_builtin_theme_id(val));
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
    fn setting_coordinated_text_cursor_animation_enabled(&self) -> bool {
        self.with_app(true, |app| app.setting_coordinated_text_cursor_animation_enabled())
    }
    fn set_setting_coordinated_text_cursor_animation_enabled(&mut self, val: bool) {
        self.with_app_mut((), |app| app.set_setting_coordinated_text_cursor_animation_enabled(val));
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
    fn setting_linux_qt_sidebar_width(&self) -> f64 {
        self.with_app(240.0, |app| app.setting_linux_qt_sidebar_width())
    }
    fn set_setting_linux_qt_sidebar_width(&mut self, val: f64) {
        self.with_app_mut((), |app| app.set_setting_linux_qt_sidebar_width(val));
        self.settings_changed();
    }
    fn setting_linux_qt_editor_width(&self) -> f64 {
        self.with_app(0.0, |app| app.setting_linux_qt_editor_width())
    }
    fn set_setting_linux_qt_editor_width(&mut self, val: f64) {
        self.with_app_mut((), |app| app.set_setting_linux_qt_editor_width(val));
        self.settings_changed();
    }
    fn setting_diagnostics_enabled(&self) -> bool {
        self.with_app(true, |app| app.setting_diagnostics_enabled())
    }
    fn set_setting_diagnostics_enabled(&mut self, val: bool) {
        self.with_app_mut((), |app| app.set_setting_diagnostics_enabled(val));
        crate::backend::diagnostics::set_diagnostics_enabled(val);
        self.settings_changed();
    }
    fn setting_diagnostics_verbose(&self) -> bool {
        self.with_app(true, |app| app.setting_diagnostics_verbose())
    }
    fn set_setting_diagnostics_verbose(&mut self, val: bool) {
        self.with_app_mut((), |app| app.set_setting_diagnostics_verbose(val));
        crate::backend::diagnostics::set_verbose_enabled(val);
        self.settings_changed();
    }
    fn load_local_settings(&mut self) {
        self.with_app_mut((), |app| app.load_local_settings());
        // 同步 diagnostics_enabled 和 verbose 状态到 diagnostics 全局变量
        let enabled = self.with_app(true, |app| app.setting_diagnostics_enabled());
        let verbose = self.with_app(true, |app| app.setting_diagnostics_verbose());
        crate::backend::diagnostics::set_diagnostics_enabled(enabled);
        crate::backend::diagnostics::set_verbose_enabled(verbose);
        self.settings_changed();
    }
    fn save_local_settings(&mut self) -> bool {
        self.pending_save = false;
        self.with_app_mut(false, |app| app.save_local_settings())
    }

    /// 标记设置已变更，需要延迟保存。
    /// QML 层用 Timer 监听 save_requested 信号，延迟 300ms 后调用 do_save_local_settings()。
    fn debounced_save_local_settings(&mut self) {
        self.pending_save = true;
        self.save_requested();
    }

    /// 实际执行保存（由 QML Timer 触发）
    fn do_save_local_settings(&mut self) -> bool {
        if !self.pending_save {
            return true; // 没有待保存的设置
        }
        self.save_local_settings()
    }

    /// 应用关闭前 flush pending save
    fn flush_pending_settings_save(&mut self) -> bool {
        if self.pending_save {
            self.save_local_settings()
        } else {
            true
        }
    }

    fn export_diagnostics_pack(&self) -> QString {
        self.with_app("{\"success\":false,\"error\":\"no workspace\"}".into(), |app| {
            let workspace_path = std::path::PathBuf::from(&app.current_workspace);
            let log_dir = crate::backend::diagnostics::get_log_dir(&workspace_path);
            match crate::backend::diagnostics::export_diagnostics_pack(
                &workspace_path,
                &log_dir,
            ) {
                Ok(path) => {
                    let path_str = path.to_string_lossy().to_string();
                    let export_dir = path
                        .parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|| std::path::PathBuf::from(""));
                    let export_dir_str = export_dir.to_string_lossy().to_string();
                    // Construct file:// URL for QML consumption.
                    // Normalize path separators for QML file:// URL consumption.
                    let dir_url_path = export_dir_str.replace('\\', "/");
                    let export_dir_url = if dir_url_path.starts_with('/') {
                        format!("file://{}", dir_url_path)
                    } else {
                        format!("file:///{}", dir_url_path)
                    };
                    let open_result = if !export_dir.as_os_str().is_empty() {
                        crate::platform_utils::open_directory(&export_dir_str)
                    } else {
                        Err("导出目录为空".to_string())
                    };
                    let envelope = serde_json::json!({
                        "success": true,
                        "path": path_str,
                        "nativePath": path_str,
                        "zipPath": path_str,
                        "nativeZipPath": path_str,
                        "exportDir": export_dir_str,
                        "nativeExportDir": export_dir_str,
                        "exportDirUrl": export_dir_url,
                        "openedExportDir": open_result.is_ok(),
                        "openExportDirError": open_result.err()
                    });
                    envelope.to_string().into()
                }
                Err(e) => {
                    eprintln!("[SettingsBackend] export_diagnostics_pack failed: {}", e);
                    let envelope = serde_json::json!({
                        "success": false,
                        "error": e
                    });
                    envelope.to_string().into()
                }
            }
        })
    }

    fn clear_logs(&self) -> QString {
        self.with_app("".into(), |app| {
            let log_dir = crate::backend::diagnostics::get_log_dir(
                &std::path::PathBuf::from(&app.current_workspace),
            );
            match crate::backend::diagnostics::clear_logs(&log_dir) {
                Ok(()) => "ok".into(),
                Err(e) => {
                    eprintln!("[SettingsBackend] clear_logs failed: {}", e);
                    e.into()
                }
            }
        })
    }

    fn copy_device_info(&self) -> QString {
        crate::backend::diagnostics::device_info_json().into()
    }

    fn open_log_directory(&self) -> QString {
        self.with_app("".into(), |app| {
            let log_dir = crate::backend::diagnostics::get_log_dir(
                &std::path::PathBuf::from(&app.current_workspace),
            );
            match crate::backend::diagnostics::open_log_directory(&log_dir) {
                Ok(()) => "ok".into(),
                Err(e) => {
                    eprintln!("[SettingsBackend] open_log_directory failed: {}", e);
                    e.into()
                }
            }
        })
    }

    fn copy_text_to_clipboard(&mut self, text: QString) -> QString {
        self.with_app_mut("".into(), |app| app.copy_text_to_clipboard(text))
    }

    fn list_palette_records_json(&self) -> QString {
        self.with_app("[]".into(), |app| {
            if let Some(core) = app.core_api() {
                match core.list_palette_records() {
                    Ok(records) => {
                        let dtos: Vec<writer_core::api::types::ThemePaletteRecordDto> =
                            records.into_iter().map(Into::into).collect();
                        QString::from(serde_json::to_string(&dtos).unwrap_or_else(|_| "[]".to_string()))
                    }
                    Err(_) => QString::from("[]"),
                }
            } else {
                QString::from("[]")
            }
        })
    }

    fn list_builtin_themes_json(&self) -> QString {
        self.with_app("[]".into(), |app| {
            if let Some(core) = app.core_api() {
                let themes = core.list_builtin_themes();
                QString::from(serde_json::to_string(&themes).unwrap_or_else(|_| "[]".to_string()))
            } else {
                QString::from("[]")
            }
        })
    }

    fn load_selected_palette_json(&self) -> QString {
        self.with_app("".into(), |app| {
            let palette_id = app.setting_selected_palette_id().to_string();
            if palette_id.is_empty() {
                return QString::from("");
            }
            let parts: Vec<&str> = palette_id.splitn(2, ':').collect();
            if parts.len() != 2 {
                return QString::from("");
            }
            if let Some(core) = app.core_api() {
                match core.load_palette_record(parts[0], parts[1]) {
                    Ok(record) => {
                        let json = serde_json::to_string(&record).unwrap_or_default();
                        QString::from(json)
                    }
                    Err(_) => QString::from(""),
                }
            } else {
                QString::from("")
            }
        })
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
    // NOTE: No longer calls save_local_settings() immediately.
    // Saving is debounced at the QML layer (debouncedSave/flushSave).
    pub(crate) fn set_setting_font_size(&mut self, val: f32) {
        self.current_setting_font_size = val;
        self.settings_changed();
    }

    // AppBackend::setting_line_spacing
    pub(crate) fn setting_line_spacing(&self) -> f32 {
        self.current_setting_line_spacing
    }

    // AppBackend::set_setting_line_spacing
    pub(crate) fn set_setting_line_spacing(&mut self, val: f32) {
        self.current_setting_line_spacing = val;
        self.settings_changed();
    }

    // AppBackend::setting_auto_save_enabled
    pub(crate) fn setting_auto_save_enabled(&self) -> bool {
        self.current_setting_auto_save_enabled
    }

    // AppBackend::set_setting_auto_save_enabled
    pub(crate) fn set_setting_auto_save_enabled(&mut self, val: bool) {
        self.current_setting_auto_save_enabled = val;
        self.settings_changed();
    }

    // AppBackend::setting_auto_save_delay_ms
    pub(crate) fn setting_auto_save_delay_ms(&self) -> u32 {
        self.current_setting_auto_save_delay_ms
    }

    // AppBackend::set_setting_auto_save_delay_ms
    pub(crate) fn set_setting_auto_save_delay_ms(&mut self, val: u32) {
        self.current_setting_auto_save_delay_ms = val;
        self.settings_changed();
    }

    // AppBackend::setting_auto_indent_enabled
    pub(crate) fn setting_auto_indent_enabled(&self) -> bool {
        self.current_setting_auto_indent_enabled
    }

    // AppBackend::set_setting_auto_indent_enabled
    pub(crate) fn set_setting_auto_indent_enabled(&mut self, val: bool) {
        self.current_setting_auto_indent_enabled = val;
        self.settings_changed();
    }

    // AppBackend::setting_auto_indent_width
    pub(crate) fn setting_auto_indent_width(&self) -> f32 {
        self.current_setting_auto_indent_width
    }

    // AppBackend::set_setting_auto_indent_width
    pub(crate) fn set_setting_auto_indent_width(&mut self, val: f32) {
        self.current_setting_auto_indent_width = val;
        self.settings_changed();
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

    // AppBackend::setting_theme_palette_json
    pub(crate) fn setting_theme_palette_json(&self) -> QString {
        self.current_setting_theme_palette_json.clone().into()
    }

    // AppBackend::set_setting_theme_palette_json
    pub(crate) fn set_setting_theme_palette_json(&mut self, val: QString) {
        self.current_setting_theme_palette_json = val.to_string();
        self.settings_changed();
    }

    pub(crate) fn setting_color_source(&self) -> QString {
        self.current_setting_color_source.clone().into()
    }

    pub(crate) fn set_setting_color_source(&mut self, val: QString) {
        self.current_setting_color_source = val.to_string();
        self.settings_changed();
    }

    pub(crate) fn setting_appearance_mode(&self) -> QString {
        self.current_setting_appearance_mode.clone().into()
    }

    pub(crate) fn set_setting_appearance_mode(&mut self, val: QString) {
        self.current_setting_appearance_mode = val.to_string();
        self.settings_changed();
    }

    pub(crate) fn setting_dynamic_color_enabled(&self) -> bool {
        self.current_setting_dynamic_color_enabled
    }

    pub(crate) fn set_setting_dynamic_color_enabled(&mut self, val: bool) {
        self.current_setting_dynamic_color_enabled = val;
        self.settings_changed();
    }

    pub(crate) fn setting_selected_palette_id(&self) -> QString {
        self.current_setting_selected_palette_id.clone().into()
    }

    pub(crate) fn set_setting_selected_palette_id(&mut self, val: QString) {
        self.current_setting_selected_palette_id = val.to_string();
        self.settings_changed();
    }

    pub(crate) fn setting_selected_builtin_theme_id(&self) -> QString {
        self.current_setting_selected_builtin_theme_id.clone().into()
    }

    pub(crate) fn set_setting_selected_builtin_theme_id(&mut self, val: QString) {
        self.current_setting_selected_builtin_theme_id = val.to_string();
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
    }

    // AppBackend::setting_smooth_cursor_enabled
    pub(crate) fn setting_smooth_cursor_enabled(&self) -> bool {
        self.current_setting_smooth_cursor_enabled
    }

    // AppBackend::set_setting_smooth_cursor_enabled
    pub(crate) fn set_setting_smooth_cursor_enabled(&mut self, val: bool) {
        self.current_setting_smooth_cursor_enabled = val;
        self.settings_changed();
    }

    // AppBackend::setting_typing_animation_duration_ms
    pub(crate) fn setting_typing_animation_duration_ms(&self) -> u32 {
        self.current_setting_typing_animation_duration_ms
    }

    // AppBackend::set_setting_typing_animation_duration_ms
    pub(crate) fn set_setting_typing_animation_duration_ms(&mut self, val: u32) {
        self.current_setting_typing_animation_duration_ms = val;
        self.settings_changed();
    }

    // AppBackend::setting_smooth_cursor_duration_ms
    pub(crate) fn setting_smooth_cursor_duration_ms(&self) -> u32 {
        self.current_setting_smooth_cursor_duration_ms
    }

    // AppBackend::set_setting_smooth_cursor_duration_ms
    pub(crate) fn set_setting_smooth_cursor_duration_ms(&mut self, val: u32) {
        self.current_setting_smooth_cursor_duration_ms = val;
        self.settings_changed();
    }

    // AppBackend::setting_coordinated_text_cursor_animation_enabled
    pub(crate) fn setting_coordinated_text_cursor_animation_enabled(&self) -> bool {
        self.current_setting_coordinated_text_cursor_animation_enabled
    }

    // AppBackend::set_setting_coordinated_text_cursor_animation_enabled
    pub(crate) fn set_setting_coordinated_text_cursor_animation_enabled(&mut self, val: bool) {
        self.current_setting_coordinated_text_cursor_animation_enabled = val;
        self.settings_changed();
    }

    // AppBackend::setting_diagnostics_enabled
    pub(crate) fn setting_diagnostics_enabled(&self) -> bool {
        self.current_setting_diagnostics_enabled
    }

    // AppBackend::set_setting_diagnostics_enabled
    pub(crate) fn set_setting_diagnostics_enabled(&mut self, val: bool) {
        self.current_setting_diagnostics_enabled = val;
        self.settings_changed();
    }

    // AppBackend::setting_diagnostics_verbose
    pub(crate) fn setting_diagnostics_verbose(&self) -> bool {
        self.current_setting_diagnostics_verbose
    }

    // AppBackend::set_setting_diagnostics_verbose
    pub(crate) fn set_setting_diagnostics_verbose(&mut self, val: bool) {
        self.current_setting_diagnostics_verbose = val;
        self.settings_changed();
    }

    // AppBackend::setting_linux_qt_sidebar_width
    pub(crate) fn setting_linux_qt_sidebar_width(&self) -> f64 {
        self.current_setting_linux_qt_sidebar_width
    }

    // AppBackend::set_setting_linux_qt_sidebar_width
    pub(crate) fn set_setting_linux_qt_sidebar_width(&mut self, val: f64) {
        self.current_setting_linux_qt_sidebar_width = val;
        self.settings_changed();
    }

    // AppBackend::setting_linux_qt_editor_width
    pub(crate) fn setting_linux_qt_editor_width(&self) -> f64 {
        self.current_setting_linux_qt_editor_width
    }

    // AppBackend::set_setting_linux_qt_editor_width
    pub(crate) fn set_setting_linux_qt_editor_width(&mut self, val: f64) {
        self.current_setting_linux_qt_editor_width = val;
        self.settings_changed();
    }

    // AppBackend::load_app_theme_mode
    pub(crate) fn load_app_theme_mode(&mut self) {
        // Load theme mode from app_config (when no workspace is open)
        // Default to "system"
        self.current_setting_theme_mode = "system".to_string();
        self.current_setting_monet_color = "".to_string();
        self.current_setting_theme_palette_json = "".to_string();
        self.current_setting_color_source = "built_in".to_string();
        self.current_setting_appearance_mode = "system".to_string();
        self.current_setting_dynamic_color_enabled = false;
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
                self.current_setting_coordinated_text_cursor_animation_enabled =
                    settings.editor_coordinated_text_cursor_animation_enabled;
                self.current_ai_enabled = settings.ai_enabled;
                if let Some(ref device_id) = settings.stats_device_id {
                    if !device_id.is_empty() {
                        self.stats_device_id = device_id.clone();
                    }
                }
                self.current_setting_linux_qt_sidebar_width = settings.linux_qt_sidebar_width;
                self.current_setting_linux_qt_editor_width = settings.linux_qt_editor_width;
                self.current_setting_diagnostics_enabled = settings.diagnostics_enabled;
                self.current_setting_diagnostics_verbose = settings.diagnostics_verbose;
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
                self.current_setting_theme_palette_json = sync_settings.theme_palette_json.clone();
                if let Ok(local) = core.load_local_settings() {
                self.current_setting_color_source = local.color_source.clone();
                self.current_setting_appearance_mode = local.appearance_mode.clone();
                self.current_setting_dynamic_color_enabled = local.dynamic_color_enabled;
                self.current_setting_selected_palette_id = local.selected_palette_id.clone();
                self.current_setting_selected_builtin_theme_id = local.selected_builtin_theme_id.clone();

                // Auto-load selected palette into theme_palette_json for QML consumption
                if local.color_source == "saved_palette" && !local.selected_palette_id.is_empty() {
                    let parts: Vec<&str> = local.selected_palette_id.splitn(2, ':').collect();
                    if parts.len() == 2 {
                        if let Ok(record) = core.load_palette_record(parts[0], parts[1]) {
                            if let Ok(json) = serde_json::to_string(&record) {
                                self.current_setting_theme_palette_json = json;
                            }
                        }
                    }
                }
                }
            } else {
                self.current_setting_monet_color = "".to_string();
                self.current_setting_theme_palette_json = "".to_string();
                self.current_setting_color_source = "built_in".to_string();
                self.current_setting_appearance_mode = "system".to_string();
                self.current_setting_dynamic_color_enabled = false;
                self.current_setting_selected_palette_id = String::new();
                self.current_setting_selected_builtin_theme_id = String::new();
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
            local.editor_coordinated_text_cursor_animation_enabled =
                self.current_setting_coordinated_text_cursor_animation_enabled;
            local.ai_enabled = self.current_ai_enabled;
            local.linux_qt_sidebar_width = self.current_setting_linux_qt_sidebar_width;
            local.linux_qt_editor_width = self.current_setting_linux_qt_editor_width;
            local.diagnostics_enabled = self.current_setting_diagnostics_enabled;
            local.diagnostics_verbose = self.current_setting_diagnostics_verbose;
            local.color_source = self.current_setting_color_source.clone();
            local.appearance_mode = self.current_setting_appearance_mode.clone();
            local.dynamic_color_enabled = self.current_setting_dynamic_color_enabled;
            local.selected_palette_id = self.current_setting_selected_palette_id.clone();
            local.selected_builtin_theme_id = self.current_setting_selected_builtin_theme_id.clone();

            let local_result = core.save_local_settings(local.clone());
            let local_json = match local_result {
                Ok(data) => writer_core::api::ResultEnvelope::success_with_changes(
                    data,
                    vec!["settings.local.json".to_string()],
                    vec![writer_core::api::ChangedEntityDto {
                        entity_type: "SettingsSaved".to_string(),
                        entity_id: None,
                    }],
                ),
                Err(error) => writer_core::api::ResultEnvelope::<bool>::error(error),
            }
            .to_json_string();
            let local_envelope: serde_json::Value = serde_json::from_str(&local_json)
                .unwrap_or(serde_json::json!({"success": false, "errorCode": "JSON_ERROR"}));
            self.debug_log(
                "settings",
                "save_local_settings_result",
                &format!("success={}", local_envelope["success"]),
            );
            if local_envelope["success"] != true {
                let error_code = local_envelope["errorCode"].as_str().unwrap_or("UNKNOWN");
                let message_key = local_envelope["messageKey"].as_str().unwrap_or("");
                let resolved_key = if !message_key.is_empty() {
                    crate::backend::message_key_mapper::resolve_message_key(message_key).to_string()
                } else {
                    "error.other".to_string()
                };
                error_msg = Some(format!("{} ({})", resolved_key, error_code));
            }

            let mut syncable = core.load_syncable_settings().unwrap_or_else(|_| {
                writer_core::api::types::SyncableSettingsDto::from(
                    writer_core::settings::SyncableSettings::default(),
                )
            });
            syncable.font_size = self.current_setting_font_size as f64;
            syncable.theme_mode = self.current_setting_theme_mode.clone();
            #[allow(deprecated)]
            { syncable.monet_color = String::new(); }
            syncable.theme_palette_json = self.current_setting_theme_palette_json.clone();

            let syncable_result = core.save_syncable_settings(syncable.clone());
            let syncable_json = match syncable_result {
                Ok(data) => writer_core::api::ResultEnvelope::success_with_changes(
                    data,
                    vec!["settings.sync.json".to_string()],
                    vec![writer_core::api::ChangedEntityDto {
                        entity_type: "SettingsSaved".to_string(),
                        entity_id: None,
                    }],
                ),
                Err(error) => writer_core::api::ResultEnvelope::<bool>::error(error),
            }
            .to_json_string();
            let syncable_envelope: serde_json::Value = serde_json::from_str(&syncable_json)
                .unwrap_or(serde_json::json!({"success": false, "errorCode": "JSON_ERROR"}));
            self.debug_log(
                "settings",
                "save_syncable_settings_result",
                &format!("success={}", syncable_envelope["success"]),
            );
            if syncable_envelope["success"] != true {
                let error_code = syncable_envelope["errorCode"].as_str().unwrap_or("UNKNOWN");
                let message_key = syncable_envelope["messageKey"].as_str().unwrap_or("");
                let resolved_key = if !message_key.is_empty() {
                    crate::backend::message_key_mapper::resolve_message_key(message_key).to_string()
                } else {
                    "error.other".to_string()
                };
                error_msg = Some(format!("{} ({})", resolved_key, error_code));
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
