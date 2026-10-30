//! Claude Code 适配器。
//!
//! 使用 `claude -p --output-format stream-json --verbose` 的结构化输出：
//! 解析 init / assistant / tool_use / tool_result / result，记录原生 session
//! ID，下一轮用 `--resume` 恢复。每轮是一个独立进程，取消时终止该进程。

use std::io::{BufRead, BufReader};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::adapter::AgentAdapter;
use super::events::{
    json_text, json_value_text, AgentEvent, AgentLaunchContext, EventSink, RuntimeEvent,
};
use super::workspace_access::WorkspaceAccess;

/// 每轮独占的子进程槽位。读取线程只能回收自己的子进程，不能触碰下一轮。
type RoundChild = Arc<Mutex<Option<Child>>>;

pub struct ClaudeAdapter {
    ctx: AgentLaunchContext,
    emit: EventSink,
    session_id: Arc<Mutex<Option<String>>>,
    /// 当前活动轮次及其独占子进程；旧读取线程完成时只在 turn id 匹配时清理。
    active_child: Arc<Mutex<Option<(String, RoundChild)>>>,
    session_started: Arc<Mutex<bool>>,
}

impl ClaudeAdapter {
    pub fn start(ctx: &AgentLaunchContext, emit: EventSink) -> Result<Self, String> {
        let session_id = Arc::new(Mutex::new(ctx.resume_native_session_id.clone()));
        if let Some(id) = ctx.resume_native_session_id.as_ref() {
            (emit)(RuntimeEvent::session(AgentEvent::SessionStarted {
                native_session_id: id.clone(),
                model: ctx.model.clone(),
            }));
        }
        Ok(Self {
            ctx: ctx.clone(),
            emit,
            session_id,
            active_child: Arc::new(Mutex::new(None)),
            session_started: Arc::new(Mutex::new(false)),
        })
    }

    fn mcp_config_json(&self) -> Option<String> {
        if self.ctx.mcp_servers.is_empty() {
            return None;
        }
        let servers: serde_json::Map<String, Value> = self
            .ctx
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
        Some(json!({ "mcpServers": servers }).to_string())
    }
}

impl AgentAdapter for ClaudeAdapter {
    fn prompt(&mut self, core_turn_id: &str, message: &str) -> Result<(), String> {
        let mut command = WorkspaceAccess::for_context(&self.ctx)?
            .command(&self.ctx.launch_path, &[])?;
        command
            .arg("-p")
            .arg(message)
            .arg("--output-format")
            .arg("stream-json")
            .arg("--verbose")
            .current_dir(&self.ctx.working_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(model) = self.ctx.model.as_ref() {
            command.arg("--model").arg(model);
        }
        if let Some(session_id) = self.session_id.lock().ok().and_then(|guard| guard.clone()) {
            command.arg("--resume").arg(session_id);
        }
        if let Some(config) = self.mcp_config_json() {
            command.arg("--mcp-config").arg(config);
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("无法启动 Claude Code：{error}"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Claude Code 没有 stdout".to_string())?;
        let stderr = child.stderr.take();
        let round_child: RoundChild = Arc::new(Mutex::new(Some(child)));
        if let Ok(mut guard) = self.active_child.lock() {
            *guard = Some((core_turn_id.to_string(), round_child.clone()));
        }
        if let Some(stderr) = stderr {
            std::thread::Builder::new()
                .name("agent-claude-stderr".to_string())
                .spawn(move || {
                    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                        log::debug!(target: "agent", "claude stderr: {line}");
                    }
                })
                .ok();
        }

        let emit = self.emit.clone();
        let session_id = self.session_id.clone();
        let active_child = self.active_child.clone();
        let session_started = self.session_started.clone();
        let round_turn_id = core_turn_id.to_string();
        let input_chars = u32::try_from(message.chars().count()).unwrap_or(u32::MAX);
        std::thread::Builder::new()
            .name("agent-claude-turn".to_string())
            .spawn(move || {
                let reader = BufReader::new(stdout);
                let mut output_chars: u32 = 0;
                let mut finished = false;
                let mut accepted = false;
                for line in reader.lines().map_while(Result::ok) {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
                        log::debug!(target: "agent", "claude stdout: {trimmed}");
                        continue;
                    };
                    let kind = value
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    match kind {
                        "system" => {
                            if value.get("subtype").and_then(Value::as_str) == Some("init") {
                                if !accepted {
                                    accepted = true;
                                    (emit)(RuntimeEvent::turn(
                                        round_turn_id.clone(),
                                        "",
                                        AgentEvent::PromptAccepted,
                                    ));
                                }
                                if let Some(id) = json_text(&value, &["session_id"]) {
                                    if let Ok(mut guard) = session_id.lock() {
                                        *guard = Some(id.clone());
                                    }
                                    let already =
                                        session_started.lock().map(|guard| *guard).unwrap_or(true);
                                    if !already {
                                        if let Ok(mut guard) = session_started.lock() {
                                            *guard = true;
                                        }
                                        (emit)(RuntimeEvent::session(
                                            AgentEvent::SessionStarted {
                                                native_session_id: id,
                                                model: json_text(&value, &["model"]),
                                            },
                                        ));
                                    }
                                }
                            }
                        }
                        "assistant" => {
                            let Some(blocks) = value
                                .get("message")
                                .and_then(|message| message.get("content"))
                                .and_then(Value::as_array)
                            else {
                                continue;
                            };
                            for block in blocks {
                                match block.get("type").and_then(Value::as_str) {
                                    Some("text") => {
                                        if let Some(text) = json_text(block, &["text"]) {
                                            if !text.is_empty() {
                                                output_chars = output_chars.saturating_add(
                                                    u32::try_from(text.chars().count())
                                                        .unwrap_or(u32::MAX),
                                                );
                                                (emit)(RuntimeEvent {
                                                    conversation_id: String::new(),
                                                    generation: 0,
                                                    core_turn_id: round_turn_id.clone(),
                                                    native_turn_id: String::new(),
                                                    event: AgentEvent::TextDelta { text },
                                                });
                                            }
                                        }
                                    }
                                    Some("thinking") => {
                                        if let Some(text) = json_text(block, &["thinking"]) {
                                            (emit)(RuntimeEvent {
                                                conversation_id: String::new(),
                                                generation: 0,
                                                core_turn_id: round_turn_id.clone(),
                                                native_turn_id: String::new(),
                                                event: AgentEvent::ReasoningDelta { text },
                                            });
                                        }
                                    }
                                    Some("tool_use") => {
                                        let tool_call_id = json_text(block, &["id"])
                                            .unwrap_or_else(|| "tool".to_string());
                                        let name = json_text(block, &["name"])
                                            .unwrap_or_else(|| "工具".to_string());
                                        (emit)(RuntimeEvent {
                                            conversation_id: String::new(),
                                            generation: 0,
                                            core_turn_id: round_turn_id.clone(),
                                            native_turn_id: String::new(),
                                            event: AgentEvent::ToolStarted {
                                                tool_call_id,
                                                name,
                                                input_json: json_value_text(block.get("input")),
                                            },
                                        });
                                    }
                                    _ => {}
                                }
                            }
                        }
                        "user" => {
                            let Some(blocks) = value
                                .get("message")
                                .and_then(|message| message.get("content"))
                                .and_then(Value::as_array)
                            else {
                                continue;
                            };
                            for block in blocks {
                                if block.get("type").and_then(Value::as_str) != Some("tool_result")
                                {
                                    continue;
                                }
                                let tool_call_id = json_text(block, &["tool_use_id"])
                                    .unwrap_or_else(|| "tool".to_string());
                                (emit)(RuntimeEvent {
                                    conversation_id: String::new(),
                                    generation: 0,
                                    core_turn_id: round_turn_id.clone(),
                                    native_turn_id: String::new(),
                                    event: AgentEvent::ToolFinished {
                                        tool_call_id,
                                        name: "工具".to_string(),
                                        output_json: json_value_text(block.get("content")),
                                        success: !block
                                            .get("is_error")
                                            .and_then(Value::as_bool)
                                            .unwrap_or(false),
                                    },
                                });
                            }
                        }
                        "result" => {
                            finished = true;
                            if let Some(id) = json_text(&value, &["session_id"]) {
                                if let Ok(mut guard) = session_id.lock() {
                                    *guard = Some(id);
                                }
                            }
                            (emit)(RuntimeEvent {
                                conversation_id: String::new(),
                                generation: 0,
                                core_turn_id: round_turn_id.clone(),
                                native_turn_id: String::new(),
                                event: AgentEvent::Usage {
                                    input_chars,
                                    output_chars,
                                },
                            });
                            let failed = value
                                .get("is_error")
                                .and_then(Value::as_bool)
                                .unwrap_or(false)
                                || value.get("subtype").and_then(Value::as_str) == Some("error");
                            if failed {
                                let message = json_text(&value, &["result"])
                                    .unwrap_or_else(|| "Claude Code 请求失败".to_string());
                                (emit)(RuntimeEvent {
                                    conversation_id: String::new(),
                                    generation: 0,
                                    core_turn_id: round_turn_id.clone(),
                                    native_turn_id: String::new(),
                                    event: AgentEvent::TurnFailed { message },
                                });
                            } else {
                                (emit)(RuntimeEvent {
                                    conversation_id: String::new(),
                                    generation: 0,
                                    core_turn_id: round_turn_id.clone(),
                                    native_turn_id: String::new(),
                                    event: AgentEvent::TurnCompleted,
                                });
                            }
                        }
                        _ => {}
                    }
                }
                // 只回收本轮自己的子进程，旧读取线程不会误取下一轮进程。
                let status = round_child
                    .lock()
                    .ok()
                    .and_then(|mut guard| guard.take())
                    .and_then(|mut child| child.wait().ok());
                if let Ok(mut guard) = active_child.lock() {
                    if guard
                        .as_ref()
                        .is_some_and(|(turn_id, _)| turn_id == &round_turn_id)
                    {
                        *guard = None;
                    }
                }
                if !finished {
                    let message = match status {
                        Some(status) if !status.success() => {
                            format!("Claude Code 退出码异常：{status}")
                        }
                        _ => "Claude Code 未返回结果".to_string(),
                    };
                    (emit)(RuntimeEvent::turn(
                        round_turn_id,
                        "",
                        AgentEvent::TurnFailed { message },
                    ));
                }
            })
            .map_err(|error| format!("无法启动 Claude Code 读取线程：{error}"))?;
        Ok(())
    }

    fn native_session_id(&self) -> Option<String> {
        self.session_id.lock().ok().and_then(|guard| guard.clone())
    }

    fn cancel(&mut self) -> Result<(), String> {
        if let Ok(guard) = self.active_child.lock() {
            if let Some((_, child)) = guard.as_ref() {
                if let Ok(mut child) = child.lock() {
                    if let Some(child) = child.as_mut() {
                        child.kill().map_err(|error| error.to_string())?;
                    }
                }
            }
        }
        Ok(())
    }
}

impl Drop for ClaudeAdapter {
    fn drop(&mut self) {
        if let Ok(guard) = self.active_child.lock() {
            if let Some((_, child)) = guard.as_ref() {
                if let Some(mut child) = child.lock().ok().and_then(|mut guard| guard.take()) {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
    }
}
