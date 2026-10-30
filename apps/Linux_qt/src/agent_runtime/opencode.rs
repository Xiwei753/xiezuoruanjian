//! OpenCode Server 适配器。
//!
//! 适配 OpenCode 2.x 的 `/api/*` Server API：启动本地 `opencode serve`，
//! 用 HTTP Basic 鉴权创建/恢复 session，通过 `/api/event` SSE 接收文本、
//! 推理、工具和权限事件，用 `/api/session/:id/prompt` 发送消息、
//! `/api/session/:id/interrupt` 中断。进程和服务器生命周期由本适配器管理。

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::adapter::AgentAdapter;
use super::events::{
    json_text, json_value_text, AgentEvent, AgentLaunchContext, EventSink, PermissionOption,
    RuntimeEvent,
};
use super::workspace_access::WorkspaceAccess;

const READY_TIMEOUT: Duration = Duration::from_secs(20);
const SERVER_USER: &str = "opencode";

pub struct OpenCodeAdapter {
    client: reqwest::blocking::Client,
    base_url: String,
    password: String,
    working_dir: PathBuf,
    session_id: Arc<Mutex<Option<String>>>,
    child: Arc<Mutex<Child>>,
    emit: EventSink,
    model_hint: Option<String>,
}

fn free_loopback_port() -> Result<u16, String> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))
        .map_err(|error| format!("无法分配本地端口：{error}"))?;
    listener
        .local_addr()
        .map(|address| address.port())
        .map_err(|error| error.to_string())
}

fn response_error(response: reqwest::blocking::Response) -> String {
    let status = response.status();
    let body = response.text().unwrap_or_default();
    let detail = body.chars().take(300).collect::<String>();
    format!("HTTP {status}: {detail}")
}

fn location_value(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn mcp_status(
    client: &reqwest::blocking::Client,
    base_url: &str,
    password: &str,
    working_dir: &Path,
    server_name: &str,
) -> Result<(String, Option<String>, Vec<String>), String> {
    let response = client
        .get(format!("{base_url}/api/mcp"))
        .basic_auth(SERVER_USER, Some(password))
        .query(&[("location[directory]", location_value(working_dir))])
        .send()
        .map_err(|error| format!("读取 OpenCode MCP 状态失败：{error}"))?;
    let value: Value = response.json().map_err(|error| error.to_string())?;
    let entry = value
        .get("data")
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .find(|item| json_text(item, &["name"]).as_deref() == Some(server_name))
        });
    let Some(entry) = entry else {
        return Err(format!("OpenCode 未返回 MCP {} 的状态", server_name));
    };
    let status_value = entry.get("status").unwrap_or(entry);
    let status = json_text(status_value, &["status"])
        .or_else(|| json_text(entry, &["status"]))
        .unwrap_or_else(|| "configured".to_string());
    let error = json_text(status_value, &["error"])
        .or_else(|| json_text(entry, &["error"]));
    let tools = status_value
        .get("tools")
        .or_else(|| entry.get("tools"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    item.as_str()
                        .map(str::to_string)
                        .or_else(|| json_text(item, &["name"]))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok((status, error, tools))
}

fn emit_mcp_status(
    emit: &EventSink,
    server: &str,
    status: String,
    error: Option<String>,
    tools: Vec<String>,
) {
    (emit)(RuntimeEvent::session(AgentEvent::McpConnectionChanged {
        server: server.to_string(),
        status,
        error,
        tools,
    }));
}

impl OpenCodeAdapter {
    pub fn start(ctx: &AgentLaunchContext, emit: EventSink) -> Result<Self, String> {
        let port = free_loopback_port()?;
        // 随机密码只用于本机回环服务，避免 opencode 把随机凭据写到用户终端，
        // 也避免同机其他进程直接调用该 Server。
        let password = format!("sujian-{}", uuid::Uuid::new_v4());
        // Agent 自身和它启动的 Shell/文件工具都进入 bwrap：作品目录只读，
        // 只有外部 MCP broker 能通过 Core 事务写回对象元数据。
        let mut command =
            WorkspaceAccess::for_context(ctx)?.command(&ctx.launch_path, &ctx.launch_args);
        let child = command
            .args([
                "serve",
                "--hostname",
                "127.0.0.1",
                "--port",
                &port.to_string(),
            ])
            .current_dir(&ctx.working_dir)
            .env("OPENCODE_PASSWORD", &password)
            .env("OPENCODE_SERVER_PASSWORD", &password)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| format!("无法启动 opencode serve：{error}"))?;
        let child = Arc::new(Mutex::new(child));
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| error.to_string())?;
        let base_url = format!("http://127.0.0.1:{port}");

        let started = Instant::now();
        loop {
            let ready = client
                .get(format!("{base_url}/api/session"))
                .basic_auth(SERVER_USER, Some(&password))
                .send()
                .map(|response| response.status().is_success())
                .unwrap_or(false);
            if ready {
                break;
            }
            if started.elapsed() >= READY_TIMEOUT {
                if let Ok(mut child) = child.lock() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
                return Err("opencode serve 启动超时".to_string());
            }
            std::thread::sleep(Duration::from_millis(150));
        }

        let resume_id = ctx
            .resume_native_session_id
            .clone()
            .filter(|id| !id.is_empty());
        let session_id = match resume_id {
            Some(id) => {
                let exists = client
                    .get(format!("{base_url}/api/session/{id}"))
                    .basic_auth(SERVER_USER, Some(&password))
                    .query(&[("location[directory]", location_value(&ctx.working_dir))])
                    .send()
                    .map(|response| response.status().is_success())
                    .unwrap_or(false);
                if exists {
                    id
                } else {
                    create_session(
                        &client,
                        &base_url,
                        &password,
                        &ctx.working_dir,
                        &ctx.project_title,
                    )?
                }
            }
            None => create_session(
                &client,
                &base_url,
                &password,
                &ctx.working_dir,
                &ctx.project_title,
            )?,
        };

        let available_models = fetch_models(
            &client,
            &base_url,
            &password,
            &ctx.working_dir,
        )
        .unwrap_or_else(|error| {
            log::warn!("[agent] OpenCode model list failed: {error}");
            Vec::new()
        });

        // MCP：走 OpenCode 2.x 的 server 配置接口；失败只记录，不影响聊天。
        for server in &ctx.mcp_servers {
            let mut command = vec![server.command.clone()];
            command.extend(server.args.iter().cloned());
            let environment: serde_json::Map<String, Value> = server
                .env
                .iter()
                .map(|(key, value)| (key.clone(), Value::String(value.clone())))
                .collect();
            let body = json!({
                "config": {
                    "type": "local",
                    "command": command,
                    "environment": environment,
                    "enabled": true
                }
            });
            let response = client
                .put(format!(
                    "{base_url}/api/experimental/mcp/{}",
                    url_path_segment(&server.name)
                ))
                .basic_auth(SERVER_USER, Some(&password))
                .query(&[("location[directory]", location_value(&ctx.working_dir))])
                .json(&body)
                .send();
            match response {
                Ok(response) if response.status().is_success() => {
                    let connect = client
                        .post(format!(
                            "{base_url}/api/experimental/mcp/{}/connect",
                            url_path_segment(&server.name)
                        ))
                        .basic_auth(SERVER_USER, Some(&password))
                        .query(&[("location[directory]", location_value(&ctx.working_dir))])
                        .send();
                    match connect {
                        Ok(response) if response.status().is_success() => {
                            match mcp_status(
                                &client,
                                &base_url,
                                &password,
                                &ctx.working_dir,
                                &server.name,
                            ) {
                                Ok((status, error, tools)) => {
                                    log::info!(
                                        "[agent] OpenCode MCP {} status={status}",
                                        server.name
                                    );
                                    emit_mcp_status(
                                        &emit,
                                        &server.name,
                                        status,
                                        error,
                                        tools,
                                    );
                                }
                                Err(error) => {
                                    log::warn!(
                                        "[agent] OpenCode MCP status unavailable: {error}"
                                    );
                                    emit_mcp_status(
                                        &emit,
                                        &server.name,
                                        "configured".to_string(),
                                        Some(error),
                                        Vec::new(),
                                    );
                                }
                            }
                        }
                        Ok(response) => {
                            let error = response_error(response);
                            log::warn!(
                                "[agent] OpenCode MCP connect rejected: {error}"
                            );
                            emit_mcp_status(
                                &emit,
                                &server.name,
                                "failed".to_string(),
                                Some(error),
                                Vec::new(),
                            );
                        }
                        Err(error) => {
                            let message = format!("连接 MCP 失败：{error}");
                            log::warn!(
                                "[agent] OpenCode MCP connect failed for {}: {message}",
                                server.name
                            );
                            emit_mcp_status(
                                &emit,
                                &server.name,
                                "failed".to_string(),
                                Some(message),
                                Vec::new(),
                            );
                        }
                    }
                }
                Ok(response) => {
                    let error = response_error(response);
                    log::warn!(
                        "[agent] OpenCode MCP rejected: {error}"
                    );
                    emit_mcp_status(
                        &emit,
                        &server.name,
                        "failed".to_string(),
                        Some(error),
                        Vec::new(),
                    );
                }
                Err(error) => {
                    let message = format!("配置 MCP 失败：{error}");
                    log::warn!("[agent] OpenCode MCP request failed: {message}");
                    emit_mcp_status(
                        &emit,
                        &server.name,
                        "failed".to_string(),
                        Some(message),
                        Vec::new(),
                    );
                }
            }
        }

        let session_id = Arc::new(Mutex::new(Some(session_id.clone())));
        let sse_session = session_id.clone();
        let sse_emit = emit.clone();
        let sse_url = base_url.clone();
        // SSE 是长连接，不能使用带 30 秒总超时的请求客户端。
        let sse_client = reqwest::blocking::Client::builder()
            .build()
            .map_err(|error| error.to_string())?;
        let sse_password = password.clone();
        let sse_directory = location_value(&ctx.working_dir);
        let sse_http = client.clone();
        std::thread::Builder::new()
            .name("agent-opencode-events".to_string())
            .spawn(move || {
                OpenCodeEventProcessor::new(
                    sse_http,
                    sse_client,
                    sse_url,
                    sse_password,
                    sse_directory,
                    sse_session,
                    sse_emit,
                )
                .run();
            })
            .map_err(|error| format!("无法启动 OpenCode 事件线程：{error}"))?;

        if !available_models.is_empty() {
            (emit)(RuntimeEvent {
                conversation_id: String::new(),
                generation: 0,
                core_turn_id: String::new(),
                native_turn_id: String::new(),
                event: AgentEvent::ModelsAvailable {
                    models: available_models,
                },
            });
        }
        (emit)(RuntimeEvent {
            conversation_id: String::new(),
            generation: 0,
            core_turn_id: String::new(),
            native_turn_id: String::new(),
            event: AgentEvent::SessionStarted {
                native_session_id: session_id
                    .lock()
                    .ok()
                    .and_then(|guard| guard.clone())
                    .unwrap_or_default(),
                model: ctx.model.clone(),
            },
        });

        Ok(Self {
            client,
            base_url,
            password,
            working_dir: ctx.working_dir.clone(),
            session_id,
            child,
            emit,
            model_hint: ctx.model.clone(),
        })
    }
}

struct OpenCodeEventProcessor {
    client: reqwest::blocking::Client,
    sse_client: reqwest::blocking::Client,
    base_url: String,
    password: String,
    directory: String,
    session_id: Arc<Mutex<Option<String>>>,
    emit: EventSink,
    text_state: HashMap<String, String>,
    seen_ids: HashSet<String>,
    last_seq: u64,
    disconnected: bool,
}

impl OpenCodeEventProcessor {
    fn new(
        client: reqwest::blocking::Client,
        sse_client: reqwest::blocking::Client,
        base_url: String,
        password: String,
        directory: String,
        session_id: Arc<Mutex<Option<String>>>,
        emit: EventSink,
    ) -> Self {
        Self {
            client,
            sse_client,
            base_url,
            password,
            directory,
            session_id,
            emit,
            text_state: HashMap::new(),
            seen_ids: HashSet::new(),
            last_seq: 0,
            disconnected: false,
        }
    }

    fn run(mut self) {
        self.prime_cursor();
        let mut failures = 0u32;
        loop {
            if self.disconnected {
                if let Err(error) = self.replay_missed_events() {
                    log::warn!("[agent] OpenCode 恢复事件失败: {error}");
                }
            }
            let started = Instant::now();
            match self.open_live_stream() {
                Ok(()) => log::warn!("[agent] OpenCode /api/event 已结束"),
                Err(error) => log::warn!("[agent] OpenCode /api/event 断开: {error}"),
            }
            self.disconnected = true;
            if started.elapsed() >= Duration::from_secs(1) {
                failures = 0;
            }
            failures = failures.saturating_add(1);
            if failures >= 20 {
                (self.emit)(RuntimeEvent::session(AgentEvent::TurnFailed {
                    message: "OpenCode 事件流已断开，无法恢复本轮输出".to_string(),
                }));
                return;
            }
            let delay = 250u64.saturating_mul(u64::from(failures.min(8)));
            std::thread::sleep(Duration::from_millis(delay));
        }
    }

    /// 建立会话时先取一次 durable log 的同步水位，避免断线重放把旧轮次
    /// 的历史文本重新灌进当前 Core Turn。
    fn prime_cursor(&mut self) {
        let Ok(response) = self.log_request(0) else {
            return;
        };
        let _ = read_sse_frames(BufReader::new(response), |event_name, _id, data| {
            let Ok(event) = serde_json::from_str::<Value>(data) else {
                return;
            };
            let event_type = json_text(&event, &["type"]).unwrap_or_default();
            if event_type == "log.synced" || !event_name.is_empty() {
                self.observe_sequence(&event);
            }
        });
    }

    fn replay_missed_events(&mut self) -> Result<(), String> {
        let response = self.log_request(self.last_seq)?;
        read_sse_frames(BufReader::new(response), |event_name, id, data| {
            self.handle_frame(event_name, id, data);
        })
    }

    fn log_request(&self, after: u64) -> Result<reqwest::blocking::Response, String> {
        let session_id = self
            .session_id
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .ok_or_else(|| "OpenCode session 尚未建立".to_string())?;
        let response = self
            .client
            .get(format!(
                "{}/api/experimental/session/{session_id}/log",
                self.base_url
            ))
            .basic_auth(SERVER_USER, Some(&self.password))
            .query(&[
                ("after", after.to_string()),
                ("follow", "false".to_string()),
                ("location[directory]", self.directory.clone()),
            ])
            .send()
            .map_err(|error| format!("读取 OpenCode 事件日志失败：{error}"))?;
        if !response.status().is_success() {
            return Err(response_error(response));
        }
        Ok(response)
    }

    fn open_live_stream(&mut self) -> Result<(), String> {
        let response = self
            .sse_client
            .get(format!("{}/api/event", self.base_url))
            .basic_auth(SERVER_USER, Some(&self.password))
            .query(&[("location[directory]", self.directory.as_str())])
            .send()
            .map_err(|error| error.to_string())?;
        if !response.status().is_success() {
            return Err(response_error(response));
        }
        read_sse_frames(BufReader::new(response), |event_name, id, data| {
            self.handle_frame(event_name, id, data);
        })
    }

    fn observe_sequence(&mut self, event: &Value) {
        let aggregate = json_text(event, &["durable", "aggregateID"])
            .or_else(|| json_text(event, &["aggregateID"]))
            .unwrap_or_default();
        let current_session = self
            .session_id
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .unwrap_or_default();
        if !aggregate.is_empty() && aggregate != current_session {
            return;
        }
        let seq = event
            .get("durable")
            .and_then(|durable| durable.get("seq"))
            .or_else(|| event.get("seq"))
            .and_then(Value::as_u64);
        if let Some(seq) = seq {
            self.last_seq = self.last_seq.max(seq);
        }
    }

    fn handle_frame(&mut self, event_name: &str, id: &str, data: &str) {
        let Ok(event) = serde_json::from_str::<Value>(data) else {
            log::debug!("[agent] 忽略无法解析的 OpenCode 事件：{data}");
            return;
        };
        let json_type = json_text(&event, &["type"]).unwrap_or_default();
        let event_type = if !event_name.is_empty() && event_name != "message" {
            event_name.to_string()
        } else {
            json_type
        };
        self.observe_sequence(&event);
        if event_type == "log.synced" {
            return;
        }
        let event_id = if !id.is_empty() {
            id.to_string()
        } else {
            json_text(&event, &["id"]).unwrap_or_default()
        };
        if !event_id.is_empty() {
            if !self.seen_ids.insert(event_id.clone()) {
                return;
            }
            if self.seen_ids.len() > 4096 {
                self.seen_ids.clear();
                self.seen_ids.insert(event_id);
            }
        }
        let payload = event
            .get("data")
            .cloned()
            .unwrap_or_else(|| event.clone());
        if !event_matches_session(&payload, &self.session_id) {
            return;
        }
        let native_turn_id = json_text(&payload, &["assistantMessageID"])
            .or_else(|| json_text(&payload, &["messageID"]))
            .unwrap_or_default();
        self.handle_event(&event_type, &payload, native_turn_id);
    }

    fn handle_event(&mut self, event_type: &str, payload: &Value, native_turn_id: String) {
        match event_type {
            "session.text.delta" | "session.reasoning.delta" => {
                let Some(delta) = json_text(payload, &["delta"]) else {
                    return;
                };
                if delta.is_empty() {
                    return;
                }
                let key = text_key(payload, event_type);
                self.text_state
                    .entry(key)
                    .or_default()
                    .push_str(delta.as_str());
                let event = if event_type == "session.text.delta" {
                    AgentEvent::TextDelta { text: delta }
                } else {
                    AgentEvent::ReasoningDelta { text: delta }
                };
                (self.emit)(RuntimeEvent::native(native_turn_id, event));
            }
            "session.text.ended" | "session.reasoning.ended" => {
                let Some(full) = json_text(payload, &["text"]) else {
                    return;
                };
                let key = text_key(payload, event_type);
                let previous = self.text_state.entry(key).or_default();
                let text = if full.starts_with(previous.as_str()) {
                    full[previous.len()..].to_string()
                } else if previous.is_empty() {
                    full.clone()
                } else {
                    String::new()
                };
                *previous = full;
                if text.is_empty() {
                    return;
                }
                let event = if event_type == "session.text.ended" {
                    AgentEvent::TextDelta { text }
                } else {
                    AgentEvent::ReasoningDelta { text }
                };
                (self.emit)(RuntimeEvent::native(native_turn_id, event));
            }
            "session.tool.input.started" | "session.tool.called" => {
                let tool_call_id = json_text(payload, &["id"])
                    .or_else(|| json_text(payload, &["toolCallID"]))
                    .unwrap_or_else(|| "tool".to_string());
                let name = json_text(payload, &["name"])
                    .or_else(|| json_text(payload, &["tool"]))
                    .unwrap_or_else(|| "工具".to_string());
                (self.emit)(RuntimeEvent::native(
                    native_turn_id,
                    AgentEvent::ToolStarted {
                        tool_call_id,
                        name,
                        input_json: json_value_text(payload.get("input")),
                    },
                ));
            }
            "session.tool.input.ended" => {
                let Some(tool_call_id) = json_text(payload, &["id"]) else {
                    return;
                };
                (self.emit)(RuntimeEvent::native(
                    native_turn_id,
                    AgentEvent::ToolStarted {
                        tool_call_id,
                        name: json_text(payload, &["name"]).unwrap_or_else(|| "工具".to_string()),
                        input_json: json_text(payload, &["text"])
                            .or_else(|| json_value_text(payload.get("input"))),
                    },
                ));
            }
            "session.tool.success" | "session.tool.failed" => {
                let tool_call_id = json_text(payload, &["id"])
                    .or_else(|| json_text(payload, &["toolCallID"]))
                    .unwrap_or_else(|| "tool".to_string());
                let success = event_type == "session.tool.success";
                let output = if success {
                    json_value_text(payload.get("content"))
                        .or_else(|| json_value_text(payload.get("metadata")))
                } else {
                    json_text(payload, &["error"])
                        .or_else(|| json_value_text(payload.get("content")))
                };
                (self.emit)(RuntimeEvent::native(
                    native_turn_id,
                    AgentEvent::ToolFinished {
                        tool_call_id,
                        name: json_text(payload, &["name"]).unwrap_or_else(|| "工具".to_string()),
                        output_json: output,
                        success,
                    },
                ));
            }
            "permission.asked" => {
                let Some(request_id) = json_text(payload, &["id"]) else {
                    return;
                };
                let description = json_text(payload, &["message"])
                    .or_else(|| json_text(payload, &["action"]))
                    .unwrap_or_else(|| "Agent 请求执行工具".to_string());
                (self.emit)(RuntimeEvent::native(
                    native_turn_id,
                    AgentEvent::PermissionRequested {
                        request_id,
                        description,
                        options: vec![
                            PermissionOption {
                                option_id: "once".to_string(),
                                name: "允许一次".to_string(),
                                kind: "allow_once".to_string(),
                            },
                            PermissionOption {
                                option_id: "always".to_string(),
                                name: "始终允许".to_string(),
                                kind: "allow_always".to_string(),
                            },
                            PermissionOption {
                                option_id: "reject".to_string(),
                                name: "拒绝".to_string(),
                                kind: "reject".to_string(),
                            },
                        ],
                    },
                ));
            }
            "session.status" => {
                let status = json_text(payload, &["status", "type"]).unwrap_or_default();
                if status == "idle" {
                    (self.emit)(RuntimeEvent::native(
                        native_turn_id,
                        AgentEvent::TurnCompleted,
                    ));
                }
            }
            "session.idle" => {
                (self.emit)(RuntimeEvent::native(
                    native_turn_id,
                    AgentEvent::TurnCompleted,
                ));
            }
            "session.error" | "session.execution.failed" | "session.step.failed" => {
                let message = json_text(payload, &["error", "message"])
                    .or_else(|| json_text(payload, &["error", "data", "message"]))
                    .or_else(|| json_text(payload, &["error"]))
                    .or_else(|| json_text(payload, &["message"]))
                    .unwrap_or_else(|| "OpenCode 请求失败".to_string());
                (self.emit)(RuntimeEvent::native(
                    native_turn_id,
                    AgentEvent::TurnFailed { message },
                ));
            }
            _ => {}
        }
    }
}

/// 按空行结帧的 SSE 解码器，支持 `event:`、`id:`、多条 `data:` 与注释心跳。
fn read_sse_frames<F>(reader: impl BufRead, mut on_frame: F) -> Result<(), String>
where
    F: FnMut(&str, &str, &str),
{
    let mut event_name = String::new();
    let mut event_id = String::new();
    let mut data_lines: Vec<String> = Vec::new();
    for line in reader.lines() {
        let line = line.map_err(|error| error.to_string())?;
        let line = line.strip_suffix('\r').unwrap_or(&line);
        if line.is_empty() {
            if !data_lines.is_empty() {
                let data = data_lines.join("\n");
                on_frame(&event_name, &event_id, &data);
                event_name.clear();
                event_id.clear();
                data_lines.clear();
            }
            continue;
        }
        if line.starts_with(':') {
            continue;
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => continue,
        };
        match field {
            "event" => event_name = value.to_string(),
            "id" => event_id = value.to_string(),
            "data" => data_lines.push(value.to_string()),
            _ => {}
        }
    }
    if !data_lines.is_empty() {
        let data = data_lines.join("\n");
        on_frame(&event_name, &event_id, &data);
    }
    Ok(())
}

fn text_key(payload: &Value, prefix: &str) -> String {
    let message_id = json_text(payload, &["assistantMessageID"])
        .or_else(|| json_text(payload, &["messageID"]))
        .unwrap_or_default();
    let ordinal = payload
        .get("ordinal")
        .and_then(Value::as_i64)
        .map(|value| value.to_string())
        .unwrap_or_default();
    // delta 与 ended 必须落在同一个累加槽位，否则 ended 会把完整文本再追加
    // 一遍，造成重复回答。
    let kind = if prefix.contains("reasoning") {
        "reasoning"
    } else {
        "text"
    };
    format!("{kind}:{message_id}:{ordinal}")
}

fn event_matches_session(payload: &Value, session: &Arc<Mutex<Option<String>>>) -> bool {
    let event_session = json_text(payload, &["sessionID"])
        .or_else(|| json_text(payload, &["sessionId"]))
        .unwrap_or_default();
    if event_session.is_empty() {
        return true;
    }
    session
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
        .is_some_and(|current| current == event_session)
}

fn url_path_segment(value: &str) -> String {
    // OpenCode 的 MCP server 名来自我们自己的固定绑定（sujian），这里只做
    // 最小转义，避免名称中的空格/斜杠破坏 endpoint。
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

fn fetch_models(
    client: &reqwest::blocking::Client,
    base_url: &str,
    password: &str,
    working_dir: &Path,
) -> Result<Vec<String>, String> {
    let response = client
        .get(format!("{base_url}/api/model"))
        .basic_auth(SERVER_USER, Some(password))
        .query(&[("location[directory]", location_value(working_dir))])
        .send()
        .map_err(|error| format!("读取 OpenCode 模型失败：{error}"))?;
    if !response.status().is_success() {
        return Err(response_error(response));
    }
    let value: Value = response.json().map_err(|error| error.to_string())?;
    let Some(models) = value.get("data").and_then(Value::as_array) else {
        return Ok(Vec::new());
    };
    let mut result = BTreeSet::new();
    for model in models {
        let provider = json_text(model, &["providerID"]);
        let model_id = json_text(model, &["modelID"]).or_else(|| json_text(model, &["id"]));
        if let (Some(provider), Some(model_id)) = (provider, model_id) {
            if !provider.is_empty() && !model_id.is_empty() {
                result.insert(format!("{provider}/{model_id}"));
            }
        }
    }
    Ok(result.into_iter().collect())
}

fn create_session(
    client: &reqwest::blocking::Client,
    base_url: &str,
    password: &str,
    working_dir: &Path,
    project_title: &str,
) -> Result<String, String> {
    let response = client
        .post(format!("{base_url}/api/session"))
        .basic_auth(SERVER_USER, Some(password))
        .query(&[("location[directory]", location_value(working_dir))])
        .json(&json!({
            "title": if project_title.is_empty() { "素笺 AI" } else { project_title },
            "location": { "directory": location_value(working_dir) }
        }))
        .send()
        .map_err(|error| format!("创建 OpenCode 会话失败：{error}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "创建 OpenCode 会话失败：{}",
            response_error(response)
        ));
    }
    let value: Value = response.json().map_err(|error| error.to_string())?;
    json_text(&value, &["data", "id"])
        .or_else(|| json_text(&value, &["id"]))
        .ok_or_else(|| "OpenCode 未返回 session id".to_string())
}

impl AgentAdapter for OpenCodeAdapter {
    fn prompt(&mut self, message: &str) -> Result<(), String> {
        let session_id = self
            .session_id
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .ok_or_else(|| "OpenCode 会话尚未建立".to_string())?;
        let mut body = json!({ "text": message });
        // 用户明确选择模型时才传；格式为 `provider/model`。
        if let Some(model) = self.model_hint.as_ref() {
            if let Some((provider, model_id)) = model.split_once('/') {
                body["model"] = json!({ "providerID": provider, "modelID": model_id });
            }
        }
        let client = self.client.clone();
        let base_url = self.base_url.clone();
        let password = self.password.clone();
        let working_dir = location_value(&self.working_dir);
        let emit = self.emit.clone();
        std::thread::Builder::new()
            .name("agent-opencode-prompt".to_string())
            .spawn(move || {
                let response = client
                    .post(format!("{base_url}/api/session/{session_id}/prompt"))
                    .basic_auth(SERVER_USER, Some(&password))
                    .query(&[("location[directory]", working_dir)])
                    .json(&body)
                    .send();
                match response {
                    Ok(response) if response.status().is_success() => {
                        (emit)(RuntimeEvent::native("", AgentEvent::PromptAccepted));
                    }
                    Ok(response) => (emit)(RuntimeEvent {
                        conversation_id: String::new(),
                        generation: 0,
                        core_turn_id: String::new(),
                        native_turn_id: String::new(),
                        event: AgentEvent::TurnFailed {
                            message: response_error(response),
                        },
                    }),
                    Err(error) => (emit)(RuntimeEvent {
                        conversation_id: String::new(),
                        generation: 0,
                        core_turn_id: String::new(),
                        native_turn_id: String::new(),
                        event: AgentEvent::TurnFailed {
                            message: format!("发送 OpenCode 消息失败：{error}"),
                        },
                    }),
                }
                // 完成事件由 `/api/event` 的 session.status idle 驱动。
            })
            .map_err(|error| format!("无法启动 OpenCode 请求线程：{error}"))?;
        Ok(())
    }

    fn native_session_id(&self) -> Option<String> {
        self.session_id.lock().ok().and_then(|guard| guard.clone())
    }

    fn cancel(&mut self) -> Result<(), String> {
        let session_id = self
            .session_id
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .ok_or_else(|| "OpenCode 会话尚未建立".to_string())?;
        let response = self
            .client
            .post(format!(
                "{}/api/session/{}/interrupt",
                self.base_url, session_id
            ))
            .basic_auth(SERVER_USER, Some(&self.password))
            .query(&[("location[directory]", location_value(&self.working_dir))])
            .send()
            .map_err(|error| format!("中断 OpenCode 请求失败：{error}"))?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(response_error(response))
        }
    }

    fn respond_permission(&mut self, request_id: &str, option_id: &str) -> Result<(), String> {
        let session_id = self
            .session_id
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .ok_or_else(|| "OpenCode 会话尚未建立".to_string())?;
        let decision = match option_id {
            "always" => "always",
            "reject" => "reject",
            _ => "once",
        };
        let response = self
            .client
            .post(format!(
                "{}/api/session/{}/permission/{}/reply",
                self.base_url, session_id, request_id
            ))
            .basic_auth(SERVER_USER, Some(&self.password))
            .query(&[("location[directory]", location_value(&self.working_dir))])
            .json(&json!({ "decision": decision }))
            .send()
            .map_err(|error| format!("回复 OpenCode 权限请求失败：{error}"))?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(response_error(response))
        }
    }
}

impl Drop for OpenCodeAdapter {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
