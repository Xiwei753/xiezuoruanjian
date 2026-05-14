import re

with open("core/writer_core/src/sync_service.rs", "r") as f:
    content = f.read()

pull_logic_old = """let has_uncommitted_changes = repo
            .statuses(Some(&mut statuses))
            .map_err(|e| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?
            .iter()
            .any(|_s| {
                // If the file is whitelisted, we should not blindly overwrite
                // We'll just be safe and say if there are any uncommitted changes,
                // we should require commit first before pull.
                true
            });"""

pull_logic_new = """let has_uncommitted_changes = repo
            .statuses(Some(&mut statuses))
            .map_err(|e| {
                crate::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            })?
            .iter()
            .any(|s| {
                if let Some(path) = s.path() {
                    SyncService::is_whitelisted_path(path)
                } else {
                    false
                }
            });"""

content = content.replace(pull_logic_old, pull_logic_new)

with open("core/writer_core/src/sync_service.rs", "w") as f:
    f.write(content)
