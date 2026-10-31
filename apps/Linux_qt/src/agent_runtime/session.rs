//! 单一 Agent 会话管理器。
//!
//! 负责一个会话的原生进程/线程、当前轮次身份、取消和权限回复。发现与选择
//! 仍由 `agent_cli` 承担；切换 Agent 时显式关闭旧适配器，再按新选择重建。

use std::sync::{Arc, Mutex};

use super::adapter::{create_adapter, AgentAdapter, CancelOutcome};
use super::events::{AgentEvent, AgentLaunchContext, AgentPromptRequest, EventSink, RuntimeEvent};
use super::mcp_binding::{prepare_mcp_brokers, McpBroker};

/// 原生会话准备结果。后端据此决定补增量历史还是完整历史。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionPreparation {
    /// 复用了管理器里已在运行的同一原生会话。
    pub reused: bool,
    /// 原生会话已经持有历史；false 时必须补完整 Core 历史。
    pub history_restored: bool,
}

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
    ) -> Result<SessionPreparation, String> {
        if self.started {
            let reusable = self.context.as_ref().is_some_and(|existing| {
                existing.agent_id == ctx.agent_id
                    && existing.launch_path == ctx.launch_path
                    && existing.launch_args == ctx.launch_args
                    && existing.protocol == ctx.protocol
                    && existing.conversation_id == ctx.conversation_id
                    && existing.project_id == ctx.project_id
            });
            if reusable {
                // Core 会话在首轮之后才拿到原生 session ID；不能让同一个
                // 进程因为“恢复参数从 None 变成 Some”而每轮重启。
                let live_id = self
                    .adapter
                    .as_ref()
                    .and_then(|adapter| adapter.native_session_id());
                let requested_id = ctx.resume_native_session_id.clone();
                if requested_id.is_none() || requested_id == live_id {
                    // 模型只是下一轮的执行参数，不允许触发进程重启，更不能
                    // 阻塞新模型的发送。适配器在收到新模型后按协议逐轮应用。
                    if let Some(existing) = self.context.as_mut() {
                        existing.model = ctx.model.clone();
                    }
                    if let Some(adapter) = self.adapter.as_mut() {
                        adapter.set_model(ctx.model.clone());
                    }
                    let history_restored = self
                        .adapter
                        .as_ref()
                        .is_some_and(|adapter| adapter.history_restored_on_reuse());
                    return Ok(SessionPreparation {
                        reused: true,
                        history_restored,
                    });
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
        let agent_identity = ctx.executor_identity();
        let current_turn_id: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let wrapped: EventSink = Arc::new(move |mut event: RuntimeEvent| {
            // 适配器显式携带的 Core Turn 是创建异步线程时的固定身份。管理器
            // 不再用事件到达时的 current_turn_id 补空：任何没有明确归属的
            // 事件都交给后端丢弃，避免停止后立即重发时旧线程污染新轮次。
            event.conversation_id = conversation_id.clone();
            event.generation = generation;
            event.agent_identity = agent_identity.clone();
            (sink)(event);
        });
        let adapter = create_adapter(&ctx, wrapped)?;
        let history_restored = adapter.history_restored();
        self.current_turn_id = current_turn_id;
        self.adapter = Some(adapter);
        self.context = Some(ctx);
        self.mcp_broker = mcp_broker;
        self.started = true;
        self.generation = generation;
        Ok(SessionPreparation {
            reused: false,
            history_restored,
        })
    }

    pub fn prompt(&mut self, turn_id: &str, request: &AgentPromptRequest) -> Result<(), String> {
        let adapter = self
            .adapter
            .as_mut()
            .ok_or_else(|| "Agent 会话尚未启动".to_string())?;
        if let Ok(mut guard) = self.current_turn_id.lock() {
            *guard = turn_id.to_string();
        }
        adapter.prompt(turn_id, request)
    }

    pub fn cancel(&mut self) -> Result<CancelOutcome, String> {
        let current = self
            .current_turn_id
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        self.adapter
            .as_mut()
            .ok_or_else(|| "Agent 会话尚未启动".to_string())?
            .cancel(&current)
    }

    /// 按 Core 轮次取消。用户停止后立刻发送下一轮时，取消线程可能晚于新
    /// 轮次执行；适配器必须按轮次 id 决定是否中断原生协议，不能因为当前
    /// 轮次已经变化就放弃取消上一轮，也不能误中止新回答。
    pub fn cancel_turn(&mut self, turn_id: &str) -> Result<CancelOutcome, String> {
        self.adapter
            .as_mut()
            .ok_or_else(|| "Agent 会话尚未启动".to_string())?
            .cancel(turn_id)
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
