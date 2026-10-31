//! 独立模型目录探测。
//!
//! Issue #1020：模型选择器只查询各家官方机器接口；OpenCode 使用官方
//! `opencode models` 命令行目录。探测不创建 Core 对话、不启动 MCP broker，
//! 也不启动聊天轮次。每次探测拥有自己的子进程和总超时，失败只影响选择器，
//! 不阻塞输入发送。

use std::collections::{BTreeMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::events::{json_text, AgentLaunchContext, AgentModelInfo};
use super::opencode::subcommand_args;
use super::rpc::RpcClient;
use super::workspace_access::WorkspaceAccess;

const PROBE_TIMEOUT: Duration = Duration::from_secs(12);
/// 单路输出最多保留的字节数。读取线程会继续排空管道，避免子进程因
/// stdout/stderr 写满而阻塞，但诊断信息只保留有界前缀。
const CLI_OUTPUT_LIMIT: usize = 1024 * 1024;

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
    let (provider_id, model_id) = id
        .split_once('/')
        .map(|(provider, model)| (provider.to_string(), model.to_string()))
        .unwrap_or_else(|| (String::new(), id.clone()));
    AgentModelInfo {
        id,
        provider_id,
        model_id,
        provider_name: String::new(),
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

/// 按 Agent 自身协议查询模型目录。`refresh` 只影响 OpenCode 的官方 CLI
/// 缓存刷新参数；其它 Agent 仍使用各自的官方 machine API。
pub fn query_models(
    ctx: &AgentLaunchContext,
    refresh: bool,
) -> Result<ModelProbeResult, String> {
    let mut result = match ctx.agent_id.as_str() {
        "codex" => query_codex(ctx),
        "opencode" => query_opencode(ctx, refresh),
        "claude" => query_claude(ctx),
        "gemini" => query_acp(ctx),
        _ if ctx.protocol == "acp" => query_acp(ctx),
        other => Ok(ModelProbeResult::Unsupported {
            message: format!("{other} 没有可用的模型目录接口"),
        }),
    }?;
    // 没有供应商目录的 CLI 用所选执行者作为来源，保证界面始终能显示
    // “来源 · 模型”，又不会伪造不存在的 provider 信息。
    if let ModelProbeResult::Models { models, .. } = &mut result {
        for model in models {
            if model.provider_id.is_empty() {
                model.provider_id = ctx.agent_id.clone();
            }
            if model.model_id.is_empty() {
                model.model_id = model
                    .id
                    .split_once('/')
                    .map(|(_, model)| model.to_string())
                    .unwrap_or_else(|| model.id.clone());
            }
            if model.provider_name.is_empty() {
                model.provider_name = if model.provider_id == ctx.agent_id {
                    ctx.agent_name.clone()
                } else {
                    model.provider_id.clone()
                };
            }
        }
    }
    Ok(result)
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
        "Codex",
        command,
        &ctx.working_dir,
        &[],
        noop_handler(),
    )?;
    client
        .request(
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
        )
        .map_err(|error| format!("Codex 模型目录 initialize 阶段失败：{error}"))?;
    client
        .notify("initialized", json!({}))
        .map_err(|error| format!("Codex 模型目录 initialized 通知失败：{error}"))?;

    let mut models = BTreeMap::new();
    let mut cursor: Option<String> = None;
    let mut seen_cursors = HashSet::new();
    let mut page_count = 0usize;
    for _ in 0..10 {
        let mut params = json!({ "limit": 100, "includeHidden": false });
        if let Some(cursor) = cursor.as_deref() {
            params["cursor"] = Value::String(cursor.to_string());
        }
        let result = client
            .request("model/list", params, remaining(deadline, "model/list")?)
            .map_err(|error| format!("Codex 模型目录 model/list 阶段失败：{error}"))?;
        let Some(data) = result.get("data").and_then(Value::as_array) else {
            return Err(format!(
                "Codex model/list 响应异常：缺少 data 数组（{}）",
                json_preview(&result)
            ));
        };
        page_count += 1;
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
        cursor = json_text(&result, &["nextCursor"]).filter(|cursor| !cursor.is_empty());
        let Some(next_cursor) = cursor.clone() else {
            break;
        };
        if !seen_cursors.insert(next_cursor.clone()) {
            return Err(format!(
                "Codex model/list 分页返回了重复 cursor：{next_cursor}"
            ));
        }
    }
    if cursor.is_some() {
        return Err("Codex model/list 分页超过 10 页仍未结束".to_string());
    }
    let models: Vec<_> = models.into_values().collect();
    if models.is_empty() {
        return Err(if page_count == 0 {
            "Codex model/list 没有返回任何模型".to_string()
        } else {
            "Codex model/list 返回的模型目录为空".to_string()
        });
    }
    let default_model = default_model(&models);
    Ok(ModelProbeResult::Models {
        models,
        default_model,
    })
}

fn json_preview(value: &Value) -> String {
    let text = value.to_string();
    let mut preview: String = text.chars().take(500).collect();
    if preview.len() < text.len() {
        preview.push_str("…");
    }
    preview
}

fn query_opencode(
    ctx: &AgentLaunchContext,
    refresh: bool,
) -> Result<ModelProbeResult, String> {
    let mut models_args = Vec::new();
    if refresh {
        models_args.push("--refresh".to_string());
    }
    // 与 OpenCodeAdapter::start 使用同一启动路径和通用参数；`models` 只是
    // 不同的子命令，用户 manifest 里的包装参数不会被目录探测丢掉。
    let mut command = WorkspaceAccess::for_context(ctx)?
        .command(&ctx.launch_path, &ctx.launch_args)?;
    command
        .args(subcommand_args("models", &models_args))
        .current_dir(&ctx.working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = run_bounded_command("opencode models", command, PROBE_TIMEOUT)?;
    if !output.status.success() {
        return Err(format!(
            "opencode models 退出码 {}：{}",
            exit_status_label(&output.status),
            stderr_summary(&output.stderr)
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut models = BTreeMap::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // 官方 `opencode models` 每行固定为 `provider/model`；只按第一个
        // `/` 拆分，模型名自身包含的 `/` 保留给 model_id。
        let Some((provider_id, model_id)) = line.split_once('/') else {
            continue;
        };
        let provider_id = provider_id.trim();
        let model_id = model_id.trim();
        if provider_id.is_empty() || model_id.is_empty() {
            continue;
        }
        let id = format!("{provider_id}/{model_id}");
        models.entry(id.clone()).or_insert_with(|| AgentModelInfo {
            id,
            provider_id: provider_id.to_string(),
            model_id: model_id.to_string(),
            // CLI 只返回 provider ID。用 ID 作为来源名，避免伪造官方显示名。
            provider_name: provider_id.to_string(),
            display_name: model_id.to_string(),
            description: String::new(),
            is_default: false,
            capabilities: Vec::new(),
        });
    }
    if models.is_empty() {
        return Err(if stdout.trim().is_empty() {
            "opencode models 没有输出任何模型".to_string()
        } else {
            format!(
                "opencode models 输出中没有可识别的 provider/model：{}",
                stdout.trim().chars().take(300).collect::<String>()
            )
        });
    }
    let models: Vec<_> = models.into_values().collect();
    let default_model = default_model(&models);
    Ok(ModelProbeResult::Models {
        models,
        default_model,
    })
}

struct CommandOutput {
    status: ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn exit_status_label(status: &ExitStatus) -> String {
    match status.code() {
        Some(code) => code.to_string(),
        None => "被信号终止".to_string(),
    }
}

fn stderr_summary(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return "无 stderr 输出".to_string();
    }
    let mut summary: String = trimmed.chars().rev().take(1200).collect();
    summary = summary.chars().rev().collect();
    summary
}

fn read_capped<R: Read>(mut reader: R, limit: usize) -> Vec<u8> {
    let mut kept = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                if kept.len() < limit {
                    let remaining = limit - kept.len();
                    kept.extend_from_slice(&buffer[..read.min(remaining)]);
                }
                // 超出上限后继续读取并丢弃，确保子进程不会因为管道写满而卡住。
            }
            Err(_) => break,
        }
    }
    kept
}

fn run_bounded_command(
    label: &str,
    mut command: Command,
    timeout: Duration,
) -> Result<CommandOutput, String> {
    let mut child = command
        .spawn()
        .map_err(|error| format!("无法启动 {label}：{error}"))?;
    let Some(stdout) = child.stdout.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("{label} 没有 stdout 管道"));
    };
    let Some(stderr) = child.stderr.take() else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(format!("{label} 没有 stderr 管道"));
    };
    let stdout_reader = std::thread::spawn(move || read_capped(stdout, CLI_OUTPUT_LIMIT));
    let stderr_reader = std::thread::spawn(move || read_capped(stderr, CLI_OUTPUT_LIMIT));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("等待 {label} 退出失败：{error}"));
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("{label} 超时（{} 秒）", timeout.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    Ok(CommandOutput {
        status,
        stdout,
        stderr,
    })
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
