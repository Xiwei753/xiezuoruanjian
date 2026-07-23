mod project_ops;
mod volume_chapter_ops;
mod settings_ops;
mod sync_ops;
mod stats_ops;
mod theme_ops;
mod starmap_ops;
mod editor_session_ops;
mod text_edit_session_ops;

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
pub struct WriterAppService {
    api: WriterCoreApi,
    editor_session: Mutex<EditorSession>,
    session_registry: Mutex<crate::editor::TextEditSessionRegistry>,
    platform_init: Option<writer_platform_api::PlatformInit>,
}

impl WriterAppService {
    pub fn new(workspace_path: String) -> Self {
        Self {
            api: WriterCoreApi::new(workspace_path),
            editor_session: Mutex::new(EditorSession {
                kernel: crate::editor::EditorKernel::new(),
                chapter_id: None,
                generation: 0,
            }),
            session_registry: Mutex::new(crate::editor::TextEditSessionRegistry::new()),
            platform_init: None,
        }
    }

    pub fn with_platform_init(workspace_path: String, init: writer_platform_api::PlatformInit) -> Self {
        Self {
            api: WriterCoreApi::new(workspace_path),
            editor_session: Mutex::new(EditorSession {
                kernel: crate::editor::EditorKernel::new(),
                chapter_id: None,
                generation: 0,
            }),
            session_registry: Mutex::new(crate::editor::TextEditSessionRegistry::new()),
            platform_init: Some(init),
        }
    }

    pub fn platform_init(&self) -> Option<&writer_platform_api::PlatformInit> {
        self.platform_init.as_ref()
    }

    pub fn platform_paths(&self) -> Option<writer_platform_api::PlatformPaths> {
        self.platform_init.as_ref().map(|init| init.paths())
    }

    pub fn device_id(&self) -> Option<&str> {
        self.platform_init.as_ref().map(|init| init.device_id.as_str())
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
