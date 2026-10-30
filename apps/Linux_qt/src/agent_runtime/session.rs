//! 单一 Agent 会话管理器。
//!
//! 负责一个会话的原生进程/线程、当前轮次身份、取消和权限回复。发现与选择
//! 仍由 `agent_cli` 承担；切换 Agent 时显式关闭旧适配器，再按新选择重建。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::adapter::{create_adapter, AgentAdapter};
use super::events::{AgentEvent, AgentLaunchContext, EventSink, RuntimeEvent};
use super::mcp_binding::{prepare_mcp_brokers, McpBroker};

pub struct AgentSessionManager {
    adapter: Option<Box<dyn AgentAdapter>>,
    current_turn_id: Arc<Mutex<String>>,
    context: Option<AgentLaunchContext>,
    mcp_broker: Option<McpBroker>,
    started: bool,
    /// 运行实例代次。每次重建原生进程都会递增，旧进程迟到的事件必须失效。
    generation: u64,
    next_generation: u64,
}

impl Default for AgentSessionManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentSessionManager {
    pub fn new() -> Self {
        Self {
            adapter: None,
            current_turn_id: Arc::new(Mutex::new(String::new())),
            context: None,
            mcp_broker: None,
            started: false,
            generation: 0,
            next_generation: 0,
        }
    }

    pub fn is_started(&self) -> bool {
        self.started
    }

    pub fn context(&self) -> Option<&AgentLaunchContext> {
        self.context.as_ref()
    }

    /// 启动（或恢复）会话。相同 Agent + 原生 session 已运行时不重复拉起。
    pub fn ensure_session(
        &mut self,
        mut ctx: AgentLaunchContext,
        sink: EventSink,
    ) -> Result<(), String> {
        if self.started {
            if let Some(existing) = self.context.as_ref() {
                if existing.agent_id == ctx.agent_id
                    && existing.launch_path == ctx.launch_path
                    && existing.launch_args == ctx.launch_args
                    && existing.protocol == ctx.protocol
                    && existing.conversation_id == ctx.conversation_id
                    && existing.project_id == ctx.project_id
                    && existing.model == ctx.model
                {
                    // Core 会话在首轮之后才拿到原生 session ID；不能让同一个
                    // 进程因为“恢复参数从 None 变成 Some”而每轮重启。
                    let live_id = self
                        .adapter
                        .as_ref()
                        .and_then(|adapter| adapter.native_session_id());
                    let requested_id = ctx.resume_native_session_id.clone();
                    if requested_id.is_none() || requested_id == live_id {
                        return Ok(());
                    }
                }
            }
        }
        self.shutdown();
        self.next_generation = self.next_generation.saturating_add(1);
        let generation = self.next_generation;
        // broker 必须在沙箱外启动；随后适配器拿到的只是 stdio bridge 配置。
        let (mcp_servers, mcp_broker) = match prepare_mcp_brokers(&ctx) {
            Ok(value) => value,
            Err(error) => {
                if let Some(server) = ctx.mcp_servers.first() {
                    (sink)(RuntimeEvent::session(AgentEvent::McpConnectionChanged {
                        server: server.name.clone(),
                        status: "failed".to_string(),
                        error: Some(error.clone()),
                        tools: Vec::new(),
                    }));
                }
                return Err(error);
            }
        };
        ctx.mcp_servers = mcp_servers;
        ctx.mcp_socket_dir = mcp_broker
            .as_ref()
            .map(|broker| broker.directory().to_path_buf());
        let conversation_id = ctx.conversation_id.clone();
        let current_turn_id: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let turn_id = current_turn_id.clone();
        // 原生轮次 ID（Codex turnId、OpenCode assistant message id 等）只做
        // 协议内部映射；第一次看到它时绑定到当时的 Core Turn，之后迟到的
        // 事件仍然回到旧 Turn，而不会被误算进新回答。
        let turn_bindings: Arc<Mutex<HashMap<String, String>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let bindings = turn_bindings.clone();
        let wrapped: EventSink = Arc::new(move |mut event: RuntimeEvent| {
            let session_event = matches!(
                event.event,
                AgentEvent::SessionStarted { .. }
                    | AgentEvent::ModelsAvailable { .. }
                    | AgentEvent::McpConnectionChanged { .. }
            );
            let current = turn_id
                .lock()
                .map(|guard| guard.clone())
                .unwrap_or_default();
            let core_turn_id = if session_event {
                String::new()
            } else if event.native_turn_id.is_empty() {
                current
            } else {
                bindings
                    .lock()
                    .map(|mut map| {
                        map.entry(event.native_turn_id.clone())
                            .or_insert(current)
                            .clone()
                    })
                    .unwrap_or_default()
            };
            event.conversation_id = conversation_id.clone();
            event.generation = generation;
            event.core_turn_id = core_turn_id;
            (sink)(event);
        });
        let adapter = create_adapter(&ctx, wrapped)?;
        self.current_turn_id = current_turn_id;
        self.adapter = Some(adapter);
        self.context = Some(ctx);
        self.mcp_broker = mcp_broker;
        self.started = true;
        self.generation = generation;
        Ok(())
    }

    pub fn prompt(&mut self, turn_id: &str, message: &str) -> Result<(), String> {
        let adapter = self
            .adapter
            .as_mut()
            .ok_or_else(|| "Agent 会话尚未启动".to_string())?;
        if let Ok(mut guard) = self.current_turn_id.lock() {
            *guard = turn_id.to_string();
        }
        adapter.prompt(message)
    }

    pub fn cancel(&mut self) -> Result<(), String> {
        self.adapter
            .as_mut()
            .ok_or_else(|| "Agent 会话尚未启动".to_string())?
            .cancel()
    }

    /// 只取消仍然处于活动状态的轮次。用户停止后立刻发送下一轮时，旧的
    /// 取消线程可能晚于新轮次拿到锁；按轮次 id 校验可避免误中止新回答。
    pub fn cancel_turn(&mut self, turn_id: &str) -> Result<(), String> {
        let current = self
            .current_turn_id
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        if current != turn_id {
            return Ok(());
        }
        self.cancel()
    }

    pub fn respond_permission(&mut self, request_id: &str, option_id: &str) -> Result<(), String> {
        self.adapter
            .as_mut()
            .ok_or_else(|| "Agent 会话尚未启动".to_string())?
            .respond_permission(request_id, option_id)
    }

    pub fn shutdown(&mut self) {
        self.adapter = None;
        self.mcp_broker = None;
        self.context = None;
        self.started = false;
        self.generation = 0;
        if let Ok(mut guard) = self.current_turn_id.lock() {
            guard.clear();
        }
    }
}

impl Drop for AgentSessionManager {
    fn drop(&mut self) {
        self.shutdown();
    }
}
