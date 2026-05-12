use crate::backup;
use crate::error::Error;
use std::path::Path;

#[test]
fn test_backup_project_not_implemented() {
    let result = backup::backup_project(Path::new("/dummy/path"), "proj1");
    assert!(matches!(result, Err(Error::NotImplemented)));
}
