//! Gemini CLI 适配器。
//!
//! 使用 `gemini -p ... --output-format stream-json` 的结构化输出，解析
//! init / message / tool_use / tool_result / result，记录原生 session ID，
//! 下一轮用官方 `--resume` 恢复。MCP 通过 `GEMINI_CLI_SYSTEM_SETTINGS_PATH`
//! 指向本次会话私有的临时系统设置文件，不修改作品目录或用户的持久配置。

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::adapter::{AgentAdapter, CancelOutcome};
use super::events::{
    json_text, json_value_text, AgentEvent, AgentLaunchContext, EventSink, RuntimeEvent,
};
use super::workspace_access::WorkspaceAccess;

/// 每轮独占的子进程槽位。读取线程只能回收自己的子进程，不能触碰下一轮。
type RoundChild = Arc<Mutex<Option<Child>>>;

pub struct GeminiAdapter {
    ctx: AgentLaunchContext,
    emit: EventSink,
    session_id: Arc<Mutex<Option<String>>>,
    /// 当前活动轮次及其独占子进程；旧读取线程完成时只在 turn id 匹配时清理。
    active_child: Arc<Mutex<Option<(String, RoundChild)>>>,
    session_started: Arc<Mutex<bool>>,
    system_settings_dir: Option<PathBuf>,
}

/// 生成仅本次 Gemini 进程可见的系统设置文件。
///
/// Gemini 的系统设置优先级最高，会叠加到用户/项目设置之上。这里先读取宿主
/// 原有的系统设置（通常由管理员维护），只追加 Sujian 的 MCP 条目，避免覆盖
/// 企业策略，同时不向作品目录写入任何文件。
fn prepare_system_settings(ctx: &AgentLaunchContext) -> Result<Option<PathBuf>, String> {
    if ctx.mcp_servers.is_empty() {
        return Ok(None);
    }
    let source = std::env::var_os("GEMINI_CLI_SYSTEM_SETTINGS_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/etc/gemini-cli/settings.json"));
    let mut settings = match std::fs::read_to_string(&source) {
        Ok(content) => serde_json::from_str::<Value>(&content)
            .map_err(|error| format!("系统 Gemini 设置无法解析：{error}"))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => json!({}),
        Err(error) => {
            return Err(format!(
                "读取系统 Gemini 设置 {} 失败：{error}",
                source.display()
            ));
        }
    };
    if !settings.is_object() {
        return Err("系统 Gemini 设置不是 JSON 对象".to_string());
    }
    let servers = settings
        .as_object_mut()
        .expect("checked object")
        .entry("mcpServers")
        .or_insert_with(|| json!({}));
    if !servers.is_object() {
        return Err("系统 Gemini 设置的 mcpServers 不是对象".to_string());
    }
    for server in &ctx.mcp_servers {
        let env: serde_json::Map<String, Value> = server
            .env
            .iter()
            .map(|(key, value)| (key.clone(), Value::String(value.clone())))
            .collect();
        servers[&server.name] = json!({
            "command": server.command,
            "args": server.args,
            "env": env,
        });
    }

    // 配置必须放在沙箱可写的 Gemini 配置目录内；宿主 /tmp 在沙箱里是私有
    // tmpfs，宿主侧创建的临时文件不会自动可见。
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "无法确定 HOME，不能生成 Gemini 私有配置".to_string())?;
    let directory = home
        .join(".gemini")
        .join(format!(".sujian-runtime-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("创建 Gemini 临时配置目录失败：{error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700));
    }
    let path = directory.join("settings.json");
    let content = serde_json::to_string_pretty(&settings).map_err(|error| error.to_string())?;
    std::fs::write(&path, content)
        .map_err(|error| format!("写入 Gemini 临时系统设置失败：{error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(Some(directory))
}

fn remove_private_settings(directory: &Path) {
    let _ = std::fs::remove_dir_all(directory);
}

impl GeminiAdapter {
    pub fn start(ctx: &AgentLaunchContext, emit: EventSink) -> Result<Self, String> {
        let system_settings_dir = prepare_system_settings(ctx)?;
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
            system_settings_dir,
        })
    }
}

impl AgentAdapter for GeminiAdapter {
    fn prompt(&mut self, core_turn_id: &str, message: &str) -> Result<(), String> {
        let mut command =
            WorkspaceAccess::for_context(&self.ctx)?.command(&self.ctx.launch_path, &[])?;
        command
            .arg("-p")
            .arg(message)
            .arg("--output-format")
            .arg("stream-json")
            .current_dir(&self.ctx.working_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(directory) = self.system_settings_dir.as_ref() {
            command.env(
                "GEMINI_CLI_SYSTEM_SETTINGS_PATH",
                directory.join("settings.json"),
            );
        }
        if let Some(model) = self.ctx.model.as_ref() {
            command.arg("--model").arg(model);
        }
        if let Some(session_id) = self.session_id.lock().ok().and_then(|guard| guard.clone()) {
            command.arg("--resume").arg(session_id);
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("无法启动 Gemini CLI：{error}"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Gemini CLI 没有 stdout".to_string())?;
        let stderr = child.stderr.take();
        let round_child: RoundChild = Arc::new(Mutex::new(Some(child)));
        if let Ok(mut guard) = self.active_child.lock() {
            *guard = Some((core_turn_id.to_string(), round_child.clone()));
        }
        if let Some(stderr) = stderr {
            std::thread::Builder::new()
                .name("agent-gemini-stderr".to_string())
                .spawn(move || {
                    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                        log::debug!(target: "agent", "gemini stderr: {line}");
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
            .name("agent-gemini-turn".to_string())
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
                        log::debug!(target: "agent", "gemini stdout: {trimmed}");
                        continue;
                    };
                    let kind = value
                        .get("type")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    match kind {
                        "init" => {
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
                                    (emit)(RuntimeEvent::session(AgentEvent::SessionStarted {
                                        native_session_id: id,
                                        model: json_text(&value, &["model"]),
                                    }));
                                }
                            }
                        }
                        "message" => {
                            if value.get("role").and_then(Value::as_str) != Some("assistant") {
                                continue;
                            }
                            let Some(text) = json_text(&value, &["content"]) else {
                                continue;
                            };
                            let delta = value.get("delta").and_then(Value::as_bool).unwrap_or(true);
                            if text.is_empty() || !delta {
                                continue;
                            }
                            output_chars = output_chars.saturating_add(
                                u32::try_from(text.chars().count()).unwrap_or(u32::MAX),
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
                        "tool_use" => {
                            let tool_call_id = json_text(&value, &["tool_id"])
                                .or_else(|| json_text(&value, &["id"]))
                                .unwrap_or_else(|| "tool".to_string());
                            let name = json_text(&value, &["tool_name"])
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
                                    input_json: json_value_text(value.get("parameters")),
                                },
                            });
                        }
                        "tool_result" => {
                            let tool_call_id = json_text(&value, &["tool_id"])
                                .or_else(|| json_text(&value, &["id"]))
                                .unwrap_or_else(|| "tool".to_string());
                            let name = json_text(&value, &["tool_name"])
                                .unwrap_or_else(|| "工具".to_string());
                            let status = json_text(&value, &["status"]).unwrap_or_default();
                            (emit)(RuntimeEvent {
                                conversation_id: String::new(),
                                generation: 0,
                                agent_identity: String::new(),
                                core_turn_id: round_turn_id.clone(),
                                native_turn_id: String::new(),
                                event: AgentEvent::ToolFinished {
                                    tool_call_id,
                                    name,
                                    output_json: json_value_text(
                                        value.get("output").or_else(|| value.get("error")),
                                    ),
                                    success: status != "error" && status != "failed",
                                },
                            });
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
                            let failed = value.get("status").and_then(Value::as_str)
                                == Some("error")
                                || value.get("error").is_some_and(|error| !error.is_null());
                            if failed {
                                let message = json_text(&value, &["error", "message"])
                                    .or_else(|| json_text(&value, &["error"]))
                                    .unwrap_or_else(|| "Gemini CLI 请求失败".to_string());
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
                    let message = match status {
                        Some(status) if !status.success() => {
                            format!("Gemini CLI 退出码异常：{status}")
                        }
                        _ => "Gemini CLI 未返回结果".to_string(),
                    };
                    (emit)(RuntimeEvent::turn(
                        round_turn_id,
                        "",
                        AgentEvent::TurnFailed { message },
                    ));
                }
            })
            .map_err(|error| format!("无法启动 Gemini CLI 读取线程：{error}"))?;
        Ok(())
    }

    fn native_session_id(&self) -> Option<String> {
        self.session_id.lock().ok().and_then(|guard| guard.clone())
    }

    fn set_model(&mut self, model: Option<String>) {
        self.ctx.model = model;
    }

    fn cancel(&mut self, core_turn_id: &str) -> Result<CancelOutcome, String> {
        let mut cancelled = false;
        if let Ok(guard) = self.active_child.lock() {
            if let Some((active_turn, child)) = guard.as_ref() {
                if active_turn != core_turn_id {
                    return Ok(CancelOutcome::AlreadyFinished);
                }
                if let Ok(mut child) = child.lock() {
                    if let Some(child) = child.as_mut() {
                        child.kill().map_err(|error| error.to_string())?;
                        cancelled = true;
                    }
                }
            }
        }
        Ok(if cancelled {
            CancelOutcome::Cancelled
        } else {
            CancelOutcome::AlreadyFinished
        })
    }
}

impl Drop for GeminiAdapter {
    fn drop(&mut self) {
        if let Ok(guard) = self.active_child.lock() {
            if let Some((_, child)) = guard.as_ref() {
                if let Some(mut child) = child.lock().ok().and_then(|mut guard| guard.take()) {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
        if let Some(directory) = self.system_settings_dir.take() {
            remove_private_settings(&directory);
        }
    }
}
