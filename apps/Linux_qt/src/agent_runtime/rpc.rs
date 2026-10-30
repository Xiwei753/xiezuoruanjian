//! 极小的 stdio JSON-RPC 客户端，供 ACP 与 Codex app-server 共用。
//!
//! 只实现协议真正需要的能力：按行读写 JSON、请求/响应关联、通知回调、
//! 进程生命周期。不把任意 stdout 当 JSON 解析；非 JSON 行记录到日志。

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

type NotificationHandler = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;

struct PendingRequest {
    response: Sender<Result<serde_json::Value, String>>,
}

pub struct RpcClient {
    stdin: Mutex<ChildStdin>,
    pending: Arc<Mutex<HashMap<i64, PendingRequest>>>,
    next_id: AtomicI64,
    child: Arc<Mutex<Child>>,
    notification_handler: NotificationHandler,
}

impl RpcClient {
    pub fn spawn(
        program: &str,
        args: &[String],
        working_dir: &std::path::Path,
        env: &[(String, String)],
        notification_handler: NotificationHandler,
    ) -> Result<Self, String> {
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(working_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in env {
            command.env(key, value);
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("无法启动 {program}：{error}"))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| format!("{program} 没有 stdin"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| format!("{program} 没有 stdout"))?;
        let stderr = child.stderr.take();

        let pending: Arc<Mutex<HashMap<i64, PendingRequest>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let reader_pending = pending.clone();
        let reader_handler = notification_handler.clone();
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
                        continue;
                    }
                    let method = value
                        .get("method")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string);
                    if let Some(method) = method {
                        // 传递完整消息：ACP 的 `session/request_permission` 是带
                        // id 的服务器→客户端请求，需要保留 id 才能回复。
                        (reader_handler)(&method, value);
                    }
                }
                // 进程退出：唤醒所有等待者，避免 UI 线程永久挂起。
                if let Ok(mut map) = reader_pending.lock() {
                    for (_, pending) in map.drain() {
                        let _ = pending.response.send(Err("Agent 进程已退出".to_string()));
                    }
                }
            })
            .map_err(|error| format!("无法启动 Agent 读取线程：{error}"))?;

        if let Some(stderr) = stderr {
            std::thread::Builder::new()
                .name("agent-stderr-reader".to_string())
                .spawn(move || {
                    let reader = BufReader::new(stderr);
                    for line in reader.lines().map_while(Result::ok) {
                        log::debug!(target: "agent", "agent stderr: {line}");
                    }
                })
                .ok();
        }

        Ok(Self {
            stdin: Mutex::new(stdin),
            pending,
            next_id: AtomicI64::new(1),
            child: Arc::new(Mutex::new(child)),
            notification_handler,
        })
    }

    pub fn request(
        &self,
        method: &str,
        params: serde_json::Value,
        timeout: Duration,
    ) -> Result<serde_json::Value, String> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let (sender, receiver): (Sender<Result<serde_json::Value, String>>, Receiver<_>) =
            mpsc::channel();
        self.pending
            .lock()
            .map_err(|_| "JSON-RPC 状态锁损坏".to_string())?
            .insert(id, PendingRequest { response: sender });
        self.write_json(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))?;
        receiver
            .recv_timeout(timeout)
            .map_err(|_| format!("{method} 超时"))?
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
