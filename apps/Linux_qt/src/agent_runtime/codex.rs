//! Codex `app-server` 适配器。
//!
//! 使用官方机器接口：`initialize` → `initialized` → `thread/start` 或
//! `thread/resume` → `turn/start`，把 app-server 通知映射为统一事件。
//! 复用用户已有的 `~/.codex` 登录环境；不启动交互式 TUI。

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::adapter::{AgentAdapter, CancelOutcome};
use super::events::{
    json_text, json_value_text, AgentEvent, AgentLaunchContext, EventSink, PermissionOption,
    RuntimeEvent,
};
use super::rpc::RpcClient;
use super::workspace_access::WorkspaceAccess;

/// 单个 Core Turn 在 Codex 侧的生命周期。取消可能发生在 `turn/start` 的
/// 响应之前，因此取消意图必须独立于任何共享的“当前 native turn id”。
#[derive(Debug, Clone, PartialEq, Eq)]
enum CodexTurnState {
    Starting,
    Running(String),
    Cancelling(Option<String>),
    Cancelled(String),
    CancelFailed(String),
    /// 自然完成/失败。取消请求看到它时必须返回 AlreadyFinished，让原生
    /// 完成事件按真实结果结算，不能改写成“用户取消”。
    Finished(CodexFinish),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexFinish {
    Completed,
    Failed,
}

#[derive(Default)]
struct CodexTurnStore {
    states: Mutex<HashMap<String, CodexTurnState>>,
    changed: Condvar,
}

impl CodexTurnStore {
    fn new() -> Self {
        Self::default()
    }

    fn set(&self, core_turn_id: &str, state: CodexTurnState) {
        if let Ok(mut states) = self.states.lock() {
            states.insert(core_turn_id.to_string(), state);
        }
        self.changed.notify_all();
    }

    fn snapshot(&self, core_turn_id: &str) -> Option<CodexTurnState> {
        self.states
            .lock()
            .ok()
            .and_then(|states| states.get(core_turn_id).cloned())
    }

    /// `turn/start` 成功后落定 native id。如果取消已经先到，返回 true 表示
    /// 必须立刻对这个 native turn 发送 interrupt。
    fn running(&self, core_turn_id: &str, native_turn_id: &str) -> bool {
        let mut interrupt = false;
        if let Ok(mut states) = self.states.lock() {
            match states.get(core_turn_id).cloned() {
                Some(CodexTurnState::Starting) => {
                    states.insert(
                        core_turn_id.to_string(),
                        CodexTurnState::Running(native_turn_id.to_string()),
                    );
                }
                Some(CodexTurnState::Cancelling(None)) => {
                    states.insert(
                        core_turn_id.to_string(),
                        CodexTurnState::Cancelling(Some(native_turn_id.to_string())),
                    );
                    interrupt = true;
                }
                Some(CodexTurnState::Cancelling(Some(_))) => {
                    // 取消线程已经拿到 native id 并负责发送 interrupt。
                }
                None => {
                    states.insert(
                        core_turn_id.to_string(),
                        CodexTurnState::Running(native_turn_id.to_string()),
                    );
                }
                _ => {}
            }
        }
        self.changed.notify_all();
        interrupt
    }

    fn started_notification(&self, core_turn_id: &str, native_turn_id: &str) {
        if let Ok(mut states) = self.states.lock() {
            if matches!(
                states.get(core_turn_id),
                Some(CodexTurnState::Starting) | None
            ) {
                states.insert(
                    core_turn_id.to_string(),
                    CodexTurnState::Running(native_turn_id.to_string()),
                );
            }
        }
        self.changed.notify_all();
    }

    fn finish(&self, core_turn_id: &str, finish: CodexFinish) {
        if let Ok(mut states) = self.states.lock() {
            states.insert(core_turn_id.to_string(), CodexTurnState::Finished(finish));
        }
        self.changed.notify_all();
    }

    fn begin_cancel(&self, core_turn_id: &str) -> Option<CodexTurnState> {
        let mut states = self.states.lock().ok()?;
        let state = states.get(core_turn_id).cloned()?;
        let active = match state.clone() {
            CodexTurnState::Starting => CodexTurnState::Cancelling(None),
            CodexTurnState::Running(native) => CodexTurnState::Cancelling(Some(native)),
            other => other,
        };
        states.insert(core_turn_id.to_string(), active);
        self.changed.notify_all();
        Some(state)
    }

    fn resolve_cancel(&self, core_turn_id: &str, result: Result<String, String>) {
        let state = match result {
            Ok(native) => CodexTurnState::Cancelled(native),
            Err(error) => CodexTurnState::CancelFailed(error),
        };
        self.set(core_turn_id, state);
    }

    fn wait_cancel(&self, core_turn_id: &str, timeout: Duration) -> Result<CancelOutcome, String> {
        let started = Instant::now();
        loop {
            let state = self.snapshot(core_turn_id);
            match state {
                // 没有状态记录说明本就不是当前适配器管理的轮次；自然完成
                // 必须交由原生终态事件结算。
                None | Some(CodexTurnState::Finished(_)) => {
                    return Ok(CancelOutcome::AlreadyFinished);
                }
                Some(CodexTurnState::Cancelled(_)) => return Ok(CancelOutcome::Cancelled),
                Some(CodexTurnState::CancelFailed(error)) => return Err(error),
                _ => {}
            }
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err("等待 Codex 确认中断超时".to_string());
            }
            match self.states.lock() {
                Ok(states) => {
                    let _ = self
                        .changed
                        .wait_timeout(states, remaining.min(Duration::from_millis(100)));
                }
                Err(_) => return Err("Codex 轮次状态锁已损坏".to_string()),
            }
        }
    }
}

pub struct CodexAdapter {
    client: Arc<RpcClient>,
    thread_id: Arc<Mutex<Option<String>>>,
    emit: EventSink,
    approval_ids: Arc<Mutex<HashMap<String, i64>>>,
    model: Option<String>,
    /// 当前轮次的固定 Core Turn。`turn/started` 等通知只有原生 turnId 时，
    /// 用它建立本轮的 native → Core 映射。
    active_core_turn_id: Arc<Mutex<String>>,
    /// 原生 turnId → Core Turn 的稳定映射。旧轮次迟到通知始终回到旧 Turn，
    /// 不会因为用户已经发送下一轮而改记到新回答。
    turn_bindings: Arc<Mutex<HashMap<String, String>>>,
    /// 每个 Core Turn 的启动/运行/取消状态。取消先落在这个表里，绝不会
    /// 读取下一轮的共享 native turn id。
    turn_store: Arc<CodexTurnStore>,
}

fn core_for_native_turn(
    native_turn_id: &str,
    active: &Arc<Mutex<String>>,
    bindings: &Arc<Mutex<HashMap<String, String>>>,
) -> String {
    let current = active.lock().map(|guard| guard.clone()).unwrap_or_default();
    if native_turn_id.is_empty() {
        return current;
    }
    bindings
        .lock()
        .map(|mut map| {
            map.entry(native_turn_id.to_string())
                .or_insert(current)
                .clone()
        })
        .unwrap_or_default()
}

fn interrupt_codex_turn(
    client: &RpcClient,
    thread_id: &str,
    native_turn_id: &str,
) -> Result<(), String> {
    if native_turn_id.is_empty() {
        return Err("Codex 尚未返回 turn id，无法确认中断".to_string());
    }
    client.request(
        "turn/interrupt",
        json!({ "threadId": thread_id, "turnId": native_turn_id }),
        Duration::from_secs(10),
    )?;
    Ok(())
}

impl CodexAdapter {
    pub fn start(ctx: &AgentLaunchContext, emit: EventSink) -> Result<Self, String> {
        let thread_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let approval_ids: Arc<Mutex<HashMap<String, i64>>> = Arc::new(Mutex::new(HashMap::new()));
        let active_core_turn_id: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let turn_bindings: Arc<Mutex<HashMap<String, String>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let turn_store: Arc<CodexTurnStore> = Arc::new(CodexTurnStore::new());
        let handler_emit = emit.clone();
        let handler_thread = thread_id.clone();
        let handler_approvals = approval_ids.clone();
        let handler_active_turn = active_core_turn_id.clone();
        let handler_bindings = turn_bindings.clone();
        let handler_turn_store = turn_store.clone();
        let handler: Arc<dyn Fn(&str, Value) + Send + Sync> =
            Arc::new(move |method: &str, message: Value| {
                let params = message.get("params").cloned().unwrap_or_default();
                let turn_id = json_text(&params, &["turnId"])
                    .or_else(|| json_text(&params, &["turn", "id"]))
                    .unwrap_or_default();
                let core_turn_id =
                    core_for_native_turn(&turn_id, &handler_active_turn, &handler_bindings);
                match method {
                    "turn/started" => {
                        if let Some(id) = json_text(&params, &["turn", "id"])
                            .or_else(|| json_text(&params, &["turnId"]))
                        {
                            let core = if core_turn_id.is_empty() {
                                handler_active_turn
                                    .lock()
                                    .map(|guard| guard.clone())
                                    .unwrap_or_default()
                            } else {
                                core_turn_id.clone()
                            };
                            if !core.is_empty() {
                                if let Ok(mut bindings) = handler_bindings.lock() {
                                    bindings.insert(id.clone(), core.clone());
                                }
                                handler_turn_store.started_notification(&core, &id);
                            }
                        }
                    }
                    "item/agentMessage/delta" => {
                        if let Some(text) = json_text(&params, &["delta"]) {
                            (handler_emit)(RuntimeEvent {
                                conversation_id: String::new(),
                                generation: 0,
                                agent_identity: String::new(),
                                core_turn_id: core_turn_id.clone(),
                                native_turn_id: turn_id,
                                event: AgentEvent::TextDelta { text },
                            });
                        }
                    }
                    "item/reasoning/textDelta" | "item/reasoning/summaryTextDelta" => {
                        if let Some(text) = json_text(&params, &["delta"]) {
                            (handler_emit)(RuntimeEvent {
                                conversation_id: String::new(),
                                generation: 0,
                                agent_identity: String::new(),
                                core_turn_id: core_turn_id.clone(),
                                native_turn_id: turn_id,
                                event: AgentEvent::ReasoningDelta { text },
                            });
                        }
                    }
                    "item/started" => {
                        let Some(item) = params.get("item") else {
                            return;
                        };
                        let Some(kind) = json_text(item, &["type"]) else {
                            return;
                        };
                        let tool_call_id =
                            json_text(item, &["id"]).unwrap_or_else(|| "tool".to_string());
                        let name = match kind.as_str() {
                            "commandExecution" => "命令".to_string(),
                            "fileChange" => "文件修改".to_string(),
                            "mcpToolCall" => {
                                json_text(item, &["tool"]).unwrap_or("MCP 工具".to_string())
                            }
                            "webSearch" => "搜索".to_string(),
                            other => other.to_string(),
                        };
                        (handler_emit)(RuntimeEvent {
                            conversation_id: String::new(),
                            generation: 0,
                            agent_identity: String::new(),
                            core_turn_id: core_turn_id.clone(),
                            native_turn_id: turn_id,
                            event: AgentEvent::ToolStarted {
                                tool_call_id,
                                name,
                                input_json: json_value_text(
                                    item.get("command").or_else(|| item.get("arguments")),
                                ),
                            },
                        });
                    }
                    "item/completed" => {
                        let Some(item) = params.get("item") else {
                            return;
                        };
                        let Some(kind) = json_text(item, &["type"]) else {
                            return;
                        };
                        if !matches!(
                            kind.as_str(),
                            "commandExecution" | "fileChange" | "mcpToolCall" | "webSearch"
                        ) {
                            return;
                        }
                        let tool_call_id =
                            json_text(item, &["id"]).unwrap_or_else(|| "tool".to_string());
                        let name = match kind.as_str() {
                            "commandExecution" => "命令".to_string(),
                            "fileChange" => "文件修改".to_string(),
                            "mcpToolCall" => {
                                json_text(item, &["tool"]).unwrap_or("MCP 工具".to_string())
                            }
                            "webSearch" => "搜索".to_string(),
                            other => other.to_string(),
                        };
                        let status = json_text(item, &["status"]).unwrap_or_default();
                        (handler_emit)(RuntimeEvent {
                            conversation_id: String::new(),
                            generation: 0,
                            agent_identity: String::new(),
                            core_turn_id: core_turn_id.clone(),
                            native_turn_id: turn_id,
                            event: AgentEvent::ToolFinished {
                                tool_call_id,
                                name,
                                output_json: item
                                    .get("aggregatedOutput")
                                    .or_else(|| item.get("result"))
                                    .or_else(|| item.get("error"))
                                    .map(|value| match value {
                                        Value::String(text) => text.clone(),
                                        other => serde_json::to_string(other).unwrap_or_default(),
                                    }),
                                success: status == "completed",
                            },
                        });
                    }
                    "turn/completed" => {
                        if !core_turn_id.is_empty() {
                            handler_turn_store.finish(&core_turn_id, CodexFinish::Completed);
                        }
                        (handler_emit)(RuntimeEvent {
                            conversation_id: String::new(),
                            generation: 0,
                            agent_identity: String::new(),
                            core_turn_id: core_turn_id.clone(),
                            native_turn_id: turn_id,
                            event: AgentEvent::TurnCompleted,
                        });
                    }
                    "turn/failed" | "error" => {
                        let message = json_text(&params, &["error", "message"])
                            .or_else(|| json_text(&params, &["message"]))
                            .unwrap_or_else(|| "Codex 请求失败".to_string());
                        if !core_turn_id.is_empty() {
                            handler_turn_store.finish(&core_turn_id, CodexFinish::Failed);
                        }
                        (handler_emit)(RuntimeEvent {
                            conversation_id: String::new(),
                            generation: 0,
                            agent_identity: String::new(),
                            core_turn_id: core_turn_id.clone(),
                            native_turn_id: turn_id,
                            event: AgentEvent::TurnFailed { message },
                        });
                    }
                    other if other.contains("Approval") => {
                        let Some(rpc_id) = message.get("id").and_then(Value::as_i64) else {
                            return;
                        };
                        let request_id = rpc_id.to_string();
                        if let Ok(mut guard) = handler_approvals.lock() {
                            guard.insert(request_id.clone(), rpc_id);
                        }
                        let description = json_text(&params, &["command"])
                            .or_else(|| json_text(&params, &["reason"]))
                            .or_else(|| json_text(&params, &["item", "command"]))
                            .unwrap_or_else(|| "Agent 请求执行操作".to_string());
                        (handler_emit)(RuntimeEvent {
                            conversation_id: String::new(),
                            generation: 0,
                            agent_identity: String::new(),
                            core_turn_id: core_turn_id.clone(),
                            native_turn_id: turn_id,
                            event: AgentEvent::PermissionRequested {
                                request_id,
                                description,
                                options: vec![
                                    PermissionOption {
                                        option_id: "accept".to_string(),
                                        name: "允许".to_string(),
                                        kind: "allow_once".to_string(),
                                    },
                                    PermissionOption {
                                        option_id: "acceptForSession".to_string(),
                                        name: "本次会话允许".to_string(),
                                        kind: "allow_session".to_string(),
                                    },
                                    PermissionOption {
                                        option_id: "decline".to_string(),
                                        name: "拒绝".to_string(),
                                        kind: "reject".to_string(),
                                    },
                                ],
                            },
                        });
                    }
                    _ => {}
                }
                if let Some(id) = params.get("threadId").and_then(Value::as_str) {
                    if let Ok(mut guard) = handler_thread.lock() {
                        if guard.is_none() {
                            *guard = Some(id.to_string());
                        }
                    }
                }
            });

        let mut args = vec!["app-server".to_string()];
        for arg in &ctx.launch_args {
            if arg != "app-server" {
                args.push(arg.clone());
            }
        }
        let command = WorkspaceAccess::for_context(ctx)?.command(&ctx.launch_path, &args)?;
        let client = Arc::new(RpcClient::spawn(
            "Codex app-server",
            command,
            &ctx.working_dir,
            &[],
            handler,
        )?);
        client.request(
            "initialize",
            json!({
                "clientInfo": {
                    "name": "sujian",
                    "title": "素笺",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": { "experimentalApi": true }
            }),
            Duration::from_secs(30),
        )?;
        client.notify("initialized", json!({}))?;

        let config = if ctx.mcp_servers.is_empty() {
            Value::Null
        } else {
            let servers: serde_json::Map<String, Value> = ctx
                .mcp_servers
                .iter()
                .map(|server| {
                    let env: serde_json::Map<String, Value> = server
                        .env
                        .iter()
                        .map(|(key, value)| (key.clone(), Value::String(value.clone())))
                        .collect();
                    (
                        server.name.clone(),
                        json!({
                            "command": server.command,
                            "args": server.args,
                            "env": env,
                        }),
                    )
                })
                .collect();
            json!({ "mcp_servers": servers })
        };
        let mut params = json!({
            "cwd": ctx.working_dir.to_string_lossy(),
            "approvalPolicy": "on-request",
            "sandbox": "read-only",
        });
        if let Some(model) = ctx.model.as_ref() {
            params["model"] = Value::String(model.clone());
        }
        if !config.is_null() {
            params["config"] = config;
        }
        let resume_id = ctx
            .resume_native_session_id
            .clone()
            .filter(|id| !id.is_empty());
        let (result, resumed) = match resume_id {
            Some(id) => {
                let mut resume_params = params.clone();
                resume_params["threadId"] = Value::String(id);
                match client.request("thread/resume", resume_params, Duration::from_secs(60)) {
                    Ok(result) => (result, true),
                    Err(error) => {
                        log::warn!("[agent] Codex thread/resume failed, starting new: {error}");
                        (
                            client.request("thread/start", params, Duration::from_secs(60))?,
                            false,
                        )
                    }
                }
            }
            None => (
                client.request("thread/start", params, Duration::from_secs(60))?,
                false,
            ),
        };
        let native_thread_id = json_text(&result, &["thread", "id"])
            .or_else(|| json_text(&result, &["threadId"]))
            .ok_or_else(|| "Codex 未返回 thread id".to_string())?;
        let model = json_text(&result, &["model"]).or_else(|| ctx.model.clone());
        if let Ok(mut guard) = thread_id.lock() {
            *guard = Some(native_thread_id.clone());
        }
        (emit)(RuntimeEvent::session(AgentEvent::SessionStarted {
            native_session_id: native_thread_id,
            model: model.clone(),
        }));
        if resumed {
            log::info!("[agent] Codex thread resumed");
        }
        Ok(Self {
            client,
            thread_id,
            emit,
            approval_ids,
            model,
            active_core_turn_id,
            turn_bindings,
            turn_store,
        })
    }
}

impl AgentAdapter for CodexAdapter {
    fn prompt(&mut self, core_turn_id: &str, message: &str) -> Result<(), String> {
        let thread_id = self
            .thread_id
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .ok_or_else(|| "Codex thread 尚未建立".to_string())?;
        if let Ok(mut guard) = self.active_core_turn_id.lock() {
            *guard = core_turn_id.to_string();
        }
        let client = self.client.clone();
        let emit = self.emit.clone();
        let bindings = self.turn_bindings.clone();
        let turn_store = self.turn_store.clone();
        let round_turn_id = core_turn_id.to_string();
        let accepted_turn_id = round_turn_id.clone();
        // 在 spawn 前落定 Starting，保证随后到达的取消一定能看到本轮。
        self.turn_store
            .set(&round_turn_id, CodexTurnState::Starting);
        let text = message.to_string();
        let model = self.model.clone();
        std::thread::Builder::new()
            .name("agent-codex-turn".to_string())
            .spawn(move || {
                let mut params = json!({
                    "threadId": thread_id,
                    "input": [{ "type": "text", "text": text }],
                });
                if let Some(model) = model {
                    params["model"] = Value::String(model);
                }
                match client.request("turn/start", params, Duration::from_secs(60)) {
                    Ok(result) => {
                        let native_turn_id = json_text(&result, &["turn", "id"])
                            .or_else(|| json_text(&result, &["turnId"]))
                            .unwrap_or_default();
                        if !native_turn_id.is_empty() {
                            if let Ok(mut map) = bindings.lock() {
                                map.insert(native_turn_id.clone(), round_turn_id.clone());
                            }
                        }
                        let interrupt = turn_store.running(&round_turn_id, &native_turn_id);
                        (emit)(RuntimeEvent::turn(
                            accepted_turn_id,
                            native_turn_id.clone(),
                            AgentEvent::PromptAccepted,
                        ));
                        if interrupt {
                            let outcome =
                                interrupt_codex_turn(&client, &thread_id, &native_turn_id);
                            match outcome {
                                Ok(()) => {
                                    turn_store.resolve_cancel(&round_turn_id, Ok(native_turn_id))
                                }
                                Err(error) => turn_store.resolve_cancel(&round_turn_id, Err(error)),
                            }
                        }
                    }
                    Err(error) => {
                        turn_store.set(
                            &round_turn_id,
                            CodexTurnState::Finished(CodexFinish::Failed),
                        );
                        (emit)(RuntimeEvent::turn(
                            round_turn_id,
                            "",
                            AgentEvent::TurnFailed { message: error },
                        ));
                    }
                }
                // 轮次完成由 `turn/completed` / `turn/failed` 通知驱动。
            })
            .map_err(|error| format!("无法启动 Codex 请求线程：{error}"))?;
        Ok(())
    }

    fn native_session_id(&self) -> Option<String> {
        self.thread_id.lock().ok().and_then(|guard| guard.clone())
    }

    fn set_model(&mut self, model: Option<String>) {
        self.model = model;
    }

    fn cancel(&mut self, core_turn_id: &str) -> Result<CancelOutcome, String> {
        let thread_id = self
            .thread_id
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .ok_or_else(|| "Codex thread 尚未建立".to_string())?;
        let previous = match self.turn_store.begin_cancel(core_turn_id) {
            Some(state) => state,
            None => return Ok(CancelOutcome::AlreadyFinished),
        };
        match previous {
            CodexTurnState::Finished(_) => Ok(CancelOutcome::AlreadyFinished),
            CodexTurnState::Cancelled(_) => Ok(CancelOutcome::Cancelled),
            CodexTurnState::CancelFailed(error) => {
                // 前一次中断失败；让调用方看到真实原因，而不是再次谎报已取消。
                self.turn_store
                    .set(core_turn_id, CodexTurnState::CancelFailed(error.clone()));
                Err(error)
            }
            CodexTurnState::Running(native_turn_id) => {
                match interrupt_codex_turn(&self.client, &thread_id, &native_turn_id) {
                    Ok(()) => {
                        self.turn_store
                            .resolve_cancel(core_turn_id, Ok(native_turn_id));
                        Ok(CancelOutcome::Cancelled)
                    }
                    Err(error) => {
                        self.turn_store
                            .resolve_cancel(core_turn_id, Err(error.clone()));
                        Err(error)
                    }
                }
            }
            CodexTurnState::Starting | CodexTurnState::Cancelling(_) => {
                // `turn/start` 还没返回 native id，或请求线程正在发送中断。
                // 等待本轮自己的响应/中断结果，绝不读取下一轮的共享 id。
                self.turn_store
                    .wait_cancel(core_turn_id, Duration::from_secs(15))
            }
        }
    }

    fn respond_permission(&mut self, request_id: &str, option_id: &str) -> Result<(), String> {
        let rpc_id = self
            .approval_ids
            .lock()
            .ok()
            .and_then(|mut guard| guard.remove(request_id))
            .ok_or_else(|| "权限请求已失效".to_string())?;
        let decision = if option_id.is_empty() {
            "cancel"
        } else {
            option_id
        };
        self.client.respond(rpc_id, json!({ "decision": decision }))
    }
}
