use super::*;
use crate::backend::SafeAppPtr;

#[allow(non_snake_case)]
#[derive(QObject, Default)]
pub struct EditorBackend {
    base: qt_base_class!(trait QObject),
    save_status: qt_property!(QString; READ save_status WRITE set_save_status NOTIFY save_status_changed),
    word_count: qt_property!(i32; READ word_count WRITE set_word_count NOTIFY word_count_changed),
    error_message: qt_property!(QString; READ error_message NOTIFY error_occurred),
    selected_item_id: qt_property!(QString; READ selected_item_id NOTIFY selected_item_changed),
    has_selected_chapter_prop: qt_property!(bool; READ has_selected_chapter_prop NOTIFY selected_item_changed),
    chapter_path: qt_property!(QString; READ chapter_path NOTIFY chapter_path_changed),
    setting_font_size: qt_property!(f32; READ setting_font_size WRITE set_setting_font_size NOTIFY settings_changed),
    setting_line_spacing: qt_property!(f32; READ setting_line_spacing WRITE set_setting_line_spacing NOTIFY settings_changed),
    setting_auto_save_enabled: qt_property!(bool; READ setting_auto_save_enabled WRITE set_setting_auto_save_enabled NOTIFY settings_changed),
    setting_auto_save_delay_ms: qt_property!(u32; READ setting_auto_save_delay_ms WRITE set_setting_auto_save_delay_ms NOTIFY settings_changed),
    setting_auto_indent_enabled: qt_property!(bool; READ setting_auto_indent_enabled WRITE set_setting_auto_indent_enabled NOTIFY settings_changed),
    setting_smooth_cursor_enabled: qt_property!(bool; READ setting_smooth_cursor_enabled NOTIFY settings_changed),
    setting_typing_animation_enabled: qt_property!(bool; READ setting_typing_animation_enabled NOTIFY settings_changed),
    setting_smooth_cursor_duration_ms: qt_property!(u32; READ setting_smooth_cursor_duration_ms NOTIFY settings_changed),
    setting_typing_animation_duration_ms: qt_property!(u32; READ setting_typing_animation_duration_ms NOTIFY settings_changed),
    has_workspace: qt_property!(bool; READ has_workspace NOTIFY workspace_state_changed),
    sync_enabled: qt_property!(bool; READ sync_enabled NOTIFY sync_config_changed),
    sync_auto_sync: qt_property!(bool; READ sync_auto_sync NOTIFY sync_config_changed),
    save_status_changed: qt_signal!(),
    word_count_changed: qt_signal!(),
    error_occurred: qt_signal!(),
    selected_item_changed: qt_signal!(),
    chapter_path_changed: qt_signal!(),
    clear_editor: qt_signal!(),
    settings_changed: qt_signal!(),
    workspace_state_changed: qt_signal!(),
    sync_config_changed: qt_signal!(),
    calculate_word_count: qt_method!(fn(&mut self, text: QString)),
    open_chapter_json: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) -> QString),
    open_chapter: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) -> QJsonObject),
    get_chapter_content: qt_method!(fn(&self, project_id: QString, volume_id: QString, chapter_id: QString) -> QString),
    save_chapter: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString, content: QString) -> QJsonObject),
    clear_chapter_content: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) -> QJsonObject),
    report_writing_event: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString, source: QString, inserted_chars: u32, deleted_chars: u32, pasted_chars: u32)),
    process_writing_event_from_text: qt_method!(fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString, old_text: QString, new_text: QString)),
    get_writing_stats_summary: qt_method!(fn(&self, start_date: QString, end_date: QString) -> QString),
    get_writing_stats_summary_object: qt_method!(fn(&self, start_date: QString, end_date: QString) -> QJsonObject),
    get_writing_stats_by_project: qt_method!(fn(&self, start_date: QString, end_date: QString) -> QString),
    get_writing_stats_by_project_object: qt_method!(fn(&self, start_date: QString, end_date: QString) -> QJsonObject),
    get_writing_stats_by_chapter: qt_method!(fn(&self, start_date: QString, end_date: QString) -> QString),
    get_writing_stats_by_chapter_object: qt_method!(fn(&self, start_date: QString, end_date: QString) -> QJsonObject),
    get_writing_stats_by_device: qt_method!(fn(&self, start_date: QString, end_date: QString) -> QString),
    get_writing_stats_by_device_object: qt_method!(fn(&self, start_date: QString, end_date: QString) -> QJsonObject),
    get_writing_speed_curve: qt_method!(fn(&self, start_date: QString, end_date: QString, bucket_minutes: u32) -> QString),
    get_writing_speed_curve_object: qt_method!(fn(&self, start_date: QString, end_date: QString, bucket_minutes: u32) -> QJsonObject),
    flush_writing_stats: qt_method!(fn(&self)),
    flush_recent_edits: qt_method!(fn(&self)),
    has_selected_chapter: qt_method!(fn(&self) -> bool),
    selected_chapter_exists: qt_method!(fn(&self) -> bool),
    clear_editor_state: qt_method!(fn(&mut self)),
    request_auto_sync: qt_method!(fn(&mut self, reason: QString)),
    log_qml: qt_method!(fn(&self, level: QString, module: QString, event: QString, message: QString)),
    list_registered_actions: qt_method!(fn(&mut self) -> QString),
    execute_action: qt_method!(fn(&mut self, action_id: QString, args_json: QString, context_json: QString) -> QString),
    app: SafeAppPtr,
}

impl EditorBackend {
    pub fn new(app: SafeAppPtr) -> Self { Self { app, ..Default::default() } }
    fn with_app<R>(&self, default: R, f: impl FnOnce(&AppBackend) -> R) -> R {
        if let Some(app) = self.app.get() {
            unsafe { f(&*app) }
        } else {
            crate::backend::app_backend::debug_error_static("editor", "BACKEND_LINK_BROKEN", "app pointer is null");
            default
        }
    }
    fn with_app_mut<R>(&mut self, default: R, f: impl FnOnce(&mut AppBackend) -> R) -> R {
        if let Some(app) = self.app.get() {
            unsafe { f(&mut *app) }
        } else {
            crate::backend::app_backend::debug_error_static("editor", "BACKEND_LINK_BROKEN", "app pointer is null");
            default
        }
    }
    fn save_status(&self) -> QString { self.with_app("".into(), |app| app.save_status()) }
    fn set_save_status(&mut self, val: QString) { self.with_app_mut((), |app| app.set_save_status(val)); self.save_status_changed(); }
    fn word_count(&self) -> i32 { self.with_app(0, |app| app.word_count()) }
    fn set_word_count(&mut self, val: i32) { self.with_app_mut((), |app| app.set_word_count(val)); self.word_count_changed(); }
    fn error_message(&self) -> QString { self.with_app("".into(), |app| app.error_message()) }
    fn selected_item_id(&self) -> QString { self.with_app("".into(), |app| app.selected_item_id()) }
    fn has_selected_chapter_prop(&self) -> bool { self.with_app(false, |app| app.has_selected_chapter_prop()) }
    fn chapter_path(&self) -> QString { self.with_app("".into(), |app| app.chapter_path()) }
    fn setting_font_size(&self) -> f32 { self.with_app(16.0, |app| app.setting_font_size()) }
    fn set_setting_font_size(&mut self, val: f32) { self.with_app_mut((), |app| app.set_setting_font_size(val)); self.settings_changed(); }
    fn setting_line_spacing(&self) -> f32 { self.with_app(1.5, |app| app.setting_line_spacing()) }
    fn set_setting_line_spacing(&mut self, val: f32) { self.with_app_mut((), |app| app.set_setting_line_spacing(val)); self.settings_changed(); }
    fn setting_auto_save_enabled(&self) -> bool { self.with_app(true, |app| app.setting_auto_save_enabled()) }
    fn set_setting_auto_save_enabled(&mut self, val: bool) { self.with_app_mut((), |app| app.set_setting_auto_save_enabled(val)); self.settings_changed(); }
    fn setting_auto_save_delay_ms(&self) -> u32 { self.with_app(1500, |app| app.setting_auto_save_delay_ms()) }
    fn set_setting_auto_save_delay_ms(&mut self, val: u32) { self.with_app_mut((), |app| app.set_setting_auto_save_delay_ms(val)); self.settings_changed(); }
    fn setting_auto_indent_enabled(&self) -> bool { self.with_app(true, |app| app.setting_auto_indent_enabled()) }
    fn set_setting_auto_indent_enabled(&mut self, val: bool) { self.with_app_mut((), |app| app.set_setting_auto_indent_enabled(val)); self.settings_changed(); }
    fn setting_smooth_cursor_enabled(&self) -> bool { self.with_app(true, |app| app.setting_smooth_cursor_enabled()) }
    fn setting_typing_animation_enabled(&self) -> bool { self.with_app(true, |app| app.setting_typing_animation_enabled()) }
    fn setting_smooth_cursor_duration_ms(&self) -> u32 { self.with_app(80, |app| app.setting_smooth_cursor_duration_ms()) }
    fn setting_typing_animation_duration_ms(&self) -> u32 { self.with_app(100, |app| app.setting_typing_animation_duration_ms()) }
    fn has_workspace(&self) -> bool { self.with_app(false, |app| app.has_workspace()) }
    fn sync_enabled(&self) -> bool { self.with_app(false, |app| app.sync_enabled()) }
    fn sync_auto_sync(&self) -> bool { self.with_app(false, |app| app.sync_auto_sync()) }
    fn calculate_word_count(&mut self, text: QString) { self.with_app_mut((), |app| app.calculate_word_count(text)); self.word_count_changed(); }
    fn open_chapter_json(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) -> QString { let out = self.with_app_mut("{}".into(), |app| app.open_chapter_json(project_id, volume_id, chapter_id)); self.selected_item_changed(); self.chapter_path_changed(); out }
    fn open_chapter(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) -> QJsonObject { let out = self.with_app_mut(QJsonObject::default(), |app| app.open_chapter(project_id, volume_id, chapter_id)); self.selected_item_changed(); self.chapter_path_changed(); out }
    fn get_chapter_content(&self, project_id: QString, volume_id: QString, chapter_id: QString) -> QString { self.with_app("".into(), |app| app.get_chapter_content(project_id, volume_id, chapter_id)) }
    fn save_chapter(&mut self, project_id: QString, volume_id: QString, chapter_id: QString, content: QString) -> QJsonObject { let out = self.with_app_mut(QJsonObject::default(), |app| app.save_chapter(project_id, volume_id, chapter_id, content)); self.save_status_changed(); out }
    fn clear_chapter_content(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) -> QJsonObject { self.with_app_mut(QJsonObject::default(), |app| app.clear_chapter_content(project_id, volume_id, chapter_id)) }
    fn report_writing_event(&mut self, project_id: QString, volume_id: QString, chapter_id: QString, source: QString, inserted_chars: u32, deleted_chars: u32, pasted_chars: u32) { self.with_app_mut((), |app| app.report_writing_event(project_id, volume_id, chapter_id, source, inserted_chars, deleted_chars, pasted_chars)); }
    fn process_writing_event_from_text(&mut self, project_id: QString, volume_id: QString, chapter_id: QString, old_text: QString, new_text: QString) { self.with_app_mut((), |app| app.process_writing_event_from_text(project_id, volume_id, chapter_id, old_text, new_text)); }
    fn get_writing_stats_summary(&self, start_date: QString, end_date: QString) -> QString { self.with_app("{}".into(), |app| app.get_writing_stats_summary(start_date, end_date)) }
    fn get_writing_stats_summary_object(&self, start_date: QString, end_date: QString) -> QJsonObject { self.with_app(QJsonObject::default(), |app| app.get_writing_stats_summary_object(start_date, end_date)) }
    fn get_writing_stats_by_project(&self, start_date: QString, end_date: QString) -> QString { self.with_app("{}".into(), |app| app.get_writing_stats_by_project(start_date, end_date)) }
    fn get_writing_stats_by_project_object(&self, start_date: QString, end_date: QString) -> QJsonObject { self.with_app(QJsonObject::default(), |app| app.get_writing_stats_by_project_object(start_date, end_date)) }
    fn get_writing_stats_by_chapter(&self, start_date: QString, end_date: QString) -> QString { self.with_app("{}".into(), |app| app.get_writing_stats_by_chapter(start_date, end_date)) }
    fn get_writing_stats_by_chapter_object(&self, start_date: QString, end_date: QString) -> QJsonObject { self.with_app(QJsonObject::default(), |app| app.get_writing_stats_by_chapter_object(start_date, end_date)) }
    fn get_writing_stats_by_device(&self, start_date: QString, end_date: QString) -> QString { self.with_app("{}".into(), |app| app.get_writing_stats_by_device(start_date, end_date)) }
    fn get_writing_stats_by_device_object(&self, start_date: QString, end_date: QString) -> QJsonObject { self.with_app(QJsonObject::default(), |app| app.get_writing_stats_by_device_object(start_date, end_date)) }
    fn get_writing_speed_curve(&self, start_date: QString, end_date: QString, bucket_minutes: u32) -> QString { self.with_app("{}".into(), |app| app.get_writing_speed_curve(start_date, end_date, bucket_minutes)) }
    fn get_writing_speed_curve_object(&self, start_date: QString, end_date: QString, bucket_minutes: u32) -> QJsonObject { self.with_app(QJsonObject::default(), |app| app.get_writing_speed_curve_object(start_date, end_date, bucket_minutes)) }
    fn flush_writing_stats(&self) { self.with_app((), |app| app.flush_writing_stats()); }
    fn flush_recent_edits(&self) { self.with_app((), |app| app.flush_recent_edits()); }
    fn has_selected_chapter(&self) -> bool { self.with_app(false, |app| app.has_selected_chapter()) }
    fn selected_chapter_exists(&self) -> bool { self.with_app(false, |app| app.selected_chapter_exists()) }
    fn clear_editor_state(&mut self) { self.with_app_mut((), |app| app.clear_editor_state()); self.clear_editor(); }
    fn request_auto_sync(&mut self, reason: QString) { self.with_app_mut((), |app| app.request_auto_sync(reason)); }
    fn log_qml(&self, level: QString, module: QString, event: QString, message: QString) { self.with_app((), |app| app.log_qml(level, module, event, message)); }
    fn list_registered_actions(&mut self) -> QString { self.with_app_mut("[]".into(), |app| app.list_registered_actions()) }
    fn execute_action(&mut self, action_id: QString, args_json: QString, context_json: QString) -> QString { self.with_app_mut("{}".into(), |app| app.execute_action(action_id, args_json, context_json)) }
}

impl AppBackend {
// Included inside impl AppBackend from app_backend.rs.
// Deprecated compatibility methods for this Linux backend domain.

// AppBackend::save_status
    pub(crate) fn save_status(&self) -> QString {
        self.current_save_status.clone().into()
    }

// AppBackend::set_save_status
    pub(crate) fn set_save_status(&mut self, status: QString) {
        self.current_save_status = status.to_string();
        self.save_status_changed();
    }

// AppBackend::word_count
    pub(crate) fn word_count(&self) -> i32 {
        self.current_word_count
    }

// AppBackend::set_word_count
    pub(crate) fn set_word_count(&mut self, count: i32) {
        self.current_word_count = count;
        self.word_count_changed();
    }

// AppBackend::error_message
    pub(crate) fn error_message(&self) -> QString {
        self.current_error_message.clone().into()
    }

// AppBackend::has_selected_chapter
    pub(crate) fn has_selected_chapter(&self) -> bool {
        self.selected_chapter_id.is_some()
    }

// AppBackend::selected_chapter_exists
    pub(crate) fn selected_chapter_exists(&self) -> bool {
        if let (Some(api), Some(p), Some(v), Some(c)) = (
            self.core_api(),
            &self.selected_project_id,
            &self.selected_volume_id,
            &self.selected_chapter_id,
        ) {
            if let Ok(chapters) = api.list_chapters(p, v) {
                return chapters.iter().any(|chap| chap.id == *c);
            }
        }
        false
    }

// AppBackend::list_registered_actions
    pub(crate) fn list_registered_actions(&mut self) -> QString {
        if let Some(api) = self.core_api() {
            match api.list_registered_actions() {
                Ok(actions) => {
                    let json = serde_json::to_string(&actions).unwrap_or_else(|_| "[]".to_string());
                    json.into()
                },
                Err(_) => "[]".into()
            }
        } else {
            "[]".into()
        }
    }

// AppBackend::execute_action
    pub(crate) fn execute_action(&mut self, action_id: QString, args_json: QString, context_json: QString) -> QString {
        if let Some(api) = self.core_api() {
            match api.execute_action_ext(&action_id.to_string(), &args_json.to_string(), &context_json.to_string()) {
                Ok(result) => {
                    let json = serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());
                    json.into()
                },
                Err(e) => {
                    let err_json = serde_json::json!({
                        "success": false,
                        "message": e.to_string()
                    });
                    err_json.to_string().into()
                }
            }
        } else {
            let err_json = serde_json::json!({
                "success": false,
                "message": "Core not initialized"
            });
            err_json.to_string().into()
        }
    }

// AppBackend::clear_editor_state
    pub(crate) fn clear_editor_state(&mut self) {
        self.selected_chapter_id = None;
        self.selected_item_changed();
        self.chapter_path_changed();
        self.clear_editor();
    }

// AppBackend::chapter_path
    pub(crate) fn chapter_path(&self) -> QString {
        if let (Some(api), Some(p), Some(v), Some(c)) = (
            self.core_api(),
            &self.selected_project_id,
            &self.selected_volume_id,
            &self.selected_chapter_id,
        ) {
            let mut path = String::new();
            if let Ok(projects) = api.list_projects() {
                if let Some(proj) = projects.iter().find(|x| x.id == *p) {
                    path.push_str(&proj.title);
                }
            }
            if let Ok(volumes) = api.list_volumes(p) {
                if let Some(vol) = volumes.iter().find(|x| x.id == *v) {
                    path.push_str(" > ");
                    path.push_str(&vol.title);
                }
            }
            if let Ok(chapters) = api.list_chapters(p, v) {
                if let Some(chap) = chapters.iter().find(|x| x.id == *c) {
                    path.push_str(" > ");
                    path.push_str(&chap.title);
                }
            }
            return path.into();
        }
        "".into()
    }

// AppBackend::has_selected_chapter_prop
    pub(crate) fn has_selected_chapter_prop(&self) -> bool {
        self.selected_chapter_id.is_some()
    }

// AppBackend::selected_item_id
    pub(crate) fn selected_item_id(&self) -> QString {
        if let Some(ref id) = self.selected_chapter_id {
            return id.clone().into();
        }
        if let Some(ref id) = self.selected_volume_id {
            return id.clone().into();
        }
        if let Some(ref id) = self.selected_project_id {
            return id.clone().into();
        }
        "".into()
    }

// AppBackend::set_error
    pub(crate) fn set_error(&mut self, msg: &str) {
        self.current_error_message = msg.to_string();
        self.error_occurred();
    }

// AppBackend::calculate_word_count
    pub(crate) fn calculate_word_count(&mut self, text: QString) {
        let text_str = text.to_string();
        let count = if let Some(api) = self.core_api() {
            api.calculate_word_count(&text_str) as i32
        } else {
            writer_core::chapter::calculate_word_count(&text_str) as i32
        };
        self.set_word_count(count);
    }

// AppBackend::get_chapter_content
    pub(crate) fn get_chapter_content(
        &self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
    ) -> QString {
        let p = project_id.to_string();
        let v = volume_id.to_string();
        let c = chapter_id.to_string();
        self.debug_log("chapter", "get_chapter_content_start", &format!("project_id={}, volume_id={}, chapter_id={}", p, v, c));
        if let Some(api) = self.core_api() {
            match api.open_chapter(&p, &v, &c) {
                Ok(content) => {
                    self.debug_log("chapter", "get_chapter_content_success", &format!("len={}", content.content.len()));
                    return content.content.into();
                }
                Err(e) => {
                    self.debug_error("chapter", "get_chapter_content_failed", &format!("error={}", e));
                }
            }
        } else {
            self.debug_error("chapter", "get_chapter_content_failed", "core_not_initialized");
        }
        "".into()
    }

// AppBackend::open_chapter_json
    pub(crate) fn open_chapter_json(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) -> QString {
        let p = project_id.to_string();
        let v = volume_id.to_string();
        let c = chapter_id.to_string();
        if let Some(core) = self.core_api() {
            return match core.open_chapter(&p, &v, &c) {
                Ok(content) => serde_json::json!({
                    "success": true,
                    "content": content.content,
                    "title": content.meta.title,
                    "projectId": p,
                    "volumeId": v,
                    "chapterId": c,
                    "meta": content.meta,
                }).to_string().into(),
                Err(e) => serde_json::json!({
                    "success": false,
                    "code": "CORE_ERROR",
                    "error": format!("读取章节失败: {}", e),
                    "message": format!("读取章节失败: {}", e),
                }).to_string().into(),
            };
        }
        serde_json::json!({
            "success": false,
            "code": "INVALID_WORKSPACE",
            "error": "后端未初始化",
            "message": "后端未初始化",
        }).to_string().into()
    }

// AppBackend::open_chapter
    pub(crate) fn open_chapter(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) -> QJsonObject {
        let p = project_id.to_string();
        let v = volume_id.to_string();
        let c = chapter_id.to_string();
        self.debug_log("chapter", "open_chapter_start", &format!("project_id={}, volume_id={}, chapter_id={}", p, v, c));
        
        if let Some(core) = self.core_api() {
            match writing_bridge::open_chapter(&core, &p, &v, &c) {
                Ok(data) => {
                    self.selected_project_id = Some(p.clone());
                    self.selected_volume_id = Some(v.clone());
                    self.selected_chapter_id = Some(c.clone());
                    self.selected_item_changed();
                    self.chapter_path_changed();
                    
                    self.debug_log("chapter", "open_chapter_success", "len_loaded");
                    
                    let mut obj = serde_to_qjson_object(serde_json::to_value(data).unwrap_or_default());
                    obj.insert("success", serde_value_to_qjson(serde_json::Value::Bool(true)));
                    return obj;
                }
                Err(e) => {
                    self.debug_error("chapter", "open_chapter_failed", &e.to_string());
                    return bridge_error_object(&format!("读取章节失败: {}", e), "CORE_ERROR");
                }
            }
        }
        
        bridge_error_object("后端未初始化", "INVALID_WORKSPACE")
    }

// AppBackend::save_chapter
    pub(crate) fn save_chapter(&mut self, project_id: QString, volume_id: QString, chapter_id: QString, content: QString) -> QJsonObject {
        let p = project_id.to_string();
        let v = volume_id.to_string();
        let c = chapter_id.to_string();
        let text_str = content.to_string();
        let len = text_str.len();
        self.debug_log("chapter", "save_chapter_start", &format!("len={}", len));
        
        let save_result = if let Some(core) = self.core_api() {
            Some(writing_bridge::save_chapter(&core, &p, &v, &c, &text_str))
        } else {
            None
        };
        
        let result_obj = match save_result {
            Some(Ok(receipt)) => {
                self.debug_log("chapter", "save_chapter_success", "");
                self.current_save_status = "已保存".to_string();
                self.workspace_content_changed();
                self.flush_writing_stats();
                bridge_success_object(serde_json::to_value(receipt).unwrap())
            }
            Some(Err(e)) => {
                self.debug_error("chapter", "save_chapter_failed", &format!("error={}", e));
                if is_empty_overwrite_blocked(&e) {
                    let msg = blocked_empty_overwrite_user_message();
                    self.current_save_status = msg.to_string();
                    self.set_error(msg);
                } else {
                    self.current_save_status = "保存失败".to_string();
                }
                bridge_error_object(&format!("保存失败: {}", e), "CORE_ERROR")
            }
            None => {
                self.debug_error("chapter", "save_chapter_failed", "core_not_initialized");
                bridge_error_object("后端未初始化", "INVALID_WORKSPACE")
            }
        };
        
        self.save_status_changed();
        result_obj
    }

// AppBackend::clear_chapter_content
    pub(crate) fn clear_chapter_content(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) -> QJsonObject {
        let p = project_id.to_string();
        let v = volume_id.to_string();
        let c = chapter_id.to_string();
        self.debug_log("chapter", "clear_chapter_content_start", &format!("chapter_id={}", c));

        let clear_result = if let Some(core) = self.core_api() {
            Some(writing_bridge::clear_chapter_content(&core, &p, &v, &c))
        } else {
            None
        };

        match clear_result {
            Some(Ok(receipt)) => {
                self.debug_log("chapter", "clear_chapter_content_success", &format!("chapter_id={}", c));
                self.current_save_status = "已清空".to_string();
                self.save_status_changed();
                self.workspace_content_changed();
                bridge_success_object(serde_json::to_value(receipt).unwrap())
            }
            Some(Err(e)) => {
                let err_msg = format!("清空章节失败: {}", e);
                self.debug_error("chapter", "clear_chapter_content_failed", &err_msg);
                self.current_save_status = "清空失败".to_string();
                self.save_status_changed();
                self.set_error(&err_msg);
                bridge_error_object(&err_msg, "CORE_ERROR")
            }
            None => {
                self.debug_error("chapter", "clear_chapter_content_failed", "core_not_initialized");
                self.current_save_status = "清空失败".to_string();
                self.save_status_changed();
                self.set_error("清空章节失败: 后端未初始化");
                bridge_error_object("清空章节失败: 后端未初始化", "INVALID_WORKSPACE")
            }
        }
    }

// AppBackend::report_writing_event
    pub(crate) fn report_writing_event(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        source: QString,
        inserted_chars: u32,
        deleted_chars: u32,
        pasted_chars: u32,
    ) {
        let pid = project_id.to_string();
        let vid = volume_id.to_string();
        let cid = chapter_id.to_string();
        let src = source.to_string();

        if let Some(core) = self.core_api() {
            writing_bridge::ensure_stats_session(&core, &mut self.stats_device_id, &mut self.stats_session_id, &mut self.stats_last_event_ms);
            
            if let Err(e) = writing_bridge::report_writing_event(
                &core,
                &pid,
                &vid,
                &cid,
                &src,
                inserted_chars,
                deleted_chars,
                pasted_chars,
                0,
                &self.stats_device_id,
                &self.stats_session_id,
            ) {
                self.debug_error("stats", "report_writing_event_failed", &e.to_string());
            }
            self.flush_writing_stats();
        }
    }

// AppBackend::process_writing_event_from_text
    pub(crate) fn process_writing_event_from_text(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        old_text: QString,
        new_text: QString,
    ) {
        let pid = project_id.to_string();
        let vid = volume_id.to_string();
        let cid = chapter_id.to_string();
        let ot = old_text.to_string();
        let nt = new_text.to_string();

        if let Some(core) = self.core_api() {
            writing_bridge::ensure_stats_session(&core, &mut self.stats_device_id, &mut self.stats_session_id, &mut self.stats_last_event_ms);
            
            if let Err(e) = writing_bridge::process_writing_event_from_text(
                &core,
                &pid,
                &vid,
                &cid,
                &ot,
                &nt,
                &self.stats_device_id,
                &self.stats_session_id,
            ) {
                self.debug_error("stats", "process_writing_event_from_text_failed", &e.to_string());
            }
            self.flush_writing_stats();
        }
    }

// AppBackend::get_writing_stats_summary
    pub(crate) fn get_writing_stats_summary(&self, start_date: QString, end_date: QString) -> QString {
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_api() {
            match core.get_writing_stats_summary_json(&sd, &ed) {
                Ok(val) => val.into(),
                Err(_) => "{}".into(),
            }
        } else {
            "{}".into()
        }
    }

// AppBackend::get_writing_stats_summary_object
    pub(crate) fn get_writing_stats_summary_object(&self, start_date: QString, end_date: QString) -> QJsonObject {
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_api() {
            match core.get_writing_stats_summary_json(&sd, &ed) {
                Ok(val) => qjson_object_from_json(&val),
                Err(_) => QJsonObject::default(),
            }
        } else {
            QJsonObject::default()
        }
    }

// AppBackend::flush_writing_stats
    pub(crate) fn flush_writing_stats(&self) {
        if let Some(core) = self.core_api() {
            let _ = core.flush_writing_stats();
        }
    }

// AppBackend::flush_recent_edits
    pub(crate) fn flush_recent_edits(&self) {
        if let Some(core) = self.core_api() {
            let _ = core.flush_recent_edits();
        }
    }

// AppBackend::get_writing_stats_by_project
    pub(crate) fn get_writing_stats_by_project(&self, start_date: QString, end_date: QString) -> QString {
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_api() {
            match core.get_writing_stats_by_project_json(&sd, &ed) {
                Ok(val) => val.into(),
                Err(_) => "{}".into(),
            }
        } else {
            "{}".into()
        }
    }

// AppBackend::get_writing_stats_by_project_object
    pub(crate) fn get_writing_stats_by_project_object(&self, start_date: QString, end_date: QString) -> QJsonObject {
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_api() {
            match core.get_writing_stats_by_project_json(&sd, &ed) {
                Ok(val) => qjson_object_from_json(&val),
                Err(_) => QJsonObject::default(),
            }
        } else {
            QJsonObject::default()
        }
    }

// AppBackend::get_writing_stats_by_chapter
    pub(crate) fn get_writing_stats_by_chapter(&self, start_date: QString, end_date: QString) -> QString {
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_api() {
            match core.get_writing_stats_by_chapter_json(&sd, &ed) {
                Ok(val) => val.into(),
                Err(_) => "{}".into(),
            }
        } else {
            "{}".into()
        }
    }

// AppBackend::get_writing_stats_by_chapter_object
    pub(crate) fn get_writing_stats_by_chapter_object(&self, start_date: QString, end_date: QString) -> QJsonObject {
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_api() {
            match core.get_writing_stats_by_chapter_json(&sd, &ed) {
                Ok(val) => qjson_object_from_json(&val),
                Err(_) => QJsonObject::default(),
            }
        } else {
            QJsonObject::default()
        }
    }

// AppBackend::get_writing_stats_by_device
    pub(crate) fn get_writing_stats_by_device(&self, start_date: QString, end_date: QString) -> QString {
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_api() {
            match core.get_writing_stats_by_device_json(&sd, &ed) {
                Ok(val) => val.into(),
                Err(_) => "{}".into(),
            }
        } else {
            "{}".into()
        }
    }

// AppBackend::get_writing_stats_by_device_object
    pub(crate) fn get_writing_stats_by_device_object(&self, start_date: QString, end_date: QString) -> QJsonObject {
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_api() {
            match core.get_writing_stats_by_device_json(&sd, &ed) {
                Ok(val) => qjson_object_from_json(&val),
                Err(_) => QJsonObject::default(),
            }
        } else {
            QJsonObject::default()
        }
    }

// AppBackend::get_writing_speed_curve
    pub(crate) fn get_writing_speed_curve(&self, start_date: QString, end_date: QString, bucket_minutes: u32) -> QString {
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_api() {
            match core.get_writing_speed_curve_json(&sd, &ed, bucket_minutes) {
                Ok(val) => val.into(),
                Err(_) => "{}".into(),
            }
        } else {
            "{}".into()
        }
    }

// AppBackend::get_writing_speed_curve_object
    pub(crate) fn get_writing_speed_curve_object(&self, start_date: QString, end_date: QString, bucket_minutes: u32) -> QJsonObject {
        let sd = start_date.to_string();
        let ed = end_date.to_string();
        if let Some(core) = self.core_api() {
            match core.get_writing_speed_curve_json(&sd, &ed, bucket_minutes) {
                Ok(val) => qjson_object_from_json(&val),
                Err(_) => QJsonObject::default(),
            }
        } else {
            QJsonObject::default()
        }
    }

}
