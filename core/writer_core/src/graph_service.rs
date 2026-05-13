pub struct GraphService;

impl GraphService {
    pub fn new() -> Self {
        Self
    }

    pub fn generate_graph(&self) -> crate::Result<()> {
        Err(crate::Error::NotImplemented)
    }
}

impl Default for GraphService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_service_not_implemented() {
        let service = GraphService::new();
        assert!(matches!(
            service.generate_graph(),
            Err(crate::Error::NotImplemented)
        ));
    }
}
