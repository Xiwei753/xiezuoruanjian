//! # Agent 运行时 QObject 后端
//!
//! Issue #1016：QML 只与本对象交互。会话历史读写走 Core，进程/协议状态由
//! [`AgentSessionManager`] 在后台线程管理。选中 Agent 仍由 `AgentCliBackend`
//! 维护，本对象通过 `set_agent()` 接收同一份选择。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use qmetaobject::prelude::*;

use crate::agent_cli::{AgentInstallation, AgentInventory};
use crate::agent_runtime::{
    bind_current_project, AgentEvent, AgentLaunchContext, AgentSessionManager, EventSink,
    RuntimeEvent, ADAPTER_VERSION,
};
use crate::backend::AppRef;
use writer_core::agent::{AgentConversation, AgentConversationStatus, AgentToolEvent};
use writer_core::writing_stats::{stable_event_id, AiUsageDirection, AiUsageEvent};

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn turn_text_chars(text: &str) -> u32 {
    u32::try_from(text.chars().count()).unwrap_or(u32::MAX)
}

#[allow(non_snake_case)]
#[derive(QObject)]
pub struct AgentRuntimeBackend {
    base: qt_base_class!(trait QObject),

    conversation_json: qt_property!(QString; READ conversation_json NOTIFY conversation_changed),
    conversations_json: qt_property!(QString; READ conversations_json NOTIFY conversations_changed),
    current_conversation_id: qt_property!(QString; READ current_conversation_id NOTIFY conversation_changed),
    current_agent_id: qt_property!(QString; READ current_agent_id NOTIFY agent_changed),
    models_json: qt_property!(QString; READ models_json NOTIFY conversation_changed),
    running: qt_property!(bool; READ running NOTIFY running_changed),
    status_text: qt_property!(QString; READ status_text NOTIFY status_changed),
    last_error: qt_property!(QString; READ last_error NOTIFY last_error_changed),
    permission_json: qt_property!(QString; READ permission_json NOTIFY permission_changed),
    mcp_status_json: qt_property!(QString; READ mcp_status_json NOTIFY mcp_status_changed),

    conversation_changed: qt_signal!(),
    conversations_changed: qt_signal!(),
    agent_changed: qt_signal!(),
    running_changed: qt_signal!(),
    status_changed: qt_signal!(),
    last_error_changed: qt_signal!(),
    permission_changed: qt_signal!(),
    mcp_status_changed: qt_signal!(),

    set_project: qt_method!(fn(&mut self, project_id: QString, project_title: QString)),
    set_agent_catalog: qt_method!(fn(&mut self, inventory_json: QString)),
    set_agent: qt_method!(
        fn(
            &mut self,
            agent_id: QString,
            agent_name: QString,
            launch_path: QString,
            protocol: QString,
            launch_args_json: QString,
        )
    ),
    switch_agent: qt_method!(
        fn(
            &mut self,
            agent_id: QString,
            agent_name: QString,
            launch_path: QString,
            protocol: QString,
            launch_args_json: QString,
        )
    ),
    new_conversation: qt_method!(fn(&mut self)),
    refresh_conversations: qt_method!(fn(&mut self)),
    open_conversation: qt_method!(fn(&mut self, conversation_id: QString)),
    resume_conversation: qt_method!(fn(&mut self, conversation_id: QString)),
    delete_conversation: qt_method!(fn(&mut self, conversation_id: QString)),
    send_message: qt_method!(fn(&mut self, text: QString)),
    cancel_turn: qt_method!(fn(&mut self)),
    choose_model: qt_method!(fn(&mut self, model: QString)),
    respond_to_permission: qt_method!(fn(&mut self, request_id: QString, option_id: QString)),
    clear_error: qt_method!(fn(&mut self)),

    app_ref: AppRef,
    runtime: Arc<Mutex<AgentSessionManager>>,
    /// 每次切换项目/Agent/会话或发送新轮次都递增；异步 shutdown 线程只有
    /// 在仍持有旧 epoch 时才允许关闭运行时，避免误杀随后新建的会话。
    runtime_epoch: Arc<AtomicU64>,
    /// 当前原生运行实例代次；只有最新代次的事件可以改动会话。
    runtime_generation: u64,

    project_id: String,
    project_title: String,
    agent_id: String,
    agent_name: String,
    launch_path: String,
    protocol: String,
    launch_args: Vec<String>,
    /// AgentCliBackend 最近一次扫描结果；恢复历史会话时按真实安装重新解析协议。
    agent_catalog: Vec<AgentInstallation>,
    model: Option<String>,
    available_models: Vec<String>,
    conversation: Option<AgentConversation>,
    conversations: Vec<AgentConversation>,
    active_turn_id: String,
    ignored_turn_id: String,
    /// 流式增量累计到一定数量才落盘一次，避免每个 token 写盘。
    stream_events_since_persist: u32,
    running_value: bool,
    status_text_value: String,
    last_error_value: String,
    permission_json_value: String,
    mcp_status_json_value: String,
}

impl AgentRuntimeBackend {
    pub fn new(app_ref: AppRef) -> Self {
        Self {
            base: Default::default(),
            conversation_json: Default::default(),
            conversations_json: Default::default(),
            current_conversation_id: Default::default(),
            current_agent_id: Default::default(),
            models_json: Default::default(),
            running: Default::default(),
            status_text: Default::default(),
            last_error: Default::default(),
            permission_json: Default::default(),
            mcp_status_json: Default::default(),
            conversation_changed: Default::default(),
            conversations_changed: Default::default(),
            agent_changed: Default::default(),
            running_changed: Default::default(),
            status_changed: Default::default(),
            last_error_changed: Default::default(),
            permission_changed: Default::default(),
            mcp_status_changed: Default::default(),
            set_project: Default::default(),
            set_agent_catalog: Default::default(),
            set_agent: Default::default(),
            switch_agent: Default::default(),
            new_conversation: Default::default(),
            refresh_conversations: Default::default(),
            open_conversation: Default::default(),
            resume_conversation: Default::default(),
            delete_conversation: Default::default(),
            send_message: Default::default(),
            cancel_turn: Default::default(),
            choose_model: Default::default(),
            respond_to_permission: Default::default(),
            clear_error: Default::default(),
            app_ref,
            runtime: Arc::new(Mutex::new(AgentSessionManager::new())),
            runtime_epoch: Arc::new(AtomicU64::new(0)),
            runtime_generation: 0,
            project_id: String::new(),
            project_title: String::new(),
            agent_id: String::new(),
            agent_name: String::new(),
            launch_path: String::new(),
            protocol: String::new(),
            launch_args: Vec::new(),
            agent_catalog: Vec::new(),
            model: None,
            available_models: Vec::new(),
            conversation: None,
            conversations: Vec::new(),
            active_turn_id: String::new(),
            ignored_turn_id: String::new(),
            stream_events_since_persist: 0,
            running_value: false,
            status_text_value: "未开始会话".to_string(),
            last_error_value: String::new(),
            permission_json_value: "null".to_string(),
            mcp_status_json_value: "{}".to_string(),
        }
    }

    fn conversation_json(&self) -> QString {
        serde_json::to_string(&self.conversation)
            .unwrap_or_else(|_| "null".to_string())
            .into()
    }

    fn conversations_json(&self) -> QString {
        serde_json::to_string(&self.conversations)
            .unwrap_or_else(|_| "[]".to_string())
            .into()
    }

    fn current_conversation_id(&self) -> QString {
        self.conversation
            .as_ref()
            .map(|conversation| conversation.id.clone())
            .unwrap_or_default()
            .into()
    }

    fn current_agent_id(&self) -> QString {
        self.agent_id.clone().into()
    }

    fn models_json(&self) -> QString {
        let mut models = self.available_models.clone();
        if let Some(current) = self
            .conversation
            .as_ref()
            .and_then(|conversation| conversation.model.clone())
        {
            if !models.contains(&current) {
                models.push(current);
            }
        }
        serde_json::to_string(&models)
            .unwrap_or_else(|_| "[]".to_string())
            .into()
    }

    fn running(&self) -> bool {
        self.running_value
    }

    fn status_text(&self) -> QString {
        self.status_text_value.clone().into()
    }

    fn last_error(&self) -> QString {
        self.last_error_value.clone().into()
    }

    fn permission_json(&self) -> QString {
        self.permission_json_value.clone().into()
    }

    fn mcp_status_json(&self) -> QString {
        self.mcp_status_json_value.clone().into()
    }

    fn core_api(&self) -> Option<writer_core::api::service::WriterCoreApi> {
        self.app_ref.with_app(|app| app.core_api()).ok().flatten()
    }

    fn set_error(&mut self, message: impl Into<String>) {
        let message = message.into();
        if self.last_error_value == message {
            return;
        }
        self.last_error_value = message;
        self.last_error_changed();
    }

    fn set_status(&mut self, message: impl Into<String>) {
        let message = message.into();
        if self.status_text_value == message {
            return;
        }
        self.status_text_value = message;
        self.status_changed();
    }

    fn set_running(&mut self, running: bool, status: &str) {
        if self.running_value != running {
            self.running_value = running;
            self.running_changed();
        }
        self.set_status(status);
    }

    fn emit_conversation(&mut self) {
        self.conversation_changed();
    }

    fn refresh_conversations_list(&mut self) {
        let Some(api) = self.core_api() else {
            return;
        };
        match api.list_agent_conversations(Some(self.project_id.as_str())) {
            Ok(conversations) => {
                self.conversations = conversations;
                self.conversations_changed();
            }
            Err(error) => self.set_error(format!("读取 AI 会话列表失败：{error}")),
        }
    }

    fn update_mcp_status(&mut self) {
        let Some((app_data_root, projects_root, project_id)) = self
            .app_ref
            .with_app(|app| app.agent_project_context())
            .ok()
            .flatten()
        else {
            self.mcp_status_json_value = serde_json::json!({
                "status": "没有已打开的作品",
                "available": false
            })
            .to_string();
            self.mcp_status_changed();
            return;
        };
        let project_id = if !self.project_id.is_empty()
            && !self.project_id.starts_with("starmap:")
        {
            self.project_id.clone()
        } else {
            project_id
        };
        let binding = bind_current_project(&app_data_root, &projects_root, &project_id);
        let working_dir = if project_id.is_empty() {
            PathBuf::from(&projects_root)
        } else {
            PathBuf::from(&projects_root).join(&project_id)
        };
        self.mcp_status_json_value = serde_json::json!({
            "status": binding.status,
            "available": binding.launch.is_some(),
            "command": binding.command,
            "projectId": project_id,
            "workingDir": working_dir.to_string_lossy(),
        })
        .to_string();
        self.mcp_status_changed();
    }

    fn launch_context(&self) -> Result<AgentLaunchContext, String> {
        if self.agent_id.is_empty() || self.launch_path.is_empty() {
            return Err("请先在 AI 面板中选择本机 Agent".to_string());
        }
        let conversation_id = self
            .conversation
            .as_ref()
            .map(|conversation| conversation.id.clone())
            .ok_or_else(|| "AI 会话尚未创建".to_string())?;
        let (app_data_root, projects_root, actual_project_id) = self
            .app_ref
            .with_app(|app| app.agent_project_context())
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "请先打开工作区".to_string())?;
        // 绑定作品的星图会把真实 projectId 作为上下文 key；未绑定的星图使用
        // `starmap:<id>`，此时工作目录退回当前选中作品/工作区根目录。
        let workspace_project_id = if !self.project_id.is_empty()
            && !self.project_id.starts_with("starmap:")
        {
            self.project_id.clone()
        } else {
            actual_project_id.clone()
        };
        let binding = bind_current_project(&app_data_root, &projects_root, &workspace_project_id);
        let mcp_servers = binding.launch.clone().into_iter().collect();
        let working_dir = if workspace_project_id.is_empty() {
            PathBuf::from(&projects_root)
        } else {
            PathBuf::from(&projects_root).join(&workspace_project_id)
        };
        let conversation_project_id = if self.project_id.is_empty() {
            actual_project_id.clone()
        } else {
            self.project_id.clone()
        };
        Ok(AgentLaunchContext {
            agent_id: self.agent_id.clone(),
            agent_name: self.agent_name.clone(),
            protocol: self.protocol.clone(),
            launch_path: self.launch_path.clone(),
            launch_args: self.launch_args.clone(),
            working_dir,
            app_data_root: PathBuf::from(app_data_root),
            projects_root: PathBuf::from(projects_root),
            mcp_socket_dir: None,
            conversation_id,
            project_id: conversation_project_id,
            project_title: self.project_title.clone(),
            resume_native_session_id: self
                .conversation
                .as_ref()
                .and_then(|conversation| conversation.native_session_id.clone()),
            model: self.model.clone(),
            mcp_servers,
        })
    }

    fn ensure_conversation(&mut self) -> Result<AgentConversation, String> {
        if let Some(conversation) = self.conversation.clone() {
            return Ok(conversation);
        }
        let Some(api) = self.core_api() else {
            return Err("当前工作区不可用".to_string());
        };
        let conversation = api
            .create_agent_conversation(
                &self.project_id,
                &self.agent_id,
                &self.agent_name,
                &self.launch_path,
                &self.protocol,
                &self.launch_args,
                ADAPTER_VERSION,
                "新会话",
                self.model.clone(),
                now_ms(),
            )
            .map_err(|error| format!("创建 AI 会话失败：{error}"))?;
        self.conversation = Some(conversation.clone());
        self.emit_conversation();
        self.refresh_conversations_list();
        Ok(conversation)
    }

    fn record_usage(&self, conversation: &AgentConversation, turn_id: &str, response: bool) {
        let Some(api) = self.core_api() else {
            return;
        };
        let chars = if response {
            conversation
                .turns
                .iter()
                .find(|turn| turn.id == turn_id)
                .map(writer_core::agent::AgentTurn::assistant_char_count)
                .unwrap_or(0)
        } else {
            conversation
                .turns
                .iter()
                .find(|turn| turn.id == turn_id)
                .map(|turn| turn_text_chars(&turn.user_message))
                .unwrap_or(0)
        };
        let direction = if response {
            AiUsageDirection::Output
        } else {
            AiUsageDirection::Input
        };
        let event_id = stable_event_id(&conversation.id, turn_id, direction);
        let event = AiUsageEvent::new(
            event_id,
            &conversation.project_id,
            now_ms(),
            direction,
            chars,
        )
        .with_conversation_turn(&conversation.id, turn_id);
        if let Err(error) = api.record_ai_usage_event(event) {
            log::warn!("[agent] 记录 AI 使用事件失败: {error}");
        }
    }

    fn set_agent_catalog(&mut self, inventory_json: QString) {
        let text = inventory_json.to_string();
        if let Ok(inventory) = serde_json::from_str::<AgentInventory>(&text) {
            self.agent_catalog = inventory.installed;
        }
    }

    /// 历史会话只能按“同一 Agent ID + 同一真实启动路径”恢复协议描述。
    /// 找不到时返回 `None`，由调用方明确提示用户重新选择，而不是沿用上一个
    /// Agent 的参数。
    fn resolve_descriptor(
        &self,
        agent_id: &str,
        launch_path: &str,
    ) -> Option<(String, Vec<String>, String)> {
        let installation = self.agent_catalog.iter().find(|installation| {
            installation.descriptor.id == agent_id
                && installation.candidates.iter().any(|candidate| {
                    candidate.executable.launch_path == launch_path
                        || candidate.executable.canonical_path.as_deref() == Some(launch_path)
                })
        })?;
        let descriptor = &installation.descriptor;
        Some((
            descriptor.protocol.clone(),
            descriptor.launch.args.clone(),
            descriptor.name.clone(),
        ))
    }

    fn set_project(&mut self, project_id: QString, project_title: QString) {
        let project_id = project_id.to_string();
        let project_title = project_title.to_string();
        if self.project_id == project_id {
            self.project_title = project_title;
            return;
        }
        if self.running_value {
            self.cancel_turn();
        }
        self.cancel_runtime_only();
        self.project_id = project_id;
        self.project_title = project_title;
        self.conversation = None;
        self.available_models.clear();
        self.active_turn_id.clear();
        self.ignored_turn_id.clear();
        self.set_running(false, "未开始会话");
        self.permission_json_value = "null".to_string();
        self.permission_changed();
        self.emit_conversation();
        self.update_mcp_status();
        self.refresh_conversations_list();
    }

    fn cancel_runtime_only(&self) {
        // 关闭进程可能等待子进程退出，不能在 UI 线程同步做；epoch 用来防止
        // 延迟执行的 shutdown 把用户随后启动的新会话一并关掉。
        let epoch = self.runtime_epoch.fetch_add(1, Ordering::SeqCst) + 1;
        let runtime = self.runtime.clone();
        let runtime_epoch = self.runtime_epoch.clone();
        std::thread::Builder::new()
            .name("agent-runtime-shutdown".to_string())
            .spawn(move || {
                if runtime_epoch.load(Ordering::SeqCst) != epoch {
                    return;
                }
                if let Ok(mut manager) = runtime.lock() {
                    if runtime_epoch.load(Ordering::SeqCst) == epoch {
                        manager.shutdown();
                    }
                }
            })
            .ok();
    }

    fn set_agent(
        &mut self,
        agent_id: QString,
        agent_name: QString,
        launch_path: QString,
        protocol: QString,
        launch_args_json: QString,
    ) {
        let agent_id = agent_id.to_string();
        let launch_path = launch_path.to_string();
        let launch_args: Vec<String> =
            serde_json::from_str(&launch_args_json.to_string()).unwrap_or_default();
        if self.agent_id == agent_id
            && self.launch_path == launch_path
            && self.launch_args == launch_args
        {
            return;
        }
        if self.running_value {
            self.cancel_turn();
        }
        self.cancel_runtime_only();
        self.agent_id = agent_id;
        self.agent_name = agent_name.to_string();
        self.launch_path = launch_path;
        self.protocol = protocol.to_string();
        self.launch_args = launch_args;
        // 不同 Agent 的原生 session 身份不可混用；保留旧会话历史，但下一轮
        // 从新的空会话开始。
        if self
            .conversation
            .as_ref()
            .is_some_and(|conversation| conversation.agent_id != self.agent_id)
        {
            self.conversation = None;
            self.available_models.clear();
            self.active_turn_id.clear();
            self.emit_conversation();
        }
        self.agent_changed();
        if self.agent_id.is_empty() {
            self.set_status("请在设置或 AI 面板中选择 Agent");
        } else {
            self.set_status(format!("已选择 {}", self.agent_name));
        }
    }

    #[allow(dead_code)]
    fn switch_agent(
        &mut self,
        agent_id: QString,
        agent_name: QString,
        launch_path: QString,
        protocol: QString,
        launch_args_json: QString,
    ) {
        self.set_agent(agent_id, agent_name, launch_path, protocol, launch_args_json);
    }

    fn new_conversation(&mut self) {
        if self.running_value {
            self.cancel_turn();
        }
        // 新会话不能复用上一段原生 session；同时换掉运行中的进程。
        self.cancel_runtime_only();
        self.conversation = None;
        self.available_models.clear();
        self.active_turn_id.clear();
        self.ignored_turn_id.clear();
        self.permission_json_value = "null".to_string();
        self.permission_changed();
        self.emit_conversation();
        self.set_running(false, "新会话");
    }

    fn refresh_conversations(&mut self) {
        self.refresh_conversations_list();
    }

    fn open_conversation(&mut self, conversation_id: QString) {
        let conversation_id = conversation_id.to_string();
        let Some(api) = self.core_api() else {
            self.set_error("当前工作区不可用");
            return;
        };
        match api.load_agent_conversation(&conversation_id) {
            Ok(Some(mut conversation)) => {
                if conversation.project_id != self.project_id {
                    self.set_error("该会话不属于当前作品");
                    return;
                }
                if conversation.status == AgentConversationStatus::Running {
                    conversation.status = AgentConversationStatus::Idle;
                    let _ = api.save_agent_conversation(&conversation);
                }
                if self.running_value {
                    self.cancel_turn();
                }
                self.cancel_runtime_only();
                self.agent_id = conversation.agent_id.clone();
                self.agent_name = conversation.agent_name.clone();
                self.launch_path = conversation.launch_path.clone();
                // 先使用会话持久化的启动描述，避免混入上一个 Agent 的参数；
                // 再从当前安装清单按真实路径重新解析并覆盖。
                self.protocol = conversation.agent_protocol.clone();
                self.launch_args = conversation.agent_launch_args.clone();
                if let Some((protocol, launch_args, agent_name)) =
                    self.resolve_descriptor(&conversation.agent_id, &conversation.launch_path)
                {
                    self.protocol = protocol.clone();
                    self.launch_args = launch_args.clone();
                    self.agent_name = agent_name.clone();
                    conversation.agent_protocol = protocol;
                    conversation.agent_launch_args = launch_args;
                    conversation.agent_name = agent_name;
                    conversation.agent_adapter_version = ADAPTER_VERSION;
                    let _ = api.save_agent_conversation(&conversation);
                } else {
                    self.set_error(
                        "该会话使用的 Agent 安装已不存在，请重新选择 Agent 后再发送消息",
                    );
                }
                self.model = conversation.model.clone();
                self.available_models.clear();
                self.conversation = Some(conversation);
                self.active_turn_id.clear();
                self.set_running(false, "已打开会话");
                self.permission_json_value = "null".to_string();
                self.permission_changed();
                self.emit_conversation();
                self.agent_changed();
            }
            Ok(None) => self.set_error("AI 会话不存在"),
            Err(error) => self.set_error(format!("读取 AI 会话失败：{error}")),
        }
    }

    fn resume_conversation(&mut self, conversation_id: QString) {
        self.open_conversation(conversation_id);
        self.set_status("已恢复会话，发送消息时继续使用原生 session");
    }

    fn delete_conversation(&mut self, conversation_id: QString) {
        let conversation_id = conversation_id.to_string();
        let Some(api) = self.core_api() else {
            return;
        };
        match api.delete_agent_conversation(&conversation_id) {
            Ok(()) => {
                if self
                    .conversation
                    .as_ref()
                    .is_some_and(|conversation| conversation.id == conversation_id)
                {
                    self.new_conversation();
                }
                self.refresh_conversations_list();
            }
            Err(error) => self.set_error(format!("删除 AI 会话失败：{error}")),
        }
    }

    fn send_message(&mut self, text: QString) {
        let text = text.to_string();
        if self.running_value {
            self.set_error("当前轮次尚未结束");
            return;
        }
        if text.trim().is_empty() {
            return;
        }
        if self.agent_id.is_empty() || self.launch_path.is_empty() {
            self.set_error("请先在本机 Agent 列表中选择一个 Agent");
            return;
        }
        let mut conversation = match self.ensure_conversation() {
            Ok(conversation) => conversation,
            Err(error) => {
                self.set_error(error);
                return;
            }
        };
        let ctx = match self.launch_context() {
            Ok(ctx) => ctx,
            Err(error) => {
                self.set_error(error);
                return;
            }
        };
        let Some(api) = self.core_api() else {
            self.set_error("当前工作区不可用");
            return;
        };
        // 原生会话身份只在下一轮发送时作为 resume 参数；确保会话文件里的
        // Agent 信息和启动路径与当前选择一致。
        conversation.agent_id = self.agent_id.clone();
        conversation.agent_name = self.agent_name.clone();
        conversation.launch_path = self.launch_path.clone();
        conversation.agent_protocol = self.protocol.clone();
        conversation.agent_launch_args = self.launch_args.clone();
        conversation.agent_adapter_version = ADAPTER_VERSION;
        conversation.model = self.model.clone();
        conversation.begin_turn(text.clone(), now_ms());
        let turn_id_value = conversation
            .turns
            .last()
            .expect("turn just appended")
            .id
            .clone();
        conversation.ensure_title_from_first_turn();
        if let Err(error) = api.save_agent_conversation(&conversation) {
            self.set_error(format!("保存 AI 会话失败：{error}"));
            return;
        }
        self.conversation = Some(conversation);
        self.active_turn_id = turn_id_value.clone();
        self.ignored_turn_id.clear();
        self.stream_events_since_persist = 0;
        self.permission_json_value = "null".to_string();
        self.permission_changed();
        self.set_running(true, "Agent 正在回答…");
        self.set_error("");
        self.emit_conversation();
        self.refresh_conversations_list();

        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |event: RuntimeEvent| {
            if let Some(pinned) = qptr.as_pinned() {
                let mut this = pinned.borrow_mut();
                this.apply_event(event);
            }
        });
        let sink: EventSink = Arc::new(move |event: RuntimeEvent| callback(event));
        let runtime = self.runtime.clone();
        let message = text;
        let failure_conversation_id = self
            .conversation
            .as_ref()
            .map(|conversation| conversation.id.clone())
            .unwrap_or_default();
        // 新轮次已经获准启动，之前排队的 shutdown 必须失效，否则会关掉
        // 本线程即将创建的新原生会话。
        self.runtime_epoch.fetch_add(1, Ordering::SeqCst);
        std::thread::Builder::new()
            .name("agent-runtime-prompt".to_string())
            .spawn(move || {
                let mut failure = None;
                match runtime.lock() {
                    Ok(mut manager) => {
                        if let Err(error) = manager.ensure_session(ctx, sink.clone()) {
                            failure = Some(error);
                        } else if let Err(error) = manager.prompt(&turn_id_value, &message) {
                            failure = Some(error);
                        }
                    }
                    Err(_) => failure = Some("Agent 运行时状态锁已损坏".to_string()),
                }
                if let Some(error) = failure {
                    (sink)(RuntimeEvent {
                        conversation_id: failure_conversation_id,
                        generation: 0,
                        core_turn_id: turn_id_value,
                        native_turn_id: String::new(),
                        event: AgentEvent::TurnFailed { message: error },
                    });
                }
            })
            .ok();
    }

    fn cancel_turn(&mut self) {
        if !self.running_value {
            return;
        }
        let turn_id = self.active_turn_id.clone();
        self.ignored_turn_id = turn_id.clone();
        self.running_value = false;
        self.running_changed();
        self.set_status("已取消");
        let api = self.core_api();
        if let Some(conversation) = self.conversation.as_mut() {
            if let Some(turn) = conversation.turn_mut(&turn_id) {
                turn.cancel(now_ms());
            }
            conversation.status = AgentConversationStatus::Idle;
            conversation.updated_at_ms = now_ms();
            if let Some(api) = api {
                let _ = api.save_agent_conversation(conversation);
            }
        }
        self.emit_conversation();
        let runtime = self.runtime.clone();
        let cancel_turn_id = turn_id;
        std::thread::Builder::new()
            .name("agent-runtime-cancel".to_string())
            .spawn(move || {
                if let Ok(mut manager) = runtime.lock() {
                    let _ = manager.cancel_turn(&cancel_turn_id);
                }
            })
            .ok();
    }

    fn choose_model(&mut self, model: QString) {
        let model = model.to_string();
        self.model = if model.is_empty() { None } else { Some(model) };
        let api = self.core_api();
        if let (Some(conversation), Some(api)) = (self.conversation.as_mut(), api) {
            conversation.model = self.model.clone();
            conversation.updated_at_ms = now_ms();
            if let Err(error) = api.save_agent_conversation(conversation) {
                self.set_error(format!("保存模型选择失败：{error}"));
            }
        }
        self.emit_conversation();
    }

    fn respond_to_permission(&mut self, request_id: QString, option_id: QString) {
        let request_id = request_id.to_string();
        let option_id = option_id.to_string();
        let runtime = self.runtime.clone();
        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(move |message: String| {
            if let Some(pinned) = qptr.as_pinned() {
                let mut this = pinned.borrow_mut();
                if message.is_empty() {
                    this.permission_json_value = "null".to_string();
                    this.permission_changed();
                    this.set_status("已回复权限请求");
                } else {
                    this.set_error(message);
                }
            }
        });
        std::thread::Builder::new()
            .name("agent-runtime-permission".to_string())
            .spawn(move || {
                let message = match runtime.lock() {
                    Ok(mut manager) => manager
                        .respond_permission(&request_id, &option_id)
                        .err()
                        .unwrap_or_default(),
                    Err(_) => "Agent 运行时状态锁已损坏".to_string(),
                };
                callback(message);
            })
            .ok();
    }

    fn clear_error(&mut self) {
        self.set_error("");
    }

    fn apply_event(&mut self, event: RuntimeEvent) {
        let Some(mut conversation) = self.conversation.take() else {
            return;
        };
        // 事件必须属于当前 Core 会话。切换会话后旧进程的迟到事件直接丢弃。
        if !event.conversation_id.is_empty() && event.conversation_id != conversation.id {
            self.conversation = Some(conversation);
            return;
        }
        let session_event = matches!(
            event.event,
            AgentEvent::SessionStarted { .. }
                    | AgentEvent::ModelsAvailable { .. }
                    | AgentEvent::McpConnectionChanged { .. }
        );
        if event.generation != 0 {
            if self.runtime_generation != 0 && event.generation < self.runtime_generation {
                // 代次只会随进程重建单调递增；旧实例的迟到事件即使先于新
                // SessionStarted 到达，也不能再操作当前会话或覆盖 MCP 状态。
                self.conversation = Some(conversation);
                return;
            }
            if session_event {
                self.runtime_generation = event.generation;
            } else if self.runtime_generation != 0
                && event.generation != self.runtime_generation
            {
                self.conversation = Some(conversation);
                return;
            }
        }
        let core_turn_id = event.core_turn_id.clone();
        if core_turn_id.is_empty()
            && matches!(
                event.event,
                AgentEvent::TurnCompleted | AgentEvent::TurnFailed { .. }
            )
        {
            // 没有活动轮次时（例如 OpenCode 恢复会话后的空闲通知）不能把
            // 界面从“未开始”改成“回答完成”。
            self.conversation = Some(conversation);
            return;
        }
        if !core_turn_id.is_empty() {
            let belongs_to_active = core_turn_id == self.active_turn_id
                && core_turn_id != self.ignored_turn_id;
            let still_running = conversation
                .turn_mut(&core_turn_id)
                .is_some_and(|turn| turn.status == writer_core::agent::AgentTurnStatus::Running);
            if !belongs_to_active || !still_running {
                // 完成/取消后的迟到增量、以及旧轮次事件不能修改当前会话。
                self.conversation = Some(conversation);
                return;
            }
        }
        let mut persist = false;
        let mut finished: Option<(bool, String)> = None;
        let mut permission: Option<(String, String, Vec<serde_json::Value>)> = None;
        match event.event {
            AgentEvent::SessionStarted {
                native_session_id,
                model,
            } => {
                conversation.native_session_id = Some(native_session_id);
                if model.is_some() {
                    self.model = model.clone();
                    conversation.model = model;
                }
                conversation.updated_at_ms = now_ms();
                persist = true;
                // 原生会话建立只代表 Agent 已连接；MCP 是否真正连上必须等
                // 适配器通过官方接口上报 McpConnectionChanged。
            }
            AgentEvent::McpConnectionChanged {
                server,
                status,
                error,
                tools,
            } => {
                let mut mcp = serde_json::from_str::<serde_json::Value>(
                    &self.mcp_status_json_value,
                )
                .unwrap_or_else(|_| serde_json::json!({}));
                let connected = status == "connected";
                let display = if connected {
                    if tools.is_empty() {
                        format!("{} 已连接", server)
                    } else {
                        format!("{} 已连接（{} 个工具）", server, tools.len())
                    }
                } else if let Some(error) = error.as_ref() {
                    format!("{} 连接失败：{}", server, error)
                } else if status == "configured" {
                    format!("{} 已配置，等待连接", server)
                } else {
                    format!("{}：{}", server, status)
                };
                mcp["connected"] = serde_json::Value::Bool(connected);
                mcp["status"] = serde_json::Value::String(display);
                mcp["server"] = serde_json::Value::String(server);
                mcp["serverState"] = serde_json::Value::String(status);
                mcp["tools"] = serde_json::json!(tools);
                mcp["error"] = error
                    .map(serde_json::Value::String)
                    .unwrap_or(serde_json::Value::Null);
                self.mcp_status_json_value = mcp.to_string();
                self.mcp_status_changed();
            }
            AgentEvent::PromptAccepted => {
                // 只有 Agent 正式接口确认接收后才记 Input；启动失败、握手
                // 失败或 HTTP 拒绝都不会走到这里。
                self.record_usage(&conversation, &core_turn_id, false);
            }
            AgentEvent::TextDelta { text } => {
                if let Some(turn) = conversation.turn_mut(&core_turn_id) {
                    turn.append_text(&text);
                }
                self.stream_events_since_persist =
                    self.stream_events_since_persist.saturating_add(1);
                if self.stream_events_since_persist >= 24 {
                    persist = true;
                    self.stream_events_since_persist = 0;
                }
            }
            AgentEvent::ReasoningDelta { text } => {
                if let Some(turn) = conversation.turn_mut(&core_turn_id) {
                    turn.append_reasoning(&text);
                }
                self.stream_events_since_persist =
                    self.stream_events_since_persist.saturating_add(1);
                if self.stream_events_since_persist >= 24 {
                    persist = true;
                    self.stream_events_since_persist = 0;
                }
            }
            AgentEvent::ToolStarted {
                tool_call_id,
                name,
                input_json,
            } => {
                if let Some(turn) = conversation.turn_mut(&core_turn_id) {
                    if let Some(existing) = turn
                        .tool_events
                        .iter_mut()
                        .find(|existing| existing.tool_call_id == tool_call_id)
                    {
                        existing.name = name;
                        if input_json.is_some() {
                            existing.input_json = input_json;
                        }
                        if existing.status == "completed" || existing.status == "failed" {
                            // 迟到的 running 更新不能把已完成的工具改回运行中。
                        } else {
                            existing.status = "running".to_string();
                        }
                    } else {
                        turn.tool_events.push(AgentToolEvent {
                            tool_call_id,
                            name,
                            input_json,
                            output_json: None,
                            status: "running".to_string(),
                            started_at_ms: now_ms(),
                            finished_at_ms: None,
                        });
                    }
                }
            }
            AgentEvent::ToolFinished {
                tool_call_id,
                name,
                output_json,
                success,
            } => {
                if let Some(turn) = conversation.turn_mut(&core_turn_id) {
                    if let Some(existing) = turn
                        .tool_events
                        .iter_mut()
                        .find(|existing| existing.tool_call_id == tool_call_id)
                    {
                        existing.name = name;
                        existing.output_json = output_json;
                        existing.status = if success {
                            "completed".to_string()
                        } else {
                            "failed".to_string()
                        };
                        existing.finished_at_ms = Some(now_ms());
                    } else {
                        turn.tool_events.push(AgentToolEvent {
                            tool_call_id,
                            name,
                            input_json: None,
                            output_json,
                            status: if success {
                                "completed".to_string()
                            } else {
                                "failed".to_string()
                            },
                            started_at_ms: now_ms(),
                            finished_at_ms: Some(now_ms()),
                        });
                    }
                }
            }
            AgentEvent::PermissionRequested {
                request_id,
                description,
                options,
            } => {
                let options = options
                    .into_iter()
                    .map(|option| {
                        serde_json::json!({
                            "optionId": option.option_id,
                            "name": option.name,
                            "kind": option.kind,
                        })
                    })
                    .collect();
                permission = Some((request_id, description, options));
            }
            AgentEvent::ModelsAvailable { models } => {
                self.available_models = models;
            }
            AgentEvent::Usage { .. } => {
                // 真实使用量在轮次完成时按消息字符数记录，避免各协议 token
                // 计数单位不同导致统计口径漂移。
            }
            AgentEvent::TurnCompleted => {
                if let Some(turn) = conversation.turn_mut(&core_turn_id) {
                    turn.complete(now_ms());
                }
                conversation.status = AgentConversationStatus::Idle;
                conversation.updated_at_ms = now_ms();
                persist = true;
                finished = Some((false, String::new()));
            }
            AgentEvent::TurnFailed { message } => {
                if let Some(turn) = conversation.turn_mut(&core_turn_id) {
                    turn.fail(now_ms(), message.clone());
                }
                conversation.status = AgentConversationStatus::Failed;
                conversation.updated_at_ms = now_ms();
                persist = true;
                finished = Some((true, message));
            }
        }
        self.conversation = Some(conversation);
        if persist {
            let api = self.core_api();
            if let Some(api) = api {
                if let Some(conversation) = self.conversation.as_ref() {
                    if let Err(error) = api.save_agent_conversation(conversation) {
                        log::warn!("[agent] 保存会话失败: {error}");
                    }
                }
            }
        }
        if let Some((failed, message)) = finished {
            let api = self.core_api();
            if let (Some(api), Some(conversation)) = (api, self.conversation.as_ref()) {
                let chars = conversation
                    .turns
                    .iter()
                    .find(|turn| turn.id == core_turn_id)
                    .map(writer_core::agent::AgentTurn::assistant_char_count)
                    .unwrap_or(0);
                let usage = AiUsageEvent::new(
                    stable_event_id(&conversation.id, &core_turn_id, AiUsageDirection::Output),
                    &conversation.project_id,
                    now_ms(),
                    AiUsageDirection::Output,
                    chars,
                )
                .with_conversation_turn(&conversation.id, &core_turn_id);
                if let Err(error) = api.record_ai_usage_event(usage) {
                    log::warn!("[agent] 记录 AI 输出事件失败: {error}");
                }
            }
            self.active_turn_id.clear();
            if failed {
                self.set_running(false, "回答失败");
                self.set_error(message);
            } else {
                self.set_running(false, "回答完成");
            }
            self.permission_json_value = "null".to_string();
            self.permission_changed();
            self.refresh_conversations_list();
        }
        if let Some((request_id, description, options)) = permission {
            self.permission_json_value = serde_json::json!({
                "requestId": request_id,
                "description": description,
                "options": options,
            })
            .to_string();
            self.permission_changed();
            self.set_status("Agent 等待权限确认");
        }
        self.emit_conversation();
    }
}

impl Default for AgentRuntimeBackend {
    fn default() -> Self {
        Self::new(AppRef::default())
    }
}
