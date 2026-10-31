//! ACP (Agent Client Protocol) stdio 适配器。
//!
//! 只按握手结果调用真实存在的能力：`initialize` 协商版本与认证方式，
//! `session/new` / `session/load` 建立或恢复会话，`session/prompt` 发送请求，
//! `session/update` 通知转成统一事件。MCP 作为 stdio server 配置随会话传递。

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::adapter::{AgentAdapter, CancelOutcome};
use super::events::{
    json_text, json_value_text, AgentEvent, AgentLaunchContext, AgentModelInfo, AgentPromptRequest,
    EventSink, PermissionOption, RuntimeEvent,
};
use super::rpc::{CloseReason, RpcClient};
use super::workspace_access::WorkspaceAccess;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(300);
/// `session/set_config_option` 响应缺少 currentValue 时，等待
/// `config_option_update` 确认实际模型的时间。不能用请求值假装确认成功。
const MODEL_UPDATE_TIMEOUT: Duration = Duration::from_secs(5);

/// ACP v1: a session permits only one foreground prompt. All cancellation and
/// permission state is protected by one lock so cancelling and permissions cannot race.
#[derive(Default)]
struct AcpTurnState {
    active_turn: String,
    queued_turns: HashSet<String>,
    skipped_turns: HashSet<String>,
    prompt_written: bool,
    cancel_requested: bool,
    cancel_notified: bool,
    pending_permissions: HashMap<String, i64>,
}

/// 当前 ACP 会话真实声明的模型配置项。`allowed` 只来自 `session/new`、
/// `session/load` 或 `config_option_update`，不能用独立的 `opencode models`
/// 目录推断某个模型一定能在本会话请求。
#[derive(Debug, Clone, PartialEq, Eq)]
struct AcpModelConfig {
    config_id: String,
    current: Option<String>,
    allowed: HashSet<String>,
}

impl AcpModelConfig {
    fn from_options(value: &Value) -> Option<Self> {
        let option = model_option(value)?;
        let config_id = option.get("id").and_then(Value::as_str)?.to_string();
        let allowed = option
            .get("options")
            .or_else(|| option.get("values"))
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(option_value_id)
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default();
        Some(Self {
            config_id,
            current: option_current_value(option),
            allowed,
        })
    }
}

pub struct AcpAdapter {
    client: Arc<RpcClient>,
    session_id: Arc<Mutex<Option<String>>>,
    emit: EventSink,
    state: Arc<Mutex<AcpTurnState>>,
    agent_id: String,
    /// 下一轮要使用的模型。模型切换不重建会话，只在 prompt 前同步给运行时。
    model: Option<String>,
    applied_model: Arc<Mutex<Option<String>>>,
    /// 原生会话声明的模型配置项与可选值；仅由会话协议更新。
    model_config: Arc<Mutex<Option<AcpModelConfig>>>,
    /// 每次收到 config_option_update 的 currentValue 时递增，供切换请求
    /// 在响应缺少 currentValue 时等待真实确认。
    model_update_generation: Arc<AtomicU64>,
    history_restored: bool,
    /// Only one session/prompt at a time. This lock belongs to prompt workers,
    /// never to the AgentSessionManager lock, so cancel can run immediately.
    dispatch_lock: Arc<Mutex<()>>,
}

impl AcpAdapter {
    pub fn start(ctx: &AgentLaunchContext, emit: EventSink) -> Result<Self, String> {
        let session_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let state: Arc<Mutex<AcpTurnState>> = Arc::new(Mutex::new(AcpTurnState::default()));
        let applied_model: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let model_config: Arc<Mutex<Option<AcpModelConfig>>> = Arc::new(Mutex::new(None));
        let model_update_generation: Arc<AtomicU64> = Arc::new(AtomicU64::new(0));
        // A Weak client avoids a cycle: RpcClient owns the reader handler.
        let handler_client: Arc<Mutex<Option<Weak<RpcClient>>>> = Arc::new(Mutex::new(None));
        let handler_emit = emit.clone();
        let handler_session = session_id.clone();
        let handler_state = state.clone();
        let handler_rpc = handler_client.clone();
        let handler_applied_model = applied_model.clone();
        let handler_model_config = model_config.clone();
        let handler_model_generation = model_update_generation.clone();
        let handler_provider_id = ctx.agent_id.clone();
        let handler_provider_name = ctx.agent_name.clone();
        let handler: Arc<dyn Fn(&str, Value) + Send + Sync> =
            Arc::new(move |method: &str, message: Value| {
                let params = message.get("params").cloned().unwrap_or_default();
                let core_turn_id = handler_state
                    .lock()
                    .map(|guard| guard.active_turn.clone())
                    .unwrap_or_default();
                match method {
                    "session/update" => {
                        let Some(update) = params.get("update") else {
                            return;
                        };
                        let update_kind = update
                            .get("sessionUpdate")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        match update_kind {
                            "agent_message_chunk" => {
                                if let Some(text) = update
                                    .get("content")
                                    .and_then(|content| json_text(content, &["text"]))
                                {
                                    (handler_emit)(RuntimeEvent::turn(
                                        core_turn_id.clone(),
                                        "",
                                        AgentEvent::TextDelta { text },
                                    ));
                                }
                            }
                            "agent_thought_chunk" => {
                                if let Some(text) = update
                                    .get("content")
                                    .and_then(|content| json_text(content, &["text"]))
                                {
                                    (handler_emit)(RuntimeEvent::turn(
                                        core_turn_id.clone(),
                                        "",
                                        AgentEvent::ReasoningDelta { text },
                                    ));
                                }
                            }
                            "tool_call" => {
                                let tool_call_id = json_text(update, &["toolCallId"])
                                    .unwrap_or_else(|| "tool".to_string());
                                let name = json_text(update, &["title"])
                                    .or_else(|| json_text(update, &["kind"]))
                                    .unwrap_or_else(|| "tool".to_string());
                                (handler_emit)(RuntimeEvent::turn(
                                    core_turn_id.clone(),
                                    "",
                                    AgentEvent::ToolStarted {
                                        tool_call_id,
                                        name,
                                        input_json: json_value_text(update.get("rawInput")),
                                    },
                                ));
                            }
                            "tool_call_update" => {
                                let tool_call_id = json_text(update, &["toolCallId"])
                                    .unwrap_or_else(|| "tool".to_string());
                                let name = json_text(update, &["title"])
                                    .unwrap_or_else(|| "tool".to_string());
                                let status = json_text(update, &["status"]).unwrap_or_default();
                                if matches!(status.as_str(), "completed" | "failed") {
                                    (handler_emit)(RuntimeEvent::turn(
                                        core_turn_id.clone(),
                                        "",
                                        AgentEvent::ToolFinished {
                                            tool_call_id,
                                            name,
                                            output_json: json_value_text(update.get("rawOutput"))
                                                .or_else(|| json_value_text(update.get("content"))),
                                            success: status == "completed",
                                        },
                                    ));
                                }
                            }
                            "config_option_update" => {
                                let options = update
                                    .get("configOptions")
                                    .cloned()
                                    .or_else(|| params.get("configOptions").cloned())
                                    .or_else(|| update.get("configOption").cloned())
                                    .unwrap_or_else(|| update.clone());
                                publish_model_options(
                                    &options,
                                    &handler_provider_id,
                                    &handler_provider_name,
                                    &handler_emit,
                                    &handler_applied_model,
                                    &handler_model_config,
                                    &handler_model_generation,
                                );
                            }
                            _ => {}
                        }
                    }
                    "config_option_update" => {
                        let options = params
                            .get("configOptions")
                            .cloned()
                            .or_else(|| params.get("configOption").cloned())
                            .unwrap_or_else(|| params.clone());
                        publish_model_options(
                            &options,
                            &handler_provider_id,
                            &handler_provider_name,
                            &handler_emit,
                            &handler_applied_model,
                            &handler_model_config,
                            &handler_model_generation,
                        );
                    }
                    "session/request_permission" => {
                        let Some(request_id) = message.get("id").and_then(Value::as_i64) else {
                            return;
                        };
                        let tool = params
                            .get("toolCall")
                            .and_then(|tool| json_text(tool, &["title"]))
                            .unwrap_or_else(|| "工具调用".to_string());
                        let options: Vec<PermissionOption> = params
                            .get("options")
                            .and_then(Value::as_array)
                            .map(|options| {
                                options
                                    .iter()
                                    .filter_map(|option| {
                                        Some(PermissionOption {
                                            option_id: json_text(option, &["optionId"])?,
                                            name: json_text(option, &["name"])
                                                .unwrap_or_else(|| "允许".to_string()),
                                            kind: json_text(option, &["kind"]).unwrap_or_default(),
                                        })
                                    })
                                    .collect()
                            })
                            .unwrap_or_default();
                        let permission_key = request_id.to_string();
                        // Register the permission under the same mutex as cancel_requested.
                        // A request arriving *after* cancel is answered immediately.
                        let reject = match handler_state.lock() {
                            Ok(mut guard) => {
                                if guard.active_turn.is_empty() || guard.cancel_requested {
                                    true
                                } else {
                                    guard
                                        .pending_permissions
                                        .insert(permission_key.clone(), request_id);
                                    false
                                }
                            }
                            Err(_) => true,
                        };
                        if reject {
                            let client = handler_rpc
                                .lock()
                                .ok()
                                .and_then(|guard| guard.as_ref().and_then(Weak::upgrade));
                            if let Some(client) = client {
                                if let Err(error) = client.respond(
                                    request_id,
                                    json!({ "outcome": { "outcome": "cancelled" } }),
                                ) {
                                    log::warn!("[agent] ACP 自动拒绝权限失败: {error}");
                                }
                            }
                            return;
                        }
                        (handler_emit)(RuntimeEvent::turn(
                            core_turn_id.clone(),
                            "",
                            AgentEvent::PermissionRequested {
                                request_id: permission_key,
                                description: format!("Agent 请求执行：{tool}"),
                                options,
                            },
                        ));
                    }
                    _ => {}
                }
                // 会话 ID 来自 session/new 或 session/load 的响应；通知里也带，
                // 作为恢复时的补充事实。
                if let Some(id) = params.get("sessionId").and_then(Value::as_str) {
                    if let Ok(mut guard) = handler_session.lock() {
                        if guard.is_none() {
                            *guard = Some(id.to_string());
                        }
                    }
                }
            });

        let args = ctx.launch_args.clone();
        let command = WorkspaceAccess::for_context(ctx)?.command(&ctx.launch_path, &args)?;
        let client = Arc::new(RpcClient::spawn(
            "OpenCode ACP",
            command,
            &ctx.working_dir,
            &[],
            handler,
        )?);
        if let Ok(mut guard) = handler_client.lock() {
            *guard = Some(Arc::downgrade(&client));
        }

        let start_client = client.clone();
        let started = (|| -> Result<Self, String> {
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
                Duration::from_secs(30),
            )?;
            if let Some(version) = init.get("protocolVersion").and_then(Value::as_i64) {
                if version < 1 {
                    return Err(format!("ACP 协议版本不受支持：{version}"));
                }
            }

            let mcp_servers: Vec<Value> = ctx
                .mcp_servers
                .iter()
                .map(|server| {
                    json!({
                        "name": server.name,
                        "command": server.command,
                        "args": server.args,
                        "env": server.env.iter().map(|(name, value)| json!({
                            "name": name,
                            "value": value,
                        })).collect::<Vec<_>>()
                    })
                })
                .collect();
            let session_params = json!({
                "cwd": ctx.working_dir.to_string_lossy(),
                "mcpServers": mcp_servers,
            });
            let resume_id = ctx
                .resume_native_session_id
                .clone()
                .filter(|id| !id.is_empty());
            let (session_result, resumed) = match resume_id {
                Some(id) => {
                    let mut params = session_params.clone();
                    params["sessionId"] = Value::String(id);
                    match client.request("session/load", params, Duration::from_secs(60)) {
                        Ok(result) => (result, true),
                        Err(error) => {
                            log::warn!("[agent] ACP session/load failed, creating new: {error}");
                            (
                                client.request(
                                    "session/new",
                                    session_params,
                                    Duration::from_secs(60),
                                )?,
                                false,
                            )
                        }
                    }
                }
                None => (
                    client.request("session/new", session_params, Duration::from_secs(60))?,
                    false,
                ),
            };
            let native_session_id = json_text(&session_result, &["sessionId"])
                .ok_or_else(|| "OpenCode ACP 未返回 sessionId".to_string())?;
            // currentModelId / configOptions.model.currentValue 是原生会话实际模型；
            // ctx.model 是用户本轮的请求模型。两者不能合并，否则启动默认值会把
            // 用户选择覆盖掉，后续也不再切换。
            let (models, actual_model) =
                models_from_session(&session_result, &ctx.agent_id, &ctx.agent_name);
            let session_model_config = AcpModelConfig::from_options(&session_result);
            if let Ok(mut guard) = session_id.lock() {
                *guard = Some(native_session_id.clone());
            }
            if let Ok(mut guard) = applied_model.lock() {
                *guard = actual_model.clone();
            }
            if let Ok(mut guard) = model_config.lock() {
                *guard = session_model_config.clone();
            }
            let model_config_detail = session_model_config
                .as_ref()
                .map(|config| {
                    format!(
                        "configId={} 当前值={} 可选值={}",
                        config.config_id,
                        config.current.as_deref().unwrap_or("<未声明>"),
                        config.allowed.len()
                    )
                })
                .unwrap_or_else(|| "未声明模型配置项".to_string());
            log::info!(
                "[agent] ACP 会话建立：实例 {}，原生会话 {}，{}，会话层可选模型 {} 个（层：session-selectable）",
                client.instance_id(),
                redact_session_id(&native_session_id),
                model_config_detail,
                models.len()
            );
            if !models.is_empty() {
                (emit)(RuntimeEvent::session(AgentEvent::ModelsAvailable {
                    models: models.clone(),
                }));
            }
            (emit)(RuntimeEvent::session(AgentEvent::SessionStarted {
                native_session_id,
                model: actual_model.clone(),
                resumed,
            }));
            if resumed {
                log::info!("[agent] OpenCode ACP session resumed");
            }

            Ok(Self {
                client,
                session_id,
                emit,
                state,
                agent_id: ctx.agent_id.clone(),
                model: ctx.model.clone(),
                applied_model,
                model_config,
                model_update_generation,
                history_restored: resumed,
                dispatch_lock: Arc::new(Mutex::new(())),
            })
        })();
        match started {
            Ok(adapter) => Ok(adapter),
            Err(error) => {
                start_client.close(CloseReason::ChannelError(error.clone()));
                Err(error)
            }
        }
    }
}

fn model_option(value: &Value) -> Option<&Value> {
    fn is_model_option(option: &&Value) -> bool {
        let id = option.get("id").and_then(Value::as_str).unwrap_or_default();
        let category = option
            .get("category")
            .and_then(Value::as_str)
            .unwrap_or_default();
        id.eq_ignore_ascii_case("model")
            || id.eq_ignore_ascii_case("models")
            || category.eq_ignore_ascii_case("model")
    }
    if let Some(options) = value.as_array() {
        return options.iter().find(is_model_option);
    }
    if let Some(options) = value.get("configOptions").and_then(Value::as_array) {
        return options.iter().find(is_model_option);
    }
    if let Some(option) = value.get("configOption") {
        if is_model_option(&option) {
            return Some(option);
        }
    }
    let id = value.get("id").and_then(Value::as_str).unwrap_or_default();
    let category = value
        .get("category")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if id.eq_ignore_ascii_case("model")
        || id.eq_ignore_ascii_case("models")
        || category.eq_ignore_ascii_case("model")
    {
        return Some(value);
    }
    None
}

fn redact_session_id(id: &str) -> String {
    let chars: Vec<char> = id.chars().collect();
    if chars.len() <= 8 {
        return format!("<已脱敏 {} 字符>", chars.len());
    }
    let head: String = chars.iter().take(4).collect();
    let tail: String = chars.iter().skip(chars.len().saturating_sub(4)).collect();
    format!("{head}…{tail}")
}

fn option_value_id(value: &Value) -> Option<String> {
    if let Some(value) = value.as_str() {
        return (!value.is_empty()).then(|| value.to_string());
    }
    for key in ["value", "id", "modelId"] {
        if let Some(value) = value.get(key).and_then(Value::as_str) {
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn option_current_value(option: &Value) -> Option<String> {
    option
        .get("currentValue")
        .and_then(option_value_id)
        .or_else(|| option.get("current").and_then(option_value_id))
}

fn split_model_id(value: &str) -> (String, String) {
    match value.find('/') {
        Some(slash) if slash > 0 && slash < value.len() - 1 => {
            (value[..slash].to_string(), value[slash + 1..].to_string())
        }
        _ => (String::new(), value.to_string()),
    }
}

fn models_from_option(
    option: &Value,
    fallback_provider_id: &str,
    fallback_provider_name: &str,
    current: Option<&str>,
) -> Vec<AgentModelInfo> {
    let entries = option
        .get("options")
        .or_else(|| option.get("values"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    entries
        .iter()
        .filter_map(|entry| {
            let id = option_value_id(entry)?;
            let (provider_id, model_id) = split_model_id(&id);
            let provider_id = if provider_id.is_empty() {
                fallback_provider_id.to_string()
            } else {
                provider_id
            };
            let label = entry
                .get("name")
                .or_else(|| entry.get("displayName"))
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
                .map(|name| {
                    // OpenCode 的显示名常带 `provider/` 前缀；界面已经单独
                    // 展示供应商，这里去掉前缀避免“供应商 · 供应商/模型”。
                    name.strip_prefix(&format!("{provider_id}/"))
                        .unwrap_or(name)
                        .to_string()
                })
                .unwrap_or_else(|| model_id.clone());
            Some(AgentModelInfo {
                provider_id: provider_id.clone(),
                model_id: model_id.clone(),
                provider_name: if provider_id.is_empty() {
                    fallback_provider_name.to_string()
                } else {
                    provider_id
                },
                id: id.clone(),
                display_name: label,
                description: entry
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                is_default: current == Some(id.as_str()),
                capabilities: vec!["session-selectable".to_string()],
            })
        })
        .collect()
}

fn legacy_models_from_session(
    session: &Value,
    provider_id: &str,
    provider_name: &str,
    current: Option<&str>,
) -> Vec<AgentModelInfo> {
    session
        .get("models")
        .and_then(|models| models.get("availableModels"))
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(|model| {
                    let id =
                        json_text(model, &["modelId"]).or_else(|| json_text(model, &["id"]))?;
                    let (split_provider, model_id) = split_model_id(&id);
                    Some(AgentModelInfo {
                        provider_id: if split_provider.is_empty() {
                            provider_id.to_string()
                        } else {
                            split_provider.clone()
                        },
                        model_id: model_id.clone(),
                        provider_name: if split_provider.is_empty() {
                            provider_name.to_string()
                        } else {
                            split_provider
                        },
                        id: id.clone(),
                        display_name: json_text(model, &["name"])
                            .or_else(|| json_text(model, &["displayName"]))
                            .unwrap_or(model_id),
                        description: json_text(model, &["description"]).unwrap_or_default(),
                        is_default: current == Some(id.as_str()),
                        capabilities: vec!["session-selectable".to_string()],
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 从 session/new、session/load 或 config_option_update 中读取 OpenCode
/// 官方模型配置，返回完整 `provider/model`（保留 `/variant` 后缀）。
fn models_from_session(
    session: &Value,
    provider_id: &str,
    provider_name: &str,
) -> (Vec<AgentModelInfo>, Option<String>) {
    let current = model_option(session)
        .and_then(option_current_value)
        .or_else(|| json_text(session, &["models", "currentModelId"]));
    if let Some(option) = model_option(session) {
        let models = models_from_option(option, provider_id, provider_name, current.as_deref());
        if !models.is_empty() {
            return (models, current);
        }
    }
    let models =
        legacy_models_from_session(session, provider_id, provider_name, current.as_deref());
    (models, current)
}

fn publish_model_options(
    options: &Value,
    provider_id: &str,
    provider_name: &str,
    emit: &EventSink,
    applied_model: &Arc<Mutex<Option<String>>>,
    model_config: &Arc<Mutex<Option<AcpModelConfig>>>,
    model_update_generation: &Arc<AtomicU64>,
) {
    let Some(option) = model_option(options) else {
        return;
    };
    let current = option_current_value(option);
    let models = models_from_option(option, provider_id, provider_name, current.as_deref());
    if let Some(config) = AcpModelConfig::from_options(options) {
        if let Ok(mut guard) = model_config.lock() {
            *guard = Some(config);
        }
    }
    if let Some(current) = current {
        if let Ok(mut guard) = applied_model.lock() {
            *guard = Some(current.clone());
        }
        model_update_generation.fetch_add(1, Ordering::SeqCst);
        log::info!(
            "[agent] ACP config_option_update：当前模型={current}，会话可选模型 {} 个",
            models.len()
        );
    }
    if !models.is_empty() {
        (emit)(RuntimeEvent::session(AgentEvent::ModelsAvailable {
            models,
        }));
    }
}

fn wait_for_model_confirmation(
    applied_model: &Arc<Mutex<Option<String>>>,
    generation: &Arc<AtomicU64>,
    generation_before: u64,
    timeout: Duration,
) -> Option<String> {
    let deadline = Instant::now() + timeout;
    loop {
        if generation.load(Ordering::SeqCst) != generation_before {
            return applied_model.lock().ok().and_then(|guard| guard.clone());
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// 在 prompt 写入之前失败时，清掉活动轮次并投递明确错误。调用方需保证
/// 该错误文案已经区分“模型切换失败”和“prompt 发送失败”。
fn fail_turn_before_prompt(
    state: &Arc<Mutex<AcpTurnState>>,
    emit: &EventSink,
    round_id: &str,
    message: String,
) {
    if let Ok(mut guard) = state.lock() {
        if guard.active_turn == round_id {
            guard.active_turn.clear();
            guard.prompt_written = false;
            guard.cancel_requested = false;
            guard.cancel_notified = false;
            guard.pending_permissions.clear();
        }
    }
    (emit)(RuntimeEvent::turn(
        round_id.to_string(),
        "",
        AgentEvent::TurnFailed { message },
    ));
}

impl AgentAdapter for AcpAdapter {
    fn prompt(&mut self, core_turn_id: &str, request: &AgentPromptRequest) -> Result<(), String> {
        let session_id = self
            .session_id
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .ok_or_else(|| "ACP 会话尚未建立".to_string())?;
        {
            let mut state = self.state.lock().map_err(|_| "ACP 轮次状态锁损坏")?;
            state.queued_turns.insert(core_turn_id.to_string());
        }
        let client = self.client.clone();
        let emit = self.emit.clone();
        let state = self.state.clone();
        let dispatch = self.dispatch_lock.clone();
        let agent_id = self.agent_id.clone();
        let selected_model = self.model.clone();
        let applied_model = self.applied_model.clone();
        let model_config = self.model_config.clone();
        let model_update_generation = self.model_update_generation.clone();
        let round_id = core_turn_id.to_string();
        let message = request.text_with_history_context();
        let task = std::thread::Builder::new()
            .name("agent-acp-prompt".to_string())
            .spawn(move || {
                // Never wait for an earlier turn while holding AgentSessionManager's lock.
                let _dispatch_guard = match dispatch.lock() {
                    Ok(guard) => guard,
                    Err(_) => {
                        (emit)(RuntimeEvent::turn(
                            round_id,
                            "",
                            AgentEvent::TurnFailed {
                                message: "ACP 派发锁损坏".to_string(),
                            },
                        ));
                        return;
                    }
                };
                let skipped = match state.lock() {
                    Ok(mut guard) => {
                        guard.queued_turns.remove(&round_id);
                        if guard.skipped_turns.remove(&round_id) {
                            true
                        } else {
                            guard.active_turn = round_id.clone();
                            guard.prompt_written = false;
                            guard.cancel_requested = false;
                            guard.cancel_notified = false;
                            guard.pending_permissions.clear();
                            false
                        }
                    }
                    Err(_) => {
                        (emit)(RuntimeEvent::turn(
                            round_id,
                            "",
                            AgentEvent::TurnFailed {
                                message: "ACP 轮次状态锁损坏".to_string(),
                            },
                        ));
                        return;
                    }
                };
                if skipped {
                    (emit)(RuntimeEvent::turn(round_id, "", AgentEvent::TurnCancelled));
                    return;
                }

                // OpenCode 通过官方 configOptions 暴露模型。用户切换后必须
                // 先让 session/set_config_option 成功并确认 currentValue，
                // 再发送本轮 prompt；不能用请求值假装确认成功。
                if let Some(model) = selected_model.as_ref() {
                    let needs_switch = applied_model
                        .lock()
                        .map(|guard| guard.as_deref() != Some(model.as_str()))
                        .unwrap_or(true);
                    if needs_switch {
                        let Some(config) = model_config.lock().ok().and_then(|guard| guard.clone())
                        else {
                            fail_turn_before_prompt(
                                &state,
                                &emit,
                                &round_id,
                                format!(
                                    "OpenCode ACP 未声明模型配置项，无法确认并切换到 {model}；尚未发送给 Agent"
                                ),
                            );
                            return;
                        };
                        if config.allowed.is_empty() {
                            fail_turn_before_prompt(
                                &state,
                                &emit,
                                &round_id,
                                format!(
                                    "当前 ACP 会话未返回可选模型值，无法确认 {model} 是否可用；尚未发送给 Agent"
                                ),
                            );
                            return;
                        }
                        if !config.allowed.contains(model) {
                            fail_turn_before_prompt(
                                &state,
                                &emit,
                                &round_id,
                                format!(
                                    "所选模型 {model} 不在当前 ACP 会话的可选模型列表中（会话可选 {} 个）；为避免使用错误模型，尚未发送给 Agent",
                                    config.allowed.len()
                                ),
                            );
                            return;
                        }
                        let generation_before = model_update_generation.load(Ordering::SeqCst);
                        let switch_started = Instant::now();
                        log::info!(
                            "[agent] ACP 切换模型：实例 {}，核心轮次 {}，原生会话 {}，configId={}，请求={}（会话可选 {} 个）",
                            client.instance_id(),
                            round_id,
                            redact_session_id(&session_id),
                            config.config_id,
                            model,
                            config.allowed.len()
                        );
                        match client.request(
                            "session/set_config_option",
                            json!({
                                "sessionId": session_id,
                                "configId": config.config_id,
                                "value": model,
                            }),
                            Duration::from_secs(30),
                        ) {
                            Ok(result) => {
                                let response_confirmed = model_option(&result)
                                    .and_then(option_current_value);
                                let actual = match response_confirmed {
                                    Some(value) => Some(value),
                                    None => wait_for_model_confirmation(
                                        &applied_model,
                                        &model_update_generation,
                                        generation_before,
                                        MODEL_UPDATE_TIMEOUT,
                                    ),
                                };
                                match actual {
                                    Some(actual) if actual == *model => {
                                        if let Ok(mut guard) = applied_model.lock() {
                                            *guard = Some(actual.clone());
                                        }
                                        log::info!(
                                            "[agent] ACP 模型切换确认：请求={}，实际={}，耗时={}ms，方法=session/set_config_option",
                                            model,
                                            actual,
                                            switch_started.elapsed().as_millis()
                                        );
                                        (emit)(RuntimeEvent::turn(
                                            round_id.clone(),
                                            "",
                                            AgentEvent::ModelSwitchApplied {
                                                requested: Some(model.clone()),
                                                actual: Some(actual),
                                            },
                                        ));
                                    }
                                    Some(actual) => {
                                        let message = format!(
                                            "模型切换未生效：请求 {model}，原生会话实际为 {actual}；尚未发送给 Agent"
                                        );
                                        log::warn!(
                                            "[agent] {message}（实例 {}，核心轮次 {}，方法 session/set_config_option，耗时 {}ms）",
                                            client.instance_id(),
                                            round_id,
                                            switch_started.elapsed().as_millis()
                                        );
                                        (emit)(RuntimeEvent::turn(
                                            round_id.clone(),
                                            "",
                                            AgentEvent::ModelSwitchFailed {
                                                requested: model.clone(),
                                                actual: Some(actual.clone()),
                                                message: message.clone(),
                                            },
                                        ));
                                        fail_turn_before_prompt(&state, &emit, &round_id, message);
                                        return;
                                    }
                                    None => {
                                        let actual = applied_model
                                            .lock()
                                            .ok()
                                            .and_then(|guard| guard.clone());
                                        let message = format!(
                                            "session/set_config_option 未确认模型切换：请求 {model}，响应缺少 currentValue，且 {}ms 内未收到 config_option_update；尚未发送给 Agent",
                                            MODEL_UPDATE_TIMEOUT.as_millis()
                                        );
                                        log::warn!(
                                            "[agent] {message}（实例 {}，核心轮次 {}，实际={}，耗时 {}ms）",
                                            client.instance_id(),
                                            round_id,
                                            actual.as_deref().unwrap_or("<未知>"),
                                            switch_started.elapsed().as_millis()
                                        );
                                        (emit)(RuntimeEvent::turn(
                                            round_id.clone(),
                                            "",
                                            AgentEvent::ModelSwitchFailed {
                                                requested: model.clone(),
                                                actual,
                                                message: message.clone(),
                                            },
                                        ));
                                        fail_turn_before_prompt(&state, &emit, &round_id, message);
                                        return;
                                    }
                                }
                            }
                            Err(error) => {
                                let actual = applied_model
                                    .lock()
                                    .ok()
                                    .and_then(|guard| guard.clone());
                                let message = format!(
                                    "模型切换请求失败（session/set_config_option，请求 {model}）：{error}；尚未发送给 Agent"
                                );
                                log::warn!(
                                    "[agent] {message}（实例 {}，核心轮次 {}，配置项 {}，耗时 {}ms）",
                                    client.instance_id(),
                                    round_id,
                                    agent_id,
                                    switch_started.elapsed().as_millis()
                                );
                                (emit)(RuntimeEvent::turn(
                                    round_id.clone(),
                                    "",
                                    AgentEvent::ModelSwitchFailed {
                                        requested: model.clone(),
                                        actual,
                                        message: message.clone(),
                                    },
                                ));
                                fail_turn_before_prompt(&state, &emit, &round_id, message);
                                return;
                            }
                        }
                    }
                }

                log::info!(
                    "[agent] ACP 发送 session/prompt：实例 {}，核心轮次 {}，原生会话 {}，模型={}",
                    client.instance_id(),
                    round_id,
                    redact_session_id(&session_id),
                    selected_model.as_deref().unwrap_or("<会话默认>")
                );

                let accepted_state = state.clone();
                let accepted_emit = emit.clone();
                let accepted_client = client.clone();
                let accepted_session = session_id.clone();
                let accepted_id = round_id.clone();
                let result = client.request_observed(
                    "session/prompt",
                    json!({
                        "sessionId": session_id,
                        "prompt": [{ "type": "text", "text": message }],
                    }),
                    REQUEST_TIMEOUT,
                    move || {
                        let cancel_now = match accepted_state.lock() {
                            Ok(mut guard) if guard.active_turn == accepted_id => {
                                guard.prompt_written = true;
                                if guard.cancel_requested && !guard.cancel_notified {
                                    guard.cancel_notified = true;
                                    true
                                } else {
                                    false
                                }
                            }
                            _ => false,
                        };
                        (accepted_emit)(RuntimeEvent::turn(
                            accepted_id.clone(),
                            "",
                            AgentEvent::PromptAccepted,
                        ));
                        if cancel_now {
                            if let Err(error) = accepted_client
                                .notify("session/cancel", json!({ "sessionId": accepted_session }))
                            {
                                if let Ok(mut guard) = accepted_state.lock() {
                                    if guard.active_turn == accepted_id {
                                        guard.cancel_notified = false;
                                    }
                                }
                                (accepted_emit)(RuntimeEvent::session(AgentEvent::SessionError {
                                    message: format!("ACP 发送取消通知失败：{error}"),
                                }));
                            }
                        }
                    },
                );
                // ACP guarantees the prompt response follows all final updates.
                // Close the current identity before releasing the dispatch lock.
                let orphaned_permissions = match state.lock() {
                    Ok(mut guard) if guard.active_turn == round_id => {
                        guard.active_turn.clear();
                        guard.prompt_written = false;
                        guard.cancel_requested = false;
                        guard.cancel_notified = false;
                        std::mem::take(&mut guard.pending_permissions)
                            .into_values()
                            .collect::<Vec<_>>()
                    }
                    _ => Vec::new(),
                };
                for request_id in orphaned_permissions {
                    let _ = client
                        .respond(request_id, json!({ "outcome": { "outcome": "cancelled" } }));
                }
                let event = match result {
                    Ok(value) => match json_text(&value, &["stopReason"]).as_deref() {
                        Some("cancelled") => AgentEvent::TurnCancelled,
                        Some("end_turn" | "max_tokens" | "max_turn_requests" | "refusal") => {
                            AgentEvent::TurnCompleted
                        }
                        Some(other) => AgentEvent::TurnFailed {
                            message: format!("ACP 未知停止原因：{other}"),
                        },
                        None => AgentEvent::TurnFailed {
                            message: "ACP session/prompt 未返回 stopReason".to_string(),
                        },
                    },
                    Err(error) => AgentEvent::TurnFailed { message: error },
                };
                (emit)(RuntimeEvent::turn(round_id, "", event));
            });
        if let Err(error) = task {
            if let Ok(mut guard) = self.state.lock() {
                guard.queued_turns.remove(core_turn_id);
            }
            return Err(format!("无法启动 ACP 请求线程：{error}"));
        }
        Ok(())
    }

    fn native_session_id(&self) -> Option<String> {
        self.session_id.lock().ok().and_then(|guard| guard.clone())
    }

    fn set_model(&mut self, model: Option<String>) {
        self.model = model;
    }

    fn history_restored(&self) -> bool {
        self.history_restored
    }

    fn cancel(&mut self, core_turn_id: &str) -> Result<CancelOutcome, String> {
        let (send_cancel, permissions) = {
            let mut guard = self.state.lock().map_err(|_| "ACP 轮次状态锁损坏")?;
            if guard.active_turn != core_turn_id {
                if guard.queued_turns.contains(core_turn_id) {
                    guard.skipped_turns.insert(core_turn_id.to_string());
                    return Ok(CancelOutcome::Cancelled);
                }
                return Ok(CancelOutcome::AlreadyFinished);
            }
            guard.cancel_requested = true;
            let send_cancel = guard.prompt_written && !guard.cancel_notified;
            if send_cancel {
                guard.cancel_notified = true;
            }
            let permissions = std::mem::take(&mut guard.pending_permissions)
                .into_values()
                .collect::<Vec<_>>();
            (send_cancel, permissions)
        };
        // session/cancel is a one-way notification, NOT proof of cancellation.
        // The original session/prompt reply must carry stopReason=cancelled.
        let mut first_error: Option<String> = None;
        if send_cancel {
            let session_id = self
                .session_id
                .lock()
                .ok()
                .and_then(|guard| guard.clone())
                .ok_or_else(|| "ACP 会话尚未建立".to_string())?;
            if let Err(error) = self
                .client
                .notify("session/cancel", json!({ "sessionId": session_id }))
            {
                if let Ok(mut guard) = self.state.lock() {
                    if guard.active_turn == core_turn_id {
                        guard.cancel_notified = false;
                    }
                }
                first_error = Some(error);
            }
        }
        // ACP requires replies for *all* outstanding permission requests.
        // Requests arriving after cancel_requested are auto-rejected by the handler.
        for request_id in permissions {
            if let Err(error) = self
                .client
                .respond(request_id, json!({ "outcome": { "outcome": "cancelled" } }))
            {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
        if let Some(error) = first_error {
            Err(error)
        } else {
            Ok(CancelOutcome::Pending)
        }
    }

    fn respond_permission(&mut self, request_id: &str, option_id: &str) -> Result<(), String> {
        // Keep the state mutex while writing the reply: cancel cannot race in and
        // accidentally let a late "allow" through after cancellation.
        let mut state = self.state.lock().map_err(|_| "ACP 轮次状态锁损坏")?;
        if state.cancel_requested {
            return Err("本轮正在取消，权限请求已失效".to_string());
        }
        let rpc_id = *state
            .pending_permissions
            .get(request_id)
            .ok_or_else(|| "权限请求已失效".to_string())?;
        let result = if option_id.is_empty() {
            json!({ "outcome": { "outcome": "cancelled" } })
        } else {
            json!({ "outcome": { "outcome": "selected", "optionId": option_id } })
        };
        self.client.respond(rpc_id, result)?;
        state.pending_permissions.remove(request_id);
        Ok(())
    }

    fn close(&mut self, reason: CloseReason) {
        self.client.close(reason);
    }
}
