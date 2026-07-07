use crate::error::Result;
use crate::workspace;

impl super::WriterCore {
    pub fn create_workspace(&self) -> Result<()> {
        workspace::create_workspace(&self.workspace_path)
    }

    pub fn validate_workspace(&self) -> Result<bool> {
        workspace::validate_workspace(&self.workspace_path)
    }

    pub fn get_recent_edits(&self) -> Result<Vec<crate::workspace::RecentEdit>> {
        crate::workspace::get_recent_edits(&self.workspace_path)
    }

    pub fn record_recent_edit(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> Result<()> {
        crate::workspace::record_recent_edit(
            &self.workspace_path,
            project_id,
            volume_id,
            chapter_id,
        )
    }

    pub fn flush_recent_edits(&self) -> Result<()> {
        crate::workspace::flush_recent_edits(&self.workspace_path)
    }
}
