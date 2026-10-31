//! 独立模型目录探测。
//!
//! Issue #1020：模型选择器只查询各家官方机器接口；OpenCode 使用官方
//! `opencode models` 命令行目录。探测不创建 Core 对话、不启动 MCP broker，
//! 也不启动聊天轮次。每次探测拥有自己的子进程和总超时，失败只影响选择器，
//! 不阻塞输入发送。

use std::collections::{BTreeMap, HashSet};
use std::io::Read;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::events::{json_text, AgentLaunchContext, AgentModelInfo};
use super::rpc::{CloseReason, RpcClient};
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

/// 按 Agent 自身协议查询模型目录。`refresh` 表示用户主动刷新；OpenCode
/// 目前提供的是 `opencode models` 目录命令，每次调用都会重新读取当前可用
/// 供应商，不能附加旧版本不支持的刷新参数。
pub fn query_models(ctx: &AgentLaunchContext, refresh: bool) -> Result<ModelProbeResult, String> {
    let mut result = match ctx.agent_id.as_str() {
        "codex" => query_codex(ctx),
        "opencode" => query_opencode(ctx, refresh),
        other => Ok(ModelProbeResult::Unsupported {
            message: format!("{other} 没有可用的模型目录接口（仅支持 OpenCode 与 Codex）"),
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
    let client = Arc::new(RpcClient::spawn(
        "Codex",
        command,
        &ctx.working_dir,
        &[],
        noop_handler(),
    )?);
    let result = query_codex_session(&client, deadline);
    client.close(CloseReason::ProbeFinished);
    result
}

fn query_codex_session(client: &RpcClient, deadline: Instant) -> Result<ModelProbeResult, String> {
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
            let mut capabilities = vec!["catalog-visible".to_string()];
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
    log::info!(
        "[agent] Codex 模型目录（catalog-visible）：{} 个模型，{} 页；会话可选范围以 thread/start 响应为准",
        models.len(),
        page_count
    );
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

fn query_opencode(ctx: &AgentLaunchContext, _refresh: bool) -> Result<ModelProbeResult, String> {
    // 与 OpenCode ACP 会话使用同一可执行文件、HOME/XDG、OpenCode 配置与
    // 认证环境；`models` 是 CLI 顶层子命令，不能带上会话专用的 `acp` 参数。
    let mut command = WorkspaceAccess::for_context(ctx)?.command(&ctx.launch_path, &[])?;
    command
        .arg("models")
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
            capabilities: vec!["catalog-visible".to_string()],
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
    let providers: std::collections::BTreeSet<&str> = models
        .iter()
        .map(|model| model.provider_id.as_str())
        .collect();
    // 只记录供应商身份和数量，不读取、不输出任何密钥；用于排查沙箱内
    // 已登录供应商是否真的对 `opencode models` 可见。
    log::info!(
        "[agent] opencode models 目录（catalog-visible）可见供应商 {} 个（{}），模型 {} 个；认证可见范围见下一条日志；会话可选范围以 ACP session/new 的 configOptions 为准",
        providers.len(),
        providers.into_iter().collect::<Vec<_>>().join(", "),
        models.len()
    );
    log_opencode_auth(ctx);
    let default_model = default_model(&models);
    Ok(ModelProbeResult::Models {
        models,
        default_model,
    })
}

/// 记录探测子进程实际能看到的 OpenCode v2 认证连接。使用官方
/// `auth list --format json`，只输出供应商 ID、连接数量与来源类型
/// （`credential` / `environment`）；不输出账号名、密钥、令牌、数据库
/// 字段或 JSON 原文。认证可见只表示存在连接记录，不代表密钥当前有效，
/// 也不代表模型一定能在本轮会话中调用。
fn log_opencode_auth(ctx: &AgentLaunchContext) {
    let command =
        WorkspaceAccess::for_context(ctx).and_then(|access| access.command(&ctx.launch_path, &[]));
    let Ok(mut command) = command else {
        return;
    };
    command
        .arg("auth")
        .arg("list")
        .arg("--format")
        .arg("json")
        .current_dir(&ctx.working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match run_bounded_command(
        "opencode auth list --format json",
        command,
        Duration::from_secs(5),
    ) {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let Ok(value) = serde_json::from_str::<Value>(&stdout) else {
                log::warn!(
                    "[agent] opencode auth list --format json 输出无法解析，已忽略（不输出原始内容）"
                );
                return;
            };
            let Some(integrations) = value.as_array() else {
                log::warn!(
                    "[agent] opencode auth list --format json 顶层不是数组，已忽略（不输出原始内容）"
                );
                return;
            };
            let mut provider_count = 0usize;
            let mut connection_count = 0usize;
            let mut source_count: BTreeMap<String, usize> = BTreeMap::new();
            for integration in integrations {
                let Some(provider_id) = integration
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    continue;
                };
                let Some(connections) = integration.get("connections").and_then(Value::as_array)
                else {
                    continue;
                };
                if connections.is_empty() {
                    continue;
                }
                let mut provider_sources: BTreeMap<String, usize> = BTreeMap::new();
                for connection in connections {
                    let source = connection
                        .get("type")
                        .and_then(Value::as_str)
                        .filter(|value| !value.is_empty())
                        .unwrap_or("unknown")
                        .to_string();
                    *provider_sources.entry(source.clone()).or_default() += 1;
                    *source_count.entry(source).or_default() += 1;
                }
                provider_count += 1;
                connection_count += connections.len();
                log::info!(
                    "[agent] opencode auth 可见连接：provider={}，连接数={}，来源={}",
                    provider_id,
                    connections.len(),
                    provider_sources
                        .into_iter()
                        .map(|(source, count)| format!("{source}:{count}"))
                        .collect::<Vec<_>>()
                        .join(",")
                );
            }
            log::info!(
                "[agent] opencode auth 汇总：供应商 {} 个，连接 {} 个，来源 {}；auth-visible 仅表示存在连接记录，不代表密钥当前有效或会话一定可调用",
                provider_count,
                connection_count,
                source_count
                    .into_iter()
                    .map(|(source, count)| format!("{source}:{count}"))
                    .collect::<Vec<_>>()
                    .join(",")
            );
        }
        Ok(output) => log::warn!(
            "[agent] opencode auth list --format json 退出码 {}（stderr 已省略，避免输出敏感内容）",
            exit_status_label(&output.status)
        ),
        Err(error) => log::warn!("[agent] opencode auth list --format json 失败：{error}"),
    }
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
