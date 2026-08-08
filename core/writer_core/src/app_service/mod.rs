mod editor_session_ops;
mod project_ops;
mod search_ops;
mod settings_ops;
mod starmap_ops;
mod stats_ops;
mod sync_ops;
mod text_edit_session_ops;
mod theme_ops;
mod volume_chapter_ops;

use crate::api::{WriterCoreApi, WriterError};

use std::sync::Mutex;

struct EditorSession {
    kernel: crate::editor::EditorKernel,
    chapter_id: Option<String>,
    generation: u64,
}

/// Thin UniFFI adapter. Stable Core API behavior lives in `api::WriterCoreApi`.
///
/// ## 线程安全
///
/// `editor_session` 和 `session_registry` 各自用 `Mutex` 保护，保证线程安全。
/// `Mutex` 只在单次 FFI 调用期间持有，不跨调用持有，避免死锁。
/// **不得在持有 `editor_session` 锁的同时获取 `session_registry` 锁**（锁序：先 session 后 registry）。
///
/// ## 双会话路径
///
/// `editor_session` 是旧版正文章节专用路径（单 EditorKernel，单 generation），
/// `session_registry` 是新版多目标会话路径（项目名/章节名/星图标题/正文等，各自独立 EditorKernel 和 generation）。
/// 两者独立维护，不共享 EditorKernel 实例。同一时刻同一章节只能通过一条路径访问。
///
/// ## 平台初始化
///
/// `WriterAppService` 持有平台初始化上下文 `platform_init`，
/// 由平台适配层在启动时注入，Core 不再自行猜测平台目录。
///
/// ## 平台能力注入
///
/// - `secure_storage`：安全存储（令牌、凭据），由平台端注入 Keychain/Keystore 实现
/// - `network_state`：网络状态（联网、代理、计费），由平台端注入系统网络信息
/// - `sync_transport_factory`：同步传输工厂，由平台端注入 HTTP 客户端实现
///
/// 同步操作优先使用 `SecureStorage` 获取 token，不再将凭据作为普通 JSON 存盘。
pub struct WriterAppService {
    api: WriterCoreApi,
    editor_session: Mutex<EditorSession>,
    session_registry: Mutex<crate::editor::TextEditSessionRegistry>,
    platform_init: Option<writer_platform_api::PlatformInit>,
    network_state: Mutex<Option<writer_platform_api::NetworkState>>,
}

impl WriterAppService {
    pub fn new(app_data_root: String, projects_root: String) -> Self {
        Self {
            api: WriterCoreApi::new(app_data_root, projects_root),
            editor_session: Mutex::new(EditorSession {
                kernel: crate::editor::EditorKernel::new(),
                chapter_id: None,
                generation: 0,
            }),
            session_registry: Mutex::new(crate::editor::TextEditSessionRegistry::new()),
            platform_init: None,
            network_state: Mutex::new(None),
        }
    }

    pub fn with_platform_init(
        app_data_root: String,
        projects_root: String,
        init: writer_platform_api::PlatformInit,
    ) -> Self {
        Self {
            api: WriterCoreApi::new(app_data_root, projects_root),
            editor_session: Mutex::new(EditorSession {
                kernel: crate::editor::EditorKernel::new(),
                chapter_id: None,
                generation: 0,
            }),
            session_registry: Mutex::new(crate::editor::TextEditSessionRegistry::new()),
            platform_init: Some(init),
            network_state: Mutex::new(None),
        }
    }

    pub fn with_platform_services(
        app_data_root: String,
        projects_root: String,
        services: writer_platform_api::PlatformServices,
    ) -> Self {
        let secure_storage_arc: Option<std::sync::Arc<dyn writer_platform_api::SecureStorage>> =
            services.secure_storage.map(std::sync::Arc::from);
        let api = WriterCoreApi::with_platform_services(
            &app_data_root,
            &projects_root,
            services.sync_transport_factory,
            secure_storage_arc,
        );

        if let Some(config_store) = services.config_store {
            crate::app_config::set_default_config_store(config_store);
        }

        Self {
            api,
            editor_session: Mutex::new(EditorSession {
                kernel: crate::editor::EditorKernel::new(),
                chapter_id: None,
                generation: 0,
            }),
            session_registry: Mutex::new(crate::editor::TextEditSessionRegistry::new()),
            platform_init: Some(services.init),
            network_state: Mutex::new(services.network_state),
        }
    }

    pub fn update_network_state(
        &self,
        is_connected: bool,
        is_metered: bool,
        proxy_host: Option<String>,
        proxy_port: Option<u16>,
    ) {
        if let Ok(mut state) = self.network_state.lock() {
            *state = Some(writer_platform_api::NetworkState {
                is_connected,
                is_metered,
                proxy_host,
                proxy_port,
            });
        }
    }

    pub fn set_network_state(&self, state: writer_platform_api::NetworkState) {
        if let Ok(mut guard) = self.network_state.lock() {
            *guard = Some(state);
        }
    }

    pub fn set_platform_init(&mut self, init: writer_platform_api::PlatformInit) {
        self.platform_init = Some(init);
    }

    pub fn set_sync_transport_factory(
        &mut self,
        factory: writer_platform_api::SyncTransportFactory,
    ) {
        let app_data_root = self.api.app_data_root.clone();
        let projects_root = self.api.projects_root.clone();
        self.api = WriterCoreApi::with_sync_transport(app_data_root, projects_root, factory);
    }

    pub fn sync_transport_factory(&self) -> Option<&writer_platform_api::SyncTransportFactory> {
        self.api.sync_transport.as_ref()
    }

    pub fn network_state(&self) -> Option<writer_platform_api::NetworkState> {
        self.network_state.lock().ok().and_then(|g| g.clone())
    }

    pub fn platform_init(&self) -> Option<&writer_platform_api::PlatformInit> {
        self.platform_init.as_ref()
    }

    pub fn platform_paths(&self) -> Option<writer_platform_api::PlatformPaths> {
        self.platform_init.as_ref().map(|init| init.paths())
    }

    pub fn device_id(&self) -> Option<&str> {
        self.platform_init
            .as_ref()
            .map(|init| init.device_id.as_str())
    }

    pub fn secure_storage_available(&self) -> bool {
        self.api.secure_storage.is_some()
    }

    pub fn secure_storage_get(&self, key: String) -> Result<Option<Vec<u8>>, WriterError> {
        if let Some(storage) = &self.api.secure_storage {
            storage.get_secret(&key).map_err(WriterError::Other)
        } else {
            Err(WriterError::Other(
                "Secure storage not available".to_string(),
            ))
        }
    }

    pub fn secure_storage_set(&self, key: String, value: Vec<u8>) -> Result<(), WriterError> {
        if let Some(storage) = &self.api.secure_storage {
            storage.set_secret(&key, &value).map_err(WriterError::Other)
        } else {
            Err(WriterError::Other(
                "Secure storage not available".to_string(),
            ))
        }
    }

    pub fn secure_storage_delete(&self, key: String) -> Result<(), WriterError> {
        if let Some(storage) = &self.api.secure_storage {
            storage.delete_secret(&key).map_err(WriterError::Other)
        } else {
            Err(WriterError::Other(
                "Secure storage not available".to_string(),
            ))
        }
    }

    // ── Actions ──

    pub fn list_registered_actions(
        &self,
    ) -> Result<Vec<crate::api::types::ActionDescriptorDto>, WriterError> {
        self.api.list_registered_actions()
    }

    pub fn execute_action(
        &self,
        action_id: String,
        args_json: String,
        context_json: String,
    ) -> Result<crate::api::types::ActionResultDto, WriterError> {
        self.api
            .execute_action_ext(&action_id, &args_json, &context_json)
    }

    pub fn ai_available(&self) -> bool {
        self.api.ai_available()
    }

    // ── Layout Policy ──

    pub fn resolve_layout(
        &self,
        metrics: crate::api::WindowMetricsDto,
    ) -> crate::api::LayoutPlanDto {
        let core_metrics: crate::layout_policy::WindowMetrics = metrics.into();
        let plan = crate::layout_policy::resolve_layout(&core_metrics);
        plan.into()
    }

    // ── Screen Policy ──

    pub fn resolve_screen_policy(
        &self,
        screen_role: crate::api::ScreenRoleDto,
        shell_mode: crate::api::ShellModeDto,
    ) -> crate::api::ScreenPolicyDto {
        let core_role: crate::screen_policy::ScreenRole = screen_role.into();
        let core_mode: crate::layout_policy::ShellMode = shell_mode.into();
        let action_slots = crate::screen_policy::resolve_screen_policy(core_role, core_mode);
        crate::api::ScreenPolicyDto {
            screen_role: core_role.into(),
            action_slots: action_slots.into_iter().map(Into::into).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn editor_kernel_load_text_rejects_invalid_offset() {
        let dir = tempfile::TempDir::new().unwrap();
        let svc = WriterAppService::new(
            dir.path().to_string_lossy().to_string(),
            dir.path().join("projects").to_string_lossy().to_string(),
        );
        let result = svc.editor_kernel_load_text("你好".to_string(), 4);
        assert_eq!(
            result.outcome,
            crate::api::EditorEditOutcomeDto::InvalidOffset
        );
    }

    #[test]
    fn editor_kernel_load_text_accepts_valid_offset() {
        let dir = tempfile::TempDir::new().unwrap();
        let svc = WriterAppService::new(
            dir.path().to_string_lossy().to_string(),
            dir.path().join("projects").to_string_lossy().to_string(),
        );
        let result = svc.editor_kernel_load_text("你好".to_string(), 3);
        assert_eq!(result.outcome, crate::api::EditorEditOutcomeDto::Applied);
    }

    #[test]
    fn text_edit_session_load_text_rejects_invalid_offset() {
        let dir = tempfile::TempDir::new().unwrap();
        let svc = WriterAppService::new(
            dir.path().to_string_lossy().to_string(),
            dir.path().join("projects").to_string_lossy().to_string(),
        );
        let session_id = svc
            .text_edit_session_create("test".to_string(), String::new(), 0, 0)
            .unwrap();
        let result = svc.text_edit_session_load_text(session_id, "你好".to_string(), 4);
        assert_eq!(
            result.outcome,
            crate::api::EditorEditOutcomeDto::InvalidOffset
        );
    }

    #[test]
    fn text_edit_session_reset_rejects_invalid_offset() {
        let dir = tempfile::TempDir::new().unwrap();
        let svc = WriterAppService::new(
            dir.path().to_string_lossy().to_string(),
            dir.path().join("projects").to_string_lossy().to_string(),
        );
        let session_id = svc
            .text_edit_session_create("test".to_string(), String::new(), 0, 0)
            .unwrap();
        let result = svc.text_edit_session_reset(session_id, "你好".to_string(), 4);
        assert_eq!(result, 0);
    }

    #[test]
    fn editor_kernel_update_composition_accepts_preedit_utf16_cursor_offset() {
        let dir = tempfile::TempDir::new().unwrap();
        let svc = WriterAppService::new(
            dir.path().to_string_lossy().to_string(),
            dir.path().join("projects").to_string_lossy().to_string(),
        );
        let load = svc.editor_kernel_load_text(String::new(), 0);
        assert_eq!(load.outcome, crate::api::EditorEditOutcomeDto::Applied);
        let begin = svc.editor_kernel_begin_composition(0, 0, load.new_revision);
        let session = begin.composition_session.expect("composition session");
        let result = svc.editor_kernel_update_composition(
            session.session_id,
            session.generation,
            "你好".to_string(),
            2,
            load.new_revision,
        );
        assert_eq!(result.outcome, crate::api::EditorEditOutcomeDto::Applied);
    }

    #[test]
    fn editor_kernel_update_composition_rejects_cursor_beyond_preedit() {
        let dir = tempfile::TempDir::new().unwrap();
        let svc = WriterAppService::new(
            dir.path().to_string_lossy().to_string(),
            dir.path().join("projects").to_string_lossy().to_string(),
        );
        let load = svc.editor_kernel_load_text(String::new(), 0);
        assert_eq!(load.outcome, crate::api::EditorEditOutcomeDto::Applied);
        let begin = svc.editor_kernel_begin_composition(0, 0, load.new_revision);
        let session = begin.composition_session.expect("composition session");
        let result = svc.editor_kernel_update_composition(
            session.session_id,
            session.generation,
            "你好".to_string(),
            3,
            load.new_revision,
        );
        assert_eq!(
            result.outcome,
            crate::api::EditorEditOutcomeDto::InvalidOffset
        );
    }

    #[test]
    fn test_global_app_service_apis() {
        let dir = tempdir().unwrap();
        let path_str = dir.path().to_string_lossy().into_owned();
        let projects_root = dir.path().join("projects");
        std::fs::create_dir_all(&projects_root).unwrap();

        let projects = crate::project::list_projects(&projects_root).unwrap();
        assert_eq!(projects.len(), 0);

        // Manually create a project for the rest of the test
        crate::project::create_project(&projects_root, "测试作品").unwrap();
        let projects = crate::project::list_projects(&projects_root).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].title, "测试作品");

        // open_app_service should succeed and return WriterAppService
        let projects_root_str = dir.path().join("projects").to_string_lossy().to_string();
        let service = crate::open_app_service(path_str.clone(), projects_root_str).unwrap();
        let service_projects = service.list_projects().unwrap();
        assert_eq!(service_projects.len(), 1);
    }

    #[test]
    fn test_create_chapter_in_project() {
        let dir = tempdir().unwrap();
        let path_str = dir.path().to_string_lossy().into_owned();
        let projects_root = dir.path().join("projects");
        std::fs::create_dir_all(&projects_root).unwrap();

        let project = crate::project::create_project(&projects_root, "测试作品").unwrap();

        let projects_root_str = dir.path().join("projects").to_string_lossy().to_string();
        let service = crate::open_app_service(path_str.clone(), projects_root_str).unwrap();
        let chapter = service
            .create_chapter_in_project(project.id.clone(), "新章：起锚".to_string())
            .unwrap();
        assert_eq!(chapter.title, "新章：起锚");

        let volumes = service.list_volumes(project.id.clone()).unwrap();
        let chapters = service
            .list_chapters(project.id.clone(), volumes[0].id.clone())
            .unwrap();
        assert_eq!(chapters.len(), 1);
        assert_eq!(chapters[0].title, "新章：起锚");
    }

    #[test]
    fn sync_secrets_override_set_and_clear_cycle() {
        // #595 十：set → has_override=true；clear → has_override=false。
        // 清除后 refresh_secrets_override 才会从磁盘重新填充，陈旧凭据不会泄漏。
        let dir = tempfile::TempDir::new().unwrap();
        let svc = WriterAppService::new(
            dir.path().to_string_lossy().to_string(),
            dir.path().join("projects").to_string_lossy().to_string(),
        );
        let secrets = crate::api::SyncSecretsDto {
            token: Some("token-a".to_string()),
        };
        svc.set_sync_secrets_override(secrets)
            .expect("set override");
        assert!(svc.api.core().has_secrets_override());
        svc.clear_sync_secrets_override().expect("clear override");
        assert!(!svc.api.core().has_secrets_override());
    }
}
