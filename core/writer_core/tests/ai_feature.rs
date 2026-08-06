//! # AI feature 独立集成测试
//!
//! 本文件是 `writer_core` 的独立集成测试 target，由 `Cargo.toml` 中的
//! `[[test]] name = "ai_feature"` 注册，并通过 `required-features = ["ai"]`
//! 仅在启用 `ai` feature 时编译运行。
//!
//! ## 覆盖范围
//!
//! - AI feature 开启验证（`ai_available` 返回 `true`）
//! - AI 服务入口（`AiService::new()` 构造、`Default` 实现）
//! - 配置解析（`AiProviderConfig` 反序列化、`AiToolDefinition` 结构）
//! - 摘要/星图转换（`AiContextReference`、`AiContextScope` 枚举、`build_ai_context`）
//! - 错误边界（无效 JSON 输入、缺失字段处理）
//! - 权限规则（`ai_available` 在 feature 开启时为 `true`）
//!
//! 测试通过 `use writer_core::ai_service::*` 和 `writer_core::facade`
//! 访问公开 API，验证跨模块契约与不变量，不绑定内部实现细节。

#![cfg(feature = "ai")]
// 独立集成测试 crate：放宽 unwrap/expect 与可读性 lint（测试代码中合理使用）
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]

#[path = "ai_feature/helpers.rs"]
mod helpers;

use writer_core::ai_service::*;
use writer_core::facade::WriterCore;

/// 启用 `ai` feature 时，`ai_available` 必须返回 `true`。
/// 这是 feature gate 的核心契约：平台端据此决定是否暴露 AI 入口。
#[test]
fn ai_available_returns_true_when_feature_enabled() {
    let temp_dir = tempfile::tempdir().expect("temp dir creation must succeed");
    let core = WriterCore::new(temp_dir.path());
    assert!(
        core.ai_available(),
        "ai_available() must return true when the 'ai' feature is enabled"
    );
}

/// `AiService::new()` 必须构造一个无配置的实例，`Default` 实现必须与之等价。
/// 验证构造契约：两个入口产生行为一致的服务。
#[test]
fn ai_service_new_and_default_are_equivalent() {
    let from_new = AiService::new();
    let from_default = AiService::default();

    // 无配置时，build_ai_context 对同一引用必须产生相同上下文。
    let reference = AiContextReference {
        scope: AiContextScope::CurrentChapter,
        reference_ids: vec!["ch-1".to_string()],
    };
    let ctx_new = from_new.build_ai_context(reference.clone());
    let ctx_default = from_default.build_ai_context(reference);
    assert!(
        ctx_new.is_ok(),
        "build_ai_context must succeed without config"
    );
    assert_eq!(
        ctx_new.as_ref().expect("ctx_new must be Ok"),
        ctx_default.as_ref().expect("ctx_default must be Ok"),
        "new() and default() must behave identically"
    );
}

/// `AiProviderConfig` 必须能从合法 JSON 反序列化，且字段映射正确。
/// 验证配置契约：provider_name / api_key / model_name 必填，base_url 可选。
#[test]
fn ai_provider_config_deserializes_valid_json() {
    let json = r#"{
        "provider_name": "deepseek",
        "api_key": "sk-test-key",
        "base_url": "https://api.deepseek.com",
        "model_name": "deepseek-chat"
    }"#;
    let config: AiProviderConfig =
        serde_json::from_str(json).expect("valid AiProviderConfig JSON must deserialize");
    assert_eq!(config.provider_name, "deepseek");
    assert_eq!(config.api_key, "sk-test-key");
    assert_eq!(config.base_url.as_deref(), Some("https://api.deepseek.com"));
    assert_eq!(config.model_name, "deepseek-chat");
}

/// `AiProviderConfig` 在省略 `base_url` 时必须反序列化成功（Option 字段）。
#[test]
fn ai_provider_config_allows_missing_base_url() {
    let json = r#"{
        "provider_name": "openai",
        "api_key": "sk-key",
        "model_name": "gpt-4"
    }"#;
    let config: AiProviderConfig =
        serde_json::from_str(json).expect("AiProviderConfig without base_url must deserialize");
    assert!(
        config.base_url.is_none(),
        "base_url must default to None when absent"
    );
}

/// `AiProviderConfig` 在缺失必填字段时必须反序列化失败。
/// 验证错误边界：外部输入不得通过 unwrap 绕过，错误必须显式传播。
#[test]
fn ai_provider_config_rejects_missing_required_field() {
    let json = r#"{
        "provider_name": "openai",
        "api_key": "sk-key"
    }"#;
    let result: Result<AiProviderConfig, _> = serde_json::from_str(json);
    assert!(
        result.is_err(),
        "AiProviderConfig without model_name must fail to deserialize"
    );
}

/// `AiToolDefinition` 必须能从符合 OpenAI function-calling 规范的 JSON 反序列化。
/// 验证工具定义契约：type/function/parameters/required 字段结构。
#[test]
fn ai_tool_definition_deserializes_valid_json() {
    let json = r#"{
        "type": "function",
        "function": {
            "name": "navigate_to_chapter",
            "description": "Navigate to a specific chapter",
            "parameters": {
                "type": "object",
                "properties": {
                    "chapter_id": {
                        "type": "string",
                        "description": "The chapter ID to navigate to"
                    }
                },
                "required": ["chapter_id"],
                "additional_properties": false
            },
            "strict": true
        }
    }"#;
    let tool: AiToolDefinition =
        serde_json::from_str(json).expect("valid AiToolDefinition JSON must deserialize");
    assert_eq!(tool.tool_type, "function");
    assert_eq!(tool.function.name, "navigate_to_chapter");
    assert!(tool.function.strict);
    assert_eq!(
        tool.function.parameters.required,
        vec!["chapter_id".to_string()]
    );
    assert!(!tool.function.parameters.additional_properties);
    assert!(tool
        .function
        .parameters
        .properties
        .contains_key("chapter_id"));
}

/// `AiContextScope` 枚举必须用 snake_case 序列化/反序列化。
/// 验证跨语言契约：平台端依赖稳定的字符串标签。
#[test]
fn ai_context_scope_serializes_as_snake_case() {
    let scope = AiContextScope::CurrentChapter;
    let serialized = serde_json::to_string(&scope).expect("AiContextScope must serialize");
    assert_eq!(serialized, "\"current_chapter\"");

    let deserialized: AiContextScope = serde_json::from_str("\"full_book\"")
        .expect("snake_case \"full_book\" must deserialize to FullBook");
    assert_eq!(deserialized, AiContextScope::FullBook);
}

/// `AiContextReference` 携带 scope 与 reference_ids，必须正确往返序列化。
#[test]
fn ai_context_reference_round_trips() {
    let reference = AiContextReference {
        scope: AiContextScope::SelectedChapters,
        reference_ids: vec!["ch-a".to_string(), "ch-b".to_string()],
    };
    let json = serde_json::to_string(&reference).expect("AiContextReference must serialize");
    let restored: AiContextReference =
        serde_json::from_str(&json).expect("AiContextReference must round-trip");
    assert_eq!(restored.scope, AiContextScope::SelectedChapters);
    assert_eq!(restored.reference_ids, reference.reference_ids);
}

/// `AiService::build_ai_context` 对 `CurrentChapter` 必须返回非空上下文。
/// 验证摘要/星图转换的核心契约：上下文不为空字符串。
#[test]
fn build_ai_context_current_chapter_returns_nonempty() {
    let service = AiService::new();
    let reference = AiContextReference {
        scope: AiContextScope::CurrentChapter,
        reference_ids: vec!["chapter-1".to_string()],
    };
    let context = service
        .build_ai_context(reference)
        .expect("build_ai_context for CurrentChapter must succeed");
    assert!(
        !context.is_empty(),
        "context for CurrentChapter must not be empty"
    );
}

/// `AiService::build_ai_context` 对 `GlobalKnowledge` 必须返回非空上下文。
#[test]
fn build_ai_context_global_knowledge_returns_nonempty() {
    let service = AiService::new();
    let reference = AiContextReference {
        scope: AiContextScope::GlobalKnowledge,
        reference_ids: vec![],
    };
    let context = service
        .build_ai_context(reference)
        .expect("build_ai_context for GlobalKnowledge must succeed");
    assert!(
        !context.is_empty(),
        "context for GlobalKnowledge must not be empty"
    );
}

/// `AiService::build_ai_context` 对所有 scope 变体必须成功（不 panic、不报错）。
/// 验证枚举穷尽性契约：任意 scope 都能产生上下文。
#[test]
fn build_ai_context_succeeds_for_all_scopes() {
    let service = AiService::new();
    let scopes = [
        AiContextScope::None,
        AiContextScope::CurrentChapter,
        AiContextScope::SelectedChapters,
        AiContextScope::FullBook,
        AiContextScope::ProjectKnowledge,
        AiContextScope::GlobalKnowledge,
    ];
    for scope in scopes {
        let reference = AiContextReference {
            scope: scope.clone(),
            reference_ids: vec![],
        };
        let result = service.build_ai_context(reference);
        assert!(
            result.is_ok(),
            "build_ai_context must succeed for scope {:?}",
            scope
        );
    }
}

/// Facade 层 `build_ai_context` 必须与 `AiService` 直接调用产生一致结果。
/// 验证 facade 是薄转发层，不引入额外业务逻辑。
#[test]
fn facade_build_ai_context_matches_service() {
    let temp_dir = tempfile::tempdir().expect("temp dir creation must succeed");
    let core = WriterCore::new(temp_dir.path());
    let reference = AiContextReference {
        scope: AiContextScope::CurrentChapter,
        reference_ids: vec!["ch-1".to_string()],
    };

    let facade_ctx = core
        .build_ai_context(reference.clone())
        .expect("facade build_ai_context must succeed");
    let service_ctx = AiService::new()
        .build_ai_context(reference)
        .expect("service build_ai_context must succeed");
    assert_eq!(
        facade_ctx, service_ctx,
        "facade must be a thin forwarder for build_ai_context"
    );
}

/// `AiConversation` 必须维护多轮对话消息列表的顺序与角色。
/// 验证对话管理契约：user / assistant / tool 消息按插入顺序排列。
#[test]
fn ai_conversation_preserves_message_order_and_roles() {
    let mut conversation = AiConversation::new();
    conversation.add_user_message("Hello");
    conversation.add_assistant_message("Hi there", None, None);
    conversation.add_tool_message("call-1", "tool result");

    assert_eq!(conversation.messages.len(), 3);
    assert_eq!(conversation.messages[0].role, "user");
    assert_eq!(conversation.messages[0].content, "Hello");
    assert_eq!(conversation.messages[1].role, "assistant");
    assert_eq!(conversation.messages[1].content, "Hi there");
    assert_eq!(conversation.messages[2].role, "tool");
    assert_eq!(
        conversation.messages[2].tool_call_id.as_deref(),
        Some("call-1")
    );
}

/// `AiConversation::default()` 必须等价于 `new()`（空消息列表）。
#[test]
fn ai_conversation_default_is_empty() {
    let conversation = AiConversation::default();
    assert!(
        conversation.messages.is_empty(),
        "default conversation must be empty"
    );
}

/// `AiService::get_ai_request_payload` 必须包含对话消息。
/// 验证请求构建契约：payload 的 messages 字段反映对话内容。
#[test]
fn get_ai_request_payload_includes_messages() {
    let service = AiService::new();
    let mut conversation = AiConversation::new();
    conversation.add_user_message("test prompt");

    let payload = service
        .get_ai_request_payload(&conversation, None)
        .expect("get_ai_request_payload must succeed");
    let messages = payload
        .get("messages")
        .expect("payload must contain messages field");
    assert!(messages.is_array(), "messages must be a JSON array");
    assert_eq!(
        messages.as_array().expect("messages array").len(),
        1,
        "payload must contain exactly one message"
    );
}

/// `AiService::get_ai_request_payload` 在设置 config 后必须包含 model 字段。
/// 验证配置注入契约：model_name 通过 config 流入 payload。
#[test]
fn get_ai_request_payload_includes_model_when_configured() {
    let mut service = AiService::new();
    let config = AiProviderConfig {
        provider_name: "deepseek".to_string(),
        api_key: "sk-key".to_string(),
        base_url: None,
        model_name: "deepseek-chat".to_string(),
    };
    service.set_config(config);

    let conversation = AiConversation::new();
    let payload = service
        .get_ai_request_payload(&conversation, None)
        .expect("get_ai_request_payload with config must succeed");
    let model = payload
        .get("model")
        .expect("payload must contain model field when config is set");
    assert_eq!(model.as_str(), Some("deepseek-chat"));
}

/// `AiService::get_ai_request_payload` 在传入工具定义时必须包含 tools 字段。
#[test]
fn get_ai_request_payload_includes_tools_when_provided() {
    let service = AiService::new();
    let conversation = AiConversation::new();
    let tool = helpers::build_navigate_tool_definition();

    let payload = service
        .get_ai_request_payload(&conversation, Some(vec![tool]))
        .expect("get_ai_request_payload with tools must succeed");
    let tools = payload
        .get("tools")
        .expect("payload must contain tools field when tools are provided");
    assert!(tools.is_array(), "tools must be a JSON array");
    assert_eq!(
        tools.as_array().expect("tools array").len(),
        1,
        "payload must contain exactly one tool"
    );
}

/// `AiService::generate_text` 在当前实现中必须返回 `NotImplemented` 错误。
/// 验证未实现功能的错误契约：错误码稳定，平台端可据此判断能力。
#[test]
fn generate_text_returns_not_implemented_error() {
    let service = AiService::new();
    let result = service.generate_text("any prompt");
    let err = result.expect_err("generate_text must return NotImplemented error");
    assert_eq!(err.code(), "NOT_IMPLEMENTED");
    assert!(!err.recoverable(), "NotImplemented must be non-recoverable");
}

/// `AiActionType` 枚举必须用 snake_case 序列化，覆盖所有变体。
/// 验证动作类型契约：平台端依赖稳定字符串标签解析动作。
#[test]
fn ai_action_type_serializes_as_snake_case() {
    let cases = [
        (AiActionType::Navigate, "navigate"),
        (AiActionType::ApplySetting, "apply_setting"),
        (AiActionType::InsertText, "insert_text"),
        (AiActionType::ReplaceText, "replace_text"),
        (AiActionType::RunCommand, "run_command"),
        (AiActionType::Custom, "custom"),
    ];
    for (action_type, expected) in cases {
        let serialized = serde_json::to_string(&action_type).expect("AiActionType must serialize");
        assert_eq!(serialized, format!("\"{}\"", expected));
    }
}

/// `AiActionResponse` 必须能从包含 display_text 和 actions 的 JSON 反序列化。
/// 验证动作响应契约：解析 AI 返回的动作指令。
#[test]
fn ai_action_response_deserializes_valid_json() {
    let json = r#"{
        "display_text": "Navigating to chapter 1",
        "actions": [
            {
                "label": "Go to Chapter 1",
                "action_type": "navigate",
                "payload": "{\"chapter_id\": \"ch-1\"}"
            }
        ]
    }"#;
    let response: AiActionResponse =
        serde_json::from_str(json).expect("valid AiActionResponse JSON must deserialize");
    assert_eq!(response.display_text, "Navigating to chapter 1");
    assert_eq!(response.actions.len(), 1);
    assert_eq!(response.actions[0].label, "Go to Chapter 1");
    assert_eq!(response.actions[0].action_type, AiActionType::Navigate);
}

/// `AiActionResponse` 在 actions 字段缺失时必须反序列化失败（必填字段）。
#[test]
fn ai_action_response_rejects_missing_actions_field() {
    let json = r#"{"display_text": "no actions"}"#;
    let result: Result<AiActionResponse, _> = serde_json::from_str(json);
    assert!(
        result.is_err(),
        "AiActionResponse without actions field must fail to deserialize"
    );
}

/// `StreamFlusher` 在达到字符阈值时必须触发 flush。
/// 验证流式输出缓冲契约：阈值边界行为正确。
#[test]
fn stream_flusher_triggers_on_char_threshold() {
    let config = StreamFlushConfig {
        char_threshold: 5,
        flush_on_newline: false,
        flush_on_sentence_end: false,
    };
    let mut flusher = StreamFlusher::new(config);
    flusher.push("ab");
    assert!(!flusher.should_flush(), "below threshold must not flush");
    flusher.push("cde");
    assert!(flusher.should_flush(), "at threshold must flush");
    let flushed = flusher.flush();
    assert_eq!(flushed, "abcde");
    assert_eq!(
        flusher.buffered_len(),
        0,
        "after flush buffer must be empty"
    );
}

/// `StreamFlusher` 在遇到换行符时必须触发 flush（当 flush_on_newline 开启）。
#[test]
fn stream_flusher_triggers_on_newline() {
    let mut flusher = StreamFlusher::new(StreamFlushConfig::default());
    flusher.push("line one\n");
    assert!(flusher.should_flush(), "newline must trigger flush");
}

/// `StreamFlusher` 的统计计数必须保持守恒：pushed == flushed + buffered。
/// 验证不变量：流式缓冲区字节守恒。
#[test]
fn stream_flusher_byte_conservation_invariant() {
    let config = StreamFlushConfig {
        char_threshold: 100,
        flush_on_newline: false,
        flush_on_sentence_end: false,
    };
    let mut flusher = StreamFlusher::new(config);
    flusher.push("hello ");
    flusher.push("world");
    let partial = flusher.flush_if_ready();
    assert!(partial.is_none(), "below threshold must not auto-flush");

    flusher.push("!");
    let flushed = flusher.flush();
    // 不变量：total_pushed == total_flushed + buffered_len
    assert_eq!(
        flusher.total_pushed(),
        flusher.total_flushed() + flusher.buffered_len(),
        "byte conservation invariant must hold"
    );
    assert_eq!(flushed, "hello world!");
    assert_eq!(flusher.total_flushed(), "hello world!".len());
}

/// Facade 层 `get_ai_request_payload` 必须与 `AiService` 直接调用产生一致结果。
/// 验证 facade 是薄转发层。
#[test]
fn facade_get_ai_request_payload_matches_service() {
    let temp_dir = tempfile::tempdir().expect("temp dir creation must succeed");
    let core = WriterCore::new(temp_dir.path());
    let mut conversation = AiConversation::new();
    conversation.add_user_message("facade test");

    let facade_payload = core
        .get_ai_request_payload(&conversation, None)
        .expect("facade get_ai_request_payload must succeed");
    let service_payload = AiService::new()
        .get_ai_request_payload(&conversation, None)
        .expect("service get_ai_request_payload must succeed");
    assert_eq!(
        facade_payload, service_payload,
        "facade must be a thin forwarder for get_ai_request_payload"
    );
}

/// `ChatMessage` 的可选字段（reasoning_content / tool_calls / tool_call_id）
/// 在未设置时必须从序列化输出中省略（skip_serializing_if）。
#[test]
fn chat_message_omits_optional_fields_when_none() {
    let mut conversation = AiConversation::new();
    conversation.add_user_message("plain user message");
    let json =
        serde_json::to_string(&conversation.messages[0]).expect("ChatMessage must serialize");
    assert!(
        !json.contains("reasoning_content"),
        "None reasoning_content must be omitted from JSON"
    );
    assert!(
        !json.contains("tool_calls"),
        "None tool_calls must be omitted from JSON"
    );
    assert!(
        !json.contains("tool_call_id"),
        "None tool_call_id must be omitted from JSON"
    );
}
