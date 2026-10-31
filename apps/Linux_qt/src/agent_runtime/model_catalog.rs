//! 独立模型目录探测。
//!
//! Issue #1019：模型选择器只查询各家官方机器接口，不创建 Core 对话、
//! 不启动 MCP broker，也不启动聊天轮次。每次探测拥有自己的子进程和总超时，
//! 失败只影响选择器，不阻塞输入发送。

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::process::Stdio;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::events::{json_text, AgentLaunchContext, AgentModelInfo};
use super::opencode::{fetch_models, free_loopback_port, SERVER_USER};
use super::rpc::RpcClient;
use super::workspace_access::WorkspaceAccess;

const PROBE_TIMEOUT: Duration = Duration::from_secs(12);

/// 模型目录探测结果。`Unsupported` 表示 CLI 正常响应但该版本不提供模型目录，
/// 与网络/进程失败区分开，平台端可显示明确的说明。
#[derive(Debug, Clone)]
pub enum ModelProbeResult {
    Models {
        models: Vec<AgentModelInfo>,
        default_model: Option<String>,
    },
    Unsupported {
        message: String,
    },
}

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

fn model_info(
    id: String,
    display_name: Option<String>,
    description: Option<String>,
    is_default: bool,
    capabilities: Vec<String>,
) -> AgentModelInfo {
    AgentModelInfo {
        id,
        display_name: display_name.unwrap_or_default(),
        description: description.unwrap_or_default(),
        is_default,
        capabilities,
    }
}

fn default_model(models: &[AgentModelInfo]) -> Option<String> {
    models
        .iter()
        .find(|model| model.is_default)
        .map(|model| model.id.clone())
}

/// 按 Agent 自身协议查询模型目录。
pub fn query_models(ctx: &AgentLaunchContext) -> Result<ModelProbeResult, String> {
    match ctx.agent_id.as_str() {
        "codex" => query_codex(ctx),
        "opencode" => query_opencode(ctx),
        "claude" => query_claude(ctx),
        "gemini" => query_acp(ctx),
        _ if ctx.protocol == "acp" => query_acp(ctx),
        other => Ok(ModelProbeResult::Unsupported {
            message: format!("{other} 没有可用的模型目录接口"),
        }),
    }
}

fn query_codex(ctx: &AgentLaunchContext) -> Result<ModelProbeResult, String> {
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

    let mut models = BTreeMap::new();
    let mut cursor: Option<String> = None;
    for _ in 0..10 {
        let mut params = json!({ "limit": 100, "includeHidden": false });
        if let Some(cursor) = cursor.as_deref() {
            params["cursor"] = Value::String(cursor.to_string());
        }
        let result = client.request("model/list", params, remaining(deadline, "model/list")?)?;
        if let Some(data) = result.get("data").and_then(Value::as_array) {
            for model in data {
                let Some(id) = json_text(model, &["id"]).or_else(|| json_text(model, &["model"]))
                else {
                    continue;
                };
                if id.is_empty() {
                    continue;
                }
                let mut capabilities = Vec::new();
                if let Some(efforts) = model
                    .get("supportedReasoningEfforts")
                    .and_then(Value::as_array)
                {
                    for effort in efforts {
                        if let Some(effort) = effort.as_str() {
                            capabilities.push(format!("reasoning:{effort}"));
                        }
                    }
                }
                if let Some(modalities) = model.get("inputModalities").and_then(Value::as_array) {
                    for modality in modalities {
                        if let Some(modality) = modality.as_str() {
                            capabilities.push(format!("input:{modality}"));
                        }
                    }
                }
                models.insert(
                    id.clone(),
                    model_info(
                        id,
                        json_text(model, &["displayName"]),
                        json_text(model, &["description"]),
                        model.get("isDefault").and_then(Value::as_bool) == Some(true),
                        capabilities,
                    ),
                );
            }
        }
        cursor = json_text(&result, &["nextCursor"]).filter(|cursor| !cursor.is_empty());
        if cursor.is_none() {
            break;
        }
    }
    let models: Vec<_> = models.into_values().collect();
    let default_model = default_model(&models);
    Ok(ModelProbeResult::Models {
        models,
        default_model,
    })
}

fn query_opencode(ctx: &AgentLaunchContext) -> Result<ModelProbeResult, String> {
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
        let models = fetch_models(&client, &base_url, &password, &ctx.working_dir)?;
        let default_model = default_model(&models);
        Ok(ModelProbeResult::Models {
            models,
            default_model,
        })
    })();

    let _ = child.kill();
    let _ = child.wait();
    result
}

fn query_claude(ctx: &AgentLaunchContext) -> Result<ModelProbeResult, String> {
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let mut args = Vec::new();
    let mut skip_value = false;
    for arg in &ctx.launch_args {
        if skip_value {
            skip_value = false;
            continue;
        }
        match arg.as_str() {
            "--print" | "-p" => continue,
            "--input-format" | "--output-format" => {
                skip_value = true;
                continue;
            }
            _ => args.push(arg.clone()),
        }
    }
    args.extend(
        [
            "--print",
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--safe-mode",
            "--no-session-persistence",
        ]
        .iter()
        .map(|arg| arg.to_string()),
    );
    let mut command = WorkspaceAccess::for_context(ctx)?.command(&ctx.launch_path, &args)?;
    command
        .current_dir(&ctx.working_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|error| format!("无法启动 Claude Code 模型探测：{error}"))?;

    let result = (|| {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Claude Code 模型探测没有 stdin".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Claude Code 模型探测没有 stdout".to_string())?;
        let request_id = format!("sujian-models-{}", uuid::Uuid::new_v4());
        let request = json!({
            "type": "control_request",
            "request_id": request_id,
            "request": { "subtype": "initialize" }
        });
        writeln!(stdin, "{request}")
            .and_then(|_| stdin.flush())
            .map_err(|error| format!("写入 Claude Code initialize 失败：{error}"))?;

        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let Ok(line) = line else {
                    break;
                };
                if line.trim().is_empty() {
                    continue;
                }
                let Ok(value) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if sender.send(value).is_err() {
                    break;
                }
            }
        });

        loop {
            let left = remaining(deadline, "initialize")?;
            let value = receiver
                .recv_timeout(left)
                .map_err(|_| "Claude Code 模型探测等待 initialize 响应超时".to_string())?;
            if value.get("type").and_then(Value::as_str) != Some("control_response") {
                continue;
            }
            let Some(response) = value.get("response") else {
                continue;
            };
            let response_id = response
                .get("request_id")
                .and_then(Value::as_str)
                .or_else(|| value.get("request_id").and_then(Value::as_str));
            if response_id != Some(request_id.as_str()) {
                continue;
            }
            match response.get("subtype").and_then(Value::as_str) {
                Some("success") => {
                    let payload = response.get("response").unwrap_or(response);
                    let Some(items) = payload.get("models").and_then(Value::as_array) else {
                        return Ok(ModelProbeResult::Unsupported {
                            message: "当前 Claude Code 版本未在初始化响应中提供模型目录"
                                .to_string(),
                        });
                    };
                    let mut models = Vec::new();
                    for model in items {
                        let Some(id) = json_text(model, &["value"])
                            .or_else(|| json_text(model, &["resolvedModel"]))
                            .or_else(|| json_text(model, &["id"]))
                        else {
                            continue;
                        };
                        if id.is_empty() {
                            continue;
                        }
                        let mut capabilities = Vec::new();
                        for key in [
                            "supportsEffort",
                            "supportsAdaptiveThinking",
                            "supportsFastMode",
                            "supportsAutoMode",
                        ] {
                            if model.get(key).and_then(Value::as_bool) == Some(true) {
                                capabilities.push(key.to_string());
                            }
                        }
                        if let Some(levels) =
                            model.get("supportedEffortLevels").and_then(Value::as_array)
                        {
                            for level in levels {
                                if let Some(level) = level.as_str() {
                                    capabilities.push(format!("effort:{level}"));
                                }
                            }
                        }
                        models.push(model_info(
                            id,
                            json_text(model, &["displayName"]),
                            json_text(model, &["description"]),
                            false,
                            capabilities,
                        ));
                    }
                    if models.is_empty() {
                        return Ok(ModelProbeResult::Unsupported {
                            message: "Claude Code 初始化响应中的模型目录为空".to_string(),
                        });
                    }
                    let default_model = default_model(&models);
                    return Ok(ModelProbeResult::Models {
                        models,
                        default_model,
                    });
                }
                Some("error") => {
                    let error = response
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("未知错误");
                    return Err(format!("Claude Code 初始化模型目录失败：{error}"));
                }
                Some(other) => {
                    return Err(format!("Claude Code 初始化响应类型异常：{other}"));
                }
                None => return Err("Claude Code 初始化响应缺少 subtype".to_string()),
            }
        }
    })();

    let _ = child.kill();
    let _ = child.wait();
    result
}

fn query_acp(ctx: &AgentLaunchContext) -> Result<ModelProbeResult, String> {
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
    let models = acp_models(&session);
    if models.is_empty() {
        return Ok(ModelProbeResult::Unsupported {
            message: "当前 ACP Agent 未在会话初始化响应中提供模型目录".to_string(),
        });
    }
    let default_model = json_text(
        session.get("models").unwrap_or(&Value::Null),
        &["currentModelId"],
    )
    .filter(|id| models.iter().any(|model| &model.id == id))
    .or_else(|| default_model(&models));
    Ok(ModelProbeResult::Models {
        models,
        default_model,
    })
}

fn acp_models(session: &Value) -> Vec<AgentModelInfo> {
    let mut models = BTreeMap::new();
    if let Some(available) = session
        .get("models")
        .and_then(|models| models.get("availableModels"))
        .and_then(Value::as_array)
    {
        for model in available {
            let Some(id) = json_text(model, &["modelId"]).or_else(|| json_text(model, &["id"]))
            else {
                continue;
            };
            if id.is_empty() {
                continue;
            }
            let mut capabilities = Vec::new();
            for key in [
                "supportsEffort",
                "supportsAdaptiveThinking",
                "supportsFastMode",
                "supportsAutoMode",
            ] {
                if model.get(key).and_then(Value::as_bool) == Some(true) {
                    capabilities.push(key.to_string());
                }
            }
            models.insert(
                id.clone(),
                model_info(
                    id,
                    json_text(model, &["name"]).or_else(|| json_text(model, &["displayName"])),
                    json_text(model, &["description"]),
                    false,
                    capabilities,
                ),
            );
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
                    let Some(model_id) = json_text(value, &["value"])
                        .or_else(|| json_text(value, &["id"]))
                        .or_else(|| json_text(value, &["modelId"]))
                    else {
                        continue;
                    };
                    if model_id.is_empty() || models.contains_key(&model_id) {
                        continue;
                    }
                    models.insert(
                        model_id.clone(),
                        model_info(
                            model_id,
                            json_text(value, &["name"])
                                .or_else(|| json_text(value, &["displayName"])),
                            json_text(value, &["description"]),
                            false,
                            Vec::new(),
                        ),
                    );
                }
            }
        }
    }
    models.into_values().collect()
}
