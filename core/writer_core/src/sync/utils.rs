use std::path::Path;

impl crate::sync::SyncService {
    pub(crate) fn compute_file_hash(path: &Path) -> std::io::Result<String> {
        let content = std::fs::read(path)?;
        Ok(format!("{:x}", md5::compute(&content)))
    }
}

#[cfg(feature = "git-https")]
impl crate::sync::SyncService {
    pub fn compute_git_hash(content: &[u8]) -> String {
        match git2::Oid::hash_object(git2::ObjectType::Blob, content) {
            Ok(oid) => oid.to_string(),
            Err(_) => format!("{:x}", md5::compute(content)),
        }
    }
}

#[cfg(all(test, feature = "git-https"))]
mod tests {
    use crate::sync::SyncService;

    #[test]
    fn test_compute_git_hash_empty_string() {
        let content = b"";
        let hash = SyncService::compute_git_hash(content);
        assert_eq!(hash, "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
    }

    #[test]
    fn test_compute_git_hash_hello_world() {
        let content = b"hello world";
        let hash = SyncService::compute_git_hash(content);
        assert_eq!(hash, "95d09f2b10159347eece71399a7e2e907ea3df4f");
    }

    #[test]
    fn test_compute_git_hash_hello() {
        let content = b"hello";
        let hash = SyncService::compute_git_hash(content);
        assert_eq!(hash, "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0");
    }
}
