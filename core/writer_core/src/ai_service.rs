use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiActionResponse {
    pub display_text: String,
    pub actions: Vec<AiAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiAction {
    pub label: String,
    pub action_type: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiProviderConfig {
    pub provider_name: String,
    pub api_key: String,
    pub base_url: Option<String>,
    pub model_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiContextScope {
    None,
    CurrentChapter,
    SelectedChapters,
    FullBook,
    ProjectKnowledge,
    GlobalKnowledge,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiContextReference {
    pub scope: AiContextScope,
    pub reference_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiToolProperty {
    #[serde(rename = "type")]
    pub property_type: String,
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<AiToolProperty>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiToolParameters {
    #[serde(rename = "type")]
    pub params_type: String,
    pub properties: std::collections::HashMap<String, AiToolProperty>,
    pub required: Vec<String>,
    pub additional_properties: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiToolFunction {
    pub name: String,
    pub description: String,
    pub parameters: AiToolParameters,
    pub strict: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiToolDefinition {
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: AiToolFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiToolCallFunction {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiToolCallRecord {
    pub id: String,
    #[serde(rename = "type")]
    pub tool_type: String,
    pub function: AiToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<AiToolCallRecord>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConversation {
    pub messages: Vec<ChatMessage>,
}

impl Default for AiConversation {
    fn default() -> Self {
        Self::new()
    }
}

impl AiConversation {
    pub fn new() -> Self {
        Self {
            messages: Vec::new(),
        }
    }

    pub fn add_user_message(&mut self, content: &str) {
        self.messages.push(ChatMessage {
            role: "user".to_string(),
            content: content.to_string(),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        });
    }

    pub fn add_assistant_message(
        &mut self,
        content: &str,
        reasoning_content: Option<String>,
        tool_calls: Option<Vec<AiToolCallRecord>>,
    ) {
        self.messages.push(ChatMessage {
            role: "assistant".to_string(),
            content: content.to_string(),
            reasoning_content,
            tool_calls,
            tool_call_id: None,
        });
    }

    pub fn add_tool_message(&mut self, tool_call_id: &str, content: &str) {
        self.messages.push(ChatMessage {
            role: "tool".to_string(),
            content: content.to_string(),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: Some(tool_call_id.to_string()),
        });
    }
}

pub struct AiService {
    config: Option<AiProviderConfig>,
}

impl AiService {
    pub fn new() -> Self {
        Self { config: None }
    }

    pub fn set_config(&mut self, config: AiProviderConfig) {
        self.config = Some(config);
    }

    pub fn build_ai_context(&self, reference: AiContextReference) -> crate::Result<String> {
        match reference.scope {
            AiContextScope::CurrentChapter => Ok("Context: Current Chapter".to_string()),
            AiContextScope::GlobalKnowledge => Ok("Context: Global Knowledge".to_string()),
            _ => Ok("Context: Other".to_string()),
        }
    }

    pub fn get_ai_request_payload(
        &self,
        conversation: &AiConversation,
        tools: Option<Vec<AiToolDefinition>>,
    ) -> crate::Result<Value> {
        let mut payload = serde_json::json!({
            "messages": conversation.messages,
        });

        if let Some(config) = &self.config {
            payload["model"] = serde_json::Value::String(config.model_name.clone());
        }

        if let Some(tools) = tools {
            payload["tools"] = serde_json::to_value(tools).unwrap_or(serde_json::Value::Null);
        }

        Ok(payload)
    }

    pub fn generate_text(&self, _prompt: &str) -> crate::Result<String> {
        Err(crate::Error::NotImplemented)
    }
}

impl Default for AiService {
    fn default() -> Self {
        Self::new()
    }
}
