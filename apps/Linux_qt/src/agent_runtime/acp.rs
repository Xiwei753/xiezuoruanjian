//! ACP (Agent Client Protocol) stdio 适配器。
//!
//! 只按握手结果调用真实存在的能力：`initialize` 协商版本与认证方式，
//! `session/new` / `session/load` 建立或恢复会话，`session/prompt` 发送请求，
//! `session/update` 通知转成统一事件。MCP 作为 stdio server 配置随会话传递。

use std::collections::{HashMap, HashSet};
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

const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);

pub struct AcpAdapter {
    client: Arc<RpcClient>,
    session_id: Arc<Mutex<Option<String>>>,
    emit: EventSink,
    permission_ids: Arc<Mutex<HashMap<String, i64>>>,
    /// 当前轮次的固定 Core Turn。ACP 的 `session/update` 通知不带轮次 ID，
    /// 由串行派发器在真正发出请求时写入；上一轮终态前不切换。
    active_core_turn_id: Arc<Mutex<String>>,
    /// 会话级异步串行派发器。每个 prompt 线程都必须先取得这把锁，才能写入
    /// `active_core_turn_id` 并发送 `session/prompt`。等待发生在线程内，不能
    /// 持有 `AgentSessionManager` 互斥锁，否则取消线程无法发送 `session/cancel`。
    dispatch_lock: Arc<Mutex<()>>,
    /// 已被用户取消、但还没轮到派发的 Core Turn。
    cancelled_turns: Arc<Mutex<HashSet<String>>>,
}

impl AcpAdapter {
    pub fn start(ctx: &AgentLaunchContext, emit: EventSink) -> Result<Self, String> {
        let session_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let permission_ids: Arc<Mutex<HashMap<String, i64>>> = Arc::new(Mutex::new(HashMap::new()));
        let active_core_turn_id: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let handler_emit = emit.clone();
        let handler_session = session_id.clone();
        let handler_permissions = permission_ids.clone();
        let handler_active_turn = active_core_turn_id.clone();
        let handler: Arc<dyn Fn(&str, Value) + Send + Sync> =
            Arc::new(move |method: &str, message: Value| {
                let params = message.get("params").cloned().unwrap_or_default();
                let core_turn_id = handler_active_turn
                    .lock()
                    .map(|guard| guard.clone())
                    .unwrap_or_default();
                match method {
                    "session/update" => {
                        let Some(update) = params.get("update") else {
                            return;
                        };
                        let update_kind = update
                            .get("sessionUpdate")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        match update_kind {
                            "agent_message_chunk" => {
                                if let Some(text) = update
                                    .get("content")
                                    .and_then(|content| json_text(content, &["text"]))
                                {
                                    (handler_emit)(RuntimeEvent::turn(core_turn_id.clone(), "", AgentEvent::TextDelta { text }));
                                }
                            }
                            "agent_thought_chunk" => {
                                if let Some(text) = update
                                    .get("content")
                                    .and_then(|content| json_text(content, &["text"]))
                                {
                                    (handler_emit)(RuntimeEvent::turn(core_turn_id.clone(), "", AgentEvent::ReasoningDelta { text }));
                                }
                            }
                            "tool_call" => {
                                let tool_call_id = json_text(update, &["toolCallId"])
                                    .unwrap_or_else(|| "tool".to_string());
                                let name = json_text(update, &["title"])
                                    .or_else(|| json_text(update, &["kind"]))
                                    .unwrap_or_else(|| "tool".to_string());
                                (handler_emit)(RuntimeEvent::turn(
                                    core_turn_id.clone(),
                                    "",
                                    AgentEvent::ToolStarted {
                                        tool_call_id,
                                        name,
                                        input_json: json_value_text(update.get("rawInput")),
                                    },
                                ));
                            }
                            "tool_call_update" => {
                                let tool_call_id = json_text(update, &["toolCallId"])
                                    .unwrap_or_else(|| "tool".to_string());
                                let name = json_text(update, &["title"])
                                    .unwrap_or_else(|| "tool".to_string());
                                let status = json_text(update, &["status"]).unwrap_or_default();
                                if matches!(status.as_str(), "completed" | "failed") {
                                    (handler_emit)(RuntimeEvent::turn(core_turn_id.clone(), "", AgentEvent::ToolFinished {
                                            tool_call_id,
                                            name,
                                            output_json: json_value_text(update.get("rawOutput"))
                                                .or_else(|| json_value_text(update.get("content"))),
                                            success: status == "completed",
                                        }));
                                }
                            }
                            _ => {}
                        }
                    }
                    "session/request_permission" => {
                        let Some(request_id) = message.get("id").and_then(Value::as_i64) else {
                            return;
                        };
                        let tool = params
                            .get("toolCall")
                            .and_then(|tool| json_text(tool, &["title"]))
                            .unwrap_or_else(|| "工具调用".to_string());
                        let options: Vec<PermissionOption> = params
                            .get("options")
                            .and_then(Value::as_array)
                            .map(|options| {
                                options
                                    .iter()
                                    .filter_map(|option| {
                                        Some(PermissionOption {
                                            option_id: json_text(option, &["optionId"])?,
                                            name: json_text(option, &["name"])
                                                .unwrap_or_else(|| "允许".to_string()),
                                            kind: json_text(option, &["kind"]).unwrap_or_default(),
                                        })
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        let permission_key = request_id.to_string();
                        if let Ok(mut guard) = handler_permissions.lock() {
                            guard.insert(permission_key.clone(), request_id);
                        }
                        (handler_emit)(RuntimeEvent::turn(core_turn_id.clone(), "", AgentEvent::PermissionRequested {
                                request_id: permission_key,
                                description: format!("Agent 请求执行：{tool}"),
                                options,
                            }));
                    }
                    _ => {}
                }
                // 会话 ID 来自 session/new 或 session/load 的响应；通知里也带，
                // 作为恢复时的补充事实。
                if let Some(id) = params.get("sessionId").and_then(Value::as_str) {
                    if let Ok(mut guard) = handler_session.lock() {
                        if guard.is_none() {
                            *guard = Some(id.to_string());
                        }
                    }
                }
            });

        let mut args = ctx.launch_args.clone();
        if ctx.agent_id == "gemini" && !args.iter().any(|arg| arg == "--acp") {
            args.push("--acp".to_string());
        }
        let command = WorkspaceAccess::for_context(ctx)?.command(&ctx.launch_path, &args)?;
        let client = Arc::new(RpcClient::spawn(
            "ACP Agent",
            command,
            &ctx.working_dir,
            &[],
            handler,
        )?);

        let init = client.request(
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientCapabilities": {
                    "fs": { "readTextFile": false, "writeTextFile": false },
                    "terminal": false
                },
                "clientInfo": {
                    "name": "sujian",
                    "title": "素笺",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
            Duration::from_secs(30),
        )?;
        if let Some(version) = init.get("protocolVersion").and_then(Value::as_i64) {
            if version < 1 {
                return Err(format!("ACP 协议版本不受支持：{version}"));
            }
        }

        let mcp_servers: Vec<Value> = ctx.mcp_servers.iter().map(|server| {
            json!({
                "name": server.name,
                "command": server.command,
                "args": server.args,
                "env": server.env.iter().cloned().map(|(key, value)| (key, serde_json::Value::String(value))).collect::<serde_json::Map<String, serde_json::Value>>()
            })
        }).collect();
        let session_params = json!({
            "cwd": ctx.working_dir.to_string_lossy(),
            "mcpServers": mcp_servers,
        });
        let resume_id = ctx
            .resume_native_session_id
            .clone()
            .filter(|id| !id.is_empty());
        let (session_result, resumed) = match resume_id {
            Some(id) => {
                let mut params = session_params.clone();
                params["sessionId"] = Value::String(id);
                match client.request("session/load", params, Duration::from_secs(60)) {
                    Ok(result) => (result, true),
                    Err(error) => {
                        log::warn!("[agent] ACP session/load failed, creating new: {error}");
                        (
                            client.request(
                                "session/new",
                                session_params,
                                Duration::from_secs(60),
                            )?,
                            false,
                        )
                    }
                }
            }
            None => (
                client.request("session/new", session_params, Duration::from_secs(60))?,
                false,
            ),
        };
        let native_session_id = json_text(&session_result, &["sessionId"])
            .ok_or_else(|| "ACP 未返回 sessionId".to_string())?;
        let model =
            json_text(&session_result, &["models", "currentModelId"]).or_else(|| ctx.model.clone());
        if let Ok(mut guard) = session_id.lock() {
            *guard = Some(native_session_id.clone());
        }
        let models: Vec<String> = session_result
            .get("models")
            .and_then(|models| models.get("availableModels"))
            .and_then(Value::as_array)
            .map(|models| {
                models
                    .iter()
                    .filter_map(|model| {
                        json_text(model, &["modelId"]).or_else(|| json_text(model, &["id"]))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if !models.is_empty() {
            (emit)(RuntimeEvent::session(AgentEvent::ModelsAvailable {
                models: models.clone(),
            }));
        }
        (emit)(RuntimeEvent::session(AgentEvent::SessionStarted {
            native_session_id,
            model,
        }));
        if resumed {
            log::info!("[agent] ACP session resumed");
        }

        Ok(Self {
            client,
            session_id,
            emit,
            permission_ids,
            active_core_turn_id,
            dispatch_lock: Arc::new(Mutex::new(())),
            cancelled_turns: Arc::new(Mutex::new(HashSet::new())),
        })
    }
}

impl AgentAdapter for AcpAdapter {
    fn prompt(&mut self, core_turn_id: &str, message: &str) -> Result<(), String> {
        let Some(session_id) = self.session_id.lock().ok().and_then(|guard| guard.clone()) else {
            return Err("ACP 会话尚未建立".to_string());
        };
        let client = self.client.clone();
        let emit = self.emit.clone();
        let active_turn = self.active_core_turn_id.clone();
        let dispatch = self.dispatch_lock.clone();
        let cancelled_turns = self.cancelled_turns.clone();
        let round_turn_id = core_turn_id.to_string();
        let accepted_turn_id = round_turn_id.clone();
        let text = message.to_string();
        std::thread::Builder::new()
            .name("agent-acp-prompt".to_string())
            .spawn(move || {
                // 等待上一轮终态只在 worker 中发生，绝不持有会话管理器锁；
                // 这样取消线程仍能及时写入 `session/cancel`。
                let _dispatch_guard = dispatch.lock();
                let cancelled_before_send = cancelled_turns
                    .lock()
                    .map(|mut guard| guard.remove(&round_turn_id))
                    .unwrap_or(false);
                if cancelled_before_send {
                    log::debug!("[agent] skip cancelled ACP turn {}", round_turn_id);
                    return;
                }
                if let Ok(mut guard) = active_turn.lock() {
                    *guard = round_turn_id.clone();
                }
                let accepted_emit = emit.clone();
                let accepted_cancelled = cancelled_turns.clone();
                let accepted_client = client.clone();
                let accepted_session = session_id.clone();
                let result = client.request_observed(
                    "session/prompt",
                    json!({
                        "sessionId": session_id,
                        "prompt": [{ "type": "text", "text": text }],
                    }),
                    REQUEST_TIMEOUT,
                    move || {
                        // 取消可能恰好发生在“检查队列”和“写入 stdin”之间。
                        // 写入成功后再次检查，若已取消则立刻补发 session/cancel。
                        let cancelled_after_send = accepted_cancelled
                            .lock()
                            .map(|mut guard| guard.remove(&accepted_turn_id))
                            .unwrap_or(false);
                        if cancelled_after_send {
                            let _ = accepted_client.notify(
                                "session/cancel",
                                json!({ "sessionId": accepted_session }),
                            );
                        }
                        (accepted_emit)(RuntimeEvent::turn(
                            accepted_turn_id.clone(),
                            "",
                            AgentEvent::PromptAccepted,
                        ));
                    },
                );
                match result {
                    Ok(_) => {
                        (emit)(RuntimeEvent::turn(round_turn_id, "", AgentEvent::TurnCompleted));
                    }
                    Err(error) => {
                        (emit)(RuntimeEvent::turn(
                            round_turn_id,
                            "",
                            AgentEvent::TurnFailed { message: error },
                        ));
                    }
                }
            })
            .map_err(|error| format!("无法启动 ACP 请求线程：{error}"))?;
        Ok(())
    }

    fn native_session_id(&self) -> Option<String> {
        self.session_id.lock().ok().and_then(|guard| guard.clone())
    }

    fn cancel(&mut self, core_turn_id: &str) -> Result<(), String> {
        let active = self
            .active_core_turn_id
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        if active != core_turn_id {
            // 轮次还在串行队列中：标记后由 worker 跳过。此时绝不能发
            // `session/cancel`，否则会误杀正在运行的另一轮。
            if let Ok(mut guard) = self.cancelled_turns.lock() {
                guard.insert(core_turn_id.to_string());
            }
            return Ok(());
        }
        let session_id = self
            .session_id
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .ok_or_else(|| "ACP 会话尚未建立".to_string())?;
        self.client
            .notify("session/cancel", json!({ "sessionId": session_id }))
    }

    fn respond_permission(&mut self, request_id: &str, option_id: &str) -> Result<(), String> {
        let rpc_id = self
            .permission_ids
            .lock()
            .ok()
            .and_then(|mut guard| guard.remove(request_id))
            .ok_or_else(|| "权限请求已失效".to_string())?;
        let result = if option_id.is_empty() {
            json!({ "outcome": { "outcome": "cancelled" } })
        } else {
            json!({ "outcome": { "outcome": "selected", "optionId": option_id } })
        };
        self.client.respond(rpc_id, result)
    }
}
