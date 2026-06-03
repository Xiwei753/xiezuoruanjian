use crate::error::Error;
use crate::trash;
use tempfile::tempdir;

#[test]
fn test_trash_chapter_not_found_when_no_workspace() {
    let dir = tempdir().unwrap();
    let result = trash::move_chapter_to_trash(dir.path(), "chap1");
    assert!(matches!(result, Err(Error::ChapterNotFound)));
}

#[test]
fn test_trash_chapter_not_found_for_nonexistent_chapter() {
    let dir = tempdir().unwrap();
    crate::workspace::create_workspace(dir.path()).unwrap();
    let result = trash::move_chapter_to_trash(dir.path(), "nonexistent_chapter");
    assert!(matches!(result, Err(Error::ChapterNotFound)));
}
