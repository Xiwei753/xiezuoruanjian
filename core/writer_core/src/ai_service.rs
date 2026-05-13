use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub r#type: String, // typically "function"
    pub function: FunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

pub struct ContextManager {
    pub history: Vec<ChatMessage>,
}

impl ContextManager {
    pub fn new() -> Self {
        Self {
            history: Vec::new(),
        }
    }

    /// The Preservation Rule (Same Turn):
    /// Keep the reasoning_content when accumulating tool call histories within the same multi-turn loop.
    pub fn add_assistant_response_with_tools(&mut self, response: ChatMessage) {
        // Assert it's an assistant role to enforce strictness
        assert_eq!(response.role, "assistant");
        self.history.push(response);
    }

    pub fn add_tool_result(&mut self, tool_id: &str, result_content: &str) {
        self.history.push(ChatMessage {
            role: "tool".to_string(),
            content: result_content.to_string(),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: Some(tool_id.to_string()),
        });
    }

    /// The Purge Rule (Next Turn):
    /// Before a new user turn, strip the reasoning_content from ALL previous assistant messages to prevent 400 errors.
    pub fn start_new_turn(&mut self, user_message: &str) {
        // Purge reasoning_content from previous rounds
        for msg in &mut self.history {
            if msg.role == "assistant" {
                msg.reasoning_content = None;
            }
        }

        self.history.push(ChatMessage {
            role: "user".to_string(),
            content: user_message.to_string(),
            reasoning_content: None,
            tool_calls: None,
            tool_call_id: None,
        });
    }
}

pub struct AiService;

impl AiService {
    pub fn new() -> Self {
        Self
    }

    pub fn generate_text(&self, _prompt: &str) -> crate::Result<String> {
        Err(crate::Error::NotImplemented)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ai_service_not_implemented() {
        let service = AiService::new();
        assert!(matches!(
            service.generate_text("test"),
            Err(crate::Error::NotImplemented)
        ));
    }
}
