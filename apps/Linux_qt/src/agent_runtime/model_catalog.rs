//! 独立模型目录探测。
//!
//! Issue #1019：模型选择器只查询各家官方机器接口，不创建 Core 对话、
//! 不启动 MCP broker，也不调用聊天用的 `session/new`。每次探测拥有自己的
//! 子进程和总超时，失败只影响选择器，不阻塞输入发送。

use std::collections::BTreeSet;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::events::{json_text, AgentLaunchContext};
use super::opencode::{fetch_models, free_loopback_port, SERVER_USER};
use super::rpc::RpcClient;
use super::workspace_access::WorkspaceAccess;

const PROBE_TIMEOUT: Duration = Duration::from_secs(12);

fn remaining(deadline: Instant, method: &str) -> Result<Duration, String> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        Err(format!("模型目录探测超时（{method}）"))
    } else {
        Ok(left)
    }
}

fn noop_handler() -> Arc<dyn Fn(&str, Value) + Send + Sync> {
    Arc::new(|_, _| {})
}

/// 按 Agent 自身协议查询模型目录。返回空列表表示该 CLI 没有可枚举的
/// 机器接口（当前是 Claude Code），由平台端显示“默认模型”而不是猜列表。
pub fn query_models(ctx: &AgentLaunchContext) -> Result<Vec<String>, String> {
    match ctx.agent_id.as_str() {
        "codex" => query_codex(ctx),
        "opencode" => query_opencode(ctx),
        // Claude Code 官方只在交互式 `/model` 中维护模型列表，`-p` 模式
        // 没有机器可读目录；不要伪造静态列表。
        "claude" => Ok(Vec::new()),
        "gemini" => query_acp(ctx),
        _ if ctx.protocol == "acp" => query_acp(ctx),
        other => Err(format!("{other} 没有可用的模型目录接口")),
    }
}

fn query_codex(ctx: &AgentLaunchContext) -> Result<Vec<String>, String> {
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let mut args = vec!["app-server".to_string()];
    for arg in &ctx.launch_args {
        if arg != "app-server" {
            args.push(arg.clone());
        }
    }
    let command = WorkspaceAccess::for_context(ctx)?.command(&ctx.launch_path, &args)?;
    let client = RpcClient::spawn(
        "Codex model/list",
        command,
        &ctx.working_dir,
        &[],
        noop_handler(),
    )?;
    client.request(
        "initialize",
        json!({
            "clientInfo": {
                "name": "sujian",
                "title": "素笺",
                "version": env!("CARGO_PKG_VERSION")
            },
            "capabilities": { "experimentalApi": true }
        }),
        remaining(deadline, "initialize")?,
    )?;
    client.notify("initialized", json!({}))?;

    let mut models = BTreeSet::new();
    let mut cursor: Option<String> = None;
    for _ in 0..10 {
        let mut params = json!({ "limit": 100, "includeHidden": false });
        if let Some(cursor) = cursor.as_deref() {
            params["cursor"] = Value::String(cursor.to_string());
        }
        let result = client.request("model/list", params, remaining(deadline, "model/list")?)?;
        if let Some(data) = result.get("data").and_then(Value::as_array) {
            for model in data {
                if let Some(id) = json_text(model, &["id"]).or_else(|| json_text(model, &["model"]))
                {
                    if !id.is_empty() {
                        models.insert(id);
                    }
                }
            }
        }
        cursor = json_text(&result, &["nextCursor"]).filter(|cursor| !cursor.is_empty());
        if cursor.is_none() {
            break;
        }
    }
    Ok(models.into_iter().collect())
}

fn query_opencode(ctx: &AgentLaunchContext) -> Result<Vec<String>, String> {
    let port = free_loopback_port()?;
    let password = format!("sujian-{}", uuid::Uuid::new_v4());
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(4))
        .build()
        .map_err(|error| error.to_string())?;
    let command = WorkspaceAccess::for_context(ctx)?.command(&ctx.launch_path, &ctx.launch_args)?;
    let mut command = command;
    let mut child = command
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
        .map_err(|error| format!("无法启动 opencode serve 探测模型：{error}"))?;
    let base_url = format!("http://127.0.0.1:{port}");

    let result = (|| {
        let deadline = Instant::now() + PROBE_TIMEOUT;
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
            if Instant::now() >= deadline {
                return Err("opencode serve 模型探测启动超时".to_string());
            }
            std::thread::sleep(Duration::from_millis(150));
        }
        fetch_models(&client, &base_url, &password, &ctx.working_dir)
    })();

    let _ = child.kill();
    let _ = child.wait();
    result
}

fn query_acp(ctx: &AgentLaunchContext) -> Result<Vec<String>, String> {
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let mut args = ctx.launch_args.clone();
    if ctx.agent_id == "gemini" && !args.iter().any(|arg| arg == "--acp") {
        args.push("--acp".to_string());
    }
    let command = WorkspaceAccess::for_context(ctx)?.command(&ctx.launch_path, &args)?;
    let client = RpcClient::spawn(
        "ACP model probe",
        command,
        &ctx.working_dir,
        &[],
        noop_handler(),
    )?;
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
        remaining(deadline, "initialize")?,
    )?;
    if let Some(version) = init.get("protocolVersion").and_then(Value::as_i64) {
        if version < 1 {
            return Err(format!("ACP 协议版本不受支持：{version}"));
        }
    }
    // 探测会话没有 MCP、也不落 Core 会话；它只用于读取 session/new 响应里
    // 官方返回的模型目录。
    let session = client.request(
        "session/new",
        json!({
            "cwd": ctx.working_dir.to_string_lossy(),
            "mcpServers": [],
        }),
        remaining(deadline, "session/new")?,
    )?;
    Ok(acp_model_ids(&session))
}

fn acp_model_ids(session: &Value) -> Vec<String> {
    let mut models = BTreeSet::new();
    if let Some(available) = session
        .get("models")
        .and_then(|models| models.get("availableModels"))
        .and_then(Value::as_array)
    {
        for model in available {
            if let Some(id) = json_text(model, &["modelId"]).or_else(|| json_text(model, &["id"])) {
                if !id.is_empty() {
                    models.insert(id);
                }
            }
        }
    }
    // 标准 ACP 也可能通过 configOptions 暴露模型选择项。
    if let Some(options) = session.get("configOptions").and_then(Value::as_array) {
        for option in options {
            let id = json_text(option, &["id"]).unwrap_or_default();
            let category = json_text(option, &["category"]).unwrap_or_default();
            if id != "model" && category != "model" {
                continue;
            }
            if let Some(values) = option.get("options").and_then(Value::as_array) {
                for value in values {
                    if let Some(model_id) = json_text(value, &["value"])
                        .or_else(|| json_text(value, &["id"]))
                        .or_else(|| json_text(value, &["modelId"]))
                    {
                        if !model_id.is_empty() {
                            models.insert(model_id);
                        }
                    }
                }
            }
        }
    }
    models.into_iter().collect()
}
