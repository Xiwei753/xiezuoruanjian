use crate::error::Result;
use crate::trash;

impl super::WriterCore {
    pub fn move_chapter_to_trash(&self, chapter_id: &str) -> Result<()> {
        trash::move_chapter_to_trash(&self.projects_root, chapter_id, &self.app_data_root)
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
