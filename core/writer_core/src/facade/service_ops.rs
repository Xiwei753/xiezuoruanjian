use crate::error::Result;
use crate::storage::project_delete_transaction;
use crate::trash;

impl super::WriterCore {
    pub fn move_chapter_to_trash(&self, chapter_id: &str) -> Result<()> {
        trash::move_chapter_to_trash(&self.projects_root, chapter_id, &self.app_data_root)
    }

    /// 恢复所有待处理的删除事务。
    ///
    /// 启动时调用，遍历 app_meta/delete-journals/ 下所有 journal，
    /// 根据 phase 和 from/trash 路径实际存在状态决定下一步。
    /// 已经完成两边后再清 journal。
    ///
    /// 返回恢复的事务数量。
    pub fn recover_pending_delete_transactions(&self) -> Result<usize> {
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
