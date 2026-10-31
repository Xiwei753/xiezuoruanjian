//! # Agent 运行时 QObject 后端
//!
//! Issue #1016：QML 只与本对象交互。会话历史读写走 Core，进程/协议状态由
//! [`AgentSessionManager`] 在后台线程管理。选中 Agent 仍由 `AgentCliBackend`
//! 维护，本对象通过 `set_agent()` 接收同一份选择。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use qmetaobject::prelude::*;

use crate::agent_cli::{AgentInstallation, AgentInventory};
use crate::agent_runtime::events::executor_identity;
use crate::agent_runtime::model_catalog::ModelProbeResult;
use crate::agent_runtime::{
    agent_working_dir, bind_current_project, AgentEvent, AgentHistoryMessage, AgentLaunchContext,
    AgentModelInfo, AgentPromptRequest, AgentSessionManager, CancelOutcome, CloseReason, EventSink,
    McpServerLaunch, RuntimeEvent, ADAPTER_VERSION,
};
use crate::backend::AppRef;
use writer_core::agent::{
    AgentConversation, AgentConversationStatus, AgentMessagePart, AgentToolEvent, AgentTurnStatus,
};
use writer_core::writing_stats::{stable_event_id, AiUsageDirection, AiUsageEvent};

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn turn_text_chars(text: &str) -> u32 {
    u32::try_from(text.chars().count()).unwrap_or(u32::MAX)
}

fn assistant_visible_text(turn: &writer_core::agent::AgentTurn) -> String {
    turn.assistant_parts
        .iter()
        .filter_map(|part| match part {
            AgentMessagePart::Text { text } => Some(text.as_str()),
            AgentMessagePart::Reasoning { .. } => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

/// 取出已完成轮次中真实的用户/助手消息。`after_turn_id` 是上一轮已经
/// 同步进当前原生会话的位置；只有原生恢复失败时才从零开始补全。
fn history_messages(
    conversation: &AgentConversation,
    after_turn_id: Option<&str>,
) -> Vec<AgentHistoryMessage> {
    let start = after_turn_id
        .and_then(|turn_id| {
            conversation
                .turns
                .iter()
                .position(|turn| turn.id == turn_id)
        })
        .map(|index| index + 1)
        .unwrap_or(0);
    let mut messages = Vec::new();
    for turn in conversation.turns.iter().skip(start) {
        if turn.status != AgentTurnStatus::Completed || turn.user_message.trim().is_empty() {
            continue;
        }
        let assistant = assistant_visible_text(turn);
        if assistant.trim().is_empty() {
            continue;
        }
        messages.push(AgentHistoryMessage {
            role: "user".to_string(),
            text: turn.user_message.clone(),
        });
        messages.push(AgentHistoryMessage {
            role: "assistant".to_string(),
            text: assistant,
        });
    }
    messages
}

#[allow(non_snake_case)]
#[derive(QObject)]
pub struct AgentRuntimeBackend {
    base: qt_base_class!(trait QObject),

    conversation_json: qt_property!(QString; READ conversation_json NOTIFY conversation_changed),
    conversations_json: qt_property!(QString; READ conversations_json NOTIFY conversations_changed),
    current_conversation_id: qt_property!(QString; READ current_conversation_id NOTIFY conversation_changed),
    current_agent_id: qt_property!(QString; READ current_agent_id NOTIFY agent_changed),
    /// 当前 Agent 的显式模型选择；新会话尚未写入 conversation.model 时也有效。
    current_model: qt_property!(QString; READ current_model NOTIFY models_changed),
    /// 真实可选模型列表。只由 Agent 协议返回，不伪造静态表。
    models_json: qt_property!(QString; READ models_json NOTIFY models_changed),
    /// `none` / `loading` / `ready` / `default` / `unsupported` / `error`。
    models_state: qt_property!(QString; READ models_state NOTIFY models_changed),
    /// 模型目录探测或模型切换的真实失败原因；成功时为空。
    models_error: qt_property!(QString; READ models_error NOTIFY models_changed),
    running: qt_property!(bool; READ running NOTIFY running_changed),
    status_text: qt_property!(QString; READ status_text NOTIFY status_changed),
    last_error: qt_property!(QString; READ last_error NOTIFY last_error_changed),
    permission_json: qt_property!(QString; READ permission_json NOTIFY permission_changed),
    mcp_status_json: qt_property!(QString; READ mcp_status_json NOTIFY mcp_status_changed),

    conversation_changed: qt_signal!(),
    conversations_changed: qt_signal!(),
    /// 用户显式打开历史会话后发出；界面只在这时把 CLI 选择同步为会话
    /// 最后执行者，日常切换 CLI 不得被反向拉回。
    conversation_opened: qt_signal!(),
    agent_changed: qt_signal!(),
    models_changed: qt_signal!(),
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
    refresh_models: qt_method!(fn(&mut self)),
    respond_to_permission: qt_method!(fn(&mut self, request_id: QString, option_id: QString)),
    copy_mcp_config: qt_method!(fn(&mut self)),
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
    /// 尚无 Core 会话可保存时，也按执行者记住用户刚选的模型。
    model_preferences: HashMap<String, Option<String>>,
    available_models: Vec<AgentModelInfo>,
    /// 按“Agent ID + 真实启动路径”缓存协议返回过的模型目录。切换回来时
    /// 先显示缓存，再在后台刷新，绝不把上一个 Agent 的列表留在界面上。
    model_cache: HashMap<String, Vec<AgentModelInfo>>,
    models_state_value: String,
    models_error_value: String,
    /// 已为哪一份 Agent 选择完成过独立模型目录探测。
    models_probed_for: String,
    /// 只允许最新一次模型探测更新选择器；旧探测的迟到结果直接丢弃。
    model_probe_epoch: Arc<AtomicU64>,
    conversation: Option<AgentConversation>,
    conversations: Vec<AgentConversation>,
    active_turn_id: String,
    ignored_turn_id: String,
    /// 用户已请求停止、正在等待 Agent 原生协议确认的 Core Turn。确认前
    /// 不得把轮次标成 Cancelled，也不得把 running 置 false。
    cancelling_turn_id: String,
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
            current_model: Default::default(),
            models_json: Default::default(),
            models_state: Default::default(),
            models_error: Default::default(),
            running: Default::default(),
            status_text: Default::default(),
            last_error: Default::default(),
            permission_json: Default::default(),
            mcp_status_json: Default::default(),
            conversation_changed: Default::default(),
            conversations_changed: Default::default(),
            conversation_opened: Default::default(),
            agent_changed: Default::default(),
            models_changed: Default::default(),
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
            refresh_models: Default::default(),
            respond_to_permission: Default::default(),
            copy_mcp_config: Default::default(),
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
            model_preferences: HashMap::new(),
            available_models: Vec::new(),
            model_cache: HashMap::new(),
            models_state_value: "none".to_string(),
            models_error_value: String::new(),
            models_probed_for: String::new(),
            model_probe_epoch: Arc::new(AtomicU64::new(0)),
            conversation: None,
            conversations: Vec::new(),
            active_turn_id: String::new(),
            ignored_turn_id: String::new(),
            cancelling_turn_id: String::new(),
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

    fn current_model(&self) -> QString {
        self.current_model_value().unwrap_or_default().into()
    }

    fn current_model_value(&self) -> Option<String> {
        self.model.clone().or_else(|| {
            self.conversation
                .as_ref()
                .filter(|conversation| self.conversation_matches_current_agent(conversation))
                .and_then(|conversation| conversation.model.clone())
        })
    }

    fn conversation_matches_current_agent(&self, conversation: &AgentConversation) -> bool {
        conversation.agent_id == self.agent_id
            && conversation.launch_path == self.launch_path
            && conversation.agent_protocol == self.protocol
            && conversation.agent_launch_args == self.launch_args
    }

    fn current_executor_identity(&self) -> String {
        executor_identity(
            &self.agent_id,
            &self.launch_path,
            &self.protocol,
            &self.launch_args,
        )
    }

    /// 恢复某个 CLI 自己最后一次确认的模型。原生绑定存在时以绑定为准，
    /// 即使值为空也表示用户明确选择了默认模型，不能再拿旧轮次覆盖。
    fn restored_model_for_executor(
        conversation: &AgentConversation,
        agent_id: &str,
        launch_path: &str,
    ) -> Option<String> {
        if let Some(model) = conversation.executor_model(agent_id, launch_path) {
            return model;
        }
        if let Some(binding) = conversation.native_session(agent_id, launch_path) {
            return binding.model.clone();
        }
        if let Some(turn) = conversation
            .turns
            .iter()
            .rev()
            .find(|turn| turn.agent_id == agent_id && turn.launch_path == launch_path)
        {
            return turn.model.clone();
        }
        (conversation.agent_id == agent_id && conversation.launch_path == launch_path)
            .then(|| conversation.model.clone())
            .flatten()
    }

    /// 只恢复当前 CLI 自己的原生会话。旧会话文件没有映射表时，只有会话
    /// 记录的执行者与当前选择完全一致才允许使用旧字段兜底。
    fn resume_session_id_for_current_agent(
        &self,
        conversation: &AgentConversation,
    ) -> Option<String> {
        conversation
            .native_session_for(&self.agent_id, &self.launch_path)
            .map(str::to_string)
            .or_else(|| {
                if self.conversation_matches_current_agent(conversation) {
                    conversation
                        .native_session_id
                        .clone()
                        .filter(|id| !id.is_empty())
                } else {
                    None
                }
            })
    }

    fn models_json(&self) -> QString {
        serde_json::to_string(&self.available_models)
            .unwrap_or_else(|_| "[]".to_string())
            .into()
    }

    fn models_state(&self) -> QString {
        self.models_state_value.clone().into()
    }

    fn models_error(&self) -> QString {
        self.models_error_value.clone().into()
    }

    fn model_cache_key(&self) -> String {
        executor_identity(
            &self.agent_id,
            &self.launch_path,
            &self.protocol,
            &self.launch_args,
        )
    }

    fn set_models_state(&mut self, state: &str) {
        if self.models_state_value == state {
            return;
        }
        self.models_state_value = state.to_string();
        self.models_changed();
    }

    /// 切换 Agent/会话时先清掉上一个 Agent 的模型，再恢复该 Agent 自己的
    /// 缓存；真正的协议目录由 [`Self::start_model_probe`] 在后台拉取。
    fn reset_models_for_agent(&mut self) {
        self.available_models.clear();
        self.models_probed_for.clear();
        self.models_error_value.clear();
        self.model_probe_epoch.fetch_add(1, Ordering::SeqCst);
        if self.agent_id.is_empty() {
            self.models_state_value = "none".to_string();
        } else {
            if let Some(models) = self.model_cache.get(&self.model_cache_key()) {
                self.available_models = models.clone();
            }
            // 没有作品上下文时无法启动探测，先明确显示默认模型；
            // set_project 后会重新进入 loading 并主动探测。
            self.models_state_value = if self.project_id.is_empty() {
                "default".to_string()
            } else {
                "loading".to_string()
            };
        }
        self.models_changed();
    }

    /// 用各家官方机器接口独立查询模型目录。探测不创建 Core 对话、不启动
    /// MCP，也不占用聊天运行时锁；失败只改变选择器状态，不阻塞发送。
    fn start_model_probe(&mut self, refresh: bool) {
        if self.agent_id.is_empty() || self.launch_path.is_empty() {
            return;
        }
        if self.project_id.is_empty() {
            self.set_models_state("default");
            return;
        }
        let key = self.model_cache_key();
        if self.models_probed_for == key && !refresh {
            return;
        }
        self.models_probed_for = key.clone();
        let epoch = self.model_probe_epoch.fetch_add(1, Ordering::SeqCst) + 1;
        self.models_state_value = "loading".to_string();
        self.models_error_value.clear();
        self.models_changed();

        let ctx = match self.model_probe_context() {
            Ok(ctx) => ctx,
            Err(error) => {
                log::warn!("[agent] 无法准备模型目录探测：{error}");
                self.models_error_value = error;
                if self.available_models.is_empty() {
                    self.models_state_value = "error".to_string();
                } else {
                    self.models_state_value = "ready".to_string();
                }
                self.models_changed();
                return;
            }
        };

        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(
            move |(key, epoch, result): (String, u64, Result<ModelProbeResult, String>)| {
                if let Some(pinned) = qptr.as_pinned() {
                    let mut this = pinned.borrow_mut();
                    if this.models_probed_for != key
                        || this.model_probe_epoch.load(Ordering::SeqCst) != epoch
                    {
                        return;
                    }
                    match result {
                        Ok(ModelProbeResult::Models {
                            mut models,
                            default_model,
                        }) => {
                            if let Some(default_model) = default_model.as_ref() {
                                for model in models.iter_mut() {
                                    model.is_default = model.id == *default_model;
                                }
                            }
                            this.models_error_value.clear();
                            if models.is_empty() {
                                this.available_models.clear();
                                this.models_state_value = "default".to_string();
                            } else {
                                this.available_models = models.clone();
                                this.model_cache.insert(key, models);
                                this.models_state_value = "ready".to_string();
                            }
                        }
                        Ok(ModelProbeResult::Unsupported { message }) => {
                            log::warn!("[agent] 该 Agent 不提供模型目录：{message}");
                            this.available_models.clear();
                            this.model_cache.remove(&key);
                            this.models_error_value = message;
                            this.models_state_value = "unsupported".to_string();
                        }
                        Err(error) => {
                            log::warn!("[agent] 模型目录探测失败：{error}");
                            this.models_state_value = if this.available_models.is_empty() {
                                "error".to_string()
                            } else {
                                "ready".to_string()
                            };
                            this.models_error_value = error;
                        }
                    }
                    this.models_changed();
                }
            },
        );
        if let Err(error) = std::thread::Builder::new()
            .name("agent-model-probe".to_string())
            .spawn(move || {
                let result = crate::agent_runtime::model_catalog::query_models(&ctx, refresh);
                callback((key, epoch, result));
            })
        {
            // 线程创建失败也必须结束 loading，不能让选择器永远转圈。
            self.models_error_value = format!("无法启动模型目录探测：{error}");
            self.models_state_value = if self.available_models.is_empty() {
                "error".to_string()
            } else {
                "ready".to_string()
            };
            self.models_changed();
        }
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
        let project_id = if !self.project_id.is_empty() && !self.project_id.starts_with("starmap:")
        {
            self.project_id.clone()
        } else {
            project_id
        };
        let binding = bind_current_project(&app_data_root, &projects_root, &project_id);
        let working_dir = agent_working_dir(Path::new(&app_data_root), &self.agent_id);
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

    /// 复制当前作品可用的 sujian-mcp 接入信息。外部 TUI 各家的 MCP 配置
    /// 外壳不同，这里只提供通用的 `command` / `args` / `env`，由用户按
    /// 对应 TUI 的语法接入；终端工作目录仍是用户 HOME。
    fn copy_mcp_config(&mut self) {
        let Some((app_data_root, projects_root, actual_project_id)) = self
            .app_ref
            .with_app(|app| app.agent_project_context())
            .ok()
            .flatten()
        else {
            self.set_error("没有已打开的作品，无法生成 MCP 配置".to_string());
            return;
        };
        let project_id = if !self.project_id.is_empty() && !self.project_id.starts_with("starmap:")
        {
            self.project_id.clone()
        } else {
            actual_project_id
        };
        let binding = bind_current_project(&app_data_root, &projects_root, &project_id);
        let Some(launch) = binding.launch else {
            self.set_error(binding.status);
            return;
        };
        let env: serde_json::Map<String, serde_json::Value> = launch
            .env
            .iter()
            .map(|(key, value)| (key.clone(), serde_json::Value::String(value.clone())))
            .collect();
        let config = serde_json::json!({
            "command": launch.command,
            "args": launch.args,
            "env": env,
        });
        let text = serde_json::to_string_pretty(&config).unwrap_or_else(|_| "{}".to_string());
        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text)) {
            Ok(_) => {
                self.set_error(String::new());
                self.set_status("MCP 接入信息已复制，请按外部 TUI 的 MCP 配置语法填入");
            }
            Err(error) => self.set_error(format!("复制 MCP 接入信息失败：{error}")),
        }
    }

    fn launch_roots(&self) -> Result<(String, String, String, String), String> {
        if self.agent_id.is_empty() || self.launch_path.is_empty() {
            return Err("请先在 AI 面板中选择本机 Agent".to_string());
        }
        let (app_data_root, projects_root, actual_project_id) = self
            .app_ref
            .with_app(|app| app.agent_project_context())
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "请先打开工作区".to_string())?;
        // 绑定作品的星图会把真实 projectId 作为上下文 key；未绑定的星图使用
        // `starmap:<id>`，此时工作目录退回当前选中作品/工作区根目录。
        let workspace_project_id =
            if !self.project_id.is_empty() && !self.project_id.starts_with("starmap:") {
                self.project_id.clone()
            } else {
                actual_project_id.clone()
            };
        Ok((
            app_data_root,
            projects_root,
            actual_project_id,
            workspace_project_id,
        ))
    }

    fn ensure_working_dir(&self, app_data_root: &str) -> Result<PathBuf, String> {
        let working_dir = agent_working_dir(Path::new(app_data_root), &self.agent_id);
        std::fs::create_dir_all(&working_dir).map_err(|error| {
            format!(
                "无法创建 Agent 独立工作目录 {}：{error}",
                working_dir.display()
            )
        })?;
        Ok(working_dir)
    }

    fn base_launch_context(
        &self,
        app_data_root: String,
        projects_root: String,
        actual_project_id: String,
        conversation_id: String,
        resume_native_session_id: Option<String>,
        model: Option<String>,
        mcp_servers: Vec<McpServerLaunch>,
        working_dir: PathBuf,
    ) -> AgentLaunchContext {
        let conversation_project_id = if self.project_id.is_empty() {
            actual_project_id
        } else {
            self.project_id.clone()
        };
        AgentLaunchContext {
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
            resume_native_session_id,
            model,
            mcp_servers,
        }
    }

    fn launch_context(&self) -> Result<AgentLaunchContext, String> {
        let (conversation_id, resume_native_session_id) = self
            .conversation
            .as_ref()
            .map(|conversation| {
                (
                    conversation.id.clone(),
                    self.resume_session_id_for_current_agent(conversation),
                )
            })
            .ok_or_else(|| "AI 会话尚未创建".to_string())?;
        let (app_data_root, projects_root, actual_project_id, workspace_project_id) =
            self.launch_roots()?;
        let binding = bind_current_project(&app_data_root, &projects_root, &workspace_project_id);
        let mcp_servers = binding.launch.clone().into_iter().collect();
        let working_dir = self.ensure_working_dir(&app_data_root)?;
        Ok(self.base_launch_context(
            app_data_root,
            projects_root,
            actual_project_id,
            conversation_id,
            resume_native_session_id,
            self.model.clone(),
            mcp_servers,
            working_dir,
        ))
    }

    /// 探测上下文没有 Core 会话、原生 resume 身份和 MCP；它只借用当前 CLI
    /// 的安装路径与独立工作目录去查询官方模型目录。
    fn model_probe_context(&self) -> Result<AgentLaunchContext, String> {
        let (app_data_root, projects_root, actual_project_id, _workspace_project_id) =
            self.launch_roots()?;
        let working_dir = self.ensure_working_dir(&app_data_root)?;
        Ok(self.base_launch_context(
            app_data_root,
            projects_root,
            actual_project_id,
            String::new(),
            None,
            None,
            Vec::new(),
            working_dir,
        ))
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
            self.abandon_active_turn();
        }
        self.cancel_runtime_only(CloseReason::UserSwitch("切换作品".to_string()));
        self.project_id = project_id;
        self.project_title = project_title;
        self.conversation = None;
        self.model = None;
        self.reset_models_for_agent();
        self.active_turn_id.clear();
        self.ignored_turn_id.clear();
        self.cancelling_turn_id.clear();
        self.set_running(false, "未开始会话");
        self.permission_json_value = "null".to_string();
        self.permission_changed();
        self.emit_conversation();
        self.update_mcp_status();
        self.refresh_conversations_list();
        self.start_model_probe(false);
    }

    fn cancel_runtime_only(&self, reason: CloseReason) {
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
                        manager.shutdown(reason);
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
        let protocol = protocol.to_string();
        let launch_args: Vec<String> =
            serde_json::from_str(&launch_args_json.to_string()).unwrap_or_default();
        if self.agent_id == agent_id
            && self.launch_path == launch_path
            && self.protocol == protocol
            && self.launch_args == launch_args
        {
            // 同一选择被 QML 重复同步时不再重建进程；但如果模型探测尚未
            // 开始（例如先 set_agent、后 set_project），补一次启动。
            self.start_model_probe(false);
            return;
        }
        if self.running_value {
            self.abandon_active_turn();
        }
        self.cancel_runtime_only(CloseReason::UserSwitch("切换 CLI".to_string()));
        // 模型选择独立于原生会话保存；即使这个 CLI 还没发送过消息，
        // 切换回来也要恢复用户上次的选择（包括明确选择默认模型）。
        let target_key = executor_identity(&agent_id, &launch_path, &protocol, &launch_args);
        let restored_model = match self.model_preferences.get(&target_key) {
            Some(model) => model.clone(),
            None => self.conversation.as_ref().and_then(|conversation| {
                Self::restored_model_for_executor(conversation, &agent_id, &launch_path)
            }),
        };
        self.agent_id = agent_id;
        self.agent_name = agent_name.to_string();
        self.launch_path = launch_path;
        self.protocol = protocol;
        self.launch_args = launch_args;
        // 切换 CLI 只改变下一轮执行者，当前素笺会话、历史与列表都保留。
        // 原生 session 身份按 CLI 分桶，旧执行者的 ID 不会传给新执行者。
        self.active_turn_id.clear();
        self.ignored_turn_id.clear();
        self.cancelling_turn_id.clear();
        // 旧 CLI 的权限请求不能留到新执行者界面上。
        self.permission_json_value = "null".to_string();
        self.permission_changed();
        self.model = restored_model;
        self.reset_models_for_agent();
        self.agent_changed();
        if self.agent_id.is_empty() {
            self.set_status("请在 AI 面板中选择本机已安装的 CLI");
        } else {
            self.set_status(format!("已选择 {}", self.agent_name));
        }
        self.start_model_probe(false);
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
        self.set_agent(
            agent_id,
            agent_name,
            launch_path,
            protocol,
            launch_args_json,
        );
    }

    fn new_conversation(&mut self) {
        if self.running_value {
            self.abandon_active_turn();
        }
        // 新会话不能复用上一段原生 session；同时换掉运行中的进程。
        self.cancel_runtime_only(CloseReason::UserSwitch("新建会话".to_string()));
        self.conversation = None;
        self.reset_models_for_agent();
        self.active_turn_id.clear();
        self.ignored_turn_id.clear();
        self.cancelling_turn_id.clear();
        self.permission_json_value = "null".to_string();
        self.permission_changed();
        self.emit_conversation();
        self.set_running(false, "新会话");
        // 新会话也不提前创建 Core 聊天或原生会话；第一条消息发送时才建立。
        self.start_model_probe(false);
    }

    fn refresh_conversations(&mut self) {
        self.refresh_conversations_list();
    }

    fn open_conversation(&mut self, conversation_id: QString) {
        let conversation_id = conversation_id.to_string();
        if self
            .conversation
            .as_ref()
            .is_some_and(|conversation| conversation.id == conversation_id)
        {
            // 重复打开同一会话不能重建原生进程；发送消息仍复用现有实例。
            self.set_status("已打开会话");
            return;
        }
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
                    self.abandon_active_turn();
                }
                self.cancel_runtime_only(CloseReason::UserSwitch("打开其他会话".to_string()));
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
                self.model = Self::restored_model_for_executor(
                    &conversation,
                    &self.agent_id,
                    &self.launch_path,
                );
                self.conversation = Some(conversation);
                self.reset_models_for_agent();
                self.active_turn_id.clear();
                self.cancelling_turn_id.clear();
                self.set_running(false, "已打开会话");
                self.permission_json_value = "null".to_string();
                self.permission_changed();
                self.emit_conversation();
                self.agent_changed();
                // 只有显式打开历史会话才允许界面反向同步 CLI/模型选择。
                self.conversation_opened();
                self.start_model_probe(false);
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
        let resume_native_session_id = self.resume_session_id_for_current_agent(&conversation);
        // 启动前只准备两套候选历史；等原生 restore 的真实结果返回后，
        // 再由发送线程决定补增量还是全量，避免“以为恢复了”导致历史丢失。
        let incremental_history = history_messages(
            &conversation,
            conversation
                .native_session(&self.agent_id, &self.launch_path)
                .and_then(|binding| binding.last_synchronized_turn_id.as_deref()),
        );
        let full_history = history_messages(&conversation, None);
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
        // 旧会话只有一个原生 ID 字段。切换执行者前先把它迁入分 CLI 映射，
        // 否则下一次换回旧 CLI 时会丢失它的原生续聊身份。
        if !conversation.agent_id.is_empty() {
            if let Some(native_session_id) = conversation.native_session_id.clone() {
                let previous_agent_id = conversation.agent_id.clone();
                let previous_launch_path = conversation.launch_path.clone();
                if conversation
                    .native_session_for(&previous_agent_id, &previous_launch_path)
                    .is_none()
                {
                    let previous_model = conversation.model.clone();
                    let previous_cursor = conversation
                        .turns
                        .iter()
                        .rev()
                        .find(|turn| {
                            turn.status == AgentTurnStatus::Completed
                                && turn.agent_id == previous_agent_id
                                && turn.launch_path == previous_launch_path
                        })
                        .map(|turn| turn.id.clone());
                    conversation.record_native_session(
                        &previous_agent_id,
                        &previous_launch_path,
                        &native_session_id,
                        previous_model,
                        previous_cursor,
                        now_ms(),
                    );
                }
            }
        }
        // 原生会话身份只在下一轮发送时作为 resume 参数；确保会话文件里的
        // Agent 信息和启动路径与当前选择一致。
        conversation.agent_id = self.agent_id.clone();
        conversation.agent_name = self.agent_name.clone();
        conversation.launch_path = self.launch_path.clone();
        conversation.agent_protocol = self.protocol.clone();
        conversation.agent_launch_args = self.launch_args.clone();
        conversation.agent_adapter_version = ADAPTER_VERSION;
        conversation.model = self.model.clone();
        conversation.native_session_id = resume_native_session_id.clone();
        // 模型偏好独立于原生会话保存：即使这个 CLI 还没成功建过
        // native session，下次切回来也应恢复用户当前选择。
        conversation.set_executor_model(
            &self.agent_id,
            &self.launch_path,
            self.model.clone(),
            now_ms(),
        );
        conversation.begin_turn(text.clone(), now_ms());
        let turn_id_value = {
            let turn = conversation.turns.last_mut().expect("turn just appended");
            turn.agent_id = self.agent_id.clone();
            turn.agent_name = self.agent_name.clone();
            turn.launch_path = self.launch_path.clone();
            turn.agent_protocol = self.protocol.clone();
            turn.agent_launch_args = self.launch_args.clone();
            turn.model = self.model.clone();
            // 真正的原生 ID 以 SessionStarted 为准；恢复失败时它可能与
            // 发送前的 resume 选择不同。
            turn.native_session_id = None;
            turn.id.clone()
        };
        conversation.ensure_title_from_first_turn();
        if let Err(error) = api.save_agent_conversation(&conversation) {
            self.set_error(format!("保存 AI 会话失败：{error}"));
            return;
        }
        self.conversation = Some(conversation);
        self.active_turn_id = turn_id_value.clone();
        self.ignored_turn_id.clear();
        self.cancelling_turn_id.clear();
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
        let diagnostic_agent_id = self.agent_id.clone();
        let diagnostic_launch_path = self.launch_path.clone();
        let failure_conversation_id = self
            .conversation
            .as_ref()
            .map(|conversation| conversation.id.clone())
            .unwrap_or_default();
        // 新轮次已经获准启动，之前排队的 shutdown 必须失效，否则会关掉
        // 本线程即将创建的新原生会话。epoch 与本次实际进程实例、轮次绑定。
        let runtime_epoch = self.runtime_epoch.clone();
        let turn_epoch = self.runtime_epoch.fetch_add(1, Ordering::SeqCst) + 1;
        std::thread::Builder::new()
            .name("agent-runtime-prompt".to_string())
            .spawn(move || {
                if runtime_epoch.load(Ordering::SeqCst) != turn_epoch {
                    log::warn!(
                        "[agent] {diagnostic_agent_id}（{diagnostic_launch_path}）轮次已被新的运行实例取代，未发送"
                    );
                    (sink)(RuntimeEvent {
                        conversation_id: failure_conversation_id,
                        generation: 0,
                        agent_identity: String::new(),
                        core_turn_id: turn_id_value,
                        native_turn_id: String::new(),
                        event: AgentEvent::TurnFailed {
                            message: "本轮已被新的运行实例取代（尚未发送给 Agent）".to_string(),
                        },
                    });
                    return;
                }
                let mut failure = None;
                let mut startup_failure = false;
                match runtime.lock() {
                    Ok(mut manager) => match manager.ensure_session(ctx, sink.clone()) {
                        Ok(preparation) => {
                            let history = if preparation.history_restored {
                                incremental_history
                            } else {
                                full_history.clone()
                            };
                            let request = AgentPromptRequest {
                                text: message,
                                history,
                                full_history,
                            };
                            if let Err(error) = manager.prompt(&turn_id_value, &request) {
                                failure = Some(error);
                            }
                        }
                        Err(error) => {
                            failure = Some(error);
                            startup_failure = true;
                        }
                    },
                    Err(_) => {
                        failure = Some("Agent 运行时状态锁已损坏".to_string());
                        startup_failure = true;
                    }
                }
                if let Some(error) = failure {
                    log::warn!(
                        "[agent] {diagnostic_agent_id}（{diagnostic_launch_path}）启动/发送失败：{error}"
                    );
                    let message = if startup_failure {
                        format!("启动 Agent 失败（尚未发送给 Agent）：{error}")
                    } else {
                        format!("尚未发送给 Agent：{error}")
                    };
                    (sink)(RuntimeEvent {
                        conversation_id: failure_conversation_id,
                        generation: 0,
                        agent_identity: String::new(),
                        core_turn_id: turn_id_value,
                        native_turn_id: String::new(),
                        event: AgentEvent::TurnFailed { message },
                    });
                }
            })
            .ok();
    }

    /// 协议确认取消后统一结算：Core Turn 失败为 Cancelled、会话回到 Idle，
    /// 并忽略旧进程随后补发的完成/增量事件。
    fn apply_cancelled_turn(&mut self, turn_id: &str) {
        self.ignored_turn_id = turn_id.to_string();
        self.cancelling_turn_id.clear();
        self.running_value = false;
        self.running_changed();
        self.set_status("已取消");
        let api = self.core_api();
        if let Some(conversation) = self.conversation.as_mut() {
            if let Some(turn) = conversation.turn_mut(turn_id) {
                turn.cancel(now_ms());
            }
            conversation.status = AgentConversationStatus::Idle;
            conversation.updated_at_ms = now_ms();
            if let Some(api) = api {
                let _ = api.save_agent_conversation(conversation);
            }
        }
        self.active_turn_id.clear();
        self.permission_json_value = "null".to_string();
        self.permission_changed();
        self.emit_conversation();
    }

    /// 切换项目/Agent/会话时直接放弃当前轮次：随后一定会销毁原生进程，
    /// 不需要等待协议确认。用户点击“停止”则走 [`Self::cancel_turn`] 的真实
    /// 确认流程，不能把两者混为一谈。
    fn abandon_active_turn(&mut self) {
        if !self.running_value {
            return;
        }
        let turn_id = self.active_turn_id.clone();
        self.ignored_turn_id = turn_id.clone();
        self.cancelling_turn_id.clear();
        self.running_value = false;
        self.running_changed();
        self.set_status("已停止");
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
    }

    fn cancel_turn(&mut self) {
        if !self.running_value || !self.cancelling_turn_id.is_empty() {
            if self.running_value {
                self.set_status("正在停止…");
            }
            return;
        }
        let turn_id = self.active_turn_id.clone();
        if turn_id.is_empty() {
            return;
        }
        self.cancelling_turn_id = turn_id.clone();
        self.set_status("正在停止…");
        let runtime = self.runtime.clone();
        let qptr = QPointer::from(&*self);
        let callback = qmetaobject::queued_callback(
            move |(turn_id, result): (String, Result<CancelOutcome, String>)| {
                if let Some(pinned) = qptr.as_pinned() {
                    let mut this = pinned.borrow_mut();
                    if this.cancelling_turn_id != turn_id {
                        return;
                    }
                    this.cancelling_turn_id.clear();
                    if this.active_turn_id != turn_id {
                        // 停止请求发出后，Agent 已经先一步完成/失败；以后者为准。
                        return;
                    }
                    match result {
                        Ok(CancelOutcome::Cancelled) => {
                            this.apply_cancelled_turn(&turn_id);
                        }
                        Ok(CancelOutcome::Pending) => {
                            // ACP v1 confirms cancellation with the original
                            // session/prompt response, not the session/cancel notification.
                            this.cancelling_turn_id = turn_id;
                            this.set_status("正在停止，等待 Agent 确认…");
                        }
                        Ok(CancelOutcome::AlreadyFinished) => {
                            // 取消请求与自然完成交错：原生完成事件仍然有效，
                            // 由它按真实状态结算，不能改写成用户取消。
                            this.set_status("本轮已结束，等待 Agent 结算");
                        }
                        Err(error) => {
                            this.set_status("停止失败，Agent 仍在运行");
                            this.set_error(format!("停止失败：{error}"));
                        }
                    }
                }
            },
        );
        std::thread::Builder::new()
            .name("agent-runtime-cancel".to_string())
            .spawn(move || {
                let result = match runtime.lock() {
                    Ok(mut manager) => manager.cancel_turn(&turn_id),
                    Err(_) => Err("Agent 运行时状态锁已损坏".to_string()),
                };
                callback((turn_id, result));
            })
            .ok();
    }

    fn choose_model(&mut self, model: QString) {
        let model = model.to_string();
        self.model = if model.is_empty() { None } else { Some(model) };
        self.model_preferences
            .insert(self.model_cache_key(), self.model.clone());
        let api = self.core_api();
        let matches_current = self
            .conversation
            .as_ref()
            .is_some_and(|conversation| self.conversation_matches_current_agent(conversation));
        if let (Some(conversation), Some(api)) = (self.conversation.as_mut(), api) {
            // 模型偏好按 agentId + launchPath 独立保存，不要求该 CLI 已有
            // native session；这样从未发送过的新 CLI 也能记住选择。
            conversation.set_executor_model(
                &self.agent_id,
                &self.launch_path,
                self.model.clone(),
                now_ms(),
            );
            if matches_current {
                conversation.model = self.model.clone();
            }
            conversation.updated_at_ms = now_ms();
            if let Err(error) = api.save_agent_conversation(conversation) {
                self.set_error(format!("保存模型选择失败：{error}"));
            }
        }
        self.models_state_value = if self.model.is_some() {
            "ready".to_string()
        } else {
            "default".to_string()
        };
        self.models_changed();
        self.emit_conversation();
    }

    fn refresh_models(&mut self) {
        self.models_probed_for.clear();
        self.model_probe_epoch.fetch_add(1, Ordering::SeqCst);
        self.start_model_probe(true);
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
        // 切换 CLI 后旧运行时可能还会补发事件；执行者身份不匹配时只能
        // 丢弃，不能把旧 CLI 的模型或原生 session 写进当前会话。
        if !event.agent_identity.is_empty()
            && event.agent_identity != self.current_executor_identity()
        {
            self.conversation = Some(conversation);
            return;
        }
        let session_event = matches!(
            event.event,
            AgentEvent::SessionStarted { .. }
                | AgentEvent::ModelsAvailable { .. }
                | AgentEvent::ModelSwitchFailed { .. }
                | AgentEvent::McpConnectionChanged { .. }
                | AgentEvent::SessionError { .. }
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
            } else if self.runtime_generation != 0 && event.generation != self.runtime_generation {
                self.conversation = Some(conversation);
                return;
            }
        }
        let core_turn_id = event.core_turn_id.clone();
        if core_turn_id.is_empty() {
            if !session_event {
                // 适配器必须给每个轮次事件绑定不可变的 Core Turn。没有
                // 归属的增量/终态一律丢弃，不能落到“当前轮次”或界面状态。
                self.conversation = Some(conversation);
                return;
            }
        } else {
            let belongs_to_active =
                core_turn_id == self.active_turn_id && core_turn_id != self.ignored_turn_id;
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
        let mut cancelled = false;
        let mut permission: Option<(String, String, Vec<serde_json::Value>)> = None;
        match event.event {
            AgentEvent::SessionStarted {
                native_session_id,
                model,
                resumed,
            } => {
                let at_ms = now_ms();
                let turn_id = if core_turn_id.is_empty() {
                    self.active_turn_id.clone()
                } else {
                    core_turn_id.clone()
                };
                // 优先采用适配器上报的实际模型；没有上报时沿用本轮开始时
                // 记录的选择，避免用户在运行中改选择器影响已完成轮次。
                let turn_model = conversation
                    .turns
                    .iter()
                    .find(|turn| turn.id == turn_id)
                    .and_then(|turn| turn.model.clone());
                // 原生会话上报的是启动时实际模型，可能还不是用户刚选择的
                // 模型；绑定先记录实际值，选择器保留待切换请求，等本轮
                // ModelSwitchApplied/ModelSwitchFailed 再落定。
                let recorded_model = model.clone().or_else(|| turn_model.clone());
                // 恢复失败意味着新原生会话里没有任何旧轮次；游标必须清空，
                // 下一轮提示词才能补全真实历史。
                let synchronized_turn = if resumed {
                    conversation
                        .native_session(&self.agent_id, &self.launch_path)
                        .and_then(|binding| binding.last_synchronized_turn_id.clone())
                } else {
                    None
                };
                conversation.native_session_id = Some(native_session_id.clone());
                conversation.record_native_session(
                    &self.agent_id,
                    &self.launch_path,
                    &native_session_id,
                    recorded_model.clone(),
                    synchronized_turn,
                    at_ms,
                );
                let pending_model_switch = turn_model
                    .as_deref()
                    .zip(model.as_deref())
                    .is_some_and(|(requested, actual)| requested != actual);
                if let Some(turn) = conversation
                    .turns
                    .iter_mut()
                    .find(|turn| turn.id == turn_id)
                {
                    turn.native_session_id = Some(native_session_id);
                    if !pending_model_switch && recorded_model.is_some() {
                        turn.model = recorded_model;
                    }
                }
                conversation.updated_at_ms = at_ms;
                persist = true;
                // 原生会话建立只代表 Agent 已连接；模型目录由独立探测维护，
                // MCP 是否真正连上必须等适配器上报 McpConnectionChanged。
            }
            AgentEvent::McpConnectionChanged {
                server,
                status,
                error,
                tools,
            } => {
                let mut mcp =
                    serde_json::from_str::<serde_json::Value>(&self.mcp_status_json_value)
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
                // 失败或 HTTP 拒绝都不会走到这里。此刻该轮及其前置历史已
                // 真正送达原生会话，推进同步游标。绑定必须已经由
                // SessionStarted 建立，否则游标没有可写入的原生会话。
                if !core_turn_id.is_empty()
                    && conversation
                        .native_session_for(&self.agent_id, &self.launch_path)
                        .is_some()
                {
                    let at_ms = now_ms();
                    conversation.set_native_session_synchronized_turn(
                        &self.agent_id,
                        &self.launch_path,
                        Some(core_turn_id.clone()),
                        at_ms,
                    );
                    conversation.updated_at_ms = at_ms;
                    persist = true;
                }
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
                if models.is_empty() {
                    self.available_models.clear();
                    self.models_state_value = "default".to_string();
                } else {
                    self.available_models = models.clone();
                    let cache_key = self.model_cache_key();
                    self.model_cache.insert(cache_key, models);
                    self.models_error_value.clear();
                    self.models_state_value = "ready".to_string();
                }
                self.models_changed();
            }
            AgentEvent::ModelSwitchApplied { requested, actual } => {
                if let Some(turn) = conversation.turn_mut(&core_turn_id) {
                    turn.model = actual.clone();
                }
                conversation.set_native_session_model(
                    &self.agent_id,
                    &self.launch_path,
                    actual.clone(),
                    now_ms(),
                );
                if requested.is_some() {
                    self.model = actual.clone();
                    if self.conversation_matches_current_agent(&conversation) {
                        conversation.model = actual.clone();
                    }
                    conversation.set_model_preference(
                        &self.agent_id,
                        &self.launch_path,
                        actual,
                        now_ms(),
                    );
                } else {
                    // 用户选的是 CLI 默认模型；原生会话可以报告具体默认值，
                    // 但选择器继续保留“默认”语义，不反向锁定到某个模型。
                    self.model = None;
                    if self.conversation_matches_current_agent(&conversation) {
                        conversation.model = None;
                    }
                    conversation.set_model_preference(
                        &self.agent_id,
                        &self.launch_path,
                        None,
                        now_ms(),
                    );
                }
                conversation.updated_at_ms = now_ms();
                persist = true;
            }
            AgentEvent::ModelSwitchFailed {
                requested,
                actual,
                message,
            } => {
                log::warn!("[agent] 模型切换失败（请求 {requested}）：{message}");
                self.model = actual.clone();
                if let Some(turn) = conversation.turn_mut(&core_turn_id) {
                    turn.model = actual.clone();
                }
                if self.conversation_matches_current_agent(&conversation) {
                    conversation.model = actual.clone();
                }
                // 实际模型未知时按 CLI 默认处理，不能把被拒绝的请求模型
                // 继续当成事实。
                conversation.set_executor_model(
                    &self.agent_id,
                    &self.launch_path,
                    actual,
                    now_ms(),
                );
                conversation.updated_at_ms = now_ms();
                persist = true;
                self.set_error(message.clone());
                self.set_status("模型切换失败，沿用的实际模型已更新");
            }
            AgentEvent::Usage { .. } => {
                // 真实使用量在轮次完成时按消息字符数记录，避免各协议 token
                // 计数单位不同导致统计口径漂移。
            }
            AgentEvent::SessionError { message } => {
                // 会话级错误没有确认的轮次终态，不能据此结束回答；保留
                // running，等 idle/消息历史/重连恢复后再由明确终态结算。
                // 允许用户重新选择同一 CLI 时再次探测模型目录。
                self.models_probed_for.clear();
                if self.models_state_value == "loading" {
                    self.models_state_value = "error".to_string();
                    self.models_changed();
                }
                self.set_error(format!("Agent 会话错误：{message}"));
                if self.running_value {
                    self.set_status("Agent 连接异常，等待服务端状态");
                } else {
                    self.set_status("Agent 会话异常");
                }
            }
            AgentEvent::TurnCancelled => {
                if let Some(turn) = conversation.turn_mut(&core_turn_id) {
                    turn.cancel(now_ms());
                }
                conversation.status = AgentConversationStatus::Idle;
                conversation.updated_at_ms = now_ms();
                persist = true;
                cancelled = true;
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
                log::warn!(
                    "[agent] 轮次失败（{} / {}）：{message}",
                    conversation.agent_id,
                    conversation.launch_path
                );
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
        if cancelled {
            self.apply_cancelled_turn(&core_turn_id);
            return;
        }
        if let Some((failed, message)) = finished {
            // 完成事件可能先于 HTTP/JSON-RPC 的受理回执抵达（尤其 OpenCode
            // 的 SSE 与 POST 回应没有顺序保证）。输入事件使用稳定 ID，重复
            // 补记是幂等的；失败事件只在确实受理后由 PromptAccepted 记录，
            // 避免启动失败被算作用户已发送。
            if !failed {
                if let Some(conversation) = self.conversation.as_ref() {
                    self.record_usage(conversation, &core_turn_id, false);
                }
            }
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
            self.cancelling_turn_id.clear();
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
