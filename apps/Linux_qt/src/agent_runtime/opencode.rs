//! OpenCode Server 适配器。
//!
//! 适配 OpenCode 2.x 的 `/api/*` Server API：启动本地 `opencode serve`，
//! 用 HTTP Basic 鉴权创建/恢复 session，通过 `/api/event` SSE 接收文本、
//! 推理、工具和权限事件，用 `/api/session/:id/prompt` 发送消息、
//! `/api/session/:id/interrupt` 中断。进程和服务器生命周期由本适配器管理。

use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::adapter::{AgentAdapter, CancelOutcome};
use super::events::{
    json_text, json_value_text, AgentEvent, AgentLaunchContext, AgentPromptRequest, EventSink,
    PermissionOption, RuntimeEvent,
};
use super::workspace_access::WorkspaceAccess;

const READY_TIMEOUT: Duration = Duration::from_secs(20);
pub(crate) const SERVER_USER: &str = "opencode";

/// 一次 Core Turn 对应的 OpenCode inbox/用户消息身份。HTTP POST 可能是异步
/// 的，取消必须等它确认本轮到底有没有被服务端受理，再决定删 inbox 还是
/// interrupt，不能只发一个全局 `/interrupt` 就宣称已取消。
struct PromptInput {
    id: Mutex<String>,
    post_done: (Mutex<bool>, Condvar),
    accepted: AtomicBool,
    cancel_requested: AtomicBool,
    /// 同一次取消只能由一个执行者认领，避免同步取消线程和 POST 回执
    /// worker 同时删除 inbox / interrupt。
    cancel_claimed: AtomicBool,
    cancel_result: (Mutex<Option<Result<CancelOutcome, String>>>, Condvar),
}

impl PromptInput {
    fn new(id: String) -> Self {
        Self {
            id: Mutex::new(id),
            post_done: (Mutex::new(false), Condvar::new()),
            accepted: AtomicBool::new(false),
            cancel_requested: AtomicBool::new(false),
            cancel_claimed: AtomicBool::new(false),
            cancel_result: (Mutex::new(None), Condvar::new()),
        }
    }

    fn id(&self) -> String {
        self.id
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    fn set_id(&self, id: String) {
        if let Ok(mut guard) = self.id.lock() {
            *guard = id;
        }
    }

    fn mark_post_done(&self) {
        if let Ok(mut done) = self.post_done.0.lock() {
            *done = true;
        }
        self.post_done.1.notify_all();
    }

    fn wait_post_done(&self, timeout: Duration) -> bool {
        let Ok(done) = self.post_done.0.lock() else {
            return false;
        };
        let (done, _) = self
            .post_done
            .1
            .wait_timeout_while(done, timeout, |done| !*done)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *done
    }

    fn claim_cancel(&self) -> bool {
        self.cancel_claimed
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    fn settle_cancel(&self, result: Result<CancelOutcome, String>) {
        if let Ok(mut guard) = self.cancel_result.0.lock() {
            *guard = Some(result);
        }
        self.cancel_result.1.notify_all();
    }

    /// 取消执行失败后释放认领，允许用户重试；已经结算的失败结果仍会
    /// 立即返回给已经等待的调用方。
    fn release_cancel_claim(&self) {
        self.cancel_claimed.store(false, Ordering::SeqCst);
    }

    fn wait_cancel_result(&self, timeout: Duration) -> Option<Result<CancelOutcome, String>> {
        let guard = self.cancel_result.0.lock().ok()?;
        let (guard, _) = self
            .cancel_result
            .1
            .wait_timeout_while(guard, timeout, |result| result.is_none())
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.clone()
    }
}

/// SSE 到达早于 HTTP POST 回应/服务端消息历史投影时，暂时保留无法归属
/// 的流式事件；等用户消息可查询后按原顺序回放，不能静默丢掉增量。
type PendingEvents = Arc<Mutex<VecDeque<(String, Value)>>>;

#[derive(Debug, Clone)]
enum TurnOutcome {
    Succeeded { assistant_message_id: String },
    Failed { message: String },
    Interrupted,
}

pub struct OpenCodeAdapter {
    client: reqwest::blocking::Client,
    base_url: String,
    password: String,
    working_dir: PathBuf,
    session_id: Arc<Mutex<Option<String>>>,
    child: Arc<Mutex<Child>>,
    emit: EventSink,
    /// 用户在本轮之前选择的模型；None 表示使用 CLI 默认模型。
    requested_model: Option<String>,
    /// OpenCode 会话当前实际生效的模型，由会话信息和 switchModel 回执更新。
    actual_model: Arc<Mutex<Option<String>>>,
    /// 每个已提交 Core Turn 的 inbox/用户消息生命周期。
    prompt_inputs: Arc<Mutex<HashMap<String, Arc<PromptInput>>>>,
    /// 服务端用户消息 id → Core Turn。助手消息 id 通过消息历史回溯到这条
    /// 用户消息后才允许归属，绝不因为“它是当前轮次”就猜。
    input_bindings: Arc<Mutex<HashMap<String, String>>>,
    /// HTTP 回应/消息历史投影之前到达、暂时无法归属的 SSE 事件。
    pending_events: PendingEvents,
    /// Drop 时通知 SSE 线程退出，避免进程结束后后台无限重连。
    stop_events: Arc<AtomicBool>,
    history_restored: bool,
}

pub(crate) fn free_loopback_port() -> Result<u16, String> {
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
    let error = json_text(status_value, &["error"]).or_else(|| json_text(entry, &["error"]));
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
            WorkspaceAccess::for_context(ctx)?.command(&ctx.launch_path, &ctx.launch_args)?;
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
        let (session_id, resumed) = match resume_id {
            Some(id) => {
                let exists = client
                    .get(format!("{base_url}/api/session/{id}"))
                    .basic_auth(SERVER_USER, Some(&password))
                    .query(&[("location[directory]", location_value(&ctx.working_dir))])
                    .send()
                    .map(|response| response.status().is_success())
                    .unwrap_or(false);
                if exists {
                    (id, true)
                } else {
                    (
                        create_session(
                            &client,
                            &base_url,
                            &password,
                            &ctx.working_dir,
                            &ctx.project_title,
                        )?,
                        false,
                    )
                }
            }
            None => (
                create_session(
                    &client,
                    &base_url,
                    &password,
                    &ctx.working_dir,
                    &ctx.project_title,
                )?,
                false,
            ),
        };

        // 会话响应里的 model 才是当前原生会话实际使用的模型；用户选择的
        // 模型可能要在本轮 prompt 前通过 switchModel 才生效。
        let actual_model =
            fetch_session_model(&client, &base_url, &password, &ctx.working_dir, &session_id);
        let actual_model = Arc::new(Mutex::new(actual_model));

        // 模型目录由独立探测线程通过 `/api/model` 获取，不绑定聊天会话，
        // 也不能在发送消息的启动路径上等待目录请求。

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
                                    emit_mcp_status(&emit, &server.name, status, error, tools);
                                }
                                Err(error) => {
                                    log::warn!("[agent] OpenCode MCP status unavailable: {error}");
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
                            log::warn!("[agent] OpenCode MCP connect rejected: {error}");
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
                    log::warn!("[agent] OpenCode MCP rejected: {error}");
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
        let prompt_inputs: Arc<Mutex<HashMap<String, Arc<PromptInput>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let input_bindings: Arc<Mutex<HashMap<String, String>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let pending_events: PendingEvents = Arc::new(Mutex::new(VecDeque::new()));
        let stop_events = Arc::new(AtomicBool::new(false));
        let sse_session = session_id.clone();
        let sse_prompt_inputs = prompt_inputs.clone();
        let sse_input_bindings = input_bindings.clone();
        let sse_pending_events = pending_events.clone();
        let sse_stop = stop_events.clone();
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
                    sse_prompt_inputs,
                    sse_input_bindings,
                    sse_pending_events,
                    sse_stop,
                    sse_emit,
                )
                .run();
            })
            .map_err(|error| format!("无法启动 OpenCode 事件线程：{error}"))?;

        (emit)(RuntimeEvent::session(AgentEvent::SessionStarted {
            native_session_id: session_id
                .lock()
                .ok()
                .and_then(|guard| guard.clone())
                .unwrap_or_default(),
            model: actual_model.lock().ok().and_then(|guard| guard.clone()),
            resumed,
        }));

        Ok(Self {
            client,
            base_url,
            password,
            working_dir: ctx.working_dir.clone(),
            session_id,
            child,
            emit,
            requested_model: ctx.model.clone(),
            actual_model,
            prompt_inputs,
            input_bindings,
            pending_events,
            stop_events,
            history_restored: resumed,
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
    prompt_inputs: Arc<Mutex<HashMap<String, Arc<PromptInput>>>>,
    input_bindings: Arc<Mutex<HashMap<String, String>>>,
    pending_events: PendingEvents,
    /// 助手消息 id → Core Turn 的稳定映射，只由服务端消息历史确认后建立。
    turn_bindings: HashMap<String, String>,
    emit: EventSink,
    text_state: HashMap<String, String>,
    seen_ids: HashSet<String>,
    last_seq: u64,
    disconnected: bool,
    /// 无明确消息归属的错误只作为会话级状态保留，等 idle/日志/服务端
    /// 历史确认终态后再结束具体轮次。
    session_error: Option<String>,
    stop: Arc<AtomicBool>,
}

impl OpenCodeEventProcessor {
    fn new(
        client: reqwest::blocking::Client,
        sse_client: reqwest::blocking::Client,
        base_url: String,
        password: String,
        directory: String,
        session_id: Arc<Mutex<Option<String>>>,
        prompt_inputs: Arc<Mutex<HashMap<String, Arc<PromptInput>>>>,
        input_bindings: Arc<Mutex<HashMap<String, String>>>,
        pending_events: PendingEvents,
        stop: Arc<AtomicBool>,
        emit: EventSink,
    ) -> Self {
        Self {
            client,
            sse_client,
            base_url,
            password,
            directory,
            session_id,
            prompt_inputs,
            input_bindings,
            pending_events,
            turn_bindings: HashMap::new(),
            emit,
            text_state: HashMap::new(),
            seen_ids: HashSet::new(),
            last_seq: 0,
            disconnected: false,
            session_error: None,
            stop,
        }
    }

    fn run(mut self) {
        self.prime_cursor();
        let mut failures = 0u32;
        let mut reported_outage = false;
        while !self.stop.load(Ordering::SeqCst) {
            if self.disconnected {
                if let Err(error) = self.replay_missed_events() {
                    log::warn!("[agent] OpenCode 恢复事件失败: {error}");
                }
            }
            // 断线期间可能已经产生 idle；先按服务端消息历史结算已确认的
            // 轮次，再重新建立长连接，避免漏掉唯一的终态事件。
            self.reconcile_terminals();
            let started = Instant::now();
            match self.open_live_stream() {
                Ok(()) => log::warn!("[agent] OpenCode /api/event 已结束"),
                Err(error) => log::warn!("[agent] OpenCode /api/event 断开: {error}"),
            }
            self.disconnected = true;
            if started.elapsed() >= Duration::from_secs(1) {
                failures = 0;
                reported_outage = false;
            }
            failures = failures.saturating_add(1);
            if failures >= 20 && !reported_outage {
                reported_outage = true;
                let message = self
                    .session_error
                    .clone()
                    .unwrap_or_else(|| "OpenCode 事件流已断开，正在等待服务端恢复".to_string());
                (self.emit)(RuntimeEvent::session(AgentEvent::SessionError { message }));
            }
            let delay = if failures >= 20 {
                5_000
            } else {
                250u64.saturating_mul(u64::from(failures.min(8)))
            };
            let deadline = Instant::now() + Duration::from_millis(delay);
            while Instant::now() < deadline && !self.stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(50));
            }
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
        let payload = event.get("data").cloned().unwrap_or_else(|| event.clone());
        if !event_matches_session(&payload, &self.session_id) {
            return;
        }
        self.handle_event(&event_type, &payload);
        // 当前帧可能刚刚让用户消息出现在服务端（prompt.admitted/idle），
        // 尝试把此前暂存的增量按顺序补上。
        let has_pending = self
            .pending_events
            .lock()
            .map(|pending| !pending.is_empty())
            .unwrap_or(false);
        if has_pending {
            self.replay_pending_events();
        }
    }

    fn messages(&self) -> Result<Vec<Value>, String> {
        let session_id = self
            .session_id
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .ok_or_else(|| "OpenCode session 尚未建立".to_string())?;
        fetch_session_messages(
            &self.client,
            &self.base_url,
            &self.password,
            Path::new(&self.directory),
            &session_id,
        )
    }

    /// 助手消息 id 必须先通过服务端消息历史回溯到它前面的用户消息，再由
    /// 用户消息定位 Core Turn。找不到归属就不显示/不结算，绝不退回到
    /// “当前轮次”这个可变状态。
    fn resolve_assistant_core(&mut self, assistant_message_id: &str) -> String {
        if assistant_message_id.is_empty() {
            return String::new();
        }
        if let Some(core) = self.turn_bindings.get(assistant_message_id) {
            return core.clone();
        }
        for attempt in 0..2 {
            let messages = match self.messages() {
                Ok(messages) => messages,
                Err(error) => {
                    log::debug!("[agent] OpenCode 消息归属查询失败: {error}");
                    return String::new();
                }
            };
            let user_id = assistant_owner(&messages, assistant_message_id);
            if let Some(user_id) = user_id {
                let core = self
                    .input_bindings
                    .lock()
                    .ok()
                    .and_then(|bindings| bindings.get(&user_id).cloned())
                    .unwrap_or_default();
                if !core.is_empty() {
                    self.turn_bindings
                        .insert(assistant_message_id.to_string(), core.clone());
                    return core;
                }
            }
            if attempt == 0 {
                std::thread::sleep(Duration::from_millis(30));
            }
        }
        String::new()
    }

    fn sole_prompt_core(&self) -> String {
        let Ok(inputs) = self.prompt_inputs.lock() else {
            return String::new();
        };
        if inputs.len() == 1 {
            return inputs.keys().next().cloned().unwrap_or_default();
        }
        String::new()
    }

    fn remove_prompt(&mut self, core_turn_id: &str) {
        let input_id = self
            .prompt_inputs
            .lock()
            .ok()
            .and_then(|mut inputs| inputs.remove(core_turn_id))
            .map(|input| input.id());
        if let Some(input_id) = input_id {
            if let Ok(mut bindings) = self.input_bindings.lock() {
                bindings.remove(&input_id);
            }
        }
        self.turn_bindings.retain(|_, core| core != core_turn_id);
    }

    /// 只按服务端确认的消息历史终态结算。`session.idle` / `session.status`
    /// 本身没有任何轮次身份，因此它只是一个“去问服务端”的触发信号。
    fn reconcile_terminals(&mut self) {
        self.replay_pending_events();
        let cores: Vec<String> = match self.prompt_inputs.lock() {
            Ok(inputs) => inputs.keys().cloned().collect(),
            Err(_) => return,
        };
        if cores.is_empty() {
            return;
        }
        let messages = match self.messages() {
            Ok(messages) => messages,
            Err(error) => {
                log::debug!("[agent] OpenCode 终态核对失败: {error}");
                return;
            }
        };
        for core_turn_id in cores {
            let input_id = self
                .prompt_inputs
                .lock()
                .ok()
                .and_then(|inputs| inputs.get(&core_turn_id).map(|input| input.id()))
                .unwrap_or_default();
            if input_id.is_empty() {
                continue;
            }
            let Some(outcome) = terminal_outcome(&messages, &input_id) else {
                continue;
            };
            match outcome {
                TurnOutcome::Succeeded {
                    assistant_message_id,
                } => {
                    self.remove_prompt(&core_turn_id);
                    (self.emit)(RuntimeEvent::turn(
                        core_turn_id,
                        assistant_message_id,
                        AgentEvent::TurnCompleted,
                    ));
                }
                TurnOutcome::Failed { message } => {
                    self.remove_prompt(&core_turn_id);
                    (self.emit)(RuntimeEvent::turn(
                        core_turn_id,
                        "",
                        AgentEvent::TurnFailed { message },
                    ));
                }
                TurnOutcome::Interrupted => {
                    self.remove_prompt(&core_turn_id);
                    (self.emit)(RuntimeEvent::turn(
                        core_turn_id,
                        "",
                        AgentEvent::TurnFailed {
                            message: "OpenCode 本轮已被中断".to_string(),
                        },
                    ));
                }
            }
        }
    }

    fn session_error(&mut self, message: String) {
        self.session_error = Some(message.clone());
        (self.emit)(RuntimeEvent::session(AgentEvent::SessionError { message }));
    }

    fn event_core_id(&mut self, event_type: &str, payload: &Value) -> String {
        let native_turn_id = json_text(payload, &["assistantMessageID"])
            .or_else(|| json_text(payload, &["messageID"]))
            .unwrap_or_default();
        match event_type {
            "session.next.prompted" | "session.next.prompt.admitted" => {
                let user_message_id = json_text(payload, &["messageID"]).unwrap_or_default();
                self.input_bindings
                    .lock()
                    .ok()
                    .and_then(|bindings| bindings.get(&user_message_id).cloned())
                    .unwrap_or_default()
            }
            "session.status" | "session.idle" => String::new(),
            "permission.asked" => self.sole_prompt_core(),
            _ => self.resolve_assistant_core(&native_turn_id),
        }
    }

    fn should_buffer_event(&self, event_type: &str, payload: &Value) -> bool {
        if event_type == "permission.asked" {
            return true;
        }
        let has_message_id = json_text(payload, &["assistantMessageID"])
            .or_else(|| json_text(payload, &["messageID"]))
            .is_some();
        if event_type.starts_with("session.tool.")
            || event_type.starts_with("session.text.")
            || event_type.starts_with("session.reasoning.")
            || event_type == "session.next.step.failed"
            || matches!(
                event_type,
                "session.error" | "session.execution.failed" | "session.step.failed"
            )
        {
            return has_message_id;
        }
        false
    }

    fn buffer_event(&self, event_type: &str, payload: &Value) {
        let Ok(mut pending) = self.pending_events.lock() else {
            return;
        };
        if pending.len() >= 4096 {
            pending.pop_front();
        }
        pending.push_back((event_type.to_string(), payload.clone()));
        log::debug!("[agent] OpenCode 事件暂存，等待消息归属：{event_type}");
    }

    /// 消息历史里出现用户输入后，按原顺序回放暂存事件。第一条仍无法归属
    /// 的事件留在队首，保证增量顺序不被打乱。
    fn replay_pending_events(&mut self) {
        loop {
            let Some((event_type, payload)) = ({
                let Ok(mut pending) = self.pending_events.lock() else {
                    return;
                };
                pending.pop_front()
            }) else {
                return;
            };
            let core_turn_id = self.event_core_id(&event_type, &payload);
            if core_turn_id.is_empty() {
                if let Ok(mut pending) = self.pending_events.lock() {
                    pending.push_front((event_type, payload));
                }
                return;
            }
            let still_active = self
                .prompt_inputs
                .lock()
                .map(|inputs| inputs.contains_key(&core_turn_id))
                .unwrap_or(false);
            if !still_active {
                continue;
            }
            let native_turn_id = json_text(&payload, &["assistantMessageID"])
                .or_else(|| json_text(&payload, &["messageID"]))
                .unwrap_or_default();
            self.dispatch_event(&event_type, &payload, core_turn_id, native_turn_id);
        }
    }

    fn handle_event(&mut self, event_type: &str, payload: &Value) {
        let native_turn_id = json_text(payload, &["assistantMessageID"])
            .or_else(|| json_text(payload, &["messageID"]))
            .unwrap_or_default();
        let core_turn_id = self.event_core_id(event_type, payload);
        if core_turn_id.is_empty() && self.should_buffer_event(event_type, payload) {
            // POST 回应与 SSE 到达顺序没有保证；先保留，等用户消息可查询
            // 或 idle 触发回放，不能把已生成的增量直接丢掉。错误同时记入
            // 会话级状态，避免在归属确认前界面完全看不到失败。
            if matches!(
                event_type,
                "session.error" | "session.execution.failed" | "session.step.failed"
            ) || event_type == "session.next.step.failed"
            {
                self.session_error(error_message(payload));
            }
            self.buffer_event(event_type, payload);
            return;
        }
        self.dispatch_event(event_type, payload, core_turn_id, native_turn_id);
    }

    fn dispatch_event(
        &mut self,
        event_type: &str,
        payload: &Value,
        core_turn_id: String,
        native_turn_id: String,
    ) {
        match event_type {
            "session.text.delta" | "session.reasoning.delta" => {
                if core_turn_id.is_empty() {
                    return;
                }
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
                (self.emit)(RuntimeEvent::turn(
                    core_turn_id.clone(),
                    native_turn_id,
                    event,
                ));
            }
            "session.text.ended" | "session.reasoning.ended" => {
                if core_turn_id.is_empty() {
                    return;
                }
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
                if !text.is_empty() {
                    let event = if event_type == "session.text.ended" {
                        AgentEvent::TextDelta { text }
                    } else {
                        AgentEvent::ReasoningDelta { text }
                    };
                    (self.emit)(RuntimeEvent::turn(
                        core_turn_id.clone(),
                        native_turn_id,
                        event,
                    ));
                }
            }
            "session.next.step.ended" => {}
            "session.next.step.failed" => {
                if core_turn_id.is_empty() {
                    let message = error_message(payload);
                    self.session_error(message);
                    self.reconcile_terminals();
                    return;
                }
                let message = error_message(payload);
                self.remove_prompt(&core_turn_id);
                (self.emit)(RuntimeEvent::turn(
                    core_turn_id,
                    native_turn_id,
                    AgentEvent::TurnFailed { message },
                ));
            }
            "session.tool.input.started" | "session.tool.called" => {
                if core_turn_id.is_empty() {
                    return;
                }
                let tool_call_id = json_text(payload, &["id"])
                    .or_else(|| json_text(payload, &["toolCallID"]))
                    .unwrap_or_else(|| "tool".to_string());
                let name = json_text(payload, &["name"])
                    .or_else(|| json_text(payload, &["tool"]))
                    .unwrap_or_else(|| "工具".to_string());
                (self.emit)(RuntimeEvent::turn(
                    core_turn_id.clone(),
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
                (self.emit)(RuntimeEvent::turn(
                    core_turn_id.clone(),
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
                (self.emit)(RuntimeEvent::turn(
                    core_turn_id.clone(),
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
                (self.emit)(RuntimeEvent::turn(
                    core_turn_id.clone(),
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
                    self.reconcile_terminals();
                }
            }
            "session.idle" => {
                self.reconcile_terminals();
            }
            "session.error" | "session.execution.failed" | "session.step.failed" => {
                let message = error_message(payload);
                if core_turn_id.is_empty() {
                    // 没有消息身份的错误只能先记为会话级状态。idle 到来或
                    // 重连恢复时，再由服务端消息历史的 outcome 决定具体轮次。
                    self.session_error(message);
                    self.reconcile_terminals();
                    return;
                }
                self.remove_prompt(&core_turn_id);
                (self.emit)(RuntimeEvent::turn(
                    core_turn_id,
                    native_turn_id,
                    AgentEvent::TurnFailed { message },
                ));
            }
            _ => {}
        }
    }
}

fn fetch_session_messages(
    client: &reqwest::blocking::Client,
    base_url: &str,
    password: &str,
    working_dir: &Path,
    session_id: &str,
) -> Result<Vec<Value>, String> {
    let response = client
        .get(format!("{base_url}/api/session/{session_id}/message"))
        .basic_auth(SERVER_USER, Some(password))
        .query(&[
            ("location[directory]", location_value(working_dir)),
            ("limit", "200".to_string()),
            // 先取最新一页，再反转为时间正序，避免长会话超过一页后找不到
            // 当前轮次的用户消息。
            ("order", "desc".to_string()),
        ])
        .send()
        .map_err(|error| format!("读取 OpenCode 消息历史失败：{error}"))?;
    if !response.status().is_success() {
        return Err(response_error(response));
    }
    let value: Value = response.json().map_err(|error| error.to_string())?;
    let messages = value
        .get("data")
        .and_then(Value::as_array)
        .or_else(|| value.as_array())
        .ok_or_else(|| "OpenCode 消息历史格式不正确".to_string())?;
    let mut result: Vec<Value> = messages
        .iter()
        .map(|message| {
            message
                .get("info")
                .cloned()
                .unwrap_or_else(|| message.clone())
        })
        .collect();
    result.reverse();
    Ok(result)
}

fn error_message(payload: &Value) -> String {
    json_text(payload, &["error", "message"])
        .or_else(|| json_text(payload, &["error", "data", "message"]))
        .or_else(|| json_text(payload, &["error"]))
        .or_else(|| json_text(payload, &["message"]))
        .or_else(|| json_value_text(payload.get("error")))
        .unwrap_or_else(|| "OpenCode 请求失败".to_string())
}

fn message_id(info: &Value) -> String {
    json_text(info, &["id"]).unwrap_or_default()
}

fn message_type(info: &Value) -> String {
    json_text(info, &["type"])
        .or_else(|| json_text(info, &["role"]))
        .unwrap_or_default()
}

/// 在按时间正序的消息列表中，找到助手消息之前最近的用户消息。该用户消息
/// id 必须等于我们提交 prompt 时使用的 inbox id，才能证明助手输出归属。
fn assistant_owner(messages: &[Value], assistant_message_id: &str) -> Option<String> {
    let index = messages
        .iter()
        .position(|message| message_id(message) == assistant_message_id)?;
    for message in messages[..index].iter().rev() {
        if message_type(message) == "user" {
            return Some(message_id(message));
        }
    }
    None
}

/// 从服务端消息历史读取某个用户输入之后的唯一 idle 终态。没有 idle 就是
/// 仍在运行；绝不根据 assistant 时间戳或 `step.ended` 猜测完成。
fn terminal_outcome(messages: &[Value], input_id: &str) -> Option<TurnOutcome> {
    let input_index = messages
        .iter()
        .position(|message| message_id(message) == input_id && message_type(message) == "user")?;
    let mut assistant_message_id = String::new();
    for message in messages[input_index + 1..].iter() {
        match message_type(message).as_str() {
            "assistant" => {
                assistant_message_id = message_id(message);
            }
            "idle" => {
                let outcome = json_text(message, &["outcome"]).unwrap_or_default();
                return Some(match outcome.as_str() {
                    "succeeded" => TurnOutcome::Succeeded {
                        assistant_message_id,
                    },
                    "interrupted" => TurnOutcome::Interrupted,
                    "failed" => TurnOutcome::Failed {
                        message: error_message(message),
                    },
                    other => TurnOutcome::Failed {
                        message: format!("OpenCode 返回未知终态：{other}"),
                    },
                });
            }
            _ => {}
        }
    }
    None
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

/// 从 `Session.Info.model` 或 `Model.Info` 中取出 switchModel 需要的引用。
fn model_ref(value: &Value) -> Option<(String, String)> {
    let data = value.get("data").unwrap_or(value);
    let reference = data.get("model").unwrap_or(data);
    let provider = json_text(reference, &["providerID"])?;
    let id = json_text(reference, &["id"]).or_else(|| json_text(reference, &["modelID"]))?;
    if provider.is_empty() || id.is_empty() {
        return None;
    }
    Some((provider, id))
}

fn parse_requested_model(model: &str) -> Result<(String, String), String> {
    let (provider, id) = model.split_once('/').ok_or_else(|| {
        format!(
            "执行器与模型不匹配：OpenCode 需要完整的 provider/model，当前模型为 {model}（可能来自其它 CLI）"
        )
    })?;
    if provider.is_empty() || id.is_empty() {
        return Err(format!(
            "执行器与模型不匹配：OpenCode 模型标识不完整：{model}"
        ));
    }
    Ok((provider.to_string(), id.to_string()))
}

fn fetch_session_model(
    client: &reqwest::blocking::Client,
    base_url: &str,
    password: &str,
    working_dir: &Path,
    session_id: &str,
) -> Option<String> {
    let response = client
        .get(format!("{base_url}/api/session/{session_id}"))
        .basic_auth(SERVER_USER, Some(password))
        .query(&[("location[directory]", location_value(working_dir))])
        .send()
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let value: Value = response.json().ok()?;
    model_ref(&value).map(|(provider, id)| format!("{provider}/{id}"))
}

fn fetch_default_model(
    client: &reqwest::blocking::Client,
    base_url: &str,
    password: &str,
    working_dir: &Path,
) -> Result<(String, String), String> {
    let response = client
        .get(format!("{base_url}/api/model/default"))
        .basic_auth(SERVER_USER, Some(password))
        .query(&[("location[directory]", location_value(working_dir))])
        .send()
        .map_err(|error| format!("读取 OpenCode 默认模型失败：{error}"))?;
    if !response.status().is_success() {
        return Err(response_error(response));
    }
    let value: Value = response.json().map_err(|error| error.to_string())?;
    model_ref(&value).ok_or_else(|| "OpenCode 未返回默认模型".to_string())
}

fn switch_session_model(
    client: &reqwest::blocking::Client,
    base_url: &str,
    password: &str,
    session_id: &str,
    provider: &str,
    id: &str,
) -> Result<(), String> {
    let response = client
        .post(format!("{base_url}/api/session/{session_id}/model"))
        .basic_auth(SERVER_USER, Some(password))
        .json(&json!({
            "model": { "providerID": provider, "id": id }
        }))
        .send()
        .map_err(|error| format!("切换 OpenCode 模型失败：{error}"))?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(response_error(response))
    }
}

impl OpenCodeAdapter {
    fn cancel_admitted_input(
        &self,
        session_id: &str,
        input_id: &str,
    ) -> Result<CancelOutcome, String> {
        cancel_admitted_input_http(
            &self.client,
            &self.base_url,
            &self.password,
            &self.working_dir,
            session_id,
            input_id,
        )
    }
}

/// 取消一个已经被服务端受理的输入。只有删除 inbox、明确 `interrupted:true`
/// 或服务端 idle outcome 为 `interrupted` 才算取消成功；自然完成返回
/// `AlreadyFinished`，由消息历史终态按真实结果结算。
fn cancel_admitted_input_http(
    client: &reqwest::blocking::Client,
    base_url: &str,
    password: &str,
    working_dir: &Path,
    session_id: &str,
    input_id: &str,
) -> Result<CancelOutcome, String> {
    let inbox_contains = |input_id: &str| -> Result<bool, String> {
        let response = client
            .get(format!("{base_url}/api/session/{session_id}/inbox"))
            .basic_auth(SERVER_USER, Some(password))
            .query(&[("location[directory]", location_value(working_dir))])
            .send()
            .map_err(|error| format!("读取 OpenCode inbox 失败：{error}"))?;
        if !response.status().is_success() {
            return Err(response_error(response));
        }
        let value: Value = response.json().map_err(|error| error.to_string())?;
        let items = value
            .get("data")
            .and_then(Value::as_array)
            .or_else(|| value.as_array())
            .ok_or_else(|| "OpenCode inbox 格式不正确".to_string())?;
        Ok(items.iter().any(|item| {
            json_text(item, &["id"]).as_deref() == Some(input_id)
                || json_text(item, &["data", "id"]).as_deref() == Some(input_id)
        }))
    };
    let delete_inbox = |input_id: &str| -> Result<(), String> {
        let response = client
            .delete(format!(
                "{base_url}/api/session/{session_id}/inbox/{}",
                url_path_segment(input_id)
            ))
            .basic_auth(SERVER_USER, Some(password))
            .query(&[("location[directory]", location_value(working_dir))])
            .send()
            .map_err(|error| format!("取消 OpenCode inbox 消息失败：{error}"))?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(response_error(response))
        }
    };
    let interrupt_once = || -> Result<bool, String> {
        let response = client
            .post(format!("{base_url}/api/session/{session_id}/interrupt"))
            .basic_auth(SERVER_USER, Some(password))
            .query(&[("location[directory]", location_value(working_dir))])
            .send()
            .map_err(|error| format!("中断 OpenCode 请求失败：{error}"))?;
        if !response.status().is_success() {
            return Err(response_error(response));
        }
        let value: Value = response.json().unwrap_or_else(|_| json!({}));
        Ok(value
            .get("interrupted")
            .and_then(Value::as_bool)
            .or_else(|| {
                value
                    .get("data")
                    .and_then(|data| data.get("interrupted"))
                    .and_then(Value::as_bool)
            })
            .unwrap_or(false))
    };

    // 1. 尚未投递的 inbox 消息优先精确删除，避免“空闲时 interrupt
    //    返回 false，但随后队列里的消息又被执行”的竞态。DELETE 对已投递
    //    消息是 no-op，因此删除后还要确认消息没有出现在服务端历史里。
    let mut removed_from_inbox = false;
    match inbox_contains(input_id) {
        Ok(true) => {
            delete_inbox(input_id)?;
            std::thread::sleep(Duration::from_millis(50));
            match inbox_contains(input_id) {
                Ok(true) => {
                    return Err("OpenCode 消息仍在待执行队列中，无法确认取消".to_string());
                }
                Ok(false) => removed_from_inbox = true,
                Err(error) => {
                    log::debug!("[agent] OpenCode inbox 删除后复核失败: {error}");
                }
            }
        }
        Ok(false) => {}
        Err(error) => log::debug!("[agent] OpenCode inbox 查询失败，改用 interrupt: {error}"),
    }

    let mut last_error = None;
    for _ in 0..3 {
        match interrupt_once() {
            Ok(true) => return Ok(CancelOutcome::Cancelled),
            Ok(false) => {}
            Err(error) => last_error = Some(error),
        }
        std::thread::sleep(Duration::from_millis(80));
    }

    // 2. interrupt 没有确认时，再以服务端消息历史的 idle outcome 为准。
    match fetch_session_messages(client, base_url, password, working_dir, session_id) {
        Ok(messages) => {
            let delivered = messages
                .iter()
                .any(|message| message_id(message) == input_id && message_type(message) == "user");
            match terminal_outcome(&messages, input_id) {
                Some(TurnOutcome::Interrupted) => Ok(CancelOutcome::Cancelled),
                Some(_) => Ok(CancelOutcome::AlreadyFinished),
                // 已经确认从 inbox 删除，且服务端历史里没有这条消息，
                // 说明它从未开始执行。
                None if removed_from_inbox && !delivered => Ok(CancelOutcome::Cancelled),
                None => Err(last_error
                    .unwrap_or_else(|| "OpenCode 未确认中断（interrupted=false）".to_string())),
            }
        }
        Err(error) => Err(last_error.unwrap_or(error)),
    }
}

impl AgentAdapter for OpenCodeAdapter {
    fn prompt(&mut self, core_turn_id: &str, request: &AgentPromptRequest) -> Result<(), String> {
        let session_id = self
            .session_id
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .ok_or_else(|| "OpenCode 会话尚未建立".to_string())?;
        let round_turn_id = core_turn_id.to_string();
        // 官方允许客户端指定 inbox/用户消息 id。先登记再 POST，取消线程
        // 才能在 HTTP 响应回来之前把“取消意图”绑定到这同一个 Core Turn。
        let input_id = format!("msg_{}", uuid::Uuid::new_v4().to_string().replace('-', ""));
        let input = Arc::new(PromptInput::new(input_id.clone()));
        if let Ok(mut inputs) = self.prompt_inputs.lock() {
            inputs.insert(round_turn_id.clone(), input.clone());
        }
        // 必须在 POST 之前同时登记 user message → Core Turn。服务端可能
        // 先通过 SSE 发出 text/tool 增量，HTTP 回应后到；若等回应再登记，
        // 这些已经持久接收的增量会被误判为无归属。
        if let Ok(mut bindings) = self.input_bindings.lock() {
            bindings.insert(input_id.clone(), round_turn_id.clone());
        }
        if let Ok(mut pending) = self.pending_events.lock() {
            pending.clear();
        }
        // V2 的 prompt 接口不接受模型字段；模型必须在发送前通过
        // `POST /api/session/{id}/model` 切换。
        let body = json!({
            "id": input_id.clone(),
            "text": request.text_with_history_context(),
        });
        let requested_model = self.requested_model.clone();
        let actual_model = self.actual_model.clone();
        let client = self.client.clone();
        let base_url = self.base_url.clone();
        let password = self.password.clone();
        let working_dir = location_value(&self.working_dir);
        let emit = self.emit.clone();
        let prompt_inputs = self.prompt_inputs.clone();
        let input_bindings = self.input_bindings.clone();
        let pending_events = self.pending_events.clone();
        let thread_input = input.clone();
        let spawn_result = std::thread::Builder::new()
            .name("agent-opencode-prompt".to_string())
            .spawn(move || {
                let requested_label = requested_model
                    .clone()
                    .unwrap_or_else(|| "CLI 默认模型".to_string());
                let target = match requested_model.as_deref() {
                    Some(model) => parse_requested_model(model),
                    None => {
                        fetch_default_model(&client, &base_url, &password, Path::new(&working_dir))
                    }
                };
                match target {
                    Ok((provider, model_id)) => {
                        let target_label = format!("{provider}/{model_id}");
                        let current = actual_model.lock().ok().and_then(|guard| guard.clone());
                        if current.as_deref() != Some(target_label.as_str()) {
                            match switch_session_model(
                                &client,
                                &base_url,
                                &password,
                                &session_id,
                                &provider,
                                &model_id,
                            ) {
                                Ok(()) => {
                                    if let Ok(mut guard) = actual_model.lock() {
                                        *guard = Some(target_label.clone());
                                    }
                                    (emit)(RuntimeEvent::turn(
                                        round_turn_id.clone(),
                                        "",
                                        AgentEvent::ModelSwitchApplied {
                                            requested: requested_model.clone(),
                                            actual: Some(target_label),
                                        },
                                    ));
                                }
                                Err(error) => {
                                    (emit)(RuntimeEvent::turn(
                                        round_turn_id.clone(),
                                        "",
                                        AgentEvent::ModelSwitchFailed {
                                            requested: requested_label,
                                            actual: current,
                                            message: format!(
                                                "OpenCode 切换模型失败，继续使用当前模型：{error}"
                                            ),
                                        },
                                    ));
                                }
                            }
                        }
                    }
                    Err(error) => {
                        let current = actual_model.lock().ok().and_then(|guard| guard.clone());
                        (emit)(RuntimeEvent::turn(
                            round_turn_id.clone(),
                            "",
                            AgentEvent::ModelSwitchFailed {
                                requested: requested_label,
                                actual: current,
                                message: format!(
                                    "无法确定 OpenCode 目标模型，继续使用当前模型：{error}"
                                ),
                            },
                        ));
                    }
                }
                let response = client
                    .post(format!("{base_url}/api/session/{session_id}/prompt"))
                    .basic_auth(SERVER_USER, Some(&password))
                    .query(&[("location[directory]", working_dir.clone())])
                    .json(&body)
                    .send();
                match response {
                    Ok(response) if response.status().is_success() => {
                        let old_id = thread_input.id();
                        let accepted_id = response
                            .json::<Value>()
                            .ok()
                            .and_then(|value| {
                                json_text(&value, &["data", "id"])
                                    .or_else(|| json_text(&value, &["id"]))
                            })
                            .filter(|id| !id.is_empty())
                            .unwrap_or_else(|| old_id.clone());
                        if accepted_id != old_id {
                            // 服务端替换了消息 id 时原子迁移映射，暂存事件
                            // 会在下一条 SSE/ idle 触发时按新用户消息回放。
                            thread_input.set_id(accepted_id.clone());
                            if let Ok(mut bindings) = input_bindings.lock() {
                                let core = bindings
                                    .remove(&old_id)
                                    .unwrap_or_else(|| round_turn_id.clone());
                                bindings.insert(accepted_id.clone(), core);
                            }
                        }
                        thread_input.accepted.store(true, Ordering::SeqCst);
                        (emit)(RuntimeEvent::turn(
                            round_turn_id.clone(),
                            "",
                            AgentEvent::PromptAccepted,
                        ));
                        // 先发受理事件再唤醒等待中的取消线程，确保立即停止
                        // 不会让“已发出”的本轮输入统计被取消回调抢先清掉。
                        thread_input.mark_post_done();
                        if thread_input.cancel_requested.load(Ordering::SeqCst)
                            && thread_input.claim_cancel()
                        {
                            // 同步 cancel() 可能已经超时返回；这里必须继续
                            // 执行已登记的取消意图，并用 TurnCancelled 把
                            // 真实结果送回 Qt 后端。
                            let result = cancel_admitted_input_http(
                                &client,
                                &base_url,
                                &password,
                                Path::new(&working_dir),
                                &session_id,
                                &accepted_id,
                            );
                            thread_input.settle_cancel(result.clone());
                            match result {
                                Ok(CancelOutcome::Cancelled) => {
                                    if let Ok(mut inputs) = prompt_inputs.lock() {
                                        inputs.remove(&round_turn_id);
                                    }
                                    if let Ok(mut bindings) = input_bindings.lock() {
                                        bindings.remove(&accepted_id);
                                    }
                                    if let Ok(mut pending) = pending_events.lock() {
                                        pending.clear();
                                    }
                                    (emit)(RuntimeEvent::turn(
                                        round_turn_id,
                                        "",
                                        AgentEvent::TurnCancelled,
                                    ));
                                }
                                Ok(CancelOutcome::AlreadyFinished) => {
                                    // 服务端已经自然结束；保留映射让处理器按
                                    // 消息历史 idle outcome 结算真实终态。
                                }
                                Ok(CancelOutcome::Pending) => {
                                    // 取消通知已发出但尚未确认；不在这里补发
                                    // TurnCancelled，保留映射等真实终态结算。
                                }
                                Err(error) => {
                                    thread_input.release_cancel_claim();
                                    (emit)(RuntimeEvent::session(AgentEvent::SessionError {
                                        message: format!("OpenCode 取消失败：{error}"),
                                    }));
                                }
                            }
                        }
                    }
                    Ok(response) => {
                        let message = response_error(response);
                        if let Ok(mut inputs) = prompt_inputs.lock() {
                            inputs.remove(&round_turn_id);
                        }
                        if let Ok(mut bindings) = input_bindings.lock() {
                            bindings.remove(&thread_input.id());
                        }
                        if let Ok(mut pending) = pending_events.lock() {
                            pending.clear();
                        }
                        thread_input.mark_post_done();
                        let event = if thread_input.cancel_requested.load(Ordering::SeqCst) {
                            AgentEvent::TurnCancelled
                        } else {
                            AgentEvent::TurnFailed { message }
                        };
                        (emit)(RuntimeEvent::turn(round_turn_id, "", event));
                    }
                    Err(error) => {
                        if let Ok(mut inputs) = prompt_inputs.lock() {
                            inputs.remove(&round_turn_id);
                        }
                        if let Ok(mut bindings) = input_bindings.lock() {
                            bindings.remove(&thread_input.id());
                        }
                        if let Ok(mut pending) = pending_events.lock() {
                            pending.clear();
                        }
                        thread_input.mark_post_done();
                        let event = if thread_input.cancel_requested.load(Ordering::SeqCst) {
                            AgentEvent::TurnCancelled
                        } else {
                            AgentEvent::TurnFailed {
                                message: format!("发送 OpenCode 消息失败：{error}"),
                            }
                        };
                        (emit)(RuntimeEvent::turn(round_turn_id, "", event));
                    }
                }
                // 完成事件由 `/api/event` 的 idle 触发，但终态必须由
                // `/api/session/:id/message` 的 idle outcome 确认。
            });
        if let Err(error) = spawn_result {
            input.mark_post_done();
            if let Ok(mut inputs) = self.prompt_inputs.lock() {
                inputs.remove(core_turn_id);
            }
            if let Ok(mut bindings) = self.input_bindings.lock() {
                bindings.remove(&input_id);
            }
            if let Ok(mut pending) = self.pending_events.lock() {
                pending.clear();
            }
            return Err(format!("无法启动 OpenCode 请求线程：{error}"));
        }
        Ok(())
    }

    fn native_session_id(&self) -> Option<String> {
        self.session_id.lock().ok().and_then(|guard| guard.clone())
    }

    fn set_model(&mut self, model: Option<String>) {
        self.requested_model = model;
    }

    fn history_restored(&self) -> bool {
        self.history_restored
    }

    fn cancel(&mut self, core_turn_id: &str) -> Result<CancelOutcome, String> {
        let session_id = self
            .session_id
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .ok_or_else(|| "OpenCode 会话尚未建立".to_string())?;
        let input = self
            .prompt_inputs
            .lock()
            .ok()
            .and_then(|inputs| inputs.get(core_turn_id).cloned());
        let Some(input) = input else {
            // 本轮已经结算或从未提交，重复停止是幂等的。
            return Ok(CancelOutcome::AlreadyFinished);
        };
        input.cancel_requested.store(true, Ordering::SeqCst);
        if !input.wait_post_done(Duration::from_secs(15)) {
            // 同步等待只负责给 UI 一个明确反馈；POST worker 稍后成功时
            // 会读取 cancel_requested 并继续执行取消，最终发 TurnCancelled。
            return Err("OpenCode 尚未确认消息是否已受理，取消任务仍在后台等待".to_string());
        }
        if !input.accepted.load(Ordering::SeqCst) {
            // POST 失败意味着消息没有进入服务端执行队列，取消已经满足。
            return Ok(CancelOutcome::Cancelled);
        }
        let input_id = input.id();
        let result = if input.claim_cancel() {
            // 同一次取消只允许一个执行者删除 inbox/interrupt。
            let result = self.cancel_admitted_input(&session_id, &input_id);
            input.settle_cancel(result.clone());
            result
        } else {
            // POST worker 已经接手取消；等待它返回真实结果，避免重复操作。
            input
                .wait_cancel_result(Duration::from_secs(15))
                .unwrap_or_else(|| Err("OpenCode 正在后台确认取消结果，请稍后重试".to_string()))
        };
        if result.is_err() {
            input.release_cancel_claim();
        }
        let result = result?;
        if result == CancelOutcome::Cancelled {
            if let Ok(mut inputs) = self.prompt_inputs.lock() {
                inputs.remove(core_turn_id);
            }
            if let Ok(mut bindings) = self.input_bindings.lock() {
                bindings.remove(&input_id);
            }
            if let Ok(mut pending) = self.pending_events.lock() {
                pending.clear();
            }
        }
        Ok(result)
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
        self.stop_events.store(true, Ordering::SeqCst);
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
