use crate::trash;
use crate::error::Error;
use std::path::Path;

#[test]
fn test_trash_chapter_not_implemented() {
    let result = trash::move_chapter_to_trash(Path::new("/dummy/path"), "chap1");
    assert!(matches!(result, Err(Error::NotImplemented)));
}
