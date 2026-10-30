//! # Agent 会话 API
//!
//! Issue #1016：平台端通过这里创建 / 读取 / 追加会话，不直接碰 Core 文件。
//! 所有写入都原子落盘；运行状态与原生 session 身份随会话一起持久化，重启后
//! 可继续恢复。

use crate::agent::conversation::{
    AgentConversation, AgentConversationStatus, AgentToolEvent, AgentTurnStatus,
};
use crate::agent::conversation_store::AgentConversationStore;

use super::WriterCoreApi;

fn store(api: &WriterCoreApi) -> AgentConversationStore {
    AgentConversationStore::new(&api.app_data_root)
}

impl WriterCoreApi {
    /// 创建并保存一个空会话。`title` 为空时使用“新会话”。
    #[allow(clippy::too_many_arguments)]
    pub fn create_agent_conversation(
        &self,
        project_id: &str,
        agent_id: &str,
        agent_name: &str,
        launch_path: &str,
        title: &str,
        model: Option<String>,
        now_ms: i64,
    ) -> super::ApiResult<AgentConversation> {
        let title = if title.trim().is_empty() {
            "新会话"
        } else {
            title.trim()
        };
        let conversation = AgentConversation::new(
            project_id,
            agent_id,
            agent_name,
            launch_path,
            title,
            model,
            now_ms,
        );
        store(self).save(&conversation)?;
        Ok(conversation)
    }

    pub fn load_agent_conversation(
        &self,
        conversation_id: &str,
    ) -> super::ApiResult<Option<AgentConversation>> {
        store(self).load(conversation_id).map_err(Into::into)
    }

    pub fn list_agent_conversations(
        &self,
        project_id: Option<&str>,
    ) -> super::ApiResult<Vec<AgentConversation>> {
        store(self).list(project_id).map_err(Into::into)
    }

    /// 保存完整会话。平台端在流式事件收口时批量保存，避免每条 delta 都写盘。
    pub fn save_agent_conversation(
        &self,
        conversation: &AgentConversation,
    ) -> super::ApiResult<()> {
        store(self).save(conversation).map_err(Into::into)
    }

    /// 追加一轮用户消息并返回更新后的会话。
    pub fn begin_agent_turn(
        &self,
        conversation_id: &str,
        user_message: &str,
        now_ms: i64,
    ) -> super::ApiResult<AgentConversation> {
        let store = store(self);
        let Some(mut conversation) = store.load(conversation_id)? else {
            return Err(crate::api::error::WriterError::Other(format!(
                "Agent 会话不存在：{conversation_id}"
            )));
        };
        conversation.begin_turn(user_message, now_ms);
        conversation.ensure_title_from_first_turn();
        conversation.updated_at_ms = now_ms;
        store.save(&conversation)?;
        Ok(conversation)
    }

    pub fn append_agent_assistant_text(
        &self,
        conversation_id: &str,
        turn_id: &str,
        text: &str,
        now_ms: i64,
    ) -> super::ApiResult<AgentConversation> {
        self.update_agent_turn(conversation_id, turn_id, now_ms, |turn| {
            turn.append_text(text);
        })
    }

    pub fn append_agent_assistant_reasoning(
        &self,
        conversation_id: &str,
        turn_id: &str,
        text: &str,
        now_ms: i64,
    ) -> super::ApiResult<AgentConversation> {
        self.update_agent_turn(conversation_id, turn_id, now_ms, |turn| {
            turn.append_reasoning(text);
        })
    }

    /// 按 `tool_call_id` 合并工具事件：相同 ID 的 finished 事件更新原记录。
    pub fn upsert_agent_tool_event(
        &self,
        conversation_id: &str,
        turn_id: &str,
        event: AgentToolEvent,
        now_ms: i64,
    ) -> super::ApiResult<AgentConversation> {
        self.update_agent_turn(conversation_id, turn_id, now_ms, |turn| {
            if let Some(existing) = turn
                .tool_events
                .iter_mut()
                .find(|existing| existing.tool_call_id == event.tool_call_id)
            {
                existing.name = event.name;
                if event.input_json.is_some() {
                    existing.input_json = event.input_json;
                }
                if event.output_json.is_some() {
                    existing.output_json = event.output_json;
                }
                existing.status = event.status;
                if event.finished_at_ms.is_some() {
                    existing.finished_at_ms = event.finished_at_ms;
                }
            } else {
                turn.tool_events.push(event);
            }
        })
    }

    pub fn finish_agent_turn(
        &self,
        conversation_id: &str,
        turn_id: &str,
        status: AgentTurnStatus,
        error: Option<String>,
        now_ms: i64,
    ) -> super::ApiResult<AgentConversation> {
        let store = store(self);
        let Some(mut conversation) = store.load(conversation_id)? else {
            return Err(crate::api::error::WriterError::Other(format!(
                "Agent 会话不存在：{conversation_id}"
            )));
        };
        if let Some(turn) = conversation.turn_mut(turn_id) {
            match status {
                AgentTurnStatus::Completed => turn.complete(now_ms),
                AgentTurnStatus::Failed => {
                    turn.fail(
                        now_ms,
                        error.unwrap_or_else(|| "Agent 请求失败".to_string()),
                    );
                }
                AgentTurnStatus::Cancelled => turn.cancel(now_ms),
                AgentTurnStatus::Running => {}
            }
        }
        conversation.status = match status {
            AgentTurnStatus::Completed | AgentTurnStatus::Cancelled => {
                AgentConversationStatus::Idle
            }
            AgentTurnStatus::Failed => AgentConversationStatus::Failed,
            AgentTurnStatus::Running => AgentConversationStatus::Running,
        };
        conversation.updated_at_ms = now_ms;
        store.save(&conversation)?;
        Ok(conversation)
    }

    pub fn set_agent_conversation_status(
        &self,
        conversation_id: &str,
        status: AgentConversationStatus,
        now_ms: i64,
    ) -> super::ApiResult<AgentConversation> {
        let store = store(self);
        let Some(mut conversation) = store.load(conversation_id)? else {
            return Err(crate::api::error::WriterError::Other(format!(
                "Agent 会话不存在：{conversation_id}"
            )));
        };
        conversation.status = status;
        conversation.updated_at_ms = now_ms;
        store.save(&conversation)?;
        Ok(conversation)
    }

    pub fn set_agent_native_session_id(
        &self,
        conversation_id: &str,
        native_session_id: Option<String>,
        now_ms: i64,
    ) -> super::ApiResult<AgentConversation> {
        let store = store(self);
        let Some(mut conversation) = store.load(conversation_id)? else {
            return Err(crate::api::error::WriterError::Other(format!(
                "Agent 会话不存在：{conversation_id}"
            )));
        };
        conversation.native_session_id = native_session_id;
        conversation.updated_at_ms = now_ms;
        store.save(&conversation)?;
        Ok(conversation)
    }

    pub fn set_agent_conversation_model(
        &self,
        conversation_id: &str,
        model: Option<String>,
        now_ms: i64,
    ) -> super::ApiResult<AgentConversation> {
        let store = store(self);
        let Some(mut conversation) = store.load(conversation_id)? else {
            return Err(crate::api::error::WriterError::Other(format!(
                "Agent 会话不存在：{conversation_id}"
            )));
        };
        conversation.model = model;
        conversation.updated_at_ms = now_ms;
        store.save(&conversation)?;
        Ok(conversation)
    }

    pub fn delete_agent_conversation(&self, conversation_id: &str) -> super::ApiResult<()> {
        store(self).delete(conversation_id).map_err(Into::into)
    }

    fn update_agent_turn(
        &self,
        conversation_id: &str,
        turn_id: &str,
        now_ms: i64,
        update: impl FnOnce(&mut crate::agent::conversation::AgentTurn),
    ) -> super::ApiResult<AgentConversation> {
        let store = store(self);
        let Some(mut conversation) = store.load(conversation_id)? else {
            return Err(crate::api::error::WriterError::Other(format!(
                "Agent 会话不存在：{conversation_id}"
            )));
        };
        let Some(turn) = conversation.turn_mut(turn_id) else {
            return Err(crate::api::error::WriterError::Other(format!(
                "Agent 轮次不存在：{turn_id}"
            )));
        };
        update(turn);
        conversation.updated_at_ms = now_ms;
        store.save(&conversation)?;
        Ok(conversation)
    }
}
