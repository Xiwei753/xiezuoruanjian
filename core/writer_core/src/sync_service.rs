pub struct SyncService;

impl SyncService {
    pub fn new() -> Self {
        Self
    }

    pub fn sync(&self) -> crate::Result<()> {
        Err(crate::Error::NotImplemented)
    }
}

impl Default for SyncService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_service_not_implemented() {
        let service = SyncService::new();
        assert!(matches!(service.sync(), Err(crate::Error::NotImplemented)));
    }
}
