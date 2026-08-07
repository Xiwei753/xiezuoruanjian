#[cfg(test)]
mod tests {
        use std::path::Path;

    #[test]
    fn test_fixture_projects_dir_is_valid() {
        let path = Path::new("../../fixtures/sample_data");
        assert!(path.join("projects").exists());
    }
}
