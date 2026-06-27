use crate::error::Result;
use crate::index;
use crate::trash;

impl super::WriterCore {
    pub fn move_chapter_to_trash(&self, chapter_id: &str) -> Result<()> {
        trash::move_chapter_to_trash(&self.workspace_path, chapter_id)
    }

    pub fn update_index(&self) -> Result<()> {
        index::update_index()
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

    pub fn load_graph(
        &self,
        project_id: Option<&str>,
    ) -> crate::error::Result<crate::graph_service::GraphDocument> {
        let graph = crate::graph_service::GraphService::new(&self.workspace_path);
        graph.load_graph(project_id)
    }

    pub fn save_graph(
        &self,
        project_id: Option<&str>,
        doc: &crate::graph_service::GraphDocument,
    ) -> crate::error::Result<()> {
        let graph = crate::graph_service::GraphService::new(&self.workspace_path);
        graph.save_graph(project_id, doc)
    }

    pub fn proofread_text(
        &self,
        text: &str,
    ) -> crate::error::Result<Vec<crate::proofreading_service::ProofreadingSuggestion>> {
        let pr = crate::proofreading_service::ProofreadingService::new();
        pr.proofread(text)
    }
}
