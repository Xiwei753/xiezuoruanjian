use crate::error::Result;
use crate::storage::journal::project_delete as project_delete_transaction;
use crate::trash;

impl super::WriterCore {
    pub fn move_chapter_to_trash(&self, chapter_id: &str) -> Result<()> {
        trash::move_chapter_to_trash(&self.projects_root, chapter_id, &self.app_data_root)
    }

    /// 恢复所有待处理的删除事务。
    ///
    /// 启动时调用，遍历 app_meta/delete-journals/ 下所有 journal，
    /// 根据 phase 和 from/trash 路径实际存在状态决定下一步。
    ///
    /// #645 评论 5504296097 缺口2修复：返回 `Vec<RecoveredProjectDelete>`，
    /// 每个元素含待补 history 的 change-set。调用方（bootstrap）用 change-set
    /// 调 `record_workspace_change_set` 写 history 后调 `ack_project_delete_history`
    /// 推进 journal 到 `HistoryRecorded` → `Completed` 并清 journal。
    pub fn recover_pending_delete_transactions(
        &self,
    ) -> Result<Vec<crate::storage::journal::project_delete::RecoveredProjectDelete>> {
        project_delete_transaction::recover_pending_delete_transactions(&self.app_data_root)
    }

    pub fn ai_available(&self) -> bool {
        cfg!(feature = "ai")
    }

    #[cfg(feature = "ai")]
    pub fn build_ai_context(
        &self,
        reference: crate::ai_service::AiContextReference,
    ) -> crate::error::Result<String> {
        let ai = crate::ai_service::AiService::new();
        ai.build_ai_context(reference)
    }

    #[cfg(feature = "ai")]
    pub fn get_ai_request_payload(
        &self,
        conversation: &crate::ai_service::AiConversation,
        tools: Option<Vec<crate::ai_service::AiToolDefinition>>,
    ) -> crate::error::Result<serde_json::Value> {
        let ai = crate::ai_service::AiService::new();
        ai.get_ai_request_payload(conversation, tools)
    }
}
