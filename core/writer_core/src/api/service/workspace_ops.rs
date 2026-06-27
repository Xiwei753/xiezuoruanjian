use super::*;

impl WriterCoreApi {
    pub fn list_projects(&self) -> ApiResult<Vec<ProjectDto>> {
        self.core()
            .list_projects()
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn create_workspace_if_needed(&self) -> ApiResult<bool> {
        self.core()
            .create_workspace()
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn validate_workspace(&self) -> ApiResult<bool> {
        self.core().validate_workspace().map_err(Into::into)
    }

    pub fn get_workspace_diagnostics(
        &self,
        has_workspace: bool,
        tree_count: u64,
    ) -> ApiResult<WorkspaceDiagnosticsDto> {
        let path_obj = self.workspace_path.as_path();
        let has_path = !self.workspace_path.as_os_str().is_empty();
        let path_exists = has_path && path_obj.exists();
        let is_dir = has_path && path_obj.is_dir();
        let manifest_path = if has_path {
            path_obj.join("workspace_manifest.json")
        } else {
            PathBuf::new()
        };
        let projects_path = if has_path {
            path_obj.join("projects")
        } else {
            PathBuf::new()
        };
        let app_meta_path = if has_path {
            path_obj.join("app-meta")
        } else {
            PathBuf::new()
        };
        let manifest_exists = has_path && manifest_path.exists();
        let projects_dir_exists = has_path && projects_path.is_dir();
        let app_meta_exists = has_path && app_meta_path.exists();
        let validate_workspace = if is_dir {
            self.validate_workspace().unwrap_or(false)
        } else {
            false
        };
        let core_initialized = has_workspace && has_path;
        let last_workspace_path = crate::app_config::get_last_workspace_path().unwrap_or_default();
        let (writable, writable_error) = Self::probe_workspace_writable(path_obj, is_dir);
        let create_project_available = has_workspace
            && core_initialized
            && validate_workspace
            && path_exists
            && is_dir
            && manifest_exists
            && projects_dir_exists
            && writable;

        Ok(WorkspaceDiagnosticsDto {
            has_workspace,
            workspace_path: self.workspace_path.to_string_lossy().to_string(),
            core_initialized,
            path_exists,
            is_dir,
            manifest_path: manifest_path.to_string_lossy().to_string(),
            manifest_exists,
            projects_path: projects_path.to_string_lossy().to_string(),
            projects_dir_exists,
            app_meta_exists,
            writable,
            writable_error,
            validate_workspace,
            tree_count,
            last_workspace_path,
            create_project_available,
        })
    }

    pub fn get_recent_edits(&self) -> ApiResult<Vec<RecentEditDto>> {
        self.core()
            .get_recent_edits()
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(Into::into)
    }

    pub fn record_recent_edit(
        &self,
        project_id: &str,
        volume_id: &str,
        chapter_id: &str,
    ) -> ApiResult<bool> {
        self.core()
            .record_recent_edit(project_id, volume_id, chapter_id)
            .map(|_| true)
            .map_err(Into::into)
    }

    pub fn flush_recent_edits(&self) -> ApiResult<bool> {
        self.core()
            .flush_recent_edits()
            .map(|_| true)
            .map_err(WriterError::from)
    }
}
