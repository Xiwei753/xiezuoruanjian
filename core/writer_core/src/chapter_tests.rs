#[cfg(test)]
mod tests {
    use crate::chapter::{
        calculate_word_count, clear_chapter_content, create_chapter, list_chapters, read_chapter,
        save_chapter_verified, save_chapter_verified_with_allow_empty_overwrite, Chapter,
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
        let chapter =
            create_chapter(workspace_path, &project.id, &volume.id, "Test Chapter").unwrap();

        (dir, project, volume, chapter)
    }

    #[test]
    fn test_calculate_word_count_includes_cjk_punctuation_and_emoji() {
        // English text (whitespace not counted)
        assert_eq!(calculate_word_count("hello world"), 10);
        // CJK text
        assert_eq!(calculate_word_count("你好世界"), 4);
        // Mixed content
        assert_eq!(calculate_word_count("hello 世界"), 7);
        // Unicode punctuation
        assert_eq!(calculate_word_count("。，！？；：“”‘’（）《》〈〉【】『』「」〔〕…—～·"), 28);
        // Emojis
        assert_eq!(calculate_word_count("🤔✨"), 2);
        // Empty string and whitespace
        assert_eq!(calculate_word_count(""), 0);
        assert_eq!(calculate_word_count(" \n\t\r"), 0);
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
        assert_eq!(
            receipt.content_hash,
            format!("{:x}", md5::compute("Hello World!".as_bytes()))
        );

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
        assert!(matches!(
            whitespace_err,
            Error::EmptyOverwriteBlocked { .. }
        ));

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
    fn user_allowed_empty_overwrite_succeeds_and_persists() {
        let (dir, project, volume, chapter) = setup_chapter();
        let workspace_path = dir.path();

        save_chapter_verified(
            workspace_path,
            &project.id,
            &volume.id,
            &chapter.id,
            "User text",
        )
        .unwrap();

        let blocked = save_chapter_verified_with_allow_empty_overwrite(
            workspace_path,
            &project.id,
            &volume.id,
            &chapter.id,
            "",
            false,
        )
        .unwrap_err();
        assert!(matches!(blocked, Error::EmptyOverwriteBlocked { .. }));

        save_chapter_verified_with_allow_empty_overwrite(
            workspace_path,
            &project.id,
            &volume.id,
            &chapter.id,
            "",
            true,
        )
        .unwrap();

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

    #[test]
    fn end_to_end_write_save_reopen_verify_hash_and_word_count() {
        let dir = tempdir().unwrap();
        let workspace_path = dir.path();
        create_workspace(workspace_path).unwrap();

        let project = create_project(workspace_path, "My Novel").unwrap();
        let volume = create_volume(workspace_path, &project.id, "Volume 1").unwrap();
        let chapter =
            create_chapter(workspace_path, &project.id, &volume.id, "Chapter 1").unwrap();

        let content = "这是一个测试章节的内容。\n它包含多行中文文本，用于验证字数统计和哈希校验。\n第三行内容，确保换行符被正确处理。";
        let receipt = save_chapter_verified(
            workspace_path,
            &project.id,
            &volume.id,
            &chapter.id,
            content,
        )
        .unwrap();

        assert_eq!(receipt.content_len, content.len());
        assert!(receipt.word_count > 0);
        assert!(!receipt.content_hash.is_empty());

        let reopened = read_chapter(workspace_path, &project.id, &volume.id, &chapter.id).unwrap();
        assert_eq!(reopened.content, content);
        assert_eq!(reopened.meta.hash, receipt.content_hash);
        assert_eq!(reopened.meta.word_count, receipt.word_count);
    }

    #[test]
    fn end_to_end_overwrite_with_new_content_updates_hash() {
        let (dir, project, volume, chapter) = setup_chapter();
        let workspace_path = dir.path();

        let first_content = "First draft content";
        let receipt1 = save_chapter_verified(
            workspace_path,
            &project.id,
            &volume.id,
            &chapter.id,
            first_content,
        )
        .unwrap();

        let second_content = "Second draft with more words and different content";
        let receipt2 = save_chapter_verified(
            workspace_path,
            &project.id,
            &volume.id,
            &chapter.id,
            second_content,
        )
        .unwrap();

        assert_ne!(receipt1.content_hash, receipt2.content_hash);
        assert_ne!(receipt1.word_count, receipt2.word_count);

        let reopened = read_chapter(workspace_path, &project.id, &volume.id, &chapter.id).unwrap();
        assert_eq!(reopened.content, second_content);
        assert_eq!(reopened.meta.hash, receipt2.content_hash);
    }

    #[test]
    fn end_to_end_empty_overwrite_blocked_for_non_empty_chapter() {
        let (dir, project, volume, chapter) = setup_chapter();
        let workspace_path = dir.path();

        save_chapter_verified(
            workspace_path,
            &project.id,
            &volume.id,
            &chapter.id,
            "Some content",
        )
        .unwrap();

        let result = save_chapter_verified(
            workspace_path,
            &project.id,
            &volume.id,
            &chapter.id,
            "",
        );
        assert!(matches!(result, Err(Error::EmptyOverwriteBlocked { .. })));

        let content = read_chapter(workspace_path, &project.id, &volume.id, &chapter.id).unwrap();
        assert_eq!(content.content, "Some content");
    }

    #[test]
    fn end_to_end_clear_chapter_content_explicit() {
        let (dir, project, volume, chapter) = setup_chapter();
        let workspace_path = dir.path();

        save_chapter_verified(
            workspace_path,
            &project.id,
            &volume.id,
            &chapter.id,
            "Content to be cleared",
        )
        .unwrap();

        clear_chapter_content(workspace_path, &project.id, &volume.id, &chapter.id).unwrap();

        let content = read_chapter(workspace_path, &project.id, &volume.id, &chapter.id).unwrap();
        assert_eq!(content.content, "");
        assert_eq!(content.meta.word_count, 0);
    }
}
