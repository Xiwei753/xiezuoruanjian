//! # AI 服务模块 (AI Service)
//!
//! 本模块实现了与 AI 服务集成的核心功能，支持与各种大语言模型进行交互。
//!
//! ## 主要功能
//!
//! - **对话管理**: 维护多轮对话上下文，支持用户消息、助手消息和工具调用消息
//! - **工具定义**: 支持定义和注册 AI 可调用的工具函数
//! - **上下文构建**: 根据不同范围（当前章节、全书、全局知识等）构建 AI 上下文
//! - **请求构建**: 生成符合 API 规范的请求载荷
//! - **动作解析**: 解析 AI 返回的动作指令（导航、设置、文本插入等）
//!
//! ## 支持的动作类型
//!
//! - `Navigate`: 页面导航
//! - `ApplySetting`: 应用设置变更
//! - `InsertText`: 插入文本
//! - `ReplaceText`: 替换文本
//! - `RunCommand`: 运行命令
//! - `Custom`: 自定义操作
//!
//! ## 依赖关系
//!
//! - `serde` / `serde_json`: 序列化/反序列化
//! - 外部 AI API（如 DeepSeek）
//!
//! ## 使用场景
//!
//! - AI 写作助手
//! - 智能文本分析和建议
//! - 自动化内容生成
//! - 工具调用式 AI 交互

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiActionResponse {
    pub display_text: String,
    pub actions: Vec<AiAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AiActionType {
    Navigate,
    ApplySetting,
    InsertText,
    ReplaceText,
    RunCommand,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiAction {
    pub label: String,
    pub action_type: AiActionType,
    pub payload: String,
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

/// 流式输出缓冲区配置
#[derive(Debug, Clone)]
pub struct StreamFlushConfig {
    /// 缓冲区字符数阈值，达到后触发 flush
    pub char_threshold: usize,
    /// 遇到换行符时是否立即 flush
    pub flush_on_newline: bool,
    /// 句尾标点触发 flush（。！？；）
    pub flush_on_sentence_end: bool,
}

impl Default for StreamFlushConfig {
    fn default() -> Self {
        Self {
            char_threshold: 64,
            flush_on_newline: true,
            flush_on_sentence_end: true,
        }
    }
}

/// 流式输出批量 flush 缓冲区
///
/// 用于 AI 流式 token 输出场景：token 逐个到达，缓冲区积累到阈值后
/// 一次性 flush 到编辑器，避免逐字渲染造成性能问题。
///
/// ## 使用方式
///
/// ```rust
/// use writer_core::ai_service::{StreamFlusher, StreamFlushConfig};
///
/// let mut flusher = StreamFlusher::new(StreamFlushConfig::default());
/// flusher.push("你");
/// flusher.push("好");
/// flusher.push("，");
/// flusher.push("世界");
/// flusher.push("。\n");
/// // flush_on_sentence_end + flush_on_newline 触发
/// let batch = flusher.flush();
/// assert_eq!(batch, "你好，世界。\n");
/// ```
pub struct StreamFlusher {
    buffer: String,
    config: StreamFlushConfig,
    total_pushed: usize,
    total_flushed: usize,
}

impl StreamFlusher {
    pub fn new(config: StreamFlushConfig) -> Self {
        Self {
            buffer: String::new(),
            config,
            total_pushed: 0,
            total_flushed: 0,
        }
    }

    /// 推入一个 token 片段
    pub fn push(&mut self, token: &str) {
        self.buffer.push_str(token);
        self.total_pushed += token.len();
    }

    /// 检查是否应该 flush
    pub fn should_flush(&self) -> bool {
        if self.buffer.len() >= self.config.char_threshold {
            return true;
        }
        if self.config.flush_on_newline && self.buffer.contains('\n') {
            return true;
        }
        if self.config.flush_on_sentence_end {
            for ch in self.buffer.chars().rev() {
                if matches!(ch, '。' | '！' | '？' | '；' | '.' | '!' | '?' | ';') {
                    return true;
                }
                if !ch.is_whitespace() {
                    break;
                }
            }
        }
        false
    }

    /// 取出缓冲区内容并清空
    pub fn flush(&mut self) -> String {
        let content = std::mem::take(&mut self.buffer);
        self.total_flushed += content.len();
        content
    }

    /// 取出缓冲区内容（如果有），否则返回空字符串
    pub fn flush_if_ready(&mut self) -> Option<String> {
        if self.should_flush() {
            Some(self.flush())
        } else {
            None
        }
    }

    /// 强制 flush 所有剩余内容（流结束时调用）
    pub fn flush_remaining(&mut self) -> Option<String> {
        if self.buffer.is_empty() {
            None
        } else {
            Some(self.flush())
        }
    }

    /// 当前缓冲区大小
    pub fn buffered_len(&self) -> usize {
        self.buffer.len()
    }

    /// 当前缓冲区内容（只读）
    pub fn buffered(&self) -> &str {
        &self.buffer
    }

    /// 统计：已推入总字节数
    pub fn total_pushed(&self) -> usize {
        self.total_pushed
    }

    /// 统计：已 flush 总字节数
    pub fn total_flushed(&self) -> usize {
        self.total_flushed
    }

    /// 清空缓冲区（不返回内容）
    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stream_flusher_basic() {
        let mut flusher = StreamFlusher::new(StreamFlushConfig::default());
        flusher.push("hello");
        assert!(!flusher.should_flush());
        assert_eq!(flusher.flush(), "hello");
    }

    #[test]
    fn test_stream_flusher_char_threshold() {
        let config = StreamFlushConfig {
            char_threshold: 5,
            flush_on_newline: false,
            flush_on_sentence_end: false,
        };
        let mut flusher = StreamFlusher::new(config);
        flusher.push("abc");
        assert!(!flusher.should_flush());
        flusher.push("de");
        assert!(flusher.should_flush());
        assert_eq!(flusher.flush(), "abcde");
    }

    #[test]
    fn test_stream_flusher_newline() {
        let mut flusher = StreamFlusher::new(StreamFlushConfig::default());
        flusher.push("hello\nworld");
        assert!(flusher.should_flush());
        let content = flusher.flush();
        assert_eq!(content, "hello\nworld");
    }

    #[test]
    fn test_stream_flusher_sentence_end() {
        let mut flusher = StreamFlusher::new(StreamFlushConfig::default());
        flusher.push("这是");
        assert!(!flusher.should_flush());
        flusher.push("一句话。");
        assert!(flusher.should_flush());
    }

    #[test]
    fn test_stream_flusher_flush_remaining() {
        let mut flusher = StreamFlusher::new(StreamFlushConfig::default());
        assert!(flusher.flush_remaining().is_none());
        flusher.push("remaining");
        assert_eq!(flusher.flush_remaining().unwrap(), "remaining");
        assert!(flusher.flush_remaining().is_none());
    }

    #[test]
    fn test_stream_flusher_flush_if_ready() {
        let config = StreamFlushConfig {
            char_threshold: 10,
            flush_on_newline: false,
            flush_on_sentence_end: false,
        };
        let mut flusher = StreamFlusher::new(config);
        flusher.push("short");
        assert!(flusher.flush_if_ready().is_none());
        flusher.push(" and longer");
        assert!(flusher.flush_if_ready().is_some());
    }
}
