//! AI feature 测试辅助模块。
//!
//! 提供构造测试夹具的工具函数，避免在每个测试中重复构建
//! `AiToolDefinition` 等复杂结构，保持测试聚焦于契约验证。

use std::collections::HashMap;
use writer_core::ai_service::{AiToolDefinition, AiToolFunction, AiToolParameters, AiToolProperty};

/// 构造一个用于测试的 `navigate_to_chapter` 工具定义。
///
/// 该工具定义符合 OpenAI function-calling 规范，
/// 包含一个必填的 `chapter_id` 字符串参数。
pub fn build_navigate_tool_definition() -> AiToolDefinition {
    let mut properties = HashMap::new();
    properties.insert(
        "chapter_id".to_string(),
        AiToolProperty {
            property_type: "string".to_string(),
            description: Some("The chapter ID to navigate to".to_string()),
            items: None,
        },
    );
    AiToolDefinition {
        tool_type: "function".to_string(),
        function: AiToolFunction {
            name: "navigate_to_chapter".to_string(),
            description: "Navigate to a specific chapter".to_string(),
            parameters: AiToolParameters {
                params_type: "object".to_string(),
                properties,
                required: vec!["chapter_id".to_string()],
                additional_properties: false,
            },
            strict: true,
        },
    }
}
