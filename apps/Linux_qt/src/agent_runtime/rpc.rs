//! 极小的 stdio JSON-RPC 客户端，供 ACP 与 Codex app-server 共用。
//!
//! 只实现协议真正需要的能力：按行读写 JSON、请求/响应关联、通知回调、
//! 进程生命周期。不把任意 stdout 当 JSON 解析；非 JSON 行记录到日志。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type NotificationHandler = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;

/// 只保留 stderr 尾部，既能让失败原因进入用户可见错误，又避免把整段
/// 日志常驻内存。
const STDERR_TAIL_LIMIT: usize = 16 * 1024;
const EXIT_WAIT_TIMEOUT: Duration = Duration::from_secs(2);

struct PendingRequest {
    method: String,
    response: Sender<Result<serde_json::Value, String>>,
}

pub struct RpcClient {
    stdin: Mutex<ChildStdin>,
    pending: Arc<Mutex<HashMap<i64, PendingRequest>>>,
    next_id: AtomicI64,
    child: Arc<Mutex<Child>>,
    notification_handler: NotificationHandler,
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
        command
            .current_dir(working_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in env {
            command.env(key, value);
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("无法启动 {label}：{error}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| format!("{label} 没有 stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| format!("{label} 没有 stdout"))?;
        let stderr = child.stderr.take();
        let child = Arc::new(Mutex::new(child));

        let pending: Arc<Mutex<HashMap<i64, PendingRequest>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let exit_reason: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
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
        let reader_child = child.clone();
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
                // stdout 已关闭：读取真实退出状态和（已脱敏的）stderr 尾部，
                // 再唤醒所有等待者。这样 initialize/model/list 失败时用户
                // 看到的是 CLI 的真实原因，而不是笼统的“进程已退出”。
                let status = wait_for_exit(&reader_child);
                if status.is_some() {
                    if let Some(handle) = stderr_handle {
                        wait_for_stderr_reader(handle);
                    }
                }
                let tail = stderr_tail
                    .lock()
                    .map(|tail| tail.clone())
                    .unwrap_or_default();
                let reason = describe_exit(&reader_label, status, &tail);
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
            stdin: Mutex::new(stdin),
            pending,
            next_id: AtomicI64::new(1),
            child,
            notification_handler,
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
        if let Err(error) = self.write_json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        })) {
            if let Ok(mut map) = self.pending.lock() {
                map.remove(&id);
            }
            if let Some(reason) = self.exited_reason() {
                return Err(format!("{method} 阶段失败：{reason}"));
            }
            return Err(error);
        }
        on_accepted();
        match receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                if let Ok(mut map) = self.pending.lock() {
                    map.remove(&id);
                }
                if let Some(reason) = self.exited_reason() {
                    return Err(format!("{method} 阶段失败：{reason}"));
                }
                Err(format!("{method} 超时"))
            }
            Err(RecvTimeoutError::Disconnected) => {
                if let Ok(mut map) = self.pending.lock() {
                    map.remove(&id);
                }
                if let Some(reason) = self.exited_reason() {
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
        let mut stdin = self
            .stdin
            .lock()
            .map_err(|_| "Agent stdin 锁损坏".to_string())?;
        serde_json::to_writer(&mut *stdin, value).map_err(|error| error.to_string())?;
        stdin
            .write_all(b"\n")
            .and_then(|()| stdin.flush())
            .map_err(|error| format!("写入 Agent 失败：{error}"))
    }

    pub fn kill(&self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    pub fn notification_handler(&self) -> NotificationHandler {
        self.notification_handler.clone()
    }
}

impl Drop for RpcClient {
    fn drop(&mut self) {
        self.kill();
    }
}

fn wait_for_exit(child: &Arc<Mutex<Child>>) -> Option<ExitStatus> {
    let deadline = Instant::now() + EXIT_WAIT_TIMEOUT;
    loop {
        match child.lock() {
            Ok(mut child) => match child.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) => {}
                Err(_) => return None,
            },
            Err(_) => return None,
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    // stdout 已关闭但进程仍未退出，协议通道已经不可用；回收子进程，
    // 避免残留 app-server 占着会话资源。
    if let Ok(mut child) = child.lock() {
        let _ = child.kill();
        return child.wait().ok();
    }
    None
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

fn describe_exit(label: &str, status: Option<ExitStatus>, tail: &str) -> String {
    let status_text = match status {
        Some(status) => match status.code() {
            Some(code) => format!("退出码 {code}"),
            None => format!("被信号终止（{status}）"),
        },
        None => "退出状态未知".to_string(),
    };
    let mut message = format!(
        "{label} 进程退出（{status_text}）：{}",
        classify_exit(tail, status)
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

fn classify_exit(tail: &str, status: Option<ExitStatus>) -> &'static str {
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
    } else if status.is_some_and(|status| status.code().is_none()) {
        "CLI 被信号终止"
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
                .find(|(_, value)| {
                    value.is_whitespace() || matches!(value, ',' | ';' | '}' | ']')
                })
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
                value.is_whitespace()
                    || matches!(value, '"' | '\'' | ',' | ';' | '}' | ']' | ')')
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
                value.is_whitespace()
                    || matches!(value, '"' | '\'' | ',' | ';' | '}' | ']' | ')')
            })
            .map(|(index, _)| index)
            .unwrap_or(value.len());
        result.push_str("***");
        cursor = after_prefix + end;
    }
    result.push_str(&line[cursor..]);
    result
}
