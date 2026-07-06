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
#[cfg(test)]
mod tests {
    use super::super::WriterCore;
    use tempfile::tempdir;

    #[test]
    fn test_workspace_lifecycle() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());

        // Initially, validation should fail because workspace is not created
        assert!(!core.validate_workspace().unwrap());

        // Create the workspace
        assert!(core.create_workspace().is_ok());

        // Validation should now pass
        assert!(core.validate_workspace().unwrap());
    }

    #[test]
    fn test_recent_edits_flow() {
        let temp_dir = tempdir().unwrap();
        let core = WriterCore::new(temp_dir.path());
        core.create_workspace().unwrap();

        // Initially empty
        let edits = core.get_recent_edits().unwrap();
        assert!(edits.is_empty());

        // Record an edit
        core.record_recent_edit("proj1", "vol1", "chap1").unwrap();

        // Retrieve edit
        let edits = core.get_recent_edits().unwrap();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].project_id, "proj1");
        assert_eq!(edits[0].volume_id, "vol1");
        assert_eq!(edits[0].chapter_id, "chap1");

        // Record another edit
        core.record_recent_edit("proj2", "vol2", "chap2").unwrap();

        // Retrieve edits
        let edits = core.get_recent_edits().unwrap();
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[0].project_id, "proj2");
        assert_eq!(edits[1].project_id, "proj1");

        // Flush should succeed
        assert!(core.flush_recent_edits().is_ok());
    }
}
