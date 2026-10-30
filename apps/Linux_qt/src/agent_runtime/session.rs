//! 单一 Agent 会话管理器。
//!
//! 负责一个会话的原生进程/线程、当前轮次身份、取消和权限回复。发现与选择
//! 仍由 `agent_cli` 承担；切换 Agent 时显式关闭旧适配器，再按新选择重建。

use std::sync::{Arc, Mutex};

use super::adapter::{create_adapter, AgentAdapter};
use super::events::{AgentEvent, AgentLaunchContext, EventSink, RuntimeEvent};

pub struct AgentSessionManager {
    adapter: Option<Box<dyn AgentAdapter>>,
    current_turn_id: Arc<Mutex<String>>,
    context: Option<AgentLaunchContext>,
    started: bool,
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
            started: false,
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
        ctx: AgentLaunchContext,
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
        let current_turn_id: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let turn_id = current_turn_id.clone();
        let wrapped: EventSink = Arc::new(move |mut event: RuntimeEvent| {
            // 会话级事件在 `prompt` 之前产生，不能借用上一轮 turn id，
            // 否则新进程的 SessionStarted / 模型列表会被后端当成旧轮次丢弃。
            let session_event = matches!(
                event.event,
                AgentEvent::SessionStarted { .. } | AgentEvent::ModelsAvailable { .. }
            );
            if event.turn_id.is_empty() && !session_event {
                event.turn_id = turn_id
                    .lock()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
            }
            (sink)(event);
        });
        let adapter = create_adapter(&ctx, wrapped)?;
        self.current_turn_id = current_turn_id;
        self.adapter = Some(adapter);
        self.context = Some(ctx);
        self.started = true;
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

    /// 把管理器内部产生的错误转成统一失败事件，调用方不必区分来源。
    pub fn fail_current_turn(sink: &EventSink, turn_id: &str, message: String) {
        (sink)(RuntimeEvent {
            turn_id: turn_id.to_string(),
            event: AgentEvent::TurnFailed { message },
        });
    }

    pub fn shutdown(&mut self) {
        self.adapter = None;
        self.context = None;
        self.started = false;
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
