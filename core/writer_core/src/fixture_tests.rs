#[cfg(test)]
mod tests {
        use std::path::Path;

    #[test]
    fn test_fixture_workspace_is_valid() {
        let path = Path::new("../../fixtures/sample_workspace");
        assert!(path.join("projects").exists());
    }
}
