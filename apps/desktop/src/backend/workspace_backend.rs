// =============================================================================
// workspace_backend.rs — 工作区领域 QObject 后端适配层
// =============================================================================
//
// 引用了什么：
// - super::*：引入 AppBackend 核心后端的全部方法与结构体。
// - crate::backend::SafeAppPtr：用于安全访问全局 AppBackend 指针以读取/更新工作区状态。
//
// 干什么的：
// - 实现 WorkspaceBackend 结构体，作为 QML 中 "workspaceBackend" 对象的桥梁。
// - 提供工作区的全生命周期交互，包含自动恢复上次工作区、唤起 RFD 文件夹框选择路径新建/打开工作区等。
// - 支持从 GitHub 克隆（init_workspace_from_github & execute_github_init）拉取已有工作区至本地，并在异步线程中进行安全性操作。
// - 负责向上层 QML 主页提供当前工作区路径（workspace_path）和是否已加载工作区（has_workspace）属性，以驱动界面渲染。
//
// 被什么引用：
// - 被 apps/desktop/src/backend/mod.rs 引用，用于实例化工作区后端并绑定为 QML 全局上下文属性。
// =============================================================================

use super::*;
use crate::backend::SafeAppPtr;

use super::super::json_utils::qjson_object_from_json;
use qmetaobject::QJsonObject;

#[path = "github_init_operations.rs"]
mod github_init_operations;

fn backend_link_broken_json() -> QString {
    crate::backend::json_utils::envelope_error_json(writer_core::api::WriterError::Other(
        "底层链接断开，请重启应用".to_string(),
    ))
    .into()
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
    clear_last_workspace: qt_method!(fn(&mut self)),
    switch_workspace: qt_method!(fn(&mut self)),
    init_workspace_from_github: qt_method!(fn(&mut self)),
    execute_github_init: qt_method!(
        fn(&mut self, path: QString, remote_url: QString, branch: QString, token: QString)
    ),
    get_workspace_diagnostics: qt_method!(fn(&self) -> QString),
    open_workspace_dir: qt_method!(fn(&mut self)),
    save_last_navigation_state: qt_method!(fn(&mut self, route: QString, project_id: QString, volume_id: QString, chapter_id: QString, starmap_id: QString)),
    get_last_navigation_state: qt_method!(fn(&self) -> QJsonObject),
    clear_last_navigation_state: qt_method!(fn(&mut self)),
    app: SafeAppPtr,
}

impl WorkspaceBackend {
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
                "workspace",
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
                "workspace",
                "BACKEND_LINK_BROKEN",
                "app pointer is null",
            );
            default
        }
    }
    fn emit_workspace_changed(&mut self) {
        self.workspace_opened();
        self.workspace_content_changed();
        self.workspace_state_changed();
    }
    fn workspace_path(&self) -> QString {
        self.with_app("".into(), |app| app.workspace_path())
    }
    fn has_workspace(&self) -> bool {
        self.with_app(false, |app| app.has_workspace())
    }
    fn pending_github_init_path(&self) -> QString {
        self.with_app("".into(), |app| app.pending_github_init_path())
    }
    fn try_restore_last_workspace(&mut self) {
        self.with_app_mut((), |app| app.try_restore_last_workspace());
        self.emit_workspace_changed();
    }
    fn create_new_workspace(&mut self) -> QJsonObject {
        let res = self.with_app_mut(backend_link_broken_json(), |app| app.create_new_workspace());
        self.emit_workspace_changed();
        qjson_object_from_json(&res.to_string())
    }
    fn open_existing_workspace(&mut self) -> QJsonObject {
        let res = self.with_app_mut(backend_link_broken_json(), |app| {
            app.open_existing_workspace()
        });
        self.emit_workspace_changed();
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
        let res = self.with_app_mut(backend_link_broken_json(), |app| {
            app.internal_open_workspace(&path_str, true)
        });
        let has = self.has_workspace();
        crate::backend::app_backend::debug_log_static(
            "workspace",
            "writer_core_create_workspace_result",
            &format!("success={}", has),
        );
        self.emit_workspace_changed();
        crate::backend::app_backend::debug_log_static(
            "workspace",
            "workspace_state_updated",
            &format!("has_workspace={}", has),
        );
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
        let res = self.with_app_mut(backend_link_broken_json(), |app| {
            app.internal_open_workspace(&path_str, false)
        });
        let has = self.has_workspace();
        crate::backend::app_backend::debug_log_static(
            "workspace",
            "writer_core_open_workspace_result",
            &format!("success={}", has),
        );
        self.emit_workspace_changed();
        crate::backend::app_backend::debug_log_static(
            "workspace",
            "workspace_state_updated",
            &format!("has_workspace={}", has),
        );
        qjson_object_from_json(&res.to_string())
    }
    fn close_workspace(&mut self) {
        self.with_app_mut((), |app| app.close_workspace());
        self.emit_workspace_changed();
    }
    fn clear_last_workspace(&mut self) {
        self.with_app_mut((), |app| app.clear_last_workspace());
        self.workspace_state_changed();
    }
    fn switch_workspace(&mut self) {
        self.with_app_mut((), |app| app.switch_workspace());
        self.emit_workspace_changed();
    }
    fn init_workspace_from_github(&mut self) {
        self.with_app_mut((), |app| app.init_workspace_from_github());
        self.pending_github_init_path_changed();
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
            &format!("path={}, url={}", path.to_string(), remote_url.to_string()),
        );
        crate::backend::app_backend::debug_log_static(
            "workspace",
            "workspace_backend_import_workspace_called",
            &format!("path={}", path.to_string()),
        );
        self.with_app_mut((), |app| {
            app.execute_github_init(path, remote_url, branch, token)
        });
        self.emit_workspace_changed();
        let has = self.has_workspace();
        crate::backend::app_backend::debug_log_static(
            "workspace",
            "workspace_state_updated",
            &format!("has_workspace={}", has),
        );
        self.pending_github_init_path_changed();
    }
    fn get_workspace_diagnostics(&self) -> QString {
        self.with_app(backend_link_broken_json(), |app| {
            app.get_workspace_diagnostics()
        })
    }
    fn open_workspace_dir(&mut self) {
        self.with_app_mut((), |app| app.open_workspace_dir());
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
    // Deprecated compatibility methods for this Linux backend domain.

    // AppBackend::has_workspace
    pub(crate) fn has_workspace(&self) -> bool {
        self.current_has_workspace
    }

    // AppBackend::pending_github_init_path
    pub(crate) fn pending_github_init_path(&self) -> QString {
        self.current_pending_github_init_path.clone().into()
    }

    // AppBackend::get_workspace_diagnostics
    pub(crate) fn get_workspace_diagnostics(&self) -> QString {
        let api = WriterCoreApi::new(&self.current_workspace);
        match api
            .get_workspace_diagnostics(self.current_has_workspace, self.cached_tree.len() as u64)
        {
            Ok(diagnostics) => crate::backend::json_utils::envelope_ok_json(diagnostics),
            Err(e) => crate::backend::json_utils::envelope_error_json(e),
        }
        .into()
    }

    // AppBackend::try_restore_last_workspace
    pub(crate) fn try_restore_last_workspace(&mut self) {
        self.debug_log("workspace", "try_restore_last_workspace_start", "");
        if let Some(path) = writer_core::app_config::get_last_workspace_path() {
            self.debug_log(
                "workspace",
                "try_restore_last_workspace_path_found",
                &format!("path={}", path),
            );
            let api = WriterCoreApi::new(&path);
            let val_res = api.validate_workspace().unwrap_or(false);
            self.debug_log(
                "workspace",
                "try_restore_last_workspace_validate",
                &format!("path={}, is_valid={}", path, val_res),
            );
            if val_res {
                self.current_workspace = path.clone();
                self.current_has_workspace = true;
                self.current_save_status = "已保存".to_string();
                self.save_status_changed();
                self.reload_tree();
                self.load_sync_config();
                self.load_local_settings();
                self.ai_available_changed();
                self.workspace_opened();
                self.workspace_content_changed();
                self.workspace_state_changed();
                self.debug_log(
                    "workspace",
                    "try_restore_last_workspace_success",
                    &format!("path={}", path),
                );
                return;
            }
            // Restore failed: clear the invalid lastWorkspacePath to avoid being stuck
            self.debug_warn(
                "workspace",
                "try_restore_last_workspace_failed_clearing",
                &format!("path={}", path),
            );
            let _ = writer_core::app_config::clear_last_workspace_path();
        } else {
            self.debug_log("workspace", "try_restore_last_workspace_no_path", "");
        }
        // No valid workspace to restore
        self.current_has_workspace = false;
        self.current_sync_status = "not_configured".to_string();
        self.sync_status_changed();
        self.workspace_state_changed();
        // Load app-level theme mode even without workspace
        self.load_app_theme_mode();
        self.ai_available_changed();
    }

    // AppBackend::internal_open_workspace
    pub(crate) fn internal_open_workspace(&mut self, path: &str, initialize: bool) -> QString {
        self.debug_log(
            "workspace",
            "internal_open_workspace_start",
            &format!("path={}, initialize={}", path, initialize),
        );
        let api = WriterCoreApi::new(path);
        let is_valid = api.validate_workspace().unwrap_or(false);
        self.debug_log(
            "workspace",
            "internal_open_workspace_validate",
            &format!("path={}, is_valid={}", path, is_valid),
        );

        if !is_valid && !initialize {
            let err_msg = "不是有效工作区。请选择其他目录，或使用「新建工作区」初始化该目录。";
            self.set_error(err_msg);
            self.debug_error("workspace", "internal_open_workspace_failed", err_msg);
            return crate::backend::json_utils::envelope_error_json(
                writer_core::api::WriterError::InvalidWorkspace,
            )
            .into();
        }

        if !is_valid && initialize {
            self.debug_log("workspace", "internal_open_workspace_creating", path);
            if let Err(e) = api.create_workspace_if_needed() {
                let err_msg = format!("无法创建工作区: {}", e);
                self.set_error(&err_msg);
                self.debug_error("workspace", "internal_open_workspace_failed", &err_msg);
                return crate::backend::json_utils::envelope_error_json(e).into();
            }
        }

        let val_res = api.validate_workspace().unwrap_or(false);
        self.debug_log(
            "workspace",
            "internal_open_workspace_revalidate",
            &format!("path={}, is_valid={}", path, val_res),
        );
        if !val_res {
            let err_msg = "工作区验证失败";
            self.set_error(err_msg);
            self.debug_error("workspace", "internal_open_workspace_failed", err_msg);
            return crate::backend::json_utils::envelope_error_json(
                writer_core::api::WriterError::InvalidWorkspace,
            )
            .into();
        }

        self.current_workspace = path.to_string();
        self.current_has_workspace = true;
        self.current_save_status = "已保存".to_string();
        self.save_status_changed();
        self.reload_tree();
        self.load_sync_config();
        self.load_local_settings();
        self.workspace_opened();
        self.workspace_content_changed();
        self.workspace_state_changed();

        let _ = writer_core::app_config::set_last_workspace_path(path);
        self.debug_log(
            "workspace",
            "internal_open_workspace_success",
            &format!("path={}", path),
        );

        workspace_success_json("OK")
    }

    // AppBackend::create_new_workspace
    pub(crate) fn create_new_workspace(&mut self) -> QString {
        self.debug_log("workspace", "create_new_workspace_clicked", "");
        if let Some(path) = FileDialog::new().pick_folder() {
            self.internal_open_workspace(&path.to_string_lossy(), true)
        } else {
            self.debug_log("workspace", "create_new_workspace_cancelled", "");
            workspace_success_json("CANCELLED")
        }
    }

    // AppBackend::open_existing_workspace
    pub(crate) fn open_existing_workspace(&mut self) -> QString {
        self.debug_log("workspace", "open_existing_workspace_clicked", "");
        if let Some(path) = FileDialog::new().pick_folder() {
            self.internal_open_workspace(&path.to_string_lossy(), false)
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
        // Clear workspace state
        self.current_workspace = "".to_string();
        self.current_has_workspace = false;
        // Clear selection state
        self.selected_project_id = None;
        self.selected_volume_id = None;
        self.selected_chapter_id = None;
        // Clear tree
        self.cached_tree = QJsonArray::default();
        // Reset sync status
        self.current_sync_status = "not_configured".to_string();
        // Reset save status
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

    // AppBackend::clear_last_workspace
    pub(crate) fn clear_last_workspace(&mut self) {
        let _ = writer_core::app_config::clear_last_workspace_path();
    }

    // AppBackend::switch_workspace
    pub(crate) fn switch_workspace(&mut self) {
        self.close_workspace();
        self.clear_last_workspace();
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
        let path = self.current_workspace.clone();
        if !path.is_empty() {
            let _ = std::process::Command::new("xdg-open")
                .arg("--")
                .arg(&path)
                .spawn();
        }
    }
}
