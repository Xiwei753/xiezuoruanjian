use crate::error::Result;
use crate::recent_edits;

impl super::WriterCore {
    pub fn get_recent_edits(&self) -> Result<Vec<recent_edits::RecentEdit>> {
        recent_edits::get_recent_edits(&self.app_data_root)
    }

    pub fn record_recent_edit(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> Result<()> {
        recent_edits::record_recent_edit(&self.app_data_root, project_id, volume_id, chapter_id)
    }

    pub fn flush_recent_edits(&self) -> Result<()> {
        recent_edits::flush_recent_edits(&self.app_data_root)
    }
}
