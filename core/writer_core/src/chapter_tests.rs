#[cfg(test)]
mod tests {
    use crate::chapter::{create_chapter, list_chapters, read_chapter, save_chapter_verified};
    use crate::project::create_project;
    use crate::volume::create_volume;
    use crate::workspace::create_workspace;
    use tempfile::tempdir;

    #[test]
    fn test_create_read_save_chapter() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        create_workspace(workspace_path).unwrap();

        let project = create_project(workspace_path, "Test Project").unwrap();
        let volume = create_volume(workspace_path, &project.id, "Test Volume").unwrap();

        let chapters = list_chapters(workspace_path, &project.id, &volume.id).unwrap();
        assert_eq!(chapters.len(), 0);

        let chapter =
            create_chapter(workspace_path, &project.id, &volume.id, "Test Chapter").unwrap();
        assert_eq!(chapter.title, "Test Chapter");

        let chapters = list_chapters(workspace_path, &project.id, &volume.id).unwrap();
        assert_eq!(chapters.len(), 1);

        let content = read_chapter(workspace_path, &project.id, &volume.id, &chapter.id).unwrap();
        assert_eq!(content.content, "");

        let receipt = save_chapter_verified(
            workspace_path,
            &project.id,
            &volume.id,
            &chapter.id,
            "Hello World!",
        )
        .unwrap();

        assert!(receipt.chapter_relative_path.ends_with("/chapter.md"));
        assert_eq!(receipt.content_hash, format!("{:x}", md5::compute("Hello World!".as_bytes())));

        let content = read_chapter(workspace_path, &project.id, &volume.id, &chapter.id).unwrap();
        assert_eq!(content.content, "Hello World!");
        assert_eq!(content.meta.word_count, 11);
        assert_eq!(content.meta.hash, receipt.content_hash);
        assert_eq!(receipt.word_count, content.meta.word_count);
    }
}
