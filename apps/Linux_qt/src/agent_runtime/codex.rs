//! Codex `app-server` 适配器。
//!
//! 使用官方机器接口：`initialize` → `initialized` → `thread/start` 或
//! `thread/resume` → `turn/start`，把 app-server 通知映射为统一事件。
//! 复用用户已有的 `~/.codex` 登录环境；不启动交互式 TUI。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{json, Value};

use super::adapter::AgentAdapter;
use super::events::{
    json_text, json_value_text, AgentEvent, AgentLaunchContext, EventSink, PermissionOption,
    RuntimeEvent,
};
use super::rpc::RpcClient;
use super::workspace_access::WorkspaceAccess;

pub struct CodexAdapter {
    client: Arc<RpcClient>,
    thread_id: Arc<Mutex<Option<String>>>,
    turn_id: Arc<Mutex<Option<String>>>,
    emit: EventSink,
    approval_ids: Arc<Mutex<HashMap<String, i64>>>,
    model: Option<String>,
    /// 当前轮次的固定 Core Turn。`turn/started` 等通知只有原生 turnId 时，
    /// 用它建立本轮的 native → Core 映射。
    active_core_turn_id: Arc<Mutex<String>>,
    /// 原生 turnId → Core Turn 的稳定映射。旧轮次迟到通知始终回到旧 Turn，
    /// 不会因为用户已经发送下一轮而改记到新回答。
    turn_bindings: Arc<Mutex<HashMap<String, String>>>,
}

fn core_for_native_turn(
    native_turn_id: &str,
    active: &Arc<Mutex<String>>,
    bindings: &Arc<Mutex<HashMap<String, String>>>,
) -> String {
    let current = active
        .lock()
        .map(|guard| guard.clone())
        .unwrap_or_default();
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

impl CodexAdapter {
    pub fn start(ctx: &AgentLaunchContext, emit: EventSink) -> Result<Self, String> {
        let thread_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let turn_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let approval_ids: Arc<Mutex<HashMap<String, i64>>> = Arc::new(Mutex::new(HashMap::new()));
        let active_core_turn_id: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let turn_bindings: Arc<Mutex<HashMap<String, String>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let handler_emit = emit.clone();
        let handler_thread = thread_id.clone();
        let handler_turn = turn_id.clone();
        let handler_approvals = approval_ids.clone();
        let handler_active_turn = active_core_turn_id.clone();
        let handler_bindings = turn_bindings.clone();
        let handler: Arc<dyn Fn(&str, Value) + Send + Sync> =
            Arc::new(move |method: &str, message: Value| {
                let params = message.get("params").cloned().unwrap_or_default();
                let turn_id = json_text(&params, &["turnId"]).unwrap_or_default();
                let core_turn_id =
                    core_for_native_turn(&turn_id, &handler_active_turn, &handler_bindings);
                match method {
                    "turn/started" => {
                        if let Some(id) = json_text(&params, &["turn", "id"])
                            .or_else(|| json_text(&params, &["turnId"]))
                        {
                            if let Ok(mut guard) = handler_turn.lock() {
                                *guard = Some(id);
                            }
                        }
                    }
                    "item/agentMessage/delta" => {
                        if let Some(text) = json_text(&params, &["delta"]) {
                            (handler_emit)(RuntimeEvent {
                                conversation_id: String::new(),
                                generation: 0,
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
                        (handler_emit)(RuntimeEvent {
                            conversation_id: String::new(),
                            generation: 0,
                            core_turn_id: core_turn_id.clone(),
                            native_turn_id: turn_id,
                            event: AgentEvent::TurnCompleted,
                        });
                    }
                    "turn/failed" | "error" => {
                        let message = json_text(&params, &["error", "message"])
                            .or_else(|| json_text(&params, &["message"]))
                            .unwrap_or_else(|| "Codex 请求失败".to_string());
                        (handler_emit)(RuntimeEvent {
                            conversation_id: String::new(),
                            generation: 0,
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
            turn_id,
            emit,
            approval_ids,
            model,
            active_core_turn_id,
            turn_bindings,
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
        let turn_state = self.turn_id.clone();
        let bindings = self.turn_bindings.clone();
        let round_turn_id = core_turn_id.to_string();
        let accepted_turn_id = round_turn_id.clone();
        if let Ok(mut guard) = turn_state.lock() {
            *guard = None;
        }
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
                        if let Some(id) = json_text(&result, &["turn", "id"])
                            .or_else(|| json_text(&result, &["turnId"]))
                        {
                            if let Ok(mut map) = bindings.lock() {
                                map.insert(id.clone(), round_turn_id.clone());
                            }
                            if let Ok(mut guard) = turn_state.lock() {
                                *guard = Some(id);
                            }
                        }
                        (emit)(RuntimeEvent::turn(
                            accepted_turn_id,
                            "",
                            AgentEvent::PromptAccepted,
                        ));
                    }
                    Err(error) => (emit)(RuntimeEvent::turn(
                        round_turn_id,
                        "",
                        AgentEvent::TurnFailed { message: error },
                    )),
                }
                // 轮次完成由 `turn/completed` / `turn/failed` 通知驱动。
            })
            .map_err(|error| format!("无法启动 Codex 请求线程：{error}"))?;
        Ok(())
    }

    fn native_session_id(&self) -> Option<String> {
        self.thread_id.lock().ok().and_then(|guard| guard.clone())
    }

    fn cancel(&mut self, core_turn_id: &str) -> Result<(), String> {
        let active = self
            .active_core_turn_id
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        if active != core_turn_id {
            return Ok(());
        }
        let thread_id = self
            .thread_id
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .ok_or_else(|| "Codex thread 尚未建立".to_string())?;
        let turn_id = self
            .turn_id
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .ok_or_else(|| "Codex 轮次尚未建立".to_string())?;
        if let Ok(mut guard) = self.active_core_turn_id.lock() {
            guard.clear();
        }
        self.client.request(
            "turn/interrupt",
            json!({ "threadId": thread_id, "turnId": turn_id }),
            Duration::from_secs(10),
        )?;
        Ok(())
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
