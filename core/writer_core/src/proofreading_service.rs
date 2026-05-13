pub struct ProofreadingService;

impl ProofreadingService {
    pub fn new() -> Self {
        Self
    }

    pub fn proofread(&self, _text: &str) -> crate::Result<String> {
        Err(crate::Error::NotImplemented)
    }
}

impl Default for ProofreadingService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proofreading_service_not_implemented() {
        let service = ProofreadingService::new();
        assert!(matches!(
            service.proofread("test"),
            Err(crate::Error::NotImplemented)
        ));
    }
}
