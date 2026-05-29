use std::path::Path;
use std::sync::Arc;
use crate::facade::WriterCore;
use crate::error::Error;

#[derive(Debug, Clone, uniffi::Record)]
pub struct ProjectDto {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

impl From<crate::project::Project> for ProjectDto {
    fn from(p: crate::project::Project) -> Self {
        Self {
            id: p.id,
            title: p.title,
            created_at: p.created_at,
            updated_at: p.updated_at,
        }
    }
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum WriterError {
    #[error("IO error: {0}")]
    Io(String),
    #[error("Other error: {0}")]
    Other(String),
}

impl From<crate::error::Error> for WriterError {
    fn from(e: crate::error::Error) -> Self {
        match e {
            Error::Io(e) => WriterError::Io(e.to_string()),
            _ => WriterError::Other(e.to_string()),
        }
    }
}

#[derive(uniffi::Object)]
pub struct WriterAppService {
    workspace_path: String,
}

#[uniffi::export]
impl WriterAppService {
    #[uniffi::constructor]
    pub fn new(workspace_path: String) -> Arc<Self> {
        Arc::new(Self { workspace_path })
    }

    pub fn list_projects(&self) -> Result<Vec<ProjectDto>, WriterError> {
        let core = WriterCore::new(Path::new(&self.workspace_path));
        core.list_projects().map(|v| v.into_iter().map(Into::into).collect()).map_err(Into::into)
    }
}
