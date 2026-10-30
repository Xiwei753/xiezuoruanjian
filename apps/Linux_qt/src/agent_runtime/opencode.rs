//! OpenCode Server 适配器。
//!
//! 适配 OpenCode 2.x 的 `/api/*` Server API：启动本地 `opencode serve`，
//! 用 HTTP Basic 鉴权创建/恢复 session，通过 `/api/event` SSE 接收文本、
//! 推理、工具和权限事件，用 `/api/session/:id/prompt` 发送消息、
//! `/api/session/:id/interrupt` 中断。进程和服务器生命周期由本适配器管理。

use std::collections::{BTreeSet, HashMap};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::adapter::AgentAdapter;
use super::events::{
    json_text, json_value_text, AgentEvent, AgentLaunchContext, EventSink, PermissionOption,
    RuntimeEvent,
};

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

impl OpenCodeAdapter {
    pub fn start(ctx: &AgentLaunchContext, emit: EventSink) -> Result<Self, String> {
        let port = free_loopback_port()?;
        // 随机密码只用于本机回环服务，避免 opencode 把随机凭据写到用户终端，
        // 也避免同机其他进程直接调用该 Server。
        let password = format!("sujian-{}", uuid::Uuid::new_v4());
        let child = Command::new(&ctx.launch_path)
            .args(&ctx.launch_args)
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
                            log::info!("[agent] OpenCode MCP connected: {}", server.name);
                        }
                        Ok(response) => {
                            log::warn!(
                                "[agent] OpenCode MCP connect rejected: {}",
                                response_error(response)
                            );
                        }
                        Err(error) => {
                            log::warn!(
                                "[agent] OpenCode MCP connect failed for {}: {error}",
                                server.name
                            );
                        }
                    }
                }
                Ok(response) => {
                    log::warn!(
                        "[agent] OpenCode MCP rejected: {}",
                        response_error(response)
                    );
                }
                Err(error) => {
                    log::warn!("[agent] OpenCode MCP request failed: {error}");
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
        std::thread::Builder::new()
            .name("agent-opencode-events".to_string())
            .spawn(move || {
                let mut text_state: HashMap<String, String> = HashMap::new();
                let response = match sse_client
                    .get(format!("{sse_url}/api/event"))
                    .basic_auth(SERVER_USER, Some(&sse_password))
                    .query(&[("location[directory]", sse_directory)])
                    .send()
                {
                    Ok(response) if response.status().is_success() => response,
                    Ok(response) => {
                        log::warn!("[agent] OpenCode /api/event: {}", response_error(response));
                        return;
                    }
                    Err(error) => {
                        log::warn!("[agent] OpenCode /api/event failed: {error}");
                        return;
                    }
                };
                let reader = BufReader::new(response);
                for line in reader.lines().map_while(Result::ok) {
                    let Some(data) = line.strip_prefix("data:") else {
                        continue;
                    };
                    let Ok(event) = serde_json::from_str::<Value>(data.trim()) else {
                        continue;
                    };
                    let event_type = event
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    let payload = event.get("data").cloned().unwrap_or_default();
                    if !event_matches_session(&payload, &sse_session) {
                        continue;
                    }
                    match event_type {
                        "session.text.delta" | "session.reasoning.delta" => {
                            let Some(delta) = json_text(&payload, &["delta"]) else {
                                continue;
                            };
                            if delta.is_empty() {
                                continue;
                            }
                            let key = text_key(&payload, event_type);
                            text_state
                                .entry(key)
                                .or_default()
                                .push_str(delta.as_str());
                            let event = if event_type == "session.text.delta" {
                                AgentEvent::TextDelta { text: delta }
                            } else {
                                AgentEvent::ReasoningDelta { text: delta }
                            };
                            (sse_emit)(RuntimeEvent {
                                turn_id: String::new(),
                                event,
                            });
                        }
                        "session.text.ended" | "session.reasoning.ended" => {
                            // `ended` 携带完整文本，用于恢复会话或补发丢失的
                            // 增量；按已累计内容取后缀，避免和 delta 重复。
                            let Some(full) = json_text(&payload, &["text"]) else {
                                continue;
                            };
                            let key = text_key(&payload, event_type);
                            let previous = text_state.entry(key).or_default();
                            let text = if full.starts_with(previous.as_str()) {
                                full[previous.len()..].to_string()
                            } else if previous.is_empty() {
                                full.clone()
                            } else {
                                String::new()
                            };
                            *previous = full;
                            if text.is_empty() {
                                continue;
                            }
                            let event = if event_type == "session.text.ended" {
                                AgentEvent::TextDelta { text }
                            } else {
                                AgentEvent::ReasoningDelta { text }
                            };
                            (sse_emit)(RuntimeEvent {
                                turn_id: String::new(),
                                event,
                            });
                        }
                        "session.tool.input.started" | "session.tool.called" => {
                            let tool_call_id = json_text(&payload, &["id"])
                                .or_else(|| json_text(&payload, &["toolCallID"]))
                                .unwrap_or_else(|| "tool".to_string());
                            let name = json_text(&payload, &["name"])
                                .or_else(|| json_text(&payload, &["tool"]))
                                .unwrap_or_else(|| "工具".to_string());
                            (sse_emit)(RuntimeEvent {
                                turn_id: String::new(),
                                event: AgentEvent::ToolStarted {
                                    tool_call_id,
                                    name,
                                    input_json: json_value_text(payload.get("input")),
                                },
                            });
                        }
                        "session.tool.input.ended" => {
                            let Some(tool_call_id) = json_text(&payload, &["id"]) else {
                                continue;
                            };
                            (sse_emit)(RuntimeEvent {
                                turn_id: String::new(),
                                event: AgentEvent::ToolStarted {
                                    tool_call_id,
                                    name: json_text(&payload, &["name"])
                                        .unwrap_or_else(|| "工具".to_string()),
                                    input_json: json_text(&payload, &["text"])
                                        .or_else(|| json_value_text(payload.get("input"))),
                                },
                            });
                        }
                        "session.tool.success" | "session.tool.failed" => {
                            let tool_call_id = json_text(&payload, &["id"])
                                .or_else(|| json_text(&payload, &["toolCallID"]))
                                .unwrap_or_else(|| "tool".to_string());
                            let success = event_type == "session.tool.success";
                            let output = if success {
                                json_value_text(payload.get("content"))
                                    .or_else(|| json_value_text(payload.get("metadata")))
                            } else {
                                json_text(&payload, &["error"])
                                    .or_else(|| json_value_text(payload.get("content")))
                            };
                            (sse_emit)(RuntimeEvent {
                                turn_id: String::new(),
                                event: AgentEvent::ToolFinished {
                                    tool_call_id,
                                    name: json_text(&payload, &["name"])
                                        .unwrap_or_else(|| "工具".to_string()),
                                    output_json: output,
                                    success,
                                },
                            });
                        }
                        "permission.asked" => {
                            let Some(request_id) = json_text(&payload, &["id"]) else {
                                continue;
                            };
                            let description = json_text(&payload, &["message"])
                                .or_else(|| json_text(&payload, &["action"]))
                                .unwrap_or_else(|| "Agent 请求执行工具".to_string());
                            (sse_emit)(RuntimeEvent {
                                turn_id: String::new(),
                                event: AgentEvent::PermissionRequested {
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
                            });
                        }
                        "session.status" => {
                            let status = json_text(&payload, &["status", "type"])
                                .unwrap_or_default();
                            if status == "idle" {
                                (sse_emit)(RuntimeEvent {
                                    turn_id: String::new(),
                                    event: AgentEvent::TurnCompleted,
                                });
                            }
                        }
                        "session.idle" => {
                            (sse_emit)(RuntimeEvent {
                                turn_id: String::new(),
                                event: AgentEvent::TurnCompleted,
                            });
                        }
                        "session.error" | "session.execution.failed" | "session.step.failed" => {
                            let message = json_text(&payload, &["error", "message"])
                                .or_else(|| json_text(&payload, &["error", "data", "message"]))
                                .or_else(|| json_text(&payload, &["error"]))
                                .or_else(|| json_text(&payload, &["message"]))
                                .unwrap_or_else(|| "OpenCode 请求失败".to_string());
                            (sse_emit)(RuntimeEvent {
                                turn_id: String::new(),
                                event: AgentEvent::TurnFailed { message },
                            });
                        }
                        _ => {}
                    }
                }
            })
            .map_err(|error| format!("无法启动 OpenCode 事件线程：{error}"))?;

        if !available_models.is_empty() {
            (emit)(RuntimeEvent {
                turn_id: String::new(),
                event: AgentEvent::ModelsAvailable {
                    models: available_models,
                },
            });
        }
        (emit)(RuntimeEvent {
            turn_id: String::new(),
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

fn text_key(payload: &Value, prefix: &str) -> String {
    let message_id = json_text(payload, &["assistantMessageID"])
        .or_else(|| json_text(payload, &["messageID"]))
        .unwrap_or_default();
    let ordinal = payload
        .get("ordinal")
        .and_then(Value::as_i64)
        .map(|value| value.to_string())
        .unwrap_or_default();
    format!("{prefix}:{message_id}:{ordinal}")
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
                    Ok(response) if response.status().is_success() => {}
                    Ok(response) => (emit)(RuntimeEvent {
                        turn_id: String::new(),
                        event: AgentEvent::TurnFailed {
                            message: response_error(response),
                        },
                    }),
                    Err(error) => (emit)(RuntimeEvent {
                        turn_id: String::new(),
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
