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
// - 客户端日志上报（log_qml）。
// - 提供动作命令注册与调度执行机制（list_registered_actions & execute_action），作为 Action-Driven UI 智能体的重要旁路底座。
//
// 被什么引用：
// - 被 apps/Linux_qt/src/backend/mod.rs 引用，用于实例化编辑器后端并绑定为 QML 全局上下文属性。
// =============================================================================

use super::*;
use crate::backend::chapter_save_queue;
use crate::backend::AppRef;
use crate::backend::DomainSnapshot;
use rfd::FileDialog;

/// Issue #1059：QML 轮询/等待后台保存回执时携带的“当前编辑器文档”身份。
///
/// 只有请求身份 + 保存会话仍匹配当前文档时才允许应用 current_save_status
/// 等副作用；旧会话的回执只返回数据，不改写新文档状态。
struct ChapterSaveCurrentDocument {
    project_id: String,
    volume_id: String,
    chapter_id: String,
    session_id: u64,
}

fn correction_dictionary_resources_value(resources: Vec<(String, bool)>) -> serde_json::Value {
    serde_json::Value::Array(
        resources
            .into_iter()
            .map(|(name, ready)| serde_json::json!({ "name": name, "ready": ready }))
            .collect(),
    )
}

#[path = "chapter_operations.rs"]
mod chapter_operations;
#[path = "writing_stats.rs"]
pub(crate) mod writing_stats;

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
    save_status_changed: qt_signal!(),
    #[allow(dead_code)]
    word_count_changed: qt_signal!(),
    #[allow(dead_code)]
    error_occurred: qt_signal!(),
    #[allow(dead_code)]
    clear_editor: qt_signal!(),
    /// Issue #935：统计面板快照异步查询完成。`request_id` 用于区分迟到的结果。
    #[allow(dead_code)]
    writing_stats_dashboard_ready: qt_signal!(request_id: u32, snapshot: QJsonObject),
    #[allow(dead_code)]
    writing_stats_dashboard_failed: qt_signal!(request_id: u32, message: QString),
    /// Issue #946：统计页 Explorer 快照异步查询完成（生涯 + 四时段卡 + 选中时段详情）。
    #[allow(dead_code)]
    writing_stats_explorer_ready: qt_signal!(request_id: u32, snapshot: QJsonObject),
    #[allow(dead_code)]
    writing_stats_explorer_failed: qt_signal!(request_id: u32, message: QString),
    /// Issue #960：统计页每日码字日历异步查询完成（指定公历年的逐日快照）。
    #[allow(dead_code)]
    writing_stats_calendar_ready: qt_signal!(request_id: u32, snapshot: QJsonObject),
    #[allow(dead_code)]
    writing_stats_calendar_failed: qt_signal!(request_id: u32, message: QString),
    /// Issue #935：实时速度 + 今日汇总异步查询完成。
    #[allow(dead_code)]
    writing_status_ready: qt_signal!(request_id: u32, status: QJsonObject),
    #[allow(dead_code)]
    writing_status_failed: qt_signal!(request_id: u32, message: QString),
    /// Issue #943/#954：校对后台任务完成；结果仅含建议和 byte/Qt 坐标标注。
    /// #954 起第三个参数是整章快照 JSON（textRevision/suggestions/checkedRanges）。
    #[allow(dead_code)]
    correction_check_ready: qt_signal!(request_id: u32, text_revision: u64, snapshot_json: QString),
    #[allow(dead_code)]
    correction_check_failed: qt_signal!(request_id: u32, text_revision: u64, message: QString),
    /// Issue #1032：异步 patch 映射完成，携带章节 + revision 让 QML 只接受
    /// 当前章节/版本的结果。
    #[allow(dead_code)]
    correction_edits_applied: qt_signal!(
        request_id: u32,
        project_id: QString,
        chapter_id: QString,
        text_revision: u64,
        success: bool,
        message: QString
    ),
    #[allow(dead_code)]
    calculate_word_count: qt_method!(fn(&mut self, text: QString)),
    /// Issue #920：正文段落格式化规则归 Core，Qt 只触发此操作。
    #[allow(dead_code)]
    format_plain_text: qt_method!(fn(&self, text: QString) -> QString),
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
    save_chapter_versioned: qt_method!(
        fn(
            &mut self,
            project_id: QString,
            volume_id: QString,
            chapter_id: QString,
            content: QString,
            allow_empty_overwrite: bool,
            expected_generation: QString,
            expected_revision: i64,
            expected_content_sha256: QString,
            base_content: QString,
        ) -> QJsonObject
    ),
    /// Issue #1059：把不可变保存快照交给后台有序提交队列，立即返回 request id。
    ///
    /// `snapshot_meta_json` 只含章节身份 / base 版本 / allow_empty / 快照
    /// revision+sha256；正文与 base 正文单独传。QML 不再同步调用 Core 写盘。
    /// 返回 0 表示快照元数据不可解析（调用方按明确的入队失败处理）。
    #[allow(dead_code)]
    enqueue_chapter_save_versioned: qt_method!(
        fn(&mut self, snapshot_meta_json: QString, content: QString, base_content: QString) -> u32
    ),
    /// Issue #1059：轮询后台保存回执（pending / ok / error）。
    #[allow(dead_code)]
    poll_chapter_save_result: qt_method!(
        fn(
            &mut self,
            request_id: u32,
            current_project_id: QString,
            current_volume_id: QString,
            current_chapter_id: QString,
            current_session_id: u64,
        ) -> QJsonObject
    ),
    /// Issue #1059：切章 / 同步前等待指定保存请求落盘（仅边界使用）。
    #[allow(dead_code)]
    wait_chapter_save_result: qt_method!(
        fn(
            &mut self,
            request_id: u32,
            timeout_ms: u32,
            current_project_id: QString,
            current_volume_id: QString,
            current_chapter_id: QString,
            current_session_id: u64,
        ) -> QJsonObject
    ),
    #[allow(dead_code)]
    read_chapter_version: qt_method!(
        fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) -> QJsonObject
    ),
    #[allow(dead_code)]
    clear_chapter_content: qt_method!(
        fn(&mut self, project_id: QString, volume_id: QString, chapter_id: QString) -> QJsonObject
    ),
    #[allow(dead_code)]
    record_editor_change_stats: qt_method!(
        fn(
            &mut self,
            project_id: QString,
            volume_id: QString,
            chapter_id: QString,
            cause: QString,
            inserted_chars: u32,
            deleted_chars: u32,
            changes_json: QString,
            base_revision: u64,
            new_revision: u64,
            old_selection_anchor: u64,
            old_selection_head: u64,
            new_selection_anchor: u64,
            new_selection_head: u64,
            baseline_text: QString,
            before_text_sha256: QString,
            after_text_sha256: QString,
            editor_session_id: u64,
            base_generation: QString,
            base_chapter_revision: u64,
            base_chapter_sha256: QString,
        ) -> QString
    ),
    /// Issue #1062 1.3：编辑器正文变化交给章节 actor 更新可恢复草稿。
    #[allow(dead_code)]
    update_chapter_draft_text: qt_method!(
        fn(
            &mut self,
            project_id: QString,
            volume_id: QString,
            chapter_id: QString,
            session_id: u64,
            text: QString,
            revision: u64,
            sha256: QString,
            base_generation: QString,
            base_revision: u64,
            base_sha256: QString,
            force: bool,
        )
    ),
    /// Issue #1062 1.3：项目进入时重放未完成草稿，返回待处理数量。
    #[allow(dead_code)]
    restore_project_chapter_drafts: qt_method!(fn(&mut self, project_id: QString) -> u32),
    /// Issue #1062 1.3：草稿恢复状态（QML 轮询）。
    #[allow(dead_code)]
    chapter_draft_restore_status: qt_method!(fn(&mut self, project_id: QString) -> QJsonObject),
    /// Issue #1062 1.3：当前章节草稿快照（只读诊断/恢复提示）。
    #[allow(dead_code)]
    chapter_draft_snapshot:
        qt_method!(fn(&mut self, project_id: QString, chapter_id: QString) -> QJsonObject),
    #[allow(dead_code)]
    get_writing_stats_summary:
        qt_method!(fn(&self, start_date: QString, end_date: QString) -> QString),
    #[allow(dead_code)]
    get_writing_stats_summary_object:
        qt_method!(fn(&self, start_date: QString, end_date: QString) -> QJsonObject),
    /// 手写证明：按章节回放。请求为 Core `ProofReplayRequest` JSON，返回同帧 JSON。
    #[allow(dead_code)]
    replay_chapter_proof_object: qt_method!(fn(&self, request_json: QString) -> QJsonObject),
    /// 手写证明：导出离线证明包 JSON（含历史正文摘要与逐次事件）。
    #[allow(dead_code)]
    export_chapter_proof_object:
        qt_method!(fn(&self, chapter_id: QString, device_id: QString) -> QJsonObject),
    /// 「今天」的写作汇总。今天是哪一天由 Core 的本地日历口径决定。
    #[allow(dead_code)]
    get_today_writing_stats_summary_object: qt_method!(fn(&self) -> QJsonObject),
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
    /// 实时写作速度（最近 N 秒）。状态栏左段「N 字/时」走这里，不读速度曲线。
    #[allow(dead_code)]
    get_current_writing_speed: qt_method!(fn(&self, window_seconds: u32) -> QJsonObject),
    /// Issue #935：异步请求统计面板快照。
    ///
    /// 立即返回；worker 生成结果后经 `writing_stats_dashboard_ready` /
    /// `writing_stats_dashboard_failed` 携 `request_id` 发回。
    #[allow(dead_code)]
    request_writing_stats_dashboard: qt_method!(fn(&mut self, request_id: u32)),
    /// Issue #946：异步请求统计页 Explorer 快照。
    ///
    /// `preset` 为 `last7Days` / `last30Days` / `last6Months` / `lifetime` /
    /// `custom`；自选区间必须带两个 `YYYY-MM-DD` 日期，其余预设两端传空字符串。
    /// 立即返回；结果经 `writing_stats_explorer_ready` /
    /// `writing_stats_explorer_failed` 携 `request_id` 发回。
    #[allow(dead_code)]
    request_writing_stats_explorer: qt_method!(
        fn(&mut self, request_id: u32, preset: QString, start_date: QString, end_date: QString)
    ),
    /// Issue #960：异步请求每日码字日历（指定公历年）。
    ///
    /// 立即返回；结果经 `writing_stats_calendar_ready` /
    /// `writing_stats_calendar_failed` 携 `request_id` 发回。
    #[allow(dead_code)]
    request_writing_stats_calendar: qt_method!(fn(&mut self, request_id: u32, year: i32)),
    /// Issue #935：异步请求「实时速度 + 今日汇总」。
    #[allow(dead_code)]
    request_writing_status: qt_method!(fn(&mut self, request_id: u32, window_seconds: u32)),
    /// Issue #985：存储用户提供的本地 ONNX 模型资源目录；Core 负责懒加载与错误返回。
    #[allow(dead_code)]
    set_correction_model_path: qt_method!(fn(&self, path: QString) -> QJsonObject),
    /// Issue #985：选择包含 model.onnx、vocab.txt 与 tokenizer.json 的模型资源目录。
    #[allow(dead_code)]
    pick_correction_model_path: qt_method!(fn(&self) -> QJsonObject),
    /// Issue #943：选择并导入固定命名的共享校对词库文件。
    #[allow(dead_code)]
    import_correction_dictionary: qt_method!(fn(&self) -> QJsonObject),
    /// Issue #943：返回 Core app_data_root 下共享词库文件的就绪状态。
    #[allow(dead_code)]
    correction_dictionary_status: qt_method!(fn(&self) -> QJsonObject),
    /// Issue #943：对当前章节快照做后台检查，不在 GUI 线程扫描正文或加载模型。
    #[allow(dead_code)]
    request_correction_chapter: qt_method!(
        fn(
            &mut self,
            request_id: u32,
            project_id: QString,
            chapter_id: QString,
            session_id: u64,
            text_revision: u64,
            text: QString,
        )
    ),
    /// Issue #943：对正文快照中的 UTF-8 byte 半开范围做后台增量检查。
    #[allow(dead_code)]
    request_correction_segment: qt_method!(
        fn(
            &mut self,
            request_id: u32,
            project_id: QString,
            chapter_id: QString,
            session_id: u64,
            text_revision: u64,
            text: QString,
            start_byte: u32,
            end_byte: u32,
        )
    ),
    /// Issue #954：读取 Core 已缓存且仍有效的校对结果，不触发模型扫描。
    #[allow(dead_code)]
    request_correction_snapshot: qt_method!(
        fn(
            &mut self,
            request_id: u32,
            project_id: QString,
            chapter_id: QString,
            session_id: u64,
            text_revision: u64,
            text: QString,
        )
    ),
    /// Issue #954：按编辑事务的真实 patch 范围增量更新并只扫描脏段。
    #[allow(dead_code)]
    request_correction_edits: qt_method!(
        fn(
            &mut self,
            request_id: u32,
            project_id: QString,
            chapter_id: QString,
            session_id: u64,
            text_revision: u64,
            base_revision: u64,
            edits_json: QString,
            text: QString,
        )
    ),
    /// Issue #1032：编辑事务的第一阶段改为后台 FIFO 任务；QML 通过
    /// `correction_edits_applied` 接收带章节/revision 的完成结果。
    #[allow(dead_code)]
    apply_correction_edits: qt_method!(
        fn(
            &mut self,
            request_id: u32,
            project_id: QString,
            chapter_id: QString,
            session_id: u64,
            text_revision: u64,
            base_revision: u64,
            edits_json: QString,
        )
    ),
    /// Issue #954：章节关闭/切换后释放共享校对队列中的章节记录。
    #[allow(dead_code)]
    release_correction_session: qt_method!(fn(&self, project_id: QString, chapter_id: QString)),
    /// Issue #954：编辑事务的第二阶段，只扫描已记录的脏段。
    #[allow(dead_code)]
    request_correction_pending: qt_method!(
        fn(
            &mut self,
            request_id: u32,
            project_id: QString,
            chapter_id: QString,
            session_id: u64,
            text_revision: u64,
            text: QString,
        )
    ),
    #[allow(dead_code)]
    ignore_correction: qt_method!(
        fn(
            &self,
            project_id: QString,
            chapter_id: QString,
            rule_id: QString,
            original: QString,
            scope: QString,
            text_revision: u64,
            start_byte: u32,
            end_byte: u32,
        ) -> QJsonObject
    ),
    #[allow(dead_code)]
    add_correction_dictionary_entry:
        qt_method!(fn(&self, project_id: QString, entry: QString) -> QJsonObject),
    #[allow(dead_code)]
    add_correction_dictionary_entry_with_kind:
        qt_method!(fn(&self, project_id: QString, entry: QString, kind: QString) -> QJsonObject),
    /// Issue #964：登记人物/地点的别名或明确错误写法。
    #[allow(dead_code)]
    add_correction_entity_name: qt_method!(
        fn(
            &self,
            project_id: QString,
            canonical: QString,
            name: QString,
            kind: QString,
        ) -> QJsonObject
    ),
    /// Issue #964：列出当前作品已登记的人物/地点名称对应关系。
    #[allow(dead_code)]
    get_correction_entity_names: qt_method!(fn(&self, project_id: QString) -> QJsonObject),
    /// Issue #964：删除一条人物/地点名称对应关系。
    #[allow(dead_code)]
    remove_correction_entity_name: qt_method!(
        fn(&self, project_id: QString, canonical: QString, name: QString) -> QJsonObject
    ),
    #[allow(dead_code)]
    flush_recent_edits: qt_method!(fn(&self)),
    /// Issue #981：前台写作计时心跳 / 暂停 / 恢复 / 结束。
    #[allow(dead_code)]
    writing_session_heartbeat: qt_method!(fn(&self)),
    #[allow(dead_code)]
    pause_writing_session: qt_method!(fn(&self, reason: QString)),
    #[allow(dead_code)]
    resume_writing_session: qt_method!(fn(&self)),
    #[allow(dead_code)]
    close_writing_session: qt_method!(fn(&self, reason: QString)),
    /// Issue #981：关闭会话并等统计 writer 落盘，供正常退出路径使用。
    #[allow(dead_code)]
    close_writing_session_and_wait: qt_method!(fn(&self, reason: QString) -> QString),
    #[allow(dead_code)]
    has_selected_chapter: qt_method!(fn(&self) -> bool),
    #[allow(dead_code)]
    selected_chapter_exists: qt_method!(fn(&self) -> bool),
    #[allow(dead_code)]
    clear_editor_state: qt_method!(fn(&mut self)),
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
    #[allow(clippy::too_many_arguments)]
    fn save_chapter_versioned(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        content: QString,
        allow_empty_overwrite: bool,
        expected_generation: QString,
        expected_revision: i64,
        expected_content_sha256: QString,
        base_content: QString,
    ) -> QJsonObject {
        let result = self.with_app_mut(|app| {
            app.save_chapter_versioned(
                project_id,
                volume_id,
                chapter_id,
                content,
                allow_empty_overwrite,
                expected_generation,
                expected_revision,
                expected_content_sha256,
                base_content,
            )
        });
        if result.is_ok() {
            self.save_status_changed();
        }
        result.unwrap_or_else(|_| {
            qjson_object_from_json(&crate::backend::json_utils::borrow_conflict_error_json())
        })
    }
    /// Issue #1059：把一份不可变保存快照交给后台有序提交队列，立刻返回 request id。
    ///
    /// QML 只构造快照（章节身份、正文、base 版本/base 正文、allow_empty、
    /// 编辑器快照 revision/sha256）；写盘、base 链推进、冲突材料落盘都在
    /// 单一 worker 上串行完成。这里不读取也不改写 QML 的 committedChapter*。
    fn enqueue_chapter_save_versioned(
        &mut self,
        snapshot_meta_json: QString,
        content: QString,
        base_content: QString,
    ) -> u32 {
        let request_id = chapter_save_queue::next_request_id();
        let meta = match chapter_save_queue::ChapterSaveSnapshotMeta::parse(
            &snapshot_meta_json.to_string(),
        ) {
            Ok(meta) => meta,
            Err(error) => {
                crate::backend::app_backend::debug_error_static(
                    "chapter_save_queue",
                    "enqueue_snapshot_invalid",
                    &error,
                );
                return 0;
            }
        };
        let identity = meta.identity(request_id);
        let content: std::sync::Arc<str> = std::sync::Arc::from(content.to_string());
        let expected = meta.expected_version();
        let base_content = base_content.to_string();
        let base_content = (!base_content.is_empty()).then_some(base_content);

        let Some((data_root, projects_root, layout)) = self
            .with_app(|app| app.annotation_task_context())
            .ok()
            .flatten()
        else {
            chapter_save_queue::fail_before_enqueue(
                identity,
                content,
                chapter_save_queue::ChapterSaveError {
                    code: "CORE_ERROR".to_string(),
                    message_key: "error.core_error".to_string(),
                    message_args: serde_json::json!({}),
                    raw_error: "Core not initialized".to_string(),
                    conflict: false,
                },
            );
            return request_id;
        };
        let request = chapter_save_queue::ChapterSaveRequest {
            identity,
            content,
            allow_empty_overwrite: meta.allow_empty_overwrite,
            expected,
            base_content,
            context: chapter_save_queue::ChapterSaveContext::new(
                &data_root,
                &projects_root,
                &layout,
            ),
        };
        if let Err(error) = chapter_save_queue::enqueue(request) {
            chapter_save_queue::complete_error(
                request_id,
                chapter_save_queue::ChapterSaveError {
                    code: "SAVE_QUEUE_UNAVAILABLE".to_string(),
                    message_key: "error.core_error".to_string(),
                    message_args: serde_json::json!({}),
                    raw_error: error,
                    conflict: false,
                },
                false,
                String::new(),
            );
        }
        request_id
    }

    /// Issue #1059：轮询后台保存回执。终态只应用一次 current_save_status /
    /// 搜索索引；重复轮询只返回同一份 JSON，不重复产生副作用。
    fn poll_chapter_save_result(
        &mut self,
        request_id: u32,
        current_project_id: QString,
        current_volume_id: QString,
        current_chapter_id: QString,
        current_session_id: u64,
    ) -> QJsonObject {
        let current = ChapterSaveCurrentDocument {
            project_id: current_project_id.to_string(),
            volume_id: current_volume_id.to_string(),
            chapter_id: current_chapter_id.to_string(),
            session_id: current_session_id,
        };
        match chapter_save_queue::poll(request_id) {
            Some(snapshot) => self.chapter_save_result_object(snapshot, &current, false),
            None => serde_to_qjson_object(serde_json::json!({
                "status": "unknown",
                "success": false,
                "terminal": true,
                "requestId": request_id,
                "timedOut": false,
                "changedEntities": []
            })),
        }
    }

    /// Issue #1059：切章 / 同步 / 跨卷移动等需要落盘保证的边界使用。
    ///
    /// 只等待 worker 写盘（I/O 不在 GUI 线程执行），超时返回 pending + timedOut，
    /// 调用方必须阻止切章/同步，不能把“还没写完”当成完成。
    fn wait_chapter_save_result(
        &mut self,
        request_id: u32,
        timeout_ms: u32,
        current_project_id: QString,
        current_volume_id: QString,
        current_chapter_id: QString,
        current_session_id: u64,
    ) -> QJsonObject {
        let current = ChapterSaveCurrentDocument {
            project_id: current_project_id.to_string(),
            volume_id: current_volume_id.to_string(),
            chapter_id: current_chapter_id.to_string(),
            session_id: current_session_id,
        };
        let timeout = std::time::Duration::from_millis(u64::from(timeout_ms.clamp(50, 10_000)));
        match chapter_save_queue::wait(request_id, timeout) {
            chapter_save_queue::ChapterSaveWait::Found(snapshot) => {
                self.chapter_save_result_object(snapshot, &current, false)
            }
            chapter_save_queue::ChapterSaveWait::Timeout => {
                serde_to_qjson_object(serde_json::json!({
                    "status": "pending",
                    "success": false,
                    "terminal": false,
                    "requestId": request_id,
                    "timedOut": true,
                    "changedEntities": []
                }))
            }
            chapter_save_queue::ChapterSaveWait::Unknown => {
                serde_to_qjson_object(serde_json::json!({
                    "status": "unknown",
                    "success": false,
                    "terminal": true,
                    "requestId": request_id,
                    "timedOut": false,
                    "changedEntities": []
                }))
            }
        }
    }

    /// 把队列回执转成 QML JSON，并在第一次读到终态时应用保存副作用。
    fn chapter_save_result_object(
        &mut self,
        snapshot: chapter_save_queue::ChapterSaveSnapshot,
        current: &ChapterSaveCurrentDocument,
        timed_out: bool,
    ) -> QJsonObject {
        let identity = &snapshot.identity;
        let document_matches = identity.project_id == current.project_id
            && identity.volume_id == current.volume_id
            && identity.chapter_id == current.chapter_id
            && identity.session_id == current.session_id;
        let apply_side_effects = snapshot.delivered_now && document_matches;
        let mut value = match &snapshot.outcome {
            chapter_save_queue::ChapterSaveOutcome::Pending => serde_json::json!({
                "success": false,
                "status": "pending",
                "changedEntities": []
            }),
            chapter_save_queue::ChapterSaveOutcome::Ok(receipt) => {
                if apply_side_effects {
                    let _ = self.with_app_mut(|app| {
                        app.apply_chapter_save_success(
                            &identity.project_id,
                            &identity.volume_id,
                            &identity.chapter_id,
                            &snapshot.content,
                        );
                    });
                    self.save_status_changed();
                }
                let mut value = AppBackend::chapter_save_success_value(receipt);
                if let serde_json::Value::Object(ref mut map) = value {
                    map.insert("status".to_string(), serde_json::json!("ok"));
                }
                value
            }
            chapter_save_queue::ChapterSaveOutcome::Superseded { by } => serde_json::json!({
                // 同一会话更新的快照已经接替本请求：不写盘、不推进 base，
                // QML 把等待方续接到接替者的回执上。
                "success": false,
                "status": "superseded",
                "supersededBy": by,
                "changedEntities": []
            }),
            chapter_save_queue::ChapterSaveOutcome::Error(error) => {
                if apply_side_effects {
                    let _ = self.with_app_mut(|app| {
                        app.record_chapter_save_failure(
                            &identity.chapter_id,
                            snapshot.content.len(),
                            &error.code,
                            &error.message_key,
                            error.message_args.clone(),
                            &error.raw_error,
                        );
                    });
                    self.save_status_changed();
                }
                let mut value = AppBackend::chapter_save_error_value(
                    &error.code,
                    &error.message_key,
                    error.message_args.clone(),
                    &error.raw_error,
                );
                if let serde_json::Value::Object(ref mut map) = value {
                    map.insert("status".to_string(), serde_json::json!("error"));
                }
                value
            }
        };
        if let serde_json::Value::Object(map) = &mut value {
            map.insert(
                "requestId".to_string(),
                serde_json::json!(identity.request_id),
            );
            map.insert(
                "sessionId".to_string(),
                serde_json::json!(identity.session_id),
            );
            map.insert(
                "projectId".to_string(),
                serde_json::json!(identity.project_id),
            );
            map.insert(
                "volumeId".to_string(),
                serde_json::json!(identity.volume_id),
            );
            map.insert(
                "chapterId".to_string(),
                serde_json::json!(identity.chapter_id),
            );
            map.insert(
                "snapshotRevision".to_string(),
                serde_json::json!(identity.snapshot_revision),
            );
            map.insert(
                "snapshotSha256".to_string(),
                serde_json::json!(identity.snapshot_sha256),
            );
            map.insert("chained".to_string(), serde_json::json!(snapshot.chained));
            map.insert(
                "conflictPersisted".to_string(),
                serde_json::json!(matches!(
                    &snapshot.outcome,
                    chapter_save_queue::ChapterSaveOutcome::Error(error) if error.conflict
                )),
            );
            map.insert(
                "conflictDraftPath".to_string(),
                serde_json::json!(snapshot.conflict_draft_path),
            );
            map.insert("terminal".to_string(), serde_json::json!(snapshot.terminal));
            map.insert("timedOut".to_string(), serde_json::json!(timed_out));
        }
        serde_to_qjson_object(value)
    }
    fn read_chapter_version(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
    ) -> QJsonObject {
        self.with_app_mut(|app| app.read_chapter_version(project_id, volume_id, chapter_id))
            .unwrap_or_else(|_| {
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
    fn record_editor_change_stats(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        cause: QString,
        inserted_chars: u32,
        deleted_chars: u32,
        changes_json: QString,
        base_revision: u64,
        new_revision: u64,
        old_selection_anchor: u64,
        old_selection_head: u64,
        new_selection_anchor: u64,
        new_selection_head: u64,
        baseline_text: QString,
        before_text_sha256: QString,
        after_text_sha256: QString,
        editor_session_id: u64,
        base_generation: QString,
        base_chapter_revision: u64,
        base_chapter_sha256: QString,
    ) -> QString {
        let parsed_cause = match cause.to_string().as_str() {
            "Typing" => writer_core::editor::EditorTransactionCause::Typing,
            "Delete" => writer_core::editor::EditorTransactionCause::Delete,
            "ImeComposition" => writer_core::editor::EditorTransactionCause::ImeComposition,
            "TypingCommit" => writer_core::editor::EditorTransactionCause::TypingCommit,
            "Paste" => writer_core::editor::EditorTransactionCause::Paste,
            "Undo" => writer_core::editor::EditorTransactionCause::Undo,
            "Redo" => writer_core::editor::EditorTransactionCause::Redo,
            "Load" => writer_core::editor::EditorTransactionCause::Load,
            "Format" => writer_core::editor::EditorTransactionCause::Format,
            "Programmatic" => writer_core::editor::EditorTransactionCause::Programmatic,
            // Issue #843: 未知 cause 不 fallback 到 Typing——那会把整批事件静默算成
            // 人工输入。记录诊断后直接 return，不落统计事件。
            unknown => {
                crate::backend::app_backend::debug_error_static(
                    "editor_backend",
                    "UNKNOWN_CAUSE",
                    &format!(
                        "record_editor_change_stats skipped: unknown cause '{}'",
                        unknown
                    ),
                );
                return QString::from("");
            }
        };
        if matches!(
            parsed_cause,
            writer_core::editor::EditorTransactionCause::Typing
                | writer_core::editor::EditorTransactionCause::TypingCommit
                | writer_core::editor::EditorTransactionCause::ImeComposition
                | writer_core::editor::EditorTransactionCause::Delete
        ) && (inserted_chars > 0 || deleted_chars > 0)
        {
            let _ = self.with_app(|app| {
                app.on_human_writing_edit(
                    &project_id.to_string(),
                    &volume_id.to_string(),
                    &chapter_id.to_string(),
                )
            });
        }
        match self.with_app(|app| {
            // 编辑事实的真实会话是编辑器文档会话（切章/重开即重建），
            // 不是写作统计的活动会话；QML 传 0 时退回统计会话。
            let session_id = if editor_session_id > 0 {
                editor_session_id.to_string()
            } else {
                app.current_writing_session_id()
            };
            app.record_editor_change_stats(
                project_id,
                volume_id,
                chapter_id,
                parsed_cause,
                inserted_chars,
                deleted_chars,
                changes_json,
                session_id.into(),
                base_revision,
                new_revision,
                old_selection_anchor,
                old_selection_head,
                new_selection_anchor,
                new_selection_head,
                baseline_text,
                before_text_sha256,
                after_text_sha256,
                base_generation,
                base_chapter_revision,
                base_chapter_sha256,
            )
        }) {
            Ok(event_id) => event_id,
            Err(_) => {
                crate::backend::app_backend::debug_error_static(
                    "editor_backend",
                    "BORROW_CONFLICT",
                    "record_editor_change_stats skipped due to borrow conflict",
                );
                QString::from("")
            }
        }
    }

    /// Issue #1062 1.3：正文变化交给章节 actor；`force` 边界立即写整章。
    #[allow(clippy::too_many_arguments)]
    fn update_chapter_draft_text(
        &mut self,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        session_id: u64,
        text: QString,
        revision: u64,
        sha256: QString,
        base_generation: QString,
        base_revision: u64,
        base_sha256: QString,
        force: bool,
    ) {
        let (project, volume, chapter, text, sha, base_generation, base_sha) = (
            project_id.to_string(),
            volume_id.to_string(),
            chapter_id.to_string(),
            text.to_string(),
            sha256.to_string(),
            base_generation.to_string(),
            base_sha256.to_string(),
        );
        let _ = self.with_app(|app| {
            app.update_chapter_draft_text(
                &project,
                &volume,
                &chapter,
                session_id,
                &text,
                revision,
                &sha,
                &base_generation,
                base_revision,
                &base_sha,
                force,
            )
        });
    }

    /// Issue #1062 1.3：项目进入时重放未完成草稿。
    fn restore_project_chapter_drafts(&mut self, project_id: QString) -> u32 {
        let project = project_id.to_string();
        self.with_app(|app| app.restore_project_chapter_drafts(&project))
            .unwrap_or(0)
    }

    /// Issue #1062 1.3：草稿恢复状态轮询。
    fn chapter_draft_restore_status(&mut self, project_id: QString) -> QJsonObject {
        let project = project_id.to_string();
        self.with_app(|app| app.chapter_draft_restore_status(&project))
            .unwrap_or_default()
    }

    /// Issue #1062 1.3：当前章节草稿快照（只读）。
    fn chapter_draft_snapshot(&mut self, project_id: QString, chapter_id: QString) -> QJsonObject {
        let (project, chapter) = (project_id.to_string(), chapter_id.to_string());
        self.with_app(|app| app.chapter_draft_snapshot(&project, &chapter))
            .unwrap_or_default()
    }

    /// Issue #935：异步请求统计面板快照（非阻塞，结果走信号回 GUI 线程）。
    fn request_writing_stats_dashboard(&mut self, request_id: u32) {
        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(
            move |(request_id, result): (u32, Result<String, String>)| {
                if let Some(pinned) = qptr.as_pinned() {
                    let this = pinned.borrow_mut();
                    match result {
                        Ok(json) => this.writing_stats_dashboard_ready(
                            request_id,
                            qjson_object_from_json(&json),
                        ),
                        Err(message) => {
                            this.writing_stats_dashboard_failed(request_id, message.into());
                        }
                    }
                }
            },
        );
        let completion: crate::backend::stats_writer::StatsQueryCallback =
            Box::new(move |request_id: u32, result: Result<String, String>| {
                callback((request_id, result));
            });
        match self.with_app(|app| app.request_writing_stats_dashboard(request_id, completion)) {
            Ok(Ok(())) => {}
            Ok(Err(message)) => {
                self.writing_stats_dashboard_failed(request_id, message.into());
            }
            Err(_) => {
                crate::backend::app_backend::debug_error_static(
                    "editor_backend",
                    "BORROW_CONFLICT",
                    "request_writing_stats_dashboard skipped due to borrow conflict",
                );
                self.writing_stats_dashboard_failed(request_id, "stats backend busy".into());
            }
        }
    }

    /// Issue #946：异步请求统计页 Explorer 快照（非阻塞，结果走信号回 GUI 线程）。
    ///
    /// QML 传空字符串表示「该端没有自选日期」（非 custom 预设），这里转成
    /// `Option`；custom 的两端必须都非空，Core 会校验非法参数并报错。
    fn request_writing_stats_explorer(
        &mut self,
        request_id: u32,
        preset: QString,
        start_date: QString,
        end_date: QString,
    ) {
        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(
            move |(request_id, result): (u32, Result<String, String>)| {
                if let Some(pinned) = qptr.as_pinned() {
                    let this = pinned.borrow_mut();
                    match result {
                        Ok(json) => this.writing_stats_explorer_ready(
                            request_id,
                            qjson_object_from_json(&json),
                        ),
                        Err(message) => {
                            this.writing_stats_explorer_failed(request_id, message.into());
                        }
                    }
                }
            },
        );
        let completion: crate::backend::stats_writer::StatsQueryCallback =
            Box::new(move |request_id: u32, result: Result<String, String>| {
                callback((request_id, result));
            });
        let start = start_date.to_string();
        let start = if start.is_empty() { None } else { Some(start) };
        let end = end_date.to_string();
        let end = if end.is_empty() { None } else { Some(end) };
        let preset = preset.to_string();
        match self.with_app(|app| {
            app.request_writing_stats_explorer(request_id, preset, start, end, completion)
        }) {
            Ok(Ok(())) => {}
            Ok(Err(message)) => {
                self.writing_stats_explorer_failed(request_id, message.into());
            }
            Err(_) => {
                crate::backend::app_backend::debug_error_static(
                    "editor_backend",
                    "BORROW_CONFLICT",
                    "request_writing_stats_explorer skipped due to borrow conflict",
                );
                self.writing_stats_explorer_failed(request_id, "stats backend busy".into());
            }
        }
    }

    /// Issue #960：异步请求每日码字日历（非阻塞，结果走信号回 GUI 线程）。
    ///
    /// 与 Explorer 使用同一条串行统计队列；Core 决定每日口径、色阶与覆盖状态，
    /// Qt 只负责定位和绘制。
    fn request_writing_stats_calendar(&mut self, request_id: u32, year: i32) {
        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(
            move |(request_id, result): (u32, Result<String, String>)| {
                if let Some(pinned) = qptr.as_pinned() {
                    let this = pinned.borrow_mut();
                    match result {
                        Ok(json) => this.writing_stats_calendar_ready(
                            request_id,
                            qjson_object_from_json(&json),
                        ),
                        Err(message) => {
                            this.writing_stats_calendar_failed(request_id, message.into());
                        }
                    }
                }
            },
        );
        let completion: crate::backend::stats_writer::StatsQueryCallback =
            Box::new(move |request_id: u32, result: Result<String, String>| {
                callback((request_id, result));
            });
        match self.with_app(|app| app.request_writing_stats_calendar(request_id, year, completion))
        {
            Ok(Ok(())) => {}
            Ok(Err(message)) => {
                self.writing_stats_calendar_failed(request_id, message.into());
            }
            Err(_) => {
                crate::backend::app_backend::debug_error_static(
                    "editor_backend",
                    "BORROW_CONFLICT",
                    "request_writing_stats_calendar skipped due to borrow conflict",
                );
                self.writing_stats_calendar_failed(request_id, "stats backend busy".into());
            }
        }
    }

    /// Issue #935：异步请求「实时速度 + 今日汇总」（结果走信号回 GUI 线程）。
    fn request_writing_status(&mut self, request_id: u32, window_seconds: u32) {
        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(
            move |(request_id, result): (u32, Result<String, String>)| {
                if let Some(pinned) = qptr.as_pinned() {
                    let this = pinned.borrow_mut();
                    match result {
                        Ok(json) => {
                            this.writing_status_ready(request_id, qjson_object_from_json(&json));
                        }
                        Err(message) => {
                            this.writing_status_failed(request_id, message.into());
                        }
                    }
                }
            },
        );
        let completion: crate::backend::stats_writer::StatsQueryCallback =
            Box::new(move |request_id: u32, result: Result<String, String>| {
                callback((request_id, result));
            });
        match self
            .with_app(|app| app.request_writing_status(request_id, window_seconds, completion))
        {
            Ok(Ok(())) => {}
            Ok(Err(message)) => {
                self.writing_status_failed(request_id, message.into());
            }
            Err(_) => {
                crate::backend::app_backend::debug_error_static(
                    "editor_backend",
                    "BORROW_CONFLICT",
                    "request_writing_status skipped due to borrow conflict",
                );
                self.writing_status_failed(request_id, "stats backend busy".into());
            }
        }
    }

    fn set_correction_model_path(&self, path: QString) -> QJsonObject {
        let result = self.with_app(|app| app.set_correction_model_path(path.to_string()));
        match result {
            Ok(Ok(())) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": true, "data": { "configured": true } }),
            ),
            Ok(Err(message)) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": false, "errorCode": "CORE_ERROR", "rawError": message }),
            ),
            Err(error) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": false, "errorCode": "APP_BUSY", "rawError": error.to_string() }),
            ),
        }
    }

    fn pick_correction_model_path(&self) -> QJsonObject {
        let Some(selected_path) = FileDialog::new().pick_folder() else {
            return crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": false, "cancelled": true }),
            );
        };

        let result = (|| -> Result<String, String> {
            let path = selected_path
                .canonicalize()
                .map_err(|error| format!("无法解析所选模型目录: {error}"))?;
            if !path.is_dir() {
                return Err("请选择模型资源目录".to_string());
            }
            if !path.join("model.onnx").is_file()
                || !path.join("vocab.txt").is_file()
                || !path.join("tokenizer.json").is_file()
            {
                return Err("模型目录必须包含 model.onnx、vocab.txt 与 tokenizer.json".to_string());
            }
            Ok(path.to_string_lossy().into_owned())
        })();

        match result {
            Ok(path) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": true, "path": path }),
            ),
            Err(message) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": false, "rawError": message }),
            ),
        }
    }

    fn import_correction_dictionary(&self) -> QJsonObject {
        let Some(source_path) = FileDialog::new()
            .add_filter("校对词库 (*.txt)", &["txt"])
            .pick_file()
        else {
            return crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": false, "cancelled": true }),
            );
        };

        let result = self.with_app(|app| {
            let file_name = app.import_correction_dictionary(source_path)?;
            let resources = app.correction_dictionary_resource_status()?;
            Ok::<_, String>((file_name, resources))
        });
        match result {
            Ok(Ok((file_name, resources))) => {
                crate::backend::json_utils::serde_to_qjson_object(serde_json::json!({
                    "success": true,
                    "fileName": file_name,
                    "resources": correction_dictionary_resources_value(resources),
                }))
            }
            Ok(Err(message)) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": false, "rawError": message }),
            ),
            Err(error) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": false, "rawError": error.to_string() }),
            ),
        }
    }

    fn correction_dictionary_status(&self) -> QJsonObject {
        match self.with_app(|app| app.correction_dictionary_resource_status()) {
            Ok(Ok(resources)) => {
                crate::backend::json_utils::serde_to_qjson_object(serde_json::json!({
                    "success": true,
                    "resources": correction_dictionary_resources_value(resources),
                }))
            }
            Ok(Err(message)) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": false, "rawError": message }),
            ),
            Err(error) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": false, "rawError": error.to_string() }),
            ),
        }
    }

    fn request_correction_chapter(
        &mut self,
        request_id: u32,
        project_id: QString,
        chapter_id: QString,
        session_id: u64,
        text_revision: u64,
        text: QString,
    ) {
        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(
            move |(request_id, text_revision, result): (u32, u64, Result<String, String>)| {
                if let Some(pinned) = qptr.as_pinned() {
                    let this = pinned.borrow_mut();
                    match result {
                        Ok(json) => {
                            this.correction_check_ready(request_id, text_revision, json.into())
                        }
                        Err(message) => {
                            this.correction_check_failed(request_id, text_revision, message.into())
                        }
                    }
                }
            },
        );
        let completion: super::correction_operations::CorrectionQueryCallback =
            Box::new(move |request_id, text_revision, result| {
                callback((request_id, text_revision, result));
            });
        let result = self.with_app(|app| {
            app.request_correction_chapter(
                request_id,
                project_id.to_string(),
                chapter_id.to_string(),
                session_id,
                text_revision,
                text.to_string(),
                completion,
            )
        });
        match result {
            Ok(Ok(())) => {}
            Ok(Err(message)) => {
                self.correction_check_failed(request_id, text_revision, message.into());
            }
            Err(error) => {
                self.correction_check_failed(request_id, text_revision, error.to_string().into());
            }
        }
    }

    fn request_correction_segment(
        &mut self,
        request_id: u32,
        project_id: QString,
        chapter_id: QString,
        session_id: u64,
        text_revision: u64,
        text: QString,
        start_byte: u32,
        end_byte: u32,
    ) {
        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(
            move |(request_id, text_revision, result): (u32, u64, Result<String, String>)| {
                if let Some(pinned) = qptr.as_pinned() {
                    let this = pinned.borrow_mut();
                    match result {
                        Ok(json) => {
                            this.correction_check_ready(request_id, text_revision, json.into())
                        }
                        Err(message) => {
                            this.correction_check_failed(request_id, text_revision, message.into())
                        }
                    }
                }
            },
        );
        let completion: super::correction_operations::CorrectionQueryCallback =
            Box::new(move |request_id, text_revision, result| {
                callback((request_id, text_revision, result));
            });
        let result = self.with_app(|app| {
            app.request_correction_segment(
                request_id,
                project_id.to_string(),
                chapter_id.to_string(),
                session_id,
                text_revision,
                text.to_string(),
                start_byte,
                end_byte,
                completion,
            )
        });
        match result {
            Ok(Ok(())) => {}
            Ok(Err(message)) => {
                self.correction_check_failed(request_id, text_revision, message.into());
            }
            Err(error) => {
                self.correction_check_failed(request_id, text_revision, error.to_string().into());
            }
        }
    }

    fn request_correction_snapshot(
        &mut self,
        request_id: u32,
        project_id: QString,
        chapter_id: QString,
        session_id: u64,
        text_revision: u64,
        text: QString,
    ) {
        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(
            move |(request_id, text_revision, result): (u32, u64, Result<String, String>)| {
                if let Some(pinned) = qptr.as_pinned() {
                    let this = pinned.borrow_mut();
                    match result {
                        Ok(json) => {
                            this.correction_check_ready(request_id, text_revision, json.into())
                        }
                        Err(message) => {
                            this.correction_check_failed(request_id, text_revision, message.into())
                        }
                    }
                }
            },
        );
        let completion: super::correction_operations::CorrectionQueryCallback =
            Box::new(move |request_id, text_revision, result| {
                callback((request_id, text_revision, result));
            });
        let result = self.with_app(|app| {
            app.request_correction_snapshot(
                request_id,
                project_id.to_string(),
                chapter_id.to_string(),
                session_id,
                text_revision,
                text.to_string(),
                completion,
            )
        });
        match result {
            Ok(Ok(())) => {}
            Ok(Err(message)) => {
                self.correction_check_failed(request_id, text_revision, message.into());
            }
            Err(error) => {
                self.correction_check_failed(request_id, text_revision, error.to_string().into());
            }
        }
    }

    fn request_correction_edits(
        &mut self,
        request_id: u32,
        project_id: QString,
        chapter_id: QString,
        session_id: u64,
        text_revision: u64,
        base_revision: u64,
        edits_json: QString,
        text: QString,
    ) {
        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(
            move |(request_id, text_revision, result): (u32, u64, Result<String, String>)| {
                if let Some(pinned) = qptr.as_pinned() {
                    let this = pinned.borrow_mut();
                    match result {
                        Ok(json) => {
                            this.correction_check_ready(request_id, text_revision, json.into())
                        }
                        Err(message) => {
                            this.correction_check_failed(request_id, text_revision, message.into())
                        }
                    }
                }
            },
        );
        let completion: super::correction_operations::CorrectionQueryCallback =
            Box::new(move |request_id, text_revision, result| {
                callback((request_id, text_revision, result));
            });
        let result = self.with_app(|app| {
            app.request_correction_edits(
                request_id,
                project_id.to_string(),
                chapter_id.to_string(),
                session_id,
                text_revision,
                base_revision,
                edits_json.to_string(),
                text.to_string(),
                completion,
            )
        });
        match result {
            Ok(Ok(())) => {}
            Ok(Err(message)) => {
                self.correction_check_failed(request_id, text_revision, message.into());
            }
            Err(error) => {
                self.correction_check_failed(request_id, text_revision, error.to_string().into());
            }
        }
    }

    fn apply_correction_edits(
        &mut self,
        request_id: u32,
        project_id: QString,
        chapter_id: QString,
        session_id: u64,
        text_revision: u64,
        base_revision: u64,
        edits_json: QString,
    ) {
        let project = project_id.to_string();
        let chapter = chapter_id.to_string();
        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(
            move |(request_id, text_revision, result): (u32, u64, Result<String, String>)| {
                if let Some(pinned) = qptr.as_pinned() {
                    let this = pinned.borrow_mut();
                    match result {
                        Ok(_) => this.correction_edits_applied(
                            request_id,
                            QString::from(project.clone()),
                            QString::from(chapter.clone()),
                            text_revision,
                            true,
                            QString::from(""),
                        ),
                        Err(message) => this.correction_edits_applied(
                            request_id,
                            QString::from(project.clone()),
                            QString::from(chapter.clone()),
                            text_revision,
                            false,
                            QString::from(message),
                        ),
                    }
                }
            },
        );
        let completion: super::correction_operations::CorrectionQueryCallback =
            Box::new(move |request_id, text_revision, result| {
                callback((request_id, text_revision, result));
            });
        let result = self.with_app(|app| {
            app.request_correction_edits_apply(
                request_id,
                project_id.to_string(),
                chapter_id.to_string(),
                session_id,
                text_revision,
                base_revision,
                edits_json.to_string(),
                completion,
            )
        });
        match result {
            Ok(Ok(())) => {}
            Ok(Err(message)) => {
                self.correction_edits_applied(
                    request_id,
                    project_id,
                    chapter_id,
                    text_revision,
                    false,
                    QString::from(message),
                );
            }
            Err(error) => {
                self.correction_edits_applied(
                    request_id,
                    project_id,
                    chapter_id,
                    text_revision,
                    false,
                    QString::from(error.to_string()),
                );
            }
        }
    }

    fn release_correction_session(&self, project_id: QString, chapter_id: QString) {
        super::correction_operations::release_correction_queue(
            &project_id.to_string(),
            &chapter_id.to_string(),
        );
        // Issue #1061 复审：离开校对文档时只回收已超过空闲宽限期的 session；
        // 活动扫描持有模型强引用，不会被这里打断。
        if let Some(api) = self.with_app(|app| app.core_api()).ok().flatten() {
            api.note_correction_session_left();
        }
    }

    fn request_correction_pending(
        &mut self,
        request_id: u32,
        project_id: QString,
        chapter_id: QString,
        session_id: u64,
        text_revision: u64,
        text: QString,
    ) {
        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(
            move |(request_id, text_revision, result): (u32, u64, Result<String, String>)| {
                if let Some(pinned) = qptr.as_pinned() {
                    let this = pinned.borrow_mut();
                    match result {
                        Ok(json) => {
                            this.correction_check_ready(request_id, text_revision, json.into())
                        }
                        Err(message) => {
                            this.correction_check_failed(request_id, text_revision, message.into())
                        }
                    }
                }
            },
        );
        let completion: super::correction_operations::CorrectionQueryCallback =
            Box::new(move |request_id, text_revision, result| {
                callback((request_id, text_revision, result));
            });
        let result = self.with_app(|app| {
            app.request_correction_pending(
                request_id,
                project_id.to_string(),
                chapter_id.to_string(),
                session_id,
                text_revision,
                text.to_string(),
                completion,
            )
        });
        match result {
            Ok(Ok(())) => {}
            Ok(Err(message)) => {
                self.correction_check_failed(request_id, text_revision, message.into());
            }
            Err(error) => {
                self.correction_check_failed(request_id, text_revision, error.to_string().into());
            }
        }
    }

    fn ignore_correction(
        &self,
        project_id: QString,
        chapter_id: QString,
        rule_id: QString,
        original: QString,
        scope: QString,
        text_revision: u64,
        start_byte: u32,
        end_byte: u32,
    ) -> QJsonObject {
        let result = self.with_app(|app| {
            app.ignore_correction(
                project_id.to_string(),
                chapter_id.to_string(),
                rule_id.to_string(),
                original.to_string(),
                scope.to_string(),
                text_revision,
                start_byte,
                end_byte,
            )
        });
        match result {
            Ok(Ok(ignored)) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": true, "data": { "ignored": ignored } }),
            ),
            Ok(Err(message)) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": false, "errorCode": "CORE_ERROR", "rawError": message }),
            ),
            Err(error) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": false, "errorCode": "APP_BUSY", "rawError": error.to_string() }),
            ),
        }
    }

    fn add_correction_dictionary_entry(&self, project_id: QString, entry: QString) -> QJsonObject {
        let result = self.with_app(|app| {
            app.add_correction_dictionary_entry(project_id.to_string(), entry.to_string())
        });
        match result {
            Ok(Ok(added)) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": true, "data": { "added": added } }),
            ),
            Ok(Err(message)) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": false, "errorCode": "CORE_ERROR", "rawError": message }),
            ),
            Err(error) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": false, "errorCode": "APP_BUSY", "rawError": error.to_string() }),
            ),
        }
    }

    fn add_correction_dictionary_entry_with_kind(
        &self,
        project_id: QString,
        entry: QString,
        kind: QString,
    ) -> QJsonObject {
        let kind = match kind.to_string().to_ascii_lowercase().as_str() {
            "person" => writer_core::api::types::CorrectionDictionaryEntryKindDto::Person,
            "place" => writer_core::api::types::CorrectionDictionaryEntryKindDto::Place,
            _ => writer_core::api::types::CorrectionDictionaryEntryKindDto::Term,
        };
        let result = self.with_app(|app| {
            app.add_correction_dictionary_entry_with_kind(
                project_id.to_string(),
                entry.to_string(),
                kind,
            )
        });
        match result {
            Ok(Ok(added)) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": true, "data": { "added": added } }),
            ),
            Ok(Err(message)) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": false, "errorCode": "CORE_ERROR", "rawError": message }),
            ),
            Err(error) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": false, "errorCode": "APP_BUSY", "rawError": error.to_string() }),
            ),
        }
    }

    fn add_correction_entity_name(
        &self,
        project_id: QString,
        canonical: QString,
        name: QString,
        kind: QString,
    ) -> QJsonObject {
        let kind = match kind.to_string().to_ascii_lowercase().as_str() {
            "misspelling" => writer_core::api::types::CorrectionEntityNameKindDto::Misspelling,
            _ => writer_core::api::types::CorrectionEntityNameKindDto::Alias,
        };
        let result = self.with_app(|app| {
            app.add_correction_entity_name(
                project_id.to_string(),
                canonical.to_string(),
                name.to_string(),
                kind,
            )
        });
        match result {
            Ok(Ok(added)) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": true, "data": { "added": added } }),
            ),
            Ok(Err(message)) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": false, "errorCode": "CORE_ERROR", "rawError": message }),
            ),
            Err(error) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": false, "errorCode": "APP_BUSY", "rawError": error.to_string() }),
            ),
        }
    }

    fn get_correction_entity_names(&self, project_id: QString) -> QJsonObject {
        let result = self.with_app(|app| app.get_correction_entity_names(project_id.to_string()));
        match result {
            Ok(Ok(entities)) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": true, "data": { "entities": entities } }),
            ),
            Ok(Err(message)) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": false, "errorCode": "CORE_ERROR", "rawError": message }),
            ),
            Err(error) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": false, "errorCode": "APP_BUSY", "rawError": error.to_string() }),
            ),
        }
    }

    fn remove_correction_entity_name(
        &self,
        project_id: QString,
        canonical: QString,
        name: QString,
    ) -> QJsonObject {
        let result = self.with_app(|app| {
            app.remove_correction_entity_name(
                project_id.to_string(),
                canonical.to_string(),
                name.to_string(),
            )
        });
        match result {
            Ok(Ok(removed)) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": true, "data": { "removed": removed } }),
            ),
            Ok(Err(message)) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": false, "errorCode": "CORE_ERROR", "rawError": message }),
            ),
            Err(error) => crate::backend::json_utils::serde_to_qjson_object(
                serde_json::json!({ "success": false, "errorCode": "APP_BUSY", "rawError": error.to_string() }),
            ),
        }
    }

    fn get_writing_stats_summary(&self, start_date: QString, end_date: QString) -> QString {
        self.with_app(|app| app.get_writing_stats_summary(start_date, end_date))
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn get_today_writing_stats_summary_object(&self) -> QJsonObject {
        self.with_app(|app| app.get_today_writing_stats_summary_object())
            .unwrap_or_else(|_| {
                qjson_object_from_json(&crate::backend::json_utils::borrow_conflict_error_json())
            })
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

    fn replay_chapter_proof_object(&self, request_json: QString) -> QJsonObject {
        self.with_app(|app| app.replay_chapter_proof_object(request_json))
            .unwrap_or_else(|_| {
                qjson_object_from_json(&crate::backend::json_utils::borrow_conflict_error_json())
            })
    }

    fn export_chapter_proof_object(&self, chapter_id: QString, device_id: QString) -> QJsonObject {
        self.with_app(|app| app.export_chapter_proof_object(chapter_id, device_id))
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
    fn get_current_writing_speed(&self, window_seconds: u32) -> QJsonObject {
        self.with_app(|app| app.get_current_writing_speed(window_seconds))
            .unwrap_or_else(|_| {
                qjson_object_from_json(&crate::backend::json_utils::borrow_conflict_error_json())
            })
    }
    fn flush_recent_edits(&self) {
        if self.with_app(|app| app.flush_recent_edits()).is_err() {
            crate::backend::app_backend::debug_error_static(
                "editor_backend",
                "BORROW_CONFLICT",
                "flush_recent_edits skipped due to borrow conflict",
            );
        }
        // Issue #1059：关窗 / 同步屏障时等后台正文保存队列消费完此前全部请求，
        // 避免“只入队未落盘”的最后一笔在该路径上被进程退出吞掉。
        if !chapter_save_queue::drain(std::time::Duration::from_secs(5)) {
            crate::backend::app_backend::debug_error_static(
                "chapter_save_queue",
                "drain_timeout",
                "chapter save queue did not drain before exit",
            );
        }
    }
    fn writing_session_heartbeat(&self) {
        if self
            .with_app(|app| app.writing_session_heartbeat())
            .is_err()
        {
            crate::backend::app_backend::debug_error_static(
                "editor_backend",
                "BORROW_CONFLICT",
                "writing_session_heartbeat skipped due to borrow conflict",
            );
        }
    }
    fn pause_writing_session(&self, reason: QString) {
        let reason = reason.to_string();
        if self
            .with_app(|app| app.pause_writing_session(&reason))
            .is_err()
        {
            crate::backend::app_backend::debug_error_static(
                "editor_backend",
                "BORROW_CONFLICT",
                "pause_writing_session skipped due to borrow conflict",
            );
        }
    }
    fn resume_writing_session(&self) {
        if self.with_app(|app| app.resume_writing_session()).is_err() {
            crate::backend::app_backend::debug_error_static(
                "editor_backend",
                "BORROW_CONFLICT",
                "resume_writing_session skipped due to borrow conflict",
            );
        }
    }
    fn close_writing_session(&self, reason: QString) {
        let reason = reason.to_string();
        if self
            .with_app(|app| app.close_writing_session(&reason))
            .is_err()
        {
            crate::backend::app_backend::debug_error_static(
                "editor_backend",
                "BORROW_CONFLICT",
                "close_writing_session skipped due to borrow conflict",
            );
        }
    }
    fn close_writing_session_and_wait(&self, reason: QString) -> QString {
        let reason = reason.to_string();
        match self.with_app(|app| app.close_writing_session_and_wait(&reason)) {
            Ok(Ok(())) => String::new().into(),
            Ok(Err(error)) => error.into(),
            Err(_) => {
                crate::backend::app_backend::debug_error_static(
                    "editor_backend",
                    "BORROW_CONFLICT",
                    "close_writing_session_and_wait skipped due to borrow conflict",
                );
                "应用后端正忙，写作统计尚未确认保存".to_string().into()
            }
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

    /// Issue #920：正文段落格式化规则由 Core 持有（`editor::formatting`），
    /// Qt 不再自带一套 JS 正则分段与重组；平台只负责把结果刷新到编辑器并保存。
    fn format_plain_text(&self, text: QString) -> QString {
        let text_str = text.to_string();
        let formatted = writer_core::editor::formatting::format_paragraphs(&text_str);
        QString::from(formatted.as_str())
    }
}

impl AppBackend {
    // Included inside impl AppBackend from app_backend.rs.
    // Deprecated compatibility methods for this Linux backend domain.

    // AppBackend::set_save_status
    pub(crate) fn set_save_status(&mut self, status: QString) {
        self.current_save_status = status.to_string();
    }

    // AppBackend::set_word_count
    pub(crate) fn set_word_count(&mut self, count: i32) {
        self.current_word_count = count;
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
            writer_core::api::ResultEnvelope::<()>::error(writer_core::api::WriterError::Other(
                "core api not available".to_string(),
            ))
            .to_json_string()
            .into()
        }
    }

    // AppBackend::clear_editor_state
    pub(crate) fn clear_editor_state(&mut self) {
        self.selected_chapter_id = None;
    }

    // AppBackend::set_error
    pub(crate) fn set_error(&mut self, msg: &str) {
        self.current_error_message = msg.to_string();
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
