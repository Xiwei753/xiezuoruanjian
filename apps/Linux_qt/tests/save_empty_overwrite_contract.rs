#![allow(clippy::unwrap_used, clippy::expect_used)]
use tempfile::tempdir;
use writer_core::api::error::WriterError;
use writer_core::api::WriterCoreApi;

#[test]
fn save_empty_requires_explicit_user_clear_flag() {
    let dir = tempdir().unwrap();
    let api = WriterCoreApi::new(dir.path());
    api.create_workspace_if_needed().unwrap();

    let project = api.create_project("Test Project").unwrap();
    let volume = api.create_volume(&project.id, "Test Volume").unwrap();
    let chapter = api
        .create_chapter(&project.id, &volume.id, "Test Chapter")
        .unwrap();

    api.save_chapter_content(&project.id, &volume.id, &chapter.id, "Original")
        .unwrap();

    let blocked = api
        .save_chapter_content_with_options(&project.id, &volume.id, &chapter.id, "", false)
        .unwrap_err();
    assert!(matches!(blocked, WriterError::EmptyOverwriteBlocked { .. }));
    assert_eq!(
        api.open_chapter(&project.id, &volume.id, &chapter.id)
            .unwrap()
            .content,
        "Original"
    );

    api.save_chapter_content_with_options(&project.id, &volume.id, &chapter.id, "", true)
        .unwrap();
    assert_eq!(
        api.open_chapter(&project.id, &volume.id, &chapter.id)
            .unwrap()
            .content,
        ""
    );
}
