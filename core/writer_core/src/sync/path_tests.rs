use super::*;

#[test]
fn normal_relative_path_accepted() {
    let vp = ValidatedSyncPath::new("volumes/1/chapter.md").unwrap();
    assert_eq!(vp.as_str(), "volumes/1/chapter.md");
}

#[test]
fn chinese_filename_accepted() {
    let vp = ValidatedSyncPath::new("volumes/第一章/正文.md").unwrap();
    assert_eq!(vp.as_str(), "volumes/第一章/正文.md");
}

#[test]
fn backslash_normalized() {
    let vp = ValidatedSyncPath::new("volumes\\1\\chapter.md").unwrap();
    assert_eq!(vp.as_str(), "volumes/1/chapter.md");
}

#[test]
fn single_file_accepted() {
    let vp = ValidatedSyncPath::new("project.json").unwrap();
    assert_eq!(vp.as_str(), "project.json");
}

#[test]
fn relative_dotdot_traversal_rejected() {
    assert_eq!(
        ValidatedSyncPath::new("../project.json").unwrap_err(),
        SyncPathError::DirectoryTraversal
    );
}

#[test]
fn nested_dotdot_traversal_rejected() {
    assert_eq!(
        ValidatedSyncPath::new("volumes/../../outside/chapter.md").unwrap_err(),
        SyncPathError::DirectoryTraversal
    );
}

#[test]
fn absolute_path_rejected() {
    assert_eq!(
        ValidatedSyncPath::new("/etc/passwd").unwrap_err(),
        SyncPathError::AbsolutePath
    );
}

#[test]
fn windows_drive_letter_rejected() {
    assert_eq!(
        ValidatedSyncPath::new("C:/Users/test/file.txt").unwrap_err(),
        SyncPathError::WindowsPrefix
    );
}

#[test]
fn windows_unc_rejected() {
    assert_eq!(
        ValidatedSyncPath::new("\\\\server\\share\\file.txt").unwrap_err(),
        SyncPathError::UncPath
    );
}

#[test]
fn windows_prefix_rejected() {
    assert_eq!(
        ValidatedSyncPath::new("\\\\?\\C:\\file.txt").unwrap_err(),
        SyncPathError::UncPath
    );
}

#[test]
fn empty_component_rejected() {
    assert_eq!(
        ValidatedSyncPath::new("volumes//chapter.md").unwrap_err(),
        SyncPathError::EmptyComponent
    );
}

#[test]
fn trailing_slash_empty_component_rejected() {
    assert_eq!(
        ValidatedSyncPath::new("volumes/").unwrap_err(),
        SyncPathError::EmptyComponent
    );
}

#[test]
fn dot_component_rejected() {
    assert_eq!(
        ValidatedSyncPath::new("./file.txt").unwrap_err(),
        SyncPathError::DotComponent
    );
}

#[test]
fn single_dotdot_rejected() {
    assert_eq!(
        ValidatedSyncPath::new("..").unwrap_err(),
        SyncPathError::DirectoryTraversal
    );
}

#[test]
fn join_under_works() {
    let vp = ValidatedSyncPath::new("volumes/1/chapter.md").unwrap();
    let full = vp.join_under(Path::new("/sync/root"));
    assert_eq!(full, Path::new("/sync/root/volumes/1/chapter.md"));
}

#[test]
fn partition_validated_paths_split() {
    let paths = vec![
        "ok/file.md".to_string(),
        "../bad.md".to_string(),
        "/abs/path.md".to_string(),
        "also/ok.md".to_string(),
    ];
    let (valid, invalid) = partition_validated_paths(&paths);
    assert_eq!(valid.len(), 2);
    assert_eq!(invalid.len(), 2);
    assert_eq!(valid[0].0.as_str(), "ok/file.md");
    assert_eq!(valid[1].0.as_str(), "also/ok.md");
    assert_eq!(invalid[0].0, "../bad.md");
    assert_eq!(invalid[1].0, "/abs/path.md");
}
