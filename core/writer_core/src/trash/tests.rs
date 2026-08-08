use crate::error::Error;
use crate::trash;
use tempfile::tempdir;

#[test]
fn test_trash_chapter_not_found_when_no_project() {
    let dir = tempdir().unwrap();
    let result = trash::move_chapter_to_trash(dir.path(), "chap1", dir.path());
    assert!(matches!(result, Err(Error::ChapterNotFound)));
}

#[test]
fn test_trash_chapter_not_found_for_nonexistent_chapter() {
    let dir = tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("projects")).unwrap();
    let result = trash::move_chapter_to_trash(dir.path(), "nonexistent_chapter", dir.path());
    assert!(matches!(result, Err(Error::ChapterNotFound)));
}
