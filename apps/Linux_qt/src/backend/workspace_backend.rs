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
    workspace_opened: qt_signal!(),
    workspace_content_changed: qt_signal!(),
    workspace_state_changed: qt_signal!(),
    try_restore_last_workspace: qt_method!(fn(&mut self)),
    create_new_workspace: qt_method!(fn(&mut self) -> QJsonObject),
    open_existing_workspace: qt_method!(fn(&mut self) -> QJsonObject),
    create_workspace_with_path: qt_method!(fn(&mut self, path: QString) -> QJsonObject),
    open_workspace_with_path: qt_method!(fn(&mut self, path: QString) -> QJsonObject),
    close_workspace: qt_method!(fn(&mut self)),
    switch_workspace: qt_method!(fn(&mut self)),
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
    /// 真正打开/恢复工作区时调用：发全部三个信号让 QML 初始化工作区 UI。
    fn emit_workspace_opened(&mut self) {
        self.workspace_opened();
        self.workspace_content_changed();
        self.workspace_state_changed();
    }
    /// 关闭/切换工作区时调用：只发 workspace_state_changed，不发 workspace_opened，
    /// 避免 QML 误认为工作区已打开而触发 workspace-open 自动同步等副作用。
    fn emit_workspace_closed(&mut self) {
        self.workspace_state_changed();
    }
    fn workspace_path(&self) -> QString {
        self.with_app(|app| app.workspace_path())
            .unwrap_or_else(|_| crate::backend::json_utils::borrow_conflict_error_json().into())
    }
    fn has_workspace(&self) -> bool {
        self.snap().has_workspace
    }
    fn try_restore_last_workspace(&mut self) {
        let restored = self
            .with_app_mut(|app| app.try_restore_last_workspace())
            .unwrap_or(false);
        if restored {
            // 真的恢复成功，发 workspace_opened 等信号
            self.emit_workspace_opened();
        }
        // 无可恢复工作区时，AppBackend 内部已发 workspace_state_changed 等信号，
        // 此处不再发 workspace_opened，避免 QML 去读未初始化的 workspace。
    }
    fn create_new_workspace(&mut self) -> QJsonObject {
        // Issue #729 评论 5765306162 问题3：用 workspace_generation 变化判断本次
        // 是否真正打开了新工作区，而非 result.is_ok() && snap().has_workspace。
        // 若原本就有工作区，用户取消选择器时 AppBackend 返回 CANCELLED，
        // has_workspace 仍 true，会误发 workspace_opened。generation 只有在
        // internal_open_data_root 成功时才递增，取消时不变。
        let gen_before = self.with_app(|app| app.workspace_generation()).unwrap_or(0);
        let result = self.with_app_mut(|app| app.create_new_workspace());
        let gen_after = self.with_app(|app| app.workspace_generation()).unwrap_or(0);
        if gen_before != gen_after && self.snap().has_workspace {
            self.emit_workspace_opened();
        }
        let res = result.unwrap_or_else(|_| backend_link_broken_json());
        qjson_object_from_json(&res.to_string())
    }
    fn open_existing_workspace(&mut self) -> QJsonObject {
        // Issue #729 评论 5765306162 问题3：同 create_new_workspace，用 generation
        // 变化判断本次是否真正打开了新工作区。
        let gen_before = self.with_app(|app| app.workspace_generation()).unwrap_or(0);
        let result = self.with_app_mut(|app| app.open_existing_workspace());
        let gen_after = self.with_app(|app| app.workspace_generation()).unwrap_or(0);
        if gen_before != gen_after && self.snap().has_workspace {
            self.emit_workspace_opened();
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
        // Issue #729 评论 5765306162 问题3：用 generation 变化判断真实打开成功，
        // 与 create_new_workspace/open_existing_workspace 一致。
        let gen_before = self.with_app(|app| app.workspace_generation()).unwrap_or(0);
        let result = self.with_app_mut(|app| app.internal_open_data_root(&path_str));
        let gen_after = self.with_app(|app| app.workspace_generation()).unwrap_or(0);
        if gen_before != gen_after && self.snap().has_workspace {
            self.emit_workspace_opened();
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
        // Issue #729 评论 5765306162 问题3：用 generation 变化判断真实打开成功。
        let gen_before = self.with_app(|app| app.workspace_generation()).unwrap_or(0);
        let result = self.with_app_mut(|app| app.internal_open_data_root(&path_str));
        let gen_after = self.with_app(|app| app.workspace_generation()).unwrap_or(0);
        if gen_before != gen_after && self.snap().has_workspace {
            self.emit_workspace_opened();
        }
        let res = result.unwrap_or_else(|_| backend_link_broken_json());
        qjson_object_from_json(&res.to_string())
    }
    fn close_workspace(&mut self) {
        if self.with_app_mut(|app| app.close_workspace()).is_ok() {
            self.emit_workspace_closed();
        }
    }
    fn switch_workspace(&mut self) {
        if self.with_app_mut(|app| app.switch_workspace()).is_ok() {
            self.emit_workspace_closed();
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

    // AppBackend::workspace_generation
    //
    // Issue #729 评论 5765306162 问题3：暴露 current_workspace_generation 给
    // WorkspaceBackend wrapper，用 generation 变化判断本次 create/open 是否真正
    // 打开了新工作区，避免用户取消选择器时误发 workspace_opened。
    pub(crate) fn workspace_generation(&self) -> u64 {
        self.current_workspace_generation
    }

    // AppBackend::try_restore_last_workspace
    //
    // 启动时尝试恢复上次打开的工作区（数据根）。
    // 读取 AppConfig 持久化的 last_workspace_path：
    //   - 路径存在且目录仍在 → 走 internal_open_data_root 恢复，返回 true 表示恢复成功。
    //   - 路径为 None 或目录已不存在 → 回到"未选择工作区"，返回 false。
    // 返回值供 WorkspaceBackend 包装层决定是否发 workspace_opened 信号。
    pub(crate) fn try_restore_last_workspace(&mut self) -> bool {
        self.debug_log("workspace", "try_restore_last_workspace_start", "");
        let saved_path = writer_core::app_config::get_last_workspace_path();
        if let Some(path) = saved_path {
            if std::path::Path::new(&path).exists() {
                self.debug_log(
                    "workspace",
                    "try_restore_last_workspace_found",
                    &format!("path={}", path),
                );
                let _ = self.internal_open_data_root(&path);
                // internal_open_data_root 成功时设置 current_has_data_root=true；
                // bootstrap 失败时保持 false。
                return self.current_has_data_root;
            }
            self.debug_log(
                "workspace",
                "try_restore_last_workspace_path_missing",
                &format!("saved path no longer exists: {}", path),
            );
        } else {
            self.debug_log(
                "workspace",
                "try_restore_last_workspace_no_saved_path",
                "no last_workspace_path saved; waiting for user selection",
            );
        }
        // 无可恢复工作区：回到未选择工作区状态
        self.current_has_data_root = false;
        self.current_sync_status = "no_workspace".to_string();
        // Load app-level theme mode even without data root
        self.load_app_theme_mode();
        false
    }

    // AppBackend::internal_open_data_root
    //
    // 打开用户选择的数据根目录。设置 app_data_root = path, projects_root = path/projects。
    // 调用 Core 统一 workspace bootstrap 确保 .git 存在、恢复未完成删除事务。
    pub(crate) fn internal_open_data_root(&mut self, path: &str) -> QString {
        let canonical_path = normalize_data_root_path(path);
        let path = canonical_path.as_str();
        self.debug_log(
            "workspace",
            "internal_open_data_root_start",
            &format!("path={}", path),
        );

        // Issue #729 评论 5764768372：开头先重置 current_has_data_root = false，
        // 成功路径才设 true。调用后读 current_has_data_root 能准确判断本次是否成功。
        // 本函数不再自己发 workspace_opened/content/state 信号，由调用方根据
        // current_has_data_root（即 snap().has_workspace）判断真实成功后发一次。
        self.current_has_data_root = false;

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

        // 调用 Core 统一 workspace bootstrap：确保 .git 存在、恢复未完成删除事务、
        // 构造已 bootstrap 的 WriterCoreApi。不再裸构造未 bootstrap 的 API。
        // bootstrap 只在打开/切换 workspace 时调用一次，成功后保存 layout 快照。
        let (api, layout) = match crate::backend::app_backend::create_core_api_with_layout(
            path,
            &projects_root_str,
        ) {
            Ok((api, layout)) => (api, layout),
            Err(e) => {
                let err_msg = format!("workspace bootstrap 失败: {}", e);
                self.set_error(&err_msg);
                self.debug_error(
                    "workspace",
                    "internal_open_data_root_bootstrap_failed",
                    &err_msg,
                );
                return crate::backend::json_utils::envelope_error_json(
                    writer_core::api::WriterError::Other(err_msg),
                )
                .into();
            }
        };

        // bootstrap 成功后再设置 current_data_root/current_projects_root
        self.current_data_root = path.to_string();
        self.current_projects_root = projects_root_str.clone();
        self.current_has_data_root = true;
        // Issue #729：递增 workspace generation，使任何正在运行的旧同步回调失效。
        // 旧同步回调捕获的是旧 generation，回调时校验不匹配会丢弃结果。
        self.current_workspace_generation = self.current_workspace_generation.wrapping_add(1);
        // 保存 layout 快照，供普通 core_api() getter 和后台同步线程使用。
        self.current_workspace_git_layout = Some(layout);
        self.current_save_status = "已保存".to_string();
        self.reload_tree();
        self.load_sync_config();
        self.load_local_settings();

        // bootstrap 成功且数据根状态已设置，持久化 last_workspace_path 供下次启动自动恢复。
        // 失败时不写 path，保留旧值（切换工作区失败时旧 path 不被覆盖）。
        if let Err(e) = writer_core::app_config::save_last_workspace_path(path) {
            self.debug_warn("workspace", "save_last_workspace_path_failed", &e);
        }

        // 写入 current_device.json 设备信息
        if let Err(e) = api.ensure_device_info("desktop", "desktop") {
            self.debug_log("workspace", "ensure_device_info_failed", &format!("{}", e));
        }

        // Issue #729 评论 5764768372：不再在此发 workspace_opened/content/state 信号。
        // 由调用方（WorkspaceBackend wrapper 或 sync_operations）根据
        // current_has_data_root 判断真实成功后发一次，避免重复发射。

        self.debug_log(
            "workspace",
            "internal_open_data_root_success",
            &format!("path={}, projects_root={}", path, projects_root_str),
        );

        workspace_success_json("OK")
    }

    /// Issue #707 评论 5724685300: 测试专用 — 打开 data root 初始化真实 Core 状态。
    ///
    /// 委托给 `internal_open_data_root`，用 `#[cfg(test)]` gate 确保只在
    /// 测试构建中可用。集成测试通过 `AppRef::with_app_mut` 调用此方法
    /// 初始化真实 Core/AppBackend 状态，使 `core_api()` 返回 `Some`，
    /// 从而让 `LinuxThemeController::rebuild_resolved_state()` 能加载
    /// builtin theme 的真实深色/浅色 scheme。
    #[cfg(any(test, feature = "test-helpers"))]
    pub fn open_data_root_for_tests(&mut self, path: &str) -> QString {
        self.internal_open_data_root(path)
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

    // AppBackend::reset_workspace_state
    //
    // 关闭/切换工作区的共享内部逻辑：清数据根状态、清选区、清树、重置同步状态、清编辑器、发信号。
    // 不清 last_workspace_path，由调用方（close_workspace / switch_workspace）决定是否清。
    fn reset_workspace_state(&mut self) {
        // Issue #729：切工作区/关闭工作区时，先取消正在运行的同步并使其回调失效。
        // a. 取消当前同步令牌：标记旧同步已取消（平台层持有，core sync 本轮不检查，
        //    但 cancel 是同步语义的一部分，未来 core sync 可集成 is_cancelled 提前终止）。
        if let Some(token) = self.current_sync_cancel_token.take() {
            token.cancel();
        }
        // b. 递增 workspace generation：使旧同步回调的 generation 校验不匹配而被丢弃。
        //    即使旧同步线程仍在运行，其回调进入 handle_sync_outcome 时会被 generation 拦截。
        self.current_workspace_generation = self.current_workspace_generation.wrapping_add(1);
        // c. 清 in_progress：旧同步不再算作进行中，新工作区的 single-flight 不会被旧同步卡住。
        self.current_sync_in_progress = false;

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
        // Clear editor
        // Emit signals
    }

    // AppBackend::close_workspace
    //
    // 显式关闭工作区：清除持久化的 last_workspace_path，避免下次启动自动恢复到已关闭的工作区。
    pub(crate) fn close_workspace(&mut self) {
        self.debug_log("workspace", "close_workspace_start", "");
        if let Err(e) = writer_core::app_config::clear_last_workspace_path() {
            self.debug_warn("workspace", "clear_last_workspace_path_failed", &e);
        }
        self.reset_workspace_state();
        self.debug_log("workspace", "close_workspace_success", "");
    }

    // AppBackend::switch_workspace
    //
    // 切换工作区：不清除 last_workspace_path，保留旧 path。
    // 用户选择新目录成功后 internal_open_data_root 会覆盖 path；
    // 失败或取消时旧 path 保留，下次启动仍可恢复。
    pub(crate) fn switch_workspace(&mut self) {
        self.debug_log("workspace", "switch_workspace_start", "");
        self.reset_workspace_state();
        self.debug_log("workspace", "switch_workspace_success", "");
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
