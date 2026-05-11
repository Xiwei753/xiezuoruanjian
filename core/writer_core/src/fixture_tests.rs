#[cfg(test)]
mod tests {
    use crate::workspace::validate_workspace;
    use std::path::Path;

    #[test]
    fn test_fixture_workspace_is_valid() {
        let path = Path::new("../../fixtures/sample_workspace");
        assert!(validate_workspace(path).unwrap_or(false));
    }
}
