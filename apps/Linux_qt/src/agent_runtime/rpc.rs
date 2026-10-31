//! 极小的 stdio JSON-RPC 客户端，供 ACP 与 Codex app-server 共用。
//!
//! 只实现协议真正需要的能力：按行读写 JSON、请求/响应关联、通知回调、
//! 进程生命周期。不把任意 stdout 当 JSON 解析；非 JSON 行记录到日志。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, ExitStatus};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::process_supervisor::{self, ProcessSupervisor, SharedStdin};

type NotificationHandler = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;

/// 只保留 stderr 尾部，既能让失败原因进入用户可见错误，又避免把整段
/// 日志常驻内存。
const STDERR_TAIL_LIMIT: usize = 16 * 1024;
/// stdout 已关闭但素笺没有发起关闭时，先给子进程一个自行退出的窗口；
/// 超过后只能视为通道不可恢复并强制回收。
const EXIT_WAIT_TIMEOUT: Duration = Duration::from_secs(2);

/// 谁、为什么结束了运行实例。关闭原因必须显式传递，不能再由 `Drop`
/// 隐式 kill，否则日志会把素笺主动回收误报成 Agent 自己退出。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CloseReason {
    /// 用户切换作品、CLI 或会话。
    UserSwitch(String),
    /// 应用退出。
    AppExit,
    /// 同一会话被新的执行者或原生实例替换。
    SessionReplaced(String),
    /// RPC 通道损坏，无法继续复用子进程。
    ChannelError(String),
    /// 请求超时后主动结束进程。
    Timeout(String),
    /// 一次性探测/查询结束，回收探测专用子进程。
    ProbeFinished,
}

impl CloseReason {
    pub fn description(&self) -> String {
        match self {
            Self::UserSwitch(detail) => format!("用户切换（{detail}）"),
            Self::AppExit => "应用退出".to_string(),
            Self::SessionReplaced(detail) => format!("会话实例替换（{detail}）"),
            Self::ChannelError(detail) => format!("RPC 通道异常（{detail}）"),
            Self::Timeout(detail) => format!("请求超时（{detail}）"),
            Self::ProbeFinished => "一次性探测结束".to_string(),
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct RequestStats {
    sent: u64,
    responses: u64,
    timeouts: u64,
    disconnects: u64,
}

struct LifecycleState {
    closed: Option<CloseReason>,
    stats: RequestStats,
}

impl Default for LifecycleState {
    fn default() -> Self {
        Self {
            closed: None,
            stats: RequestStats::default(),
        }
    }
}

struct PendingRequest {
    method: String,
    response: Sender<Result<serde_json::Value, String>>,
}

pub struct RpcClient {
    label: String,
    pid: u32,
    stdin: SharedStdin,
    pending: Arc<Mutex<HashMap<i64, PendingRequest>>>,
    next_id: AtomicI64,
    /// 子进程由所有者线程持有并回收；这里只共享控制通道与退出状态。
    supervisor: ProcessSupervisor,
    notification_handler: NotificationHandler,
    lifecycle: Arc<Mutex<LifecycleState>>,
    /// stdout 已关闭后记录的真实退出原因，供之后才发出的请求直接失败。
    exit_reason: Arc<Mutex<Option<String>>>,
}

impl RpcClient {
    pub fn spawn(
        label: &str,
        mut command: Command,
        working_dir: &std::path::Path,
        env: &[(String, String)],
        notification_handler: NotificationHandler,
    ) -> Result<Self, String> {
        command.current_dir(working_dir);
        for (key, value) in env {
            command.env(key, value);
        }
        // `spawn` 必须发生在与 CLI 同寿命的所有者线程里：bwrap 的
        // --die-with-parent 绑定创建线程，而不是整个进程（Issue #1024）。
        let process_supervisor::SpawnedProcess {
            pid,
            stdin,
            stdout,
            stderr,
            supervisor,
        } = process_supervisor::spawn(label, command)?;

        let pending: Arc<Mutex<HashMap<i64, PendingRequest>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let exit_reason: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let lifecycle: Arc<Mutex<LifecycleState>> = Arc::new(Mutex::new(LifecycleState::default()));
        let stderr_tail: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
        let stderr_tail_reader = stderr_tail.clone();
        let stderr_handle = stderr.and_then(|stderr| {
            std::thread::Builder::new()
                .name("agent-stderr-reader".to_string())
                .spawn(move || {
                    let reader = BufReader::new(stderr);
                    for line in reader.lines().map_while(Result::ok) {
                        let sanitized = sanitize_stderr_line(&line);
                        log::debug!(target: "agent", "agent stderr: {sanitized}");
                        append_stderr_tail(&stderr_tail_reader, &sanitized);
                    }
                })
                .ok()
        });
        let reader_pending = pending.clone();
        let reader_handler = notification_handler.clone();
        let reader_supervisor = supervisor.clone();
        let reader_lifecycle = lifecycle.clone();
        let reader_label = label.to_string();
        let reader_exit_reason = exit_reason.clone();
        std::thread::Builder::new()
            .name("agent-jsonrpc-reader".to_string())
            .spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines().map_while(Result::ok) {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    let value: serde_json::Value = match serde_json::from_str(trimmed) {
                        Ok(value) => value,
                        Err(_) => {
                            log::debug!(target: "agent", "agent stdout: {trimmed}");
                            continue;
                        }
                    };
                    // JSON-RPC 先按消息形态分流，不能先看 `id`：ACP 权限和
                    // Codex 审批都是“method + id”的对端请求，若按响应取走会
                    // 让 Agent 永久等待，甚至误完成同号的客户端请求。
                    if let Some(method) = value
                        .get("method")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                    {
                        // 传递完整消息：对端请求保留原始 id 供 respond() 回复；
                        // 无 id 的通知也走同一 handler。
                        (reader_handler)(&method, value);
                        continue;
                    }
                    if value.get("result").is_none() && value.get("error").is_none() {
                        log::debug!(target: "agent", "agent message without method/result: {trimmed}");
                        continue;
                    }
                    if let Some(id) = value.get("id").and_then(serde_json::Value::as_i64) {
                        if let Some(pending) = reader_pending
                            .lock()
                            .ok()
                            .and_then(|mut map| map.remove(&id))
                        {
                            if let Ok(mut state) = reader_lifecycle.lock() {
                                state.stats.responses += 1;
                            }
                            let response = match value.get("error") {
                                Some(error) => Err(error
                                    .get("message")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or("JSON-RPC 请求失败")
                                    .to_string()),
                                None => Ok(value
                                    .get("result")
                                    .cloned()
                                    .unwrap_or(serde_json::Value::Null)),
                            };
                            let _ = pending.response.send(response);
                        }
                    }
                }
                // stdout 已关闭。先区分“素笺主动关闭”和“Agent 自己退出”：
                // 退出状态与回收由所有者线程统一记录，reader 只等待结果，
                // 不能再直接 try_wait/kill，避免与 close() 重复回收。
                let requested_close = reader_lifecycle
                    .lock()
                    .ok()
                    .and_then(|state| state.closed.clone())
                    .or_else(|| reader_supervisor.close_reason());
                let wait = if requested_close.is_some() {
                    process_supervisor::CLOSE_GRACE
                } else {
                    EXIT_WAIT_TIMEOUT
                };
                let mut exit = reader_supervisor.wait_exit(wait);
                if exit.is_none() {
                    let reason = requested_close.clone().unwrap_or_else(|| {
                        CloseReason::ChannelError(format!(
                            "{reader_label} 的 stdout 已关闭，但进程仍在运行"
                        ))
                    });
                    let _ = mark_closed(&reader_lifecycle, reason.clone());
                    reader_supervisor.close(reason);
                    // 所有者线程关闭 stdin 后还有一段协议退出宽限期。
                    exit = reader_supervisor.wait_exit(
                        process_supervisor::CLOSE_GRACE + Duration::from_secs(1),
                    );
                }
                if exit.is_some() {
                    if let Some(handle) = stderr_handle {
                        wait_for_stderr_reader(handle);
                    }
                }
                let tail = stderr_tail
                    .lock()
                    .map(|tail| tail.clone())
                    .unwrap_or_default();
                let requested_close = exit
                    .as_ref()
                    .and_then(|info| info.requested_close.clone())
                    .or(requested_close);
                let forced = exit.as_ref().is_some_and(|info| info.forced);
                let forced_reason = exit.as_ref().and_then(|info| info.forced_reason.clone());
                let status = exit.as_ref().and_then(|info| info.status);
                let cause = if forced {
                    ExitCause::ForcedReap(
                        forced_reason
                            .or_else(|| requested_close.clone())
                            .unwrap_or_else(|| {
                                CloseReason::ChannelError("进程未随协议通道退出".to_string())
                            }),
                    )
                } else if let Some(reason) = requested_close {
                    ExitCause::RequestedClose(reason)
                } else if status.and_then(|status| status.signal()).is_some() {
                    ExitCause::ExternalSignal
                } else {
                    ExitCause::SelfExit
                };
                let reason = describe_exit(&reader_label, pid, status, &tail, cause);
                if let Ok(mut slot) = reader_exit_reason.lock() {
                    *slot = Some(reason.clone());
                }
                if let Ok(mut map) = reader_pending.lock() {
                    for (_, pending) in map.drain() {
                        let _ = pending.response.send(Err(format!(
                            "{}（{} 阶段）失败：{}",
                            reader_label, pending.method, reason
                        )));
                    }
                }
            })
            .map_err(|error| format!("无法启动 Agent 读取线程：{error}"))?;

        Ok(Self {
            label: label.to_string(),
            pid,
            stdin,
            pending,
            next_id: AtomicI64::new(1),
            supervisor,
            notification_handler,
            lifecycle,
            exit_reason,
        })
    }

    pub fn request(
        &self,
        method: &str,
        params: serde_json::Value,
        timeout: Duration,
    ) -> Result<serde_json::Value, String> {
        self.request_observed(method, params, timeout, || {})
    }

    /// 运行实例标识（协议名 + PID），用于把模型切换、请求与进程退出串在
    /// 同一条诊断链路里。
    pub fn instance_id(&self) -> String {
        format!("{}#{}", self.label, self.pid)
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// 发送请求，并在 JSON-RPC 报文真正写入 stdin 后立刻回调。
    ///
    /// ACP 的 `session/prompt` 是长请求：写入成功即表示消息已交给 Agent，
    /// 不能等整个回答结束才把它算作“已发送输入”。回调只表示协议通道受理，
    /// 轮次结果仍由后续响应的 `Ok/Err` 决定。
    pub fn request_observed(
        &self,
        method: &str,
        params: serde_json::Value,
        timeout: Duration,
        on_accepted: impl FnOnce(),
    ) -> Result<serde_json::Value, String> {
        if let Some(reason) = self.closed_reason() {
            return Err(format!(
                "{method} 阶段失败：{} 已关闭（{}）",
                self.label,
                reason.description()
            ));
        }
        if let Some(reason) = self.exited_reason() {
            return Err(format!("{method} 阶段失败：{reason}"));
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver): (Sender<Result<serde_json::Value, String>>, Receiver<_>) =
            mpsc::channel();
        self.pending
            .lock()
            .map_err(|_| "JSON-RPC 状态锁损坏".to_string())?
            .insert(
                id,
                PendingRequest {
                    method: method.to_string(),
                    response: sender,
                },
            );
        let started = Instant::now();
        log::info!(
            "[agent] RPC #{} {} 发送阶段开始（实例 {}）",
            id,
            method,
            self.instance_id()
        );
        if let Err(error) = self.write_json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        })) {
            if let Ok(mut map) = self.pending.lock() {
                map.remove(&id);
            }
            if let Some(reason) = self.closed_reason() {
                return Err(format!(
                    "{method} 阶段失败：{} 已关闭（{}）",
                    self.label,
                    reason.description()
                ));
            }
            if let Some(reason) = self.exited_reason() {
                return Err(format!("{method} 阶段失败：{reason}"));
            }
            return Err(error);
        }
        if let Ok(mut state) = self.lifecycle.lock() {
            state.stats.sent += 1;
        }
        log::debug!(
            "[agent] RPC #{} {} 已写入 stdin（实例 {}，{}ms）",
            id,
            method,
            self.instance_id(),
            started.elapsed().as_millis()
        );
        on_accepted();
        match receiver.recv_timeout(timeout) {
            Ok(result) => {
                log::info!(
                    "[agent] RPC #{} {} 收到响应（实例 {}，{}ms，结果：{}）",
                    id,
                    method,
                    self.instance_id(),
                    started.elapsed().as_millis(),
                    if result.is_ok() { "ok" } else { "error" }
                );
                result
            }
            Err(RecvTimeoutError::Timeout) => {
                if let Ok(mut map) = self.pending.lock() {
                    map.remove(&id);
                }
                if let Ok(mut state) = self.lifecycle.lock() {
                    state.stats.timeouts += 1;
                }
                if let Some(reason) = self.exited_reason() {
                    log::warn!(
                        "[agent] RPC #{} {} 超时但进程已退出（实例 {}，{}ms）：{}",
                        id,
                        method,
                        self.instance_id(),
                        started.elapsed().as_millis(),
                        reason
                    );
                    return Err(format!("{method} 阶段失败：{reason}"));
                }
                log::warn!(
                    "[agent] RPC #{} {} 超时（实例 {}，{}ms）",
                    id,
                    method,
                    self.instance_id(),
                    started.elapsed().as_millis()
                );
                Err(format!(
                    "{method} 超时（{} 秒，请求 #{id}）",
                    timeout.as_secs()
                ))
            }
            Err(RecvTimeoutError::Disconnected) => {
                if let Ok(mut map) = self.pending.lock() {
                    map.remove(&id);
                }
                if let Ok(mut state) = self.lifecycle.lock() {
                    state.stats.disconnects += 1;
                }
                if let Some(reason) = self.exited_reason() {
                    log::warn!(
                        "[agent] RPC #{} {} 等待响应时断连（实例 {}，{}ms）：{}",
                        id,
                        method,
                        self.instance_id(),
                        started.elapsed().as_millis(),
                        reason
                    );
                    return Err(format!("{method} 阶段失败：{reason}"));
                }
                Err(format!("{method} 等待响应时 Agent 已退出"))
            }
        }
    }

    fn exited_reason(&self) -> Option<String> {
        self.exit_reason
            .lock()
            .ok()
            .and_then(|reason| reason.clone())
    }

    fn closed_reason(&self) -> Option<CloseReason> {
        self.lifecycle
            .lock()
            .ok()
            .and_then(|state| state.closed.clone())
    }

    pub fn notify(&self, method: &str, params: serde_json::Value) -> Result<(), String> {
        self.write_json(&serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
        }))
    }

    pub fn respond(&self, id: i64, result: serde_json::Value) -> Result<(), String> {
        self.write_json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": result,
        }))
    }

    fn write_json(&self, value: &serde_json::Value) -> Result<(), String> {
        if let Some(reason) = self.closed_reason() {
            return Err(format!(
                "{} 已关闭，不能继续写入（{}）",
                self.label,
                reason.description()
            ));
        }
        let mut guard = self
            .stdin
            .lock()
            .map_err(|_| "Agent stdin 锁损坏".to_string())?;
        let stdin = guard
            .as_mut()
            .ok_or_else(|| format!("{} 的 stdin 已关闭", self.label))?;
        serde_json::to_writer(&mut *stdin, value).map_err(|error| error.to_string())?;
        stdin
            .write_all(b"\n")
            .and_then(|()| stdin.flush())
            .map_err(|error| format!("写入 Agent 失败：{error}"))
    }

    /// 显式关闭入口：先停止接收新请求，再处理在途请求、关闭 stdin，
    /// 最后在宽限期后回收子进程。只有 RPC 通道已不可恢复时才发送 SIGKILL，
    /// 并记录为素笺主动触发。
    pub fn close(&self, reason: CloseReason) {
        if !mark_closed(&self.lifecycle, reason.clone()) {
            return;
        }
        let stats = self
            .lifecycle
            .lock()
            .map(|state| state.stats)
            .unwrap_or_default();
        log::info!(
            "[agent] 关闭 {}（PID {}）：{}；请求统计：发送 {}，响应 {}，超时 {}，断连 {}",
            self.label,
            self.pid,
            reason.description(),
            stats.sent,
            stats.responses,
            stats.timeouts,
            stats.disconnects
        );
        if let Ok(mut map) = self.pending.lock() {
            for (id, pending) in map.drain() {
                let _ = pending.response.send(Err(format!(
                    "{}（{} 阶段）失败：进程关闭（{}，请求 #{id}）",
                    self.label,
                    pending.method,
                    reason.description()
                )));
            }
        }
        // 关闭 stdin、等待协议退出、超时 kill 都交给进程所有者线程；
        // reader EOF 与外部 close 不会各自回收同一子进程。
        self.supervisor.close(reason);
    }

    pub fn notification_handler(&self) -> NotificationHandler {
        self.notification_handler.clone()
    }
}

impl Drop for RpcClient {
    fn drop(&mut self) {
        // 正常路径由适配器显式 close()；这里是最后一道兜底，避免调用方
        // 忘记关闭时留下仍在沙箱内运行的 CLI。
        if self.closed_reason().is_none() {
            self.supervisor.close(CloseReason::AppExit);
        }
    }
}

fn mark_closed(lifecycle: &Arc<Mutex<LifecycleState>>, reason: CloseReason) -> bool {
    let Ok(mut state) = lifecycle.lock() else {
        return false;
    };
    if state.closed.is_some() {
        return false;
    }
    state.closed = Some(reason);
    true
}

fn wait_for_stderr_reader(handle: std::thread::JoinHandle<()>) {
    let deadline = Instant::now() + Duration::from_millis(500);
    while !handle.is_finished() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    if handle.is_finished() {
        let _ = handle.join();
    }
}

enum ExitCause {
    /// 素笺显式请求结束，子进程随后自行退出。
    RequestedClose(CloseReason),
    /// 素笺在关闭宽限期后主动发送 SIGKILL 回收。
    ForcedReap(CloseReason),
    /// 素笺没有请求结束，进程被外部信号终止。
    ExternalSignal,
    /// 素笺没有请求结束，只有退出码；无法判断真正来源。
    SelfExit,
}

fn describe_exit(
    label: &str,
    pid: u32,
    status: Option<ExitStatus>,
    tail: &str,
    cause: ExitCause,
) -> String {
    let status_text = match status {
        Some(status) => match status.code() {
            Some(code) => format!("退出码 {code}"),
            None => format!("被信号终止（{status}）"),
        },
        None => "退出状态未知".to_string(),
    };
    let provenance = match &cause {
        ExitCause::RequestedClose(reason) => {
            format!("素笺主动请求结束（原因：{}）", reason.description())
        }
        ExitCause::ForcedReap(reason) => format!(
            "素笺主动发送 SIGKILL 回收（原因：{}）",
            reason.description()
        ),
        ExitCause::ExternalSignal => "结束来源：外部信号（素笺未发送结束信号）".to_string(),
        ExitCause::SelfExit => "结束来源未知（素笺未发送结束信号，仅有退出码）".to_string(),
    };
    let mut message = format!(
        "{label} 进程退出（PID {pid}，{status_text}）：{provenance}；{}",
        classify_exit(tail)
    );
    let tail = tail.trim();
    if !tail.is_empty() {
        let mut detail: Vec<char> = tail.chars().rev().take(1500).collect();
        detail.reverse();
        message.push_str("；stderr：");
        message.push_str(&detail.into_iter().collect::<String>().replace('\n', " | "));
    }
    message
}

fn classify_exit(tail: &str) -> &'static str {
    let lower = tail.to_ascii_lowercase();
    let contains = |needles: &[&str]| needles.iter().any(|needle| lower.contains(needle));
    if contains(&["bwrap", "namespace", "sandbox", "permission denied"]) {
        "沙箱启动失败，请检查 bubblewrap 与用户命名空间权限"
    } else if contains(&[
        "unauthorized",
        "api key",
        "api_key",
        "not logged in",
        "login required",
        "auth",
        "credential",
        "401",
        "403",
    ]) {
        "CLI 认证或凭证读取失败，请检查登录状态与配置文件"
    } else if contains(&[
        "unrecognized",
        "unknown option",
        "unexpected argument",
        "invalid argument",
        "usage:",
        "unknown command",
    ]) {
        "CLI 参数不受支持，可能是版本不兼容"
    } else if contains(&[
        "invalid json",
        "parse error",
        "json parse",
        "protocol",
        "method not found",
        "unknown method",
        "unsupported",
    ]) {
        "CLI 协议不兼容或返回了无法识别的响应"
    } else if contains(&[
        "server error",
        "internal error",
        "overloaded",
        "rate limit",
        "too many requests",
        "429",
        "500",
        "502",
        "503",
        "504",
    ]) {
        "服务端返回错误（认证、限流或远端故障）"
    } else {
        "CLI 进程异常退出"
    }
}

fn append_stderr_tail(tail: &Arc<Mutex<String>>, line: &str) {
    let Ok(mut tail) = tail.lock() else {
        return;
    };
    tail.push_str(line);
    tail.push('\n');
    if tail.len() > STDERR_TAIL_LIMIT {
        let mut cut = tail.len() - STDERR_TAIL_LIMIT;
        while cut < tail.len() && !tail.is_char_boundary(cut) {
            cut += 1;
        }
        tail.drain(..cut);
    }
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

fn sanitize_stderr_line(line: &str) -> String {
    let line = redact_after_marker(line, "bearer");
    let line = redact_sensitive_assignments(&line);
    redact_known_tokens(&line)
}

/// 把敏感键后面的值替换为 `***`，避免 stderr 把 API Key、token 或
/// 认证头原样带进用户可见错误。
fn redact_sensitive_assignments(line: &str) -> String {
    const KEYS: &[&str] = &[
        "access_token",
        "refresh_token",
        "authorization",
        "private_key",
        "credential",
        "password",
        "api_key",
        "apikey",
        "api-key",
        "secret",
        "cookie",
        "token",
    ];
    let mut result = line.to_string();
    for key in KEYS {
        result = redact_assignment(&result, key);
    }
    result
}

fn redact_assignment(line: &str, key: &str) -> String {
    let mut result = String::with_capacity(line.len());
    let mut cursor = 0usize;
    while let Some(relative) = find_ascii_case_insensitive(&line[cursor..], key) {
        let start = cursor + relative;
        let after_key = start + key.len();
        let before = line[..start].chars().next_back();
        let after = line[after_key..].chars().next();
        // 下划线/连字符常出现在环境变量名里（如 DEEPSEEK_API_KEY），
        // 应视为键名边界而不是普通单词的一部分。
        let is_partial_word = before
            .map(|value| value.is_ascii_alphanumeric())
            .unwrap_or(false)
            || after
                .map(|value| value.is_ascii_alphanumeric() || value == '_' || value == '-')
                .unwrap_or(false);
        if is_partial_word {
            result.push_str(&line[cursor..after_key]);
            cursor = after_key;
            continue;
        }
        // 兼容 JSON 风格的 `"apiKey": "..."`：键名后可能先出现引号，
        // 再出现冒号。
        let mut probe = after_key;
        if let Some(quote) = line[probe..].chars().next() {
            if quote == '"' || quote == '\'' {
                probe += quote.len_utf8();
            }
        }
        let rest = &line[probe..];
        let whitespace = rest.len() - rest.trim_start_matches([' ', '\t']).len();
        let delimiter_at = probe + whitespace;
        let Some(delimiter) = line[delimiter_at..].chars().next() else {
            break;
        };
        if delimiter != ':' && delimiter != '=' {
            result.push_str(&line[cursor..after_key]);
            cursor = after_key;
            continue;
        }
        let after_delimiter = delimiter_at + delimiter.len_utf8();
        let rest = &line[after_delimiter..];
        let whitespace = rest.len() - rest.trim_start_matches([' ', '\t']).len();
        let value_start = after_delimiter + whitespace;
        result.push_str(&line[cursor..value_start]);
        let value = &line[value_start..];
        let Some(first) = value.chars().next() else {
            cursor = value_start;
            continue;
        };
        let (consumed, replacement) = if first == '"' || first == '\'' {
            if let Some(end) = value[1..].find(first) {
                (1 + end + first.len_utf8(), format!("{first}***{first}"))
            } else {
                (value.len(), format!("{first}***"))
            }
        } else {
            let end = value
                .char_indices()
                .find(|(_, value)| value.is_whitespace() || matches!(value, ',' | ';' | '}' | ']'))
                .map(|(index, _)| index)
                .unwrap_or(value.len());
            (end, "***".to_string())
        };
        result.push_str(&replacement);
        cursor = value_start + consumed;
    }
    result.push_str(&line[cursor..]);
    result
}

fn redact_after_marker(line: &str, marker: &str) -> String {
    let mut result = String::with_capacity(line.len());
    let mut cursor = 0usize;
    while let Some(relative) = find_ascii_case_insensitive(&line[cursor..], marker) {
        let start = cursor + relative;
        let after_marker = start + marker.len();
        result.push_str(&line[cursor..after_marker]);
        let rest = &line[after_marker..];
        let whitespace = rest.len() - rest.trim_start_matches([' ', '\t']).len();
        result.push_str(&rest[..whitespace]);
        let value_start = after_marker + whitespace;
        let value = &line[value_start..];
        let end = value
            .char_indices()
            .find(|(_, value)| {
                value.is_whitespace() || matches!(value, '"' | '\'' | ',' | ';' | '}' | ']' | ')')
            })
            .map(|(index, _)| index)
            .unwrap_or(value.len());
        result.push_str("***");
        cursor = value_start + end;
    }
    result.push_str(&line[cursor..]);
    result
}

fn redact_known_tokens(line: &str) -> String {
    const PREFIXES: &[&str] = &[
        "github_pat_",
        "sk-",
        "sk_",
        "ghp_",
        "xoxb-",
        "xoxp-",
        "xoxa-",
        "xoxr-",
        "xoxs-",
        "AIza",
        "AKIA",
        "hf_",
        "gsk_",
    ];
    let mut result = line.to_string();
    for prefix in PREFIXES {
        result = redact_prefixed_token(&result, prefix);
    }
    result
}

fn redact_prefixed_token(line: &str, prefix: &str) -> String {
    let mut result = String::with_capacity(line.len());
    let mut cursor = 0usize;
    while let Some(relative) = find_ascii_case_insensitive(&line[cursor..], prefix) {
        let start = cursor + relative;
        let before = line[..start].chars().next_back();
        if before
            .map(|value| value.is_ascii_alphanumeric() || value == '_')
            .unwrap_or(false)
        {
            let after_prefix = start + prefix.len();
            result.push_str(&line[cursor..after_prefix]);
            cursor = after_prefix;
            continue;
        }
        result.push_str(&line[cursor..start]);
        result.push_str(prefix);
        let after_prefix = start + prefix.len();
        let value = &line[after_prefix..];
        let end = value
            .char_indices()
            .find(|(_, value)| {
                value.is_whitespace() || matches!(value, '"' | '\'' | ',' | ';' | '}' | ']' | ')')
            })
            .map(|(index, _)| index)
            .unwrap_or(value.len());
        result.push_str("***");
        cursor = after_prefix + end;
    }
    result.push_str(&line[cursor..]);
    result
}
