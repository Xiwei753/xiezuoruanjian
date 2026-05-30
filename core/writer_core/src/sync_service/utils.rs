use std::path::Path;

impl crate::sync_service::SyncService {
    pub(crate) fn compute_file_hash(path: &Path) -> std::io::Result<String> {
        let content = std::fs::read(path)?;
        Ok(format!("{:x}", md5::compute(&content)))
    }

}

impl crate::sync_service::SyncService {
    pub fn compute_git_hash(content: &[u8]) -> String {
        match git2::Oid::hash_object(git2::ObjectType::Blob, content) {
            Ok(oid) => oid.to_string(),
            Err(_) => format!("{:x}", md5::compute(content)),
        }
    }

}
