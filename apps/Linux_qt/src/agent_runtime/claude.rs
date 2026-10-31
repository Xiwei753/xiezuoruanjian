//! Claude Code 适配器。
//!
//! 每轮是一个独立进程，使用官方 stream-json 输入/输出。进程先完成
//! `system/init`，确认 `--resume` 是否真的恢复成功，再通过 stdin 提交用户
//! 消息；这样恢复失败时可以重建原生会话并补入完整历史，而不是把增量历史
//! 当成已送达。

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::adapter::{AgentAdapter, CancelOutcome};
use super::events::{
    json_text, json_value_text, AgentEvent, AgentLaunchContext, AgentPromptRequest, EventSink,
    RuntimeEvent,
};
use super::workspace_access::WorkspaceAccess;

/// 每轮独占的子进程槽位。读取线程只能回收自己的子进程，不能触碰下一轮。
type RoundChild = Arc<Mutex<Option<Child>>>;

const INIT_TIMEOUT: Duration = Duration::from_secs(20);

pub struct ClaudeAdapter {
    ctx: AgentLaunchContext,
    emit: EventSink,
    session_id: Arc<Mutex<Option<String>>>,
    /// 当前活动轮次及其独占子进程；旧读取线程完成时只在 turn id 匹配时清理。
    active_child: Arc<Mutex<Option<(String, RoundChild)>>>,
    /// 只有真实 init 确认原生 session 后才为 true。
    history_restored: Arc<AtomicBool>,
}

impl ClaudeAdapter {
    pub fn start(ctx: &AgentLaunchContext, emit: EventSink) -> Result<Self, String> {
        Ok(Self {
            ctx: ctx.clone(),
            emit,
            session_id: Arc::new(Mutex::new(ctx.resume_native_session_id.clone())),
            active_child: Arc::new(Mutex::new(None)),
            // 在真实 init 之前不能假设恢复成功；后端因此会同时提供完整历史，
            // 由本轮进程根据实际恢复结果决定是否使用。
            history_restored: Arc::new(AtomicBool::new(false)),
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

    /// 启动一轮 Claude 进程。用户消息稍后通过 stdin 发送，先观察 init。
    fn spawn_turn_process(
        &self,
        request: &AgentPromptRequest,
        resume_id: Option<&str>,
        include_full_history: bool,
    ) -> Result<(Child, ChildStdin, Receiver<Value>), String> {
        let mut command = WorkspaceAccess::for_context(&self.ctx)?
            .command(&self.ctx.launch_path, &self.ctx.launch_args)?;
        command
            .arg("-p")
            .arg("--input-format")
            .arg("stream-json")
            .arg("--output-format")
            .arg("stream-json")
            .arg("--verbose")
            // 恢复旧会话时可能重放旧消息；读取线程用本轮 UUID 确认真正
            // 接受输入后才转发增量。
            .arg("--replay-user-messages")
            .current_dir(&self.ctx.working_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if include_full_history {
            if let Some(history) = request.full_history_context_block() {
                // 官方追加系统提示接口，不覆盖 Claude 自带系统指令。
                command.arg("--append-system-prompt").arg(history);
            }
        }
        if let Some(model) = self.ctx.model.as_ref() {
            command.arg("--model").arg(model);
        }
        if let Some(session_id) = resume_id {
            command.arg("--resume").arg(session_id);
        }
        if let Some(config) = self.mcp_config_json() {
            command.arg("--mcp-config").arg(config);
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("无法启动 Claude Code：{error}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Claude Code 没有 stdin".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Claude Code 没有 stdout".to_string())?;
        if let Some(stderr) = child.stderr.take() {
            std::thread::Builder::new()
                .name("agent-claude-stderr".to_string())
                .spawn(move || {
                    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                        log::debug!(target: "agent", "claude stderr: {line}");
                    }
                })
                .ok();
        }
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("agent-claude-stdout".to_string())
            .spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<Value>(trimmed) {
                        Ok(value) => {
                            if sender.send(value).is_err() {
                                break;
                            }
                        }
                        Err(_) => log::debug!(target: "agent", "claude stdout: {trimmed}"),
                    }
                }
            })
            .map_err(|error| format!("无法启动 Claude Code 读取线程：{error}"))?;
        Ok((child, stdin, receiver))
    }

    fn wait_for_init(receiver: &Receiver<Value>) -> Result<(String, Option<String>), String> {
        let deadline = Instant::now() + INIT_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("等待 Claude Code 初始化超时".to_string());
            }
            match receiver.recv_timeout(remaining) {
                Ok(value)
                    if value.get("type").and_then(Value::as_str) == Some("system")
                        && value.get("subtype").and_then(Value::as_str) == Some("init") =>
                {
                    let id = json_text(&value, &["session_id"])
                        .filter(|id| !id.is_empty())
                        .ok_or_else(|| "Claude Code 初始化未返回 session id".to_string())?;
                    return Ok((id, json_text(&value, &["model"])));
                }
                Ok(_) => continue,
                Err(RecvTimeoutError::Timeout) => {
                    return Err("等待 Claude Code 初始化超时".to_string());
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err("Claude Code 在初始化前退出".to_string());
                }
            }
        }
    }

    fn stop_process(mut child: Child, stdin: ChildStdin) {
        drop(stdin);
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl AgentAdapter for ClaudeAdapter {
    fn prompt(&mut self, core_turn_id: &str, request: &AgentPromptRequest) -> Result<(), String> {
        let requested_resume_id = self.session_id.lock().ok().and_then(|guard| guard.clone());
        let mut resume_id = requested_resume_id.clone();
        let mut include_full_history = resume_id.is_none();
        let mut attempts = 0u8;
        let (child, mut stdin, receiver, actual_session_id, reported_model, resumed) = loop {
            attempts = attempts.saturating_add(1);
            let (child, stdin, receiver) =
                self.spawn_turn_process(request, resume_id.as_deref(), include_full_history)?;
            match Self::wait_for_init(&receiver) {
                Ok((actual_id, reported_model))
                    if resume_id
                        .as_deref()
                        .is_none_or(|requested| requested == actual_id) =>
                {
                    break (
                        child,
                        stdin,
                        receiver,
                        actual_id,
                        reported_model,
                        resume_id.is_some(),
                    );
                }
                result => {
                    Self::stop_process(child, stdin);
                    if resume_id.is_some() && attempts < 2 {
                        // --resume 被拒绝或恢复了别的 session：重建新会话，
                        // 下一轮用完整 Core 历史补齐上下文。
                        resume_id = None;
                        include_full_history = true;
                        continue;
                    }
                    return Err(match result {
                        Err(error) => error,
                        Ok(_) => "Claude Code 恢复了不同的原生会话".to_string(),
                    });
                }
            }
        };

        if let Ok(mut guard) = self.session_id.lock() {
            *guard = Some(actual_session_id.clone());
        }
        let input = json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": [{ "type": "text", "text": request.text }],
            },
            "uuid": core_turn_id,
        });
        let mut line = input.to_string();
        line.push('\n');
        if let Err(error) = stdin.write_all(line.as_bytes()).and_then(|_| stdin.flush()) {
            Self::stop_process(child, stdin);
            return Err(format!("Claude Code 无法接收本轮消息：{error}"));
        }
        let round_child: RoundChild = Arc::new(Mutex::new(Some(child)));
        if let Ok(mut guard) = self.active_child.lock() {
            *guard = Some((core_turn_id.to_string(), round_child.clone()));
        }

        let emit = self.emit.clone();
        let session_id = self.session_id.clone();
        let active_child = self.active_child.clone();
        let history_restored = self.history_restored.clone();
        let round_turn_id = core_turn_id.to_string();
        let expected_uuid = core_turn_id.to_string();
        let expected_text = request.text.clone();
        let actual_session_id = actual_session_id.clone();
        let session_model = reported_model.or_else(|| self.ctx.model.clone());
        let input_chars = u32::try_from(request.text.chars().count()).unwrap_or(u32::MAX);
        std::thread::Builder::new()
            .name("agent-claude-turn".to_string())
            .spawn(move || {
                let mut output_chars: u32 = 0;
                let mut finished = false;
                let mut accepted = false;
                while let Ok(value) = receiver.recv() {
                    let kind = value
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if !accepted {
                        if kind != "user" {
                            continue;
                        }
                        if !claude_echo_matches(&value, &expected_uuid, &expected_text) {
                            continue;
                        }
                        accepted = true;
                        // 只有原生侧回显并确认接收本轮消息后，才认为历史
                        // 已经同步；启动 init 本身不代表本轮已送达。两者在
                        // 同一线程按顺序发出，确保 Core 先建立会话绑定。
                        history_restored.store(true, Ordering::SeqCst);
                        (emit)(RuntimeEvent::session(AgentEvent::SessionStarted {
                            native_session_id: actual_session_id.clone(),
                            model: session_model.clone(),
                            resumed,
                        }));
                        (emit)(RuntimeEvent::turn(
                            round_turn_id.clone(),
                            "",
                            AgentEvent::PromptAccepted,
                        ));
                        continue;
                    }
                    match kind {
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
                                                    agent_identity: String::new(),
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
                                                agent_identity: String::new(),
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
                                            agent_identity: String::new(),
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
                                    agent_identity: String::new(),
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
                                agent_identity: String::new(),
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
                                    agent_identity: String::new(),
                                    core_turn_id: round_turn_id.clone(),
                                    native_turn_id: String::new(),
                                    event: AgentEvent::TurnFailed { message },
                                });
                            } else {
                                (emit)(RuntimeEvent {
                                    conversation_id: String::new(),
                                    generation: 0,
                                    agent_identity: String::new(),
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
                    let message = if !accepted {
                        "Claude Code 未确认收到本轮消息".to_string()
                    } else {
                        match status {
                            Some(status) if !status.success() => {
                                format!("Claude Code 退出码异常：{status}")
                            }
                            _ => "Claude Code 未返回结果".to_string(),
                        }
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

    fn set_model(&mut self, model: Option<String>) {
        self.ctx.model = model;
    }

    fn history_restored(&self) -> bool {
        self.history_restored.load(Ordering::SeqCst)
    }

    fn history_restored_on_reuse(&self) -> bool {
        self.history_restored.load(Ordering::SeqCst)
    }

    fn cancel(&mut self, core_turn_id: &str) -> Result<CancelOutcome, String> {
        if let Ok(guard) = self.active_child.lock() {
            if let Some((active_turn, child)) = guard.as_ref() {
                if active_turn != core_turn_id {
                    return Ok(CancelOutcome::AlreadyFinished);
                }
                if let Ok(mut child) = child.lock() {
                    if let Some(child) = child.as_mut() {
                        child.kill().map_err(|error| error.to_string())?;
                        return Ok(CancelOutcome::Cancelled);
                    }
                }
            }
        }
        Ok(CancelOutcome::AlreadyFinished)
    }
}

/// `--replay-user-messages` 的旧消息不能触发本轮接受；优先按 UUID 匹配，
/// 兼容不返回 uuid 的版本时再比较正文。
fn claude_echo_matches(value: &Value, uuid: &str, text: &str) -> bool {
    if let Some(echo_uuid) = value.get("uuid").and_then(Value::as_str) {
        return echo_uuid == uuid;
    }
    let Some(content) = value
        .get("message")
        .and_then(|message| message.get("content"))
    else {
        return false;
    };
    if let Some(content) = content.as_str() {
        return content == text;
    }
    content.as_array().is_some_and(|blocks| {
        blocks.iter().any(|block| {
            block.get("type").and_then(Value::as_str) == Some("text")
                && block.get("text").and_then(Value::as_str) == Some(text)
        })
    })
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
