// =============================================================================
// workspace_backend.rs — 平台数据目录管理 + 作品目录打开/发现 QObject 后端适配层
// =============================================================================
//
// 引用了什么：
// - super::*：引入 AppBackend 核心后端的全部方法与结构体。
// - crate::backend::AppRef：用于安全访问全局 AppBackend 指针以读取/更新数据根状态。
//
// 干什么的：
// - 实现 WorkspaceBackend 结构体，作为 QML 中 "workspaceBackend" 对象的桥梁。
// - 平台数据目录管理：Linux 允许用户自己选择素笺数据根目录；选择结果由 Linux 平台层保存，
//   然后把目录信息注入 Core 的两路径 API (app_data_root, projects_root)。
// - 作品目录打开/发现：基于数据根目录发现并打开作品。
// - 支持从 GitHub 克隆（init_workspace_from_github & execute_github_init）拉取已有数据至本地。
// - 负责向上层 QML 主页提供当前数据根路径（workspace_path）和是否已加载（has_workspace）属性。
//   QML 仍把作品首页叫"工作区"，这只是 UI 命名；底层不再调用 Core workspace API。
//
// 被什么引用：
// - 被 apps/Linux_qt/src/backend/mod.rs 引用，用于实例化工作区后端并绑定为 QML 全局上下文属性。
// =============================================================================

use super::*;
use crate::backend::AppRef;
use crate::backend::DomainSnapshot;

use crate::backend::json_utils::qjson_object_from_json;
use qmetaobject::QJsonObject;

#[path = "github_init_operations.rs"]
mod github_init_operations;

fn backend_link_broken_json() -> QString {
    QString::from(crate::backend::json_utils::borrow_conflict_error_json())
}

fn workspace_success_json(data: &str) -> QString {
    writer_core::api::ResultEnvelope::success(data.to_string())
        .to_json_string()
        .into()
}

#[allow(non_snake_case)]
#[derive(QObject, Default)]
pub struct WorkspaceBackend {
    base: qt_base_class!(trait QObject),
    workspace_path: qt_property!(QString; READ workspace_path NOTIFY workspace_opened),
    has_workspace: qt_property!(bool; READ has_workspace NOTIFY workspace_state_changed),
    pending_github_init_path: qt_property!(QString; READ pending_github_init_path NOTIFY pending_github_init_path_changed),
    workspace_opened: qt_signal!(),
    workspace_content_changed: qt_signal!(),
    workspace_state_changed: qt_signal!(),
    pending_github_init_path_changed: qt_signal!(),
    try_restore_last_workspace: qt_method!(fn(&mut self)),
    create_new_workspace: qt_method!(fn(&mut self) -> QJsonObject),
    open_existing_workspace: qt_method!(fn(&mut self) -> QJsonObject),
    create_workspace_with_path: qt_method!(fn(&mut self, path: QString) -> QJsonObject),
    open_workspace_with_path: qt_method!(fn(&mut self, path: QString) -> QJsonObject),
    close_workspace: qt_method!(fn(&mut self)),
    switch_workspace: qt_method!(fn(&mut self)),
    init_workspace_from_github: qt_method!(fn(&mut self)),
    execute_github_init: qt_method!(
        fn(&mut self, path: QString, remote_url: QString, branch: QString, token: QString)
    ),
    open_workspace_dir: qt_method!(fn(&mut self)),
    save_last_navigation_state: qt_method!(
        fn(
            &mut self,
            route: QString,
            project_id: QString,
            volume_id: QString,
            chapter_id: QString,
            starmap_id: QString,
        )
    ),
    get_last_navigation_state: qt_method!(fn(&self) -> QJsonObject),
    clear_last_navigation_state: qt_method!(fn(&mut self)),
    app: AppRef,
}

impl WorkspaceBackend {
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
    fn emit_workspace_changed(&mut self) {
        self.workspace_opened();
        self.workspace_content_changed();
        self.workspace_state_changed();
    }
    fn workspace_path(&self) -> QString {
        self.with_app(|app| app.workspace_path())
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn has_workspace(&self) -> bool {
        self.snap().has_workspace
    }
    fn pending_github_init_path(&self) -> QString {
        self.with_app(|app| app.pending_github_init_path())
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn try_restore_last_workspace(&mut self) {
        if self
            .with_app_mut(|app| app.try_restore_last_workspace())
            .is_ok()
        {
            self.emit_workspace_changed();
        }
    }
    fn create_new_workspace(&mut self) -> QJsonObject {
        let result = self.with_app_mut(|app| app.create_new_workspace());
        if result.is_ok() {
            self.emit_workspace_changed();
        }
        let res = result.unwrap_or_else(|_| backend_link_broken_json());
        qjson_object_from_json(&res.to_string())
    }
    fn open_existing_workspace(&mut self) -> QJsonObject {
        let result = self.with_app_mut(|app| app.open_existing_workspace());
        if result.is_ok() {
            self.emit_workspace_changed();
        }
        let res = result.unwrap_or_else(|_| backend_link_broken_json());
        qjson_object_from_json(&res.to_string())
    }
    fn create_workspace_with_path(&mut self, path: QString) -> QJsonObject {
        let path_str = path.to_string();
        crate::backend::app_backend::debug_log_static(
            "workspace",
            "qml_click_create_workspace",
            &format!("path={}", path_str),
        );
        crate::backend::app_backend::debug_log_static(
            "workspace",
            "workspace_backend_create_workspace_called",
            &format!("path={}", path_str),
        );
        let result = self.with_app_mut(|app| app.internal_open_data_root(&path_str));
        if result.is_ok() {
            self.emit_workspace_changed();
        }
        let res = result.unwrap_or_else(|_| backend_link_broken_json());
        qjson_object_from_json(&res.to_string())
    }
    fn open_workspace_with_path(&mut self, path: QString) -> QJsonObject {
        let path_str = path.to_string();
        crate::backend::app_backend::debug_log_static(
            "workspace",
            "qml_click_open_workspace",
            &format!("path={}", path_str),
        );
        crate::backend::app_backend::debug_log_static(
            "workspace",
            "workspace_backend_open_workspace_called",
            &format!("path={}", path_str),
        );
        let result = self.with_app_mut(|app| app.internal_open_data_root(&path_str));
        if result.is_ok() {
            self.emit_workspace_changed();
        }
        let res = result.unwrap_or_else(|_| backend_link_broken_json());
        qjson_object_from_json(&res.to_string())
    }
    fn close_workspace(&mut self) {
        if self.with_app_mut(|app| app.close_workspace()).is_ok() {
            self.emit_workspace_changed();
        }
    }
    fn switch_workspace(&mut self) {
        if self.with_app_mut(|app| app.switch_workspace()).is_ok() {
            self.emit_workspace_changed();
        }
    }
    fn init_workspace_from_github(&mut self) {
        if self
            .with_app_mut(|app| app.init_workspace_from_github())
            .is_ok()
        {
            self.pending_github_init_path_changed();
        }
    }
    fn execute_github_init(
        &mut self,
        path: QString,
        remote_url: QString,
        branch: QString,
        token: QString,
    ) {
        crate::backend::app_backend::debug_log_static(
            "workspace",
            "qml_click_import_workspace",
            &format!("path={}, url={}", path, remote_url),
        );
        crate::backend::app_backend::debug_log_static(
            "workspace",
            "workspace_backend_import_workspace_called",
            &format!("path={}", path),
        );
        if self
            .with_app_mut(|app| app.execute_github_init(path, remote_url, branch, token))
            .is_ok()
        {
            self.emit_workspace_changed();
            self.pending_github_init_path_changed();
        }
    }
    fn open_workspace_dir(&mut self) {
        if self.with_app_mut(|app| app.open_workspace_dir()).is_err() {
            crate::backend::app_backend::debug_error_static(
                "workspace_backend",
                "BORROW_CONFLICT",
                "open_workspace_dir skipped due to borrow conflict",
            );
        }
    }
    fn save_last_navigation_state(
        &mut self,
        route: QString,
        project_id: QString,
        volume_id: QString,
        chapter_id: QString,
        starmap_id: QString,
    ) {
        let r = route.to_string();
        let p = project_id.to_string();
        let v = volume_id.to_string();
        let c = chapter_id.to_string();
        let s = starmap_id.to_string();
        let _ = writer_core::app_config::save_last_navigation_state(
            &r,
            if p.is_empty() { None } else { Some(&p) },
            if v.is_empty() { None } else { Some(&v) },
            if c.is_empty() { None } else { Some(&c) },
            if s.is_empty() { None } else { Some(&s) },
        );
    }
    fn get_last_navigation_state(&self) -> QJsonObject {
        let state = writer_core::app_config::get_last_navigation_state();
        let json = serde_json::json!({
            "route": state.route.unwrap_or_default(),
            "projectId": state.project_id.unwrap_or_default(),
            "volumeId": state.volume_id.unwrap_or_default(),
            "chapterId": state.chapter_id.unwrap_or_default(),
            "starmapId": state.starmap_id.unwrap_or_default(),
        });
        qjson_object_from_json(&serde_json::to_string(&json).unwrap_or_else(|_| "{}".to_string()))
    }
    fn clear_last_navigation_state(&mut self) {
        let _ = writer_core::app_config::clear_last_navigation_state();
    }
}

impl AppBackend {
    // Included inside impl AppBackend from app_backend.rs.
    // 平台数据目录管理 + 作品目录打开/发现方法。

    // AppBackend::has_workspace
    pub(crate) fn has_workspace(&self) -> bool {
        self.current_has_data_root
    }

    // AppBackend::pending_github_init_path
    pub(crate) fn pending_github_init_path(&self) -> QString {
        self.current_pending_github_init_path.clone().into()
    }

    // AppBackend::try_restore_last_workspace
    //
    // last_workspace_path 已从 AppConfig 删除。Linux 平台层不再自动恢复上次数据根；
    // 用户需要手动选择数据根目录。此处只加载应用级主题等设置。
    pub(crate) fn try_restore_last_workspace(&mut self) {
        self.debug_log("workspace", "try_restore_last_workspace_start", "");
        self.debug_log(
            "workspace",
            "try_restore_last_workspace_no_saved_path",
            "last_workspace_path removed; waiting for user selection",
        );
        // No saved data root to restore
        self.current_has_data_root = false;
        self.current_sync_status = "no_workspace".to_string();
        self.sync_status_changed();
        self.workspace_state_changed();
        // Load app-level theme mode even without data root
        self.load_app_theme_mode();
        self.ai_available_changed();
    }

    // AppBackend::internal_open_data_root
    //
    // 打开用户选择的数据根目录。设置 app_data_root = path, projects_root = path/projects。
    // 不再调用 Core workspace API (validate_workspace / create_workspace_if_needed)。
    pub(crate) fn internal_open_data_root(&mut self, path: &str) -> QString {
        let canonical_path = normalize_data_root_path(path);
        let path = canonical_path.as_str();
        self.debug_log(
            "workspace",
            "internal_open_data_root_start",
            &format!("path={}", path),
        );

        // 确保 projects 子目录存在
        let projects_root = std::path::Path::new(path).join("projects");
        let projects_root_str = projects_root.to_string_lossy().to_string();
        if let Err(e) = std::fs::create_dir_all(&projects_root) {
            let err_msg = format!("无法创建作品目录: {}", e);
            self.set_error(&err_msg);
            self.debug_error("workspace", "internal_open_data_root_failed", &err_msg);
            return crate::backend::json_utils::envelope_error_json(
                writer_core::api::WriterError::Other(err_msg),
            )
            .into();
        }

        self.current_data_root = path.to_string();
        self.current_projects_root = projects_root_str.clone();
        self.current_has_data_root = true;
        self.current_save_status = "已保存".to_string();
        self.save_status_changed();
        self.reload_tree();
        self.load_sync_config();
        self.load_local_settings();

        // 写入 current_device.json 设备信息
        let api = crate::backend::app_backend::create_core_api(path, &projects_root_str);
        if let Err(e) = api.ensure_device_info("desktop", "desktop") {
            self.debug_log("workspace", "ensure_device_info_failed", &format!("{}", e));
        }

        self.workspace_opened();
        self.workspace_content_changed();
        self.workspace_state_changed();

        self.debug_log(
            "workspace",
            "internal_open_data_root_success",
            &format!("path={}, projects_root={}", path, projects_root_str),
        );

        workspace_success_json("OK")
    }

    // AppBackend::create_new_workspace
    pub(crate) fn create_new_workspace(&mut self) -> QString {
        self.debug_log("workspace", "create_new_workspace_clicked", "");
        if let Some(path) = FileDialog::new().pick_folder() {
            self.internal_open_data_root(&path.to_string_lossy())
        } else {
            self.debug_log("workspace", "create_new_workspace_cancelled", "");
            workspace_success_json("CANCELLED")
        }
    }

    // AppBackend::open_existing_workspace
    pub(crate) fn open_existing_workspace(&mut self) -> QString {
        self.debug_log("workspace", "open_existing_workspace_clicked", "");
        if let Some(path) = FileDialog::new().pick_folder() {
            self.internal_open_data_root(&path.to_string_lossy())
        } else {
            self.debug_log("workspace", "open_existing_workspace_cancelled", "");
            workspace_success_json("CANCELLED")
        }
    }

    // AppBackend::close_workspace
    pub(crate) fn close_workspace(&mut self) {
        self.debug_log("workspace", "close_workspace_start", "");
        self.flush_writing_stats();
        self.flush_recent_edits();
        // Clear data root state
        self.current_data_root = "".to_string();
        self.current_projects_root = "".to_string();
        self.current_has_data_root = false;
        // Clear selection state
        self.selected_project_id = None;
        self.selected_volume_id = None;
        self.selected_chapter_id = None;
        // Clear tree
        self.cached_tree = QJsonArray::default();
        // Reset sync status
        self.current_sync_status = "no_workspace".to_string();
        self.current_save_status = "未打开工作区".to_string();
        self.save_status_changed();
        // Clear editor
        self.clear_editor();
        // Emit signals
        self.workspace_state_changed();
        self.trigger_projects_reloaded();
        self.sync_status_changed();
        self.debug_log("workspace", "close_workspace_success", "");
    }

    // AppBackend::switch_workspace
    pub(crate) fn switch_workspace(&mut self) {
        self.close_workspace();
    }

    // AppBackend::init_workspace_from_github
    pub(crate) fn init_workspace_from_github(&mut self) {
        if let Some(path) = FileDialog::new().pick_folder() {
            self.current_pending_github_init_path = path.to_string_lossy().to_string();
            self.pending_github_init_path_changed();
        }
    }

    // AppBackend::open_workspace_dir
    pub(crate) fn open_workspace_dir(&mut self) {
        let path = self.current_data_root.clone();
        if !path.is_empty() {
            if let Err(e) = crate::platform_utils::open_directory(&path) {
                self.debug_warn("workspace", "open_workspace_dir_failed", &e);
            }
        }
    }
}

/// Normalize a data root path: canonicalize if possible, otherwise try to
/// fix missing leading `/` (e.g. `home/xiwei/...` → `/home/xiwei/...`).
fn normalize_data_root_path(raw: &str) -> String {
    let path = std::path::Path::new(raw);
    if let Ok(canon) = path.canonicalize() {
        return canon.to_string_lossy().to_string();
    }
    // canonicalize failed — try to fix missing leading /
    if !raw.starts_with('/') && raw.contains('/') {
        let fixed = format!("/{}", raw);
        if std::path::Path::new(&fixed).canonicalize().is_ok() {
            return fixed;
        }
        return fixed;
    }
    // Path already starts with / or has no / — return as-is
    raw.to_string()
}
