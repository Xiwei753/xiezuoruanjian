use std::path::PathBuf;

use crate::platform_init::PlatformPaths;

impl PlatformPaths {
    pub fn config_file_path(&self, file_name: &str) -> PathBuf {
        self.config_dir.join(file_name)
    }

    pub fn app_data_dir(&self) -> &std::path::Path {
        &self.app_data_dir
    }

    pub fn cache_dir(&self) -> &std::path::Path {
        &self.cache_dir
    }

    pub fn log_dir(&self) -> &std::path::Path {
        &self.log_dir
    }

    pub fn no_backup_dir(&self) -> Option<&std::path::Path> {
        self.no_backup_dir.as_deref()
    }

    pub fn config_dir(&self) -> &std::path::Path {
        &self.config_dir
    }
}
