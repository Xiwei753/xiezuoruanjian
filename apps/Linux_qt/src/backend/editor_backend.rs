// =============================================================================
// editor_backend.rs — 编辑器与写作统计领域 QObject 后端适配层
// =============================================================================
//
// 引用了什么：
// - super::*：引入 AppBackend 核心后端的全部方法与结构体。
// - crate::backend::AppRef：用于安全访问全局 AppBackend 指针以读取/更新正文状态与字数信息。
//
// 干什么的：
// - 实现 EditorBackend 结构体，作为 QML 中 "editorBackend" 对象的桥梁。
// - 负责编辑器核心章节读取（open_chapter & get_chapter_content）、防误删安全保存（save_chapter）、正文清空（clear_chapter_content）以及字数重新计算（calculate_word_count）。
// - 记录并上报界面层触发的高频字符录入事件流，并按时间、项目、章节、设备等维度提供图表所需的统计快照 JSON 对象（get_writing_stats_summary_object 等）。
// - 接收来自界面的自动同步请求（request_auto_sync）与客户端日志上报（log_qml）。
// - 提供动作命令注册与调度执行机制（list_registered_actions & execute_action），作为 Action-Driven UI 智能体的重要旁路底座。
//
// 被什么引用：
// - 被 apps/Linux_qt/src/backend/mod.rs 引用，用于实例化编辑器后端并绑定为 QML 全局上下文属性。
// =============================================================================

use super::*;
use crate::backend::AppRef;
use crate::backend::DomainSnapshot;

#[path = "chapter_operations.rs"]
mod chapter_operations;
#[path = "writing_stats.rs"]
mod writing_stats;

/// 编辑器 QML 后端——桥接 QML 前端与 Core API。
///
/// 所有 QML 可调用方法均通过 `#[allow(non_snake_case)]` 保持 Qt 命名惯例。
/// 必须在 GUI 线程创建和使用——QObject 不可跨线程。
/// 内部通过 `AppRef` 访问全局 `AppBackend`，后者持有 `WriterCoreApi` 实例。
#[allow(non_snake_case)] // Qt QML naming convention (e.g. projectsReloaded)
#[derive(QObject, Default)]
pub struct EditorBackend {
    #[allow(dead_code)]
    base: qt_base_class!(trait QObject),
    #[allow(dead_code)]
    save_status: qt_property!(QString; READ save_status WRITE set_save_status NOTIFY save_status_changed),
    #[allow(dead_code)]
    word_count: qt_property!(i32; READ word_count WRITE set_word_count NOTIFY word_count_changed),
    #[allow(dead_code)]
    error_message: qt_property!(QString; READ error_message NOTIFY error_occurred),
    #[allow(dead_code)]
    selected_item_id: qt_property!(QString; READ selected_item_id NOTIFY selected_item_changed),
    #[allow(dead_code)]
    has_selected_chapter_prop: qt_property!(bool; READ has_selected_chapter_prop NOTIFY selected_item_changed),
    #[allow(dead_code)]
    chapter_path: qt_property!(QString; READ chapter_path NOTIFY chapter_path_changed),
    #[allow(dead_code)]
    setting_font_size: qt_property!(f32; READ setting_font_size WRITE set_setting_font_size NOTIFY settings_changed),
    #[allow(dead_code)]
    setting_line_spacing: qt_property!(f32; READ setting_line_spacing WRITE set_setting_line_spacing NOTIFY settings_changed),
    #[allow(dead_code)]
    setting_auto_save_enabled: qt_property!(bool; READ setting_auto_save_enabled WRITE set_setting_auto_save_enabled NOTIFY settings_changed),
    #[allow(dead_code)]
    setting_auto_save_delay_ms: qt_property!(u32; READ setting_auto_save_delay_ms WRITE set_setting_auto_save_delay_ms NOTIFY settings_changed),
    #[allow(dead_code)]
    setting_auto_indent_enabled: qt_property!(bool; READ setting_auto_indent_enabled WRITE set_setting_auto_indent_enabled NOTIFY settings_changed),
    #[allow(dead_code)]
    setting_smooth_cursor_enabled: qt_property!(bool; READ setting_smooth_cursor_enabled NOTIFY settings_changed),
    #[allow(dead_code)]
    setting_typing_animation_enabled: qt_property!(bool; READ setting_typing_animation_enabled NOTIFY settings_changed),
    #[allow(dead_code)]
    setting_smooth_cursor_duration_ms: qt_property!(u32; READ setting_smooth_cursor_duration_ms NOTIFY settings_changed),
    #[allow(dead_code)]
    setting_typing_animation_duration_ms: qt_property!(u32; READ setting_typing_animation_duration_ms NOTIFY settings_changed),
    #[allow(dead_code)]
    has_workspace: qt_property!(bool; READ has_workspace NOTIFY workspace_state_changed),
    #[allow(dead_code)]
    sync_enabled: qt_property!(bool; READ sync_enabled NOTIFY sync_config_changed),
    #[allow(dead_code)]
    sync_auto_sync: qt_property!(bool; READ sync_auto_sync NOTIFY sync_config_changed),
    #[allow(dead_code)]
    save_status_changed: qt_signal!(),
    #[allow(dead_code)]
    word_count_changed: qt_signal!(),
    #[allow(dead_code)]
    error_occurred: qt_signal!(),
    #[allow(dead_code)]
    selected_item_changed: qt_signal!(),
    #[allow(dead_code)]
    chapter_path_changed: qt_signal!(),
    #[allow(dead_code)]
    clear_editor: qt_signal!(),
    #[allow(dead_code)]
    settings_changed: qt_signal!(),
    #[allow(dead_code)]
    workspace_state_changed: qt_signal!(),
    #[allow(dead_code)]
    sync_config_changed: qt_signal!(),
    #[allow(dead_code)]
    calculate_word_count: qt_method!(fn(&mut self, text: QString)),
    #[allow(dead_code)]
    open_chapter_json: qt_method!(
        fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) -> QString
    ),
    #[allow(dead_code)]
    open_chapter: qt_method!(
        fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) -> QJsonObject
    ),
    #[allow(dead_code)]
    get_chapter_content: qt_method!(
        fn(&self, project_id: QString, volume_id: QString, chapter_id: QString) -> QString
    ),
    #[allow(dead_code)]
    save_chapter: qt_method!(
        fn(
            &mut self,
            project_id: QString,
            volume_id: QString,
            chapter_id: QString,
            content: QString,
            allow_empty_overwrite: bool,
        ) -> QJsonObject
    ),
    #[allow(dead_code)]
    clear_chapter_content: qt_method!(
        fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) -> QJsonObject
    ),
    #[allow(dead_code)]
    report_writing_event: qt_method!(
        fn(
            &mut self,
            project_id: QString,
            volume_id: QString,
            chapter_id: QString,
            source: QString,
            inserted_chars: u32,
            deleted_chars: u32,
            pasted_chars: u32,
        )
    ),
    #[allow(dead_code)]
    process_writing_event_from_text: qt_method!(
        fn(
            &mut self,
            project_id: QString,
            volume_id: QString,
            chapter_id: QString,
            old_text: QString,
            new_text: QString,
        )
    ),
    #[allow(dead_code)]
    get_writing_stats_summary:
        qt_method!(fn(&self, start_date: QString, end_date: QString) -> QString),
    #[allow(dead_code)]
    get_writing_stats_summary_object:
        qt_method!(fn(&self, start_date: QString, end_date: QString) -> QJsonObject),
    #[allow(dead_code)]
    get_writing_stats_by_project:
        qt_method!(fn(&self, start_date: QString, end_date: QString) -> QString),
    #[allow(dead_code)]
    get_writing_stats_by_project_object:
        qt_method!(fn(&self, start_date: QString, end_date: QString) -> QJsonObject),
    #[allow(dead_code)]
    get_writing_stats_by_chapter:
        qt_method!(fn(&self, start_date: QString, end_date: QString) -> QString),
    #[allow(dead_code)]
    get_writing_stats_by_chapter_object:
        qt_method!(fn(&self, start_date: QString, end_date: QString) -> QJsonObject),
    #[allow(dead_code)]
    get_writing_stats_by_device:
        qt_method!(fn(&self, start_date: QString, end_date: QString) -> QString),
    #[allow(dead_code)]
    get_writing_stats_by_device_object:
        qt_method!(fn(&self, start_date: QString, end_date: QString) -> QJsonObject),
    #[allow(dead_code)]
    get_writing_speed_curve: qt_method!(
        fn(&self, start_date: QString, end_date: QString, bucket_minutes: u32) -> QString
    ),
    #[allow(dead_code)]
    get_writing_speed_curve_object: qt_method!(
        fn(&self, start_date: QString, end_date: QString, bucket_minutes: u32) -> QJsonObject
    ),
    #[allow(dead_code)]
    flush_writing_stats: qt_method!(fn(&self)),
    #[allow(dead_code)]
    flush_recent_edits: qt_method!(fn(&self)),
    #[allow(dead_code)]
    has_selected_chapter: qt_method!(fn(&self) -> bool),
    #[allow(dead_code)]
    selected_chapter_exists: qt_method!(fn(&self) -> bool),
    #[allow(dead_code)]
    clear_editor_state: qt_method!(fn(&mut self)),
    #[allow(dead_code)]
    request_auto_sync: qt_method!(fn(&mut self, reason: QString)),
    #[allow(dead_code)]
    log_qml:
        qt_method!(fn(&self, level: QString, module: QString, event: QString, message: QString)),
    #[allow(dead_code)]
    list_registered_actions: qt_method!(fn(&mut self) -> QString),
    #[allow(dead_code)]
    execute_action: qt_method!(
        fn(&mut self, action_id: QString, args_json: QString, context_json: QString) -> QString
    ),
    app: AppRef,
}

impl EditorBackend {
    pub fn new(app: AppRef) -> Self {
        Self {
            app,
            ..Default::default()
        }
    }
    fn with_app<R>(
        &self,
        f: impl FnOnce(&AppBackend) -> R,
    ) -> Result<R, crate::backend::AppBorrowError> {
        self.app.with_app(f)
    }
    fn with_app_mut<R>(
        &self,
        f: impl FnOnce(&mut AppBackend) -> R,
    ) -> Result<R, crate::backend::AppBorrowError> {
        self.app.with_app_mut(f)
    }
    fn snap(&self) -> std::cell::Ref<'_, DomainSnapshot> {
        self.app.snapshot().borrow()
    }
    fn save_status(&self) -> QString {
        self.snap().save_status.clone().into()
    }
    fn set_save_status(&mut self, val: QString) {
        if self.with_app_mut(|app| app.set_save_status(val)).is_ok() {
            self.save_status_changed();
        }
    }
    fn word_count(&self) -> i32 {
        self.snap().word_count
    }
    fn set_word_count(&mut self, val: i32) {
        if self.with_app_mut(|app| app.set_word_count(val)).is_ok() {
            self.word_count_changed();
        }
    }
    fn error_message(&self) -> QString {
        self.snap().error_message.clone().into()
    }
    fn selected_item_id(&self) -> QString {
        self.snap().selected_item_id.clone().into()
    }
    fn has_selected_chapter_prop(&self) -> bool {
        self.snap().has_selected_chapter_prop
    }
    fn chapter_path(&self) -> QString {
        self.snap().chapter_path.clone().into()
    }
    fn setting_font_size(&self) -> f32 {
        self.snap().setting_font_size
    }
    fn set_setting_font_size(&mut self, val: f32) {
        if self
            .with_app_mut(|app| app.set_setting_font_size(val))
            .is_ok()
        {
            self.settings_changed();
        }
    }
    fn setting_line_spacing(&self) -> f32 {
        self.snap().setting_line_spacing
    }
    fn set_setting_line_spacing(&mut self, val: f32) {
        if self
            .with_app_mut(|app| app.set_setting_line_spacing(val))
            .is_ok()
        {
            self.settings_changed();
        }
    }
    fn setting_auto_save_enabled(&self) -> bool {
        self.snap().setting_auto_save_enabled
    }
    fn set_setting_auto_save_enabled(&mut self, val: bool) {
        if self
            .with_app_mut(|app| app.set_setting_auto_save_enabled(val))
            .is_ok()
        {
            self.settings_changed();
        }
    }
    fn setting_auto_save_delay_ms(&self) -> u32 {
        self.snap().setting_auto_save_delay_ms
    }
    fn set_setting_auto_save_delay_ms(&mut self, val: u32) {
        if self
            .with_app_mut(|app| app.set_setting_auto_save_delay_ms(val))
            .is_ok()
        {
            self.settings_changed();
        }
    }
    fn setting_auto_indent_enabled(&self) -> bool {
        self.snap().setting_auto_indent_enabled
    }
    fn set_setting_auto_indent_enabled(&mut self, val: bool) {
        if self
            .with_app_mut(|app| app.set_setting_auto_indent_enabled(val))
            .is_ok()
        {
            self.settings_changed();
        }
    }
    fn setting_smooth_cursor_enabled(&self) -> bool {
        self.snap().setting_smooth_cursor_enabled
    }
    fn setting_typing_animation_enabled(&self) -> bool {
        self.snap().setting_typing_animation_enabled
    }
    fn setting_smooth_cursor_duration_ms(&self) -> u32 {
        self.snap().setting_smooth_cursor_duration_ms
    }
    fn setting_typing_animation_duration_ms(&self) -> u32 {
        self.snap().setting_typing_animation_duration_ms
    }
    fn has_workspace(&self) -> bool {
        self.snap().has_workspace
    }
    fn sync_enabled(&self) -> bool {
        self.snap().sync_enabled
    }
    fn sync_auto_sync(&self) -> bool {
        self.snap().sync_auto_sync
    }
    fn calculate_word_count(&mut self, text: QString) {
        if self
            .with_app_mut(|app| app.calculate_word_count(text))
            .is_ok()
        {
            self.word_count_changed();
        }
    }
    fn open_chapter_json(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
    ) -> QString {
        let result =
            self.with_app_mut(|app| app.open_chapter_json(project_id, volume_id, chapter_id));
        if result.is_ok() {
            self.selected_item_changed();
            self.chapter_path_changed();
        }
        result.unwrap_or_else(|_| {
            QString::from(crate::backend::json_utils::borrow_conflict_error_json())
        })
    }
    fn open_chapter(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
    ) -> QJsonObject {
        let result = self.with_app_mut(|app| app.open_chapter(project_id, volume_id, chapter_id));
        if result.is_ok() {
            self.selected_item_changed();
            self.chapter_path_changed();
        }
        result.unwrap_or_else(|_| {
            qjson_object_from_json(&crate::backend::json_utils::borrow_conflict_error_json())
        })
    }
    fn get_chapter_content(
        &self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
    ) -> QString {
        self.with_app(|app| app.get_chapter_content(project_id, volume_id, chapter_id))
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn save_chapter(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        content: QString,
        allow_empty_overwrite: bool,
    ) -> QJsonObject {
        let result = self.with_app_mut(|app| {
            app.save_chapter(
                project_id,
                volume_id,
                chapter_id,
                content,
                allow_empty_overwrite,
            )
        });
        if result.is_ok() {
            self.save_status_changed();
        }
        result.unwrap_or_else(|_| {
            qjson_object_from_json(&crate::backend::json_utils::borrow_conflict_error_json())
        })
    }
    fn clear_chapter_content(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
    ) -> QJsonObject {
        self.with_app_mut(|app| app.clear_chapter_content(project_id, volume_id, chapter_id))
            .unwrap_or_else(|_| {
                qjson_object_from_json(&crate::backend::json_utils::borrow_conflict_error_json())
            })
    }
    fn report_writing_event(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        source: QString,
        inserted_chars: u32,
        deleted_chars: u32,
        pasted_chars: u32,
    ) {
        if self
            .with_app_mut(|app| {
                app.report_writing_event(
                    project_id,
                    volume_id,
                    chapter_id,
                    source,
                    inserted_chars,
                    deleted_chars,
                    pasted_chars,
                )
            })
            .is_err()
        {
            crate::backend::app_backend::debug_error_static(
                "editor_backend",
                "BORROW_CONFLICT",
                "report_writing_event skipped due to borrow conflict",
            );
        }
    }
    fn process_writing_event_from_text(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        old_text: QString,
        new_text: QString,
    ) {
        if self
            .with_app_mut(|app| {
                app.process_writing_event_from_text(
                    project_id, volume_id, chapter_id, old_text, new_text,
                )
            })
            .is_err()
        {
            crate::backend::app_backend::debug_error_static(
                "editor_backend",
                "BORROW_CONFLICT",
                "process_writing_event_from_text skipped due to borrow conflict",
            );
        }
    }
    fn get_writing_stats_summary(&self, start_date: QString, end_date: QString) -> QString {
        self.with_app(|app| app.get_writing_stats_summary(start_date, end_date))
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn get_writing_stats_summary_object(
        &self,
        start_date: QString,
        end_date: QString,
    ) -> QJsonObject {
        self.with_app(|app| app.get_writing_stats_summary_object(start_date, end_date))
            .unwrap_or_else(|_| {
                qjson_object_from_json(&crate::backend::json_utils::borrow_conflict_error_json())
            })
    }
    fn get_writing_stats_by_project(&self, start_date: QString, end_date: QString) -> QString {
        self.with_app(|app| app.get_writing_stats_by_project(start_date, end_date))
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn get_writing_stats_by_project_object(
        &self,
        start_date: QString,
        end_date: QString,
    ) -> QJsonObject {
        self.with_app(|app| app.get_writing_stats_by_project_object(start_date, end_date))
            .unwrap_or_else(|_| {
                qjson_object_from_json(&crate::backend::json_utils::borrow_conflict_error_json())
            })
    }
    fn get_writing_stats_by_chapter(&self, start_date: QString, end_date: QString) -> QString {
        self.with_app(|app| app.get_writing_stats_by_chapter(start_date, end_date))
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn get_writing_stats_by_chapter_object(
        &self,
        start_date: QString,
        end_date: QString,
    ) -> QJsonObject {
        self.with_app(|app| app.get_writing_stats_by_chapter_object(start_date, end_date))
            .unwrap_or_else(|_| {
                qjson_object_from_json(&crate::backend::json_utils::borrow_conflict_error_json())
            })
    }
    fn get_writing_stats_by_device(&self, start_date: QString, end_date: QString) -> QString {
        self.with_app(|app| app.get_writing_stats_by_device(start_date, end_date))
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn get_writing_stats_by_device_object(
        &self,
        start_date: QString,
        end_date: QString,
    ) -> QJsonObject {
        self.with_app(|app| app.get_writing_stats_by_device_object(start_date, end_date))
            .unwrap_or_else(|_| {
                qjson_object_from_json(&crate::backend::json_utils::borrow_conflict_error_json())
            })
    }
    fn get_writing_speed_curve(
        &self,
        start_date: QString,
        end_date: QString,
        bucket_minutes: u32,
    ) -> QString {
        self.with_app(|app| app.get_writing_speed_curve(start_date, end_date, bucket_minutes))
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn get_writing_speed_curve_object(
        &self,
        start_date: QString,
        end_date: QString,
        bucket_minutes: u32,
    ) -> QJsonObject {
        self.with_app(|app| {
            app.get_writing_speed_curve_object(start_date, end_date, bucket_minutes)
        })
        .unwrap_or_else(|_| {
            qjson_object_from_json(&crate::backend::json_utils::borrow_conflict_error_json())
        })
    }
    fn flush_writing_stats(&self) {
        if self.with_app(|app| app.flush_writing_stats()).is_err() {
            crate::backend::app_backend::debug_error_static(
                "editor_backend",
                "BORROW_CONFLICT",
                "flush_writing_stats skipped due to borrow conflict",
            );
        }
    }
    fn flush_recent_edits(&self) {
        if self.with_app(|app| app.flush_recent_edits()).is_err() {
            crate::backend::app_backend::debug_error_static(
                "editor_backend",
                "BORROW_CONFLICT",
                "flush_recent_edits skipped due to borrow conflict",
            );
        }
    }
    fn has_selected_chapter(&self) -> bool {
        self.snap().has_selected_chapter
    }
    fn selected_chapter_exists(&self) -> bool {
        self.snap().selected_chapter_exists
    }
    fn clear_editor_state(&mut self) {
        if self.with_app_mut(|app| app.clear_editor_state()).is_ok() {
            self.clear_editor();
        }
    }
    fn request_auto_sync(&mut self, reason: QString) {
        if self
            .with_app_mut(|app| app.request_auto_sync(reason))
            .is_err()
        {
            crate::backend::app_backend::debug_error_static(
                "editor_backend",
                "BORROW_CONFLICT",
                "request_auto_sync skipped due to borrow conflict",
            );
        }
    }
    fn log_qml(&self, level: QString, module: QString, event: QString, message: QString) {
        if self
            .with_app(|app| app.log_qml(level, module, event, message))
            .is_err()
        {
            crate::backend::app_backend::debug_error_static(
                "editor_backend",
                "BORROW_CONFLICT",
                "log_qml skipped due to borrow conflict",
            );
        }
    }
    fn list_registered_actions(&mut self) -> QString {
        self.with_app_mut(|app| app.list_registered_actions())
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn execute_action(
        &mut self,
        action_id: QString,
        args_json: QString,
        context_json: QString,
    ) -> QString {
        self.with_app_mut(|app| app.execute_action(action_id, args_json, context_json))
            .unwrap_or_else(|_| {
                QString::from(crate::backend::json_utils::borrow_conflict_error_json())
            })
    }
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
                }
                Err(_) => "[]".into(),
            }
        } else {
            "[]".into()
        }
    }

    // AppBackend::execute_action
    pub(crate) fn execute_action(
        &mut self,
        action_id: QString,
        args_json: QString,
        context_json: QString,
    ) -> QString {
        if let Some(api) = self.core_api() {
            match api.execute_action_ext(
                &action_id.to_string(),
                &args_json.to_string(),
                &context_json.to_string(),
            ) {
                Ok(result) => {
                    let json = serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());
                    json.into()
                }
                Err(e) => writer_core::api::ResultEnvelope::<()>::error(
                    writer_core::api::WriterError::Io(e.to_string()),
                )
                .to_json_string()
                .into(),
            }
        } else {
            writer_core::api::ResultEnvelope::<()>::error(
                writer_core::api::WriterError::Other("core api not available".to_string()),
            )
            .to_json_string()
            .into()
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
}
