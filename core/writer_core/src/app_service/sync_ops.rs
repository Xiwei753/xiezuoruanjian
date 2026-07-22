use crate::api::{
    SyncConfigDto, SyncDiagnosticsResultDto, SyncPlanDto, SyncResultDto, WriterError,
};

impl super::WriterAppService {
    pub fn perform_sync_diagnostics(
        &self,
        config: SyncConfigDto,
    ) -> Result<SyncDiagnosticsResultDto, WriterError> {
        self.api.perform_sync_diagnostics(config)
    }

    pub fn perform_sync_dry_run(&self, config: SyncConfigDto) -> Result<SyncPlanDto, WriterError> {
        self.api.perform_sync_dry_run(config)
    }

    pub fn perform_sync(&self, config: SyncConfigDto, force_sync: bool) -> Result<SyncResultDto, WriterError> {
        self.api.perform_sync(config, force_sync)
    }

    pub fn resolve_conflict_keep_local(&self, path: String) -> Result<bool, WriterError> {
        self.api.resolve_conflict_keep_local(&path)
    }

    pub fn resolve_conflict_take_remote(&self, path: String) -> Result<bool, WriterError> {
        self.api.resolve_conflict_take_remote(&path)
    }

    pub fn resolve_conflict_mark_merged(&self, path: String) -> Result<bool, WriterError> {
        self.api.resolve_conflict_mark_merged(&path)
    }
}
