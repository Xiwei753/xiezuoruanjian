#[cfg(test)]
mod tests {
    use crate::chapter::{
        clear_chapter_content, create_chapter, list_chapters, read_chapter, save_chapter_verified,
        Chapter,
    };
    use crate::error::Error;
    use crate::project::{create_project, Project};
    use crate::volume::{create_volume, Volume};
    use crate::workspace::create_workspace;
    use std::fs;
    use tempfile::{tempdir, TempDir};

    fn setup_chapter() -> (TempDir, Project, Volume, Chapter) {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        create_workspace(workspace_path).unwrap();

        let project = create_project(workspace_path, "Test Project").unwrap();
        let volume = create_volume(workspace_path, &project.id, "Test Volume").unwrap();
        let chapter = create_chapter(workspace_path, &project.id, &volume.id, "Test Chapter").unwrap();

        (dir, project, volume, chapter)
    }

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

    #[test]
    fn non_empty_chapter_can_save_non_empty_content() {
        let (dir, project, volume, chapter) = setup_chapter();
        let workspace_path = dir.path();

        save_chapter_verified(
            workspace_path,
            &project.id,
            &volume.id,
            &chapter.id,
            "First draft",
        )
        .unwrap();
        save_chapter_verified(
            workspace_path,
            &project.id,
            &volume.id,
            &chapter.id,
            "Second draft",
        )
        .unwrap();

        let content = read_chapter(workspace_path, &project.id, &volume.id, &chapter.id).unwrap();
        assert_eq!(content.content, "Second draft");
    }

    #[test]
    fn empty_chapter_can_save_empty_content() {
        let (dir, project, volume, chapter) = setup_chapter();
        let workspace_path = dir.path();

        save_chapter_verified(workspace_path, &project.id, &volume.id, &chapter.id, "").unwrap();

        let content = read_chapter(workspace_path, &project.id, &volume.id, &chapter.id).unwrap();
        assert_eq!(content.content, "");
    }

    #[test]
    fn non_empty_chapter_blocks_empty_overwrite_and_keeps_original() {
        let (dir, project, volume, chapter) = setup_chapter();
        let workspace_path = dir.path();

        save_chapter_verified(
            workspace_path,
            &project.id,
            &volume.id,
            &chapter.id,
            "Original content",
        )
        .unwrap();

        let err = save_chapter_verified(workspace_path, &project.id, &volume.id, &chapter.id, "")
            .unwrap_err();

        match err {
            Error::EmptyOverwriteBlocked {
                chapter_id,
                old_len,
                new_len,
                reason,
            } => {
                assert_eq!(chapter_id, chapter.id);
                assert_eq!(old_len, "Original content".len());
                assert_eq!(new_len, 0);
                assert_eq!(
                    reason,
                    "new_content_empty_or_whitespace_without_allow_empty_overwrite"
                );
            }
            other => panic!("expected EmptyOverwriteBlocked, got {other:?}"),
        }

        let content = read_chapter(workspace_path, &project.id, &volume.id, &chapter.id).unwrap();
        assert_eq!(content.content, "Original content");

        let whitespace_err = save_chapter_verified(
            workspace_path,
            &project.id,
            &volume.id,
            &chapter.id,
            " \n\t",
        )
        .unwrap_err();
        assert!(matches!(whitespace_err, Error::EmptyOverwriteBlocked { .. }));

        let content = read_chapter(workspace_path, &project.id, &volume.id, &chapter.id).unwrap();
        assert_eq!(content.content, "Original content");
    }

    #[test]
    fn explicit_clear_chapter_content_succeeds() {
        let (dir, project, volume, chapter) = setup_chapter();
        let workspace_path = dir.path();

        save_chapter_verified(
            workspace_path,
            &project.id,
            &volume.id,
            &chapter.id,
            "Text to clear",
        )
        .unwrap();
        clear_chapter_content(workspace_path, &project.id, &volume.id, &chapter.id).unwrap();

        let content = read_chapter(workspace_path, &project.id, &volume.id, &chapter.id).unwrap();
        assert_eq!(content.content, "");
    }

    #[test]
    fn overwriting_non_empty_content_creates_backup() {
        let (dir, project, volume, chapter) = setup_chapter();
        let workspace_path = dir.path();

        save_chapter_verified(
            workspace_path,
            &project.id,
            &volume.id,
            &chapter.id,
            "First draft",
        )
        .unwrap();
        save_chapter_verified(
            workspace_path,
            &project.id,
            &volume.id,
            &chapter.id,
            "Second draft",
        )
        .unwrap();

        let backup_dir = workspace_path.join("backups").join("chapters");
        let backups: Vec<_> = fs::read_dir(&backup_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .contains(&chapter.id)
            })
            .collect();

        assert_eq!(backups.len(), 1);
        let file_name = backups[0].file_name().unwrap().to_string_lossy();
        assert!(file_name.contains(&project.id));
        assert!(file_name.contains(&volume.id));
        assert!(file_name.contains(&chapter.id));
        assert!(file_name.ends_with(".md"));
        assert_eq!(fs::read_to_string(&backups[0]).unwrap(), "First draft");
    }
}
