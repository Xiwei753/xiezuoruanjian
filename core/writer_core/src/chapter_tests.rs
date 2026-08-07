#[cfg(test)]
mod tests {
    use crate::chapter::{
        calculate_word_count, clear_chapter_content, create_chapter, delete_chapter, list_chapters,
        read_chapter, save_chapter_verified, save_chapter_verified_with_allow_empty_overwrite,
        Chapter,
    };
    use crate::error::Error;
    use crate::project::{create_project, Project};
    use crate::volume::{create_volume, Volume};
    
    use tempfile::{tempdir, TempDir};

    fn setup_chapter() -> (TempDir, Project, Volume, Chapter) {
        let dir = tempdir().unwrap();
        let data_root = dir.path();
        std::fs::create_dir_all(data_root.join("projects")).unwrap();

        let project = create_project(&data_root.join("projects"), "Test Project").unwrap();
        let volume = create_volume(&data_root.join("projects").join(&project.id), "Test Volume").unwrap();
        let chapter =
            create_chapter(&data_root.join("projects").join(&project.id), &volume.id, "Test Chapter").unwrap();

        (dir, project, volume, chapter)
    }

    #[test]
    fn test_create_read_save_chapter() {
        let dir = tempdir().unwrap();
        let data_root = dir.path();
        std::fs::create_dir_all(data_root.join("projects")).unwrap();

        let project = create_project(&data_root.join("projects"), "Test Project").unwrap();
        let volume = create_volume(&data_root.join("projects").join(&project.id), "Test Volume").unwrap();

        let chapters = list_chapters(&data_root.join("projects").join(&project.id), &volume.id).unwrap();
        assert_eq!(chapters.len(), 0);

        let chapter =
            create_chapter(&data_root.join("projects").join(&project.id), &volume.id, "Test Chapter").unwrap();
        assert_eq!(chapter.title, "Test Chapter");

        let chapters = list_chapters(&data_root.join("projects").join(&project.id), &volume.id).unwrap();
        assert_eq!(chapters.len(), 1);

        let content = read_chapter(&data_root.join("projects").join(&project.id), &volume.id, &chapter.id).unwrap();
        assert_eq!(content.content, "");

        let receipt = save_chapter_verified(&data_root.join("projects").join(&project.id),
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

        let content = read_chapter(&data_root.join("projects").join(&project.id), &volume.id, &chapter.id).unwrap();
        assert_eq!(content.content, "Hello World!");
        assert_eq!(content.meta.word_count, 11);
        assert_eq!(content.meta.hash, receipt.content_hash);
        assert_eq!(receipt.word_count, content.meta.word_count);
    }

    #[test]
    fn non_empty_chapter_can_save_non_empty_content() {
        let (dir, project, volume, chapter) = setup_chapter();
        let data_root = dir.path();

        save_chapter_verified(&data_root.join("projects").join(&project.id),
            &volume.id,
            &chapter.id,
            "First draft",
        )
        .unwrap();
        save_chapter_verified(&data_root.join("projects").join(&project.id),
            &volume.id,
            &chapter.id,
            "Second draft",
        )
        .unwrap();

        let content = read_chapter(&data_root.join("projects").join(&project.id), &volume.id, &chapter.id).unwrap();
        assert_eq!(content.content, "Second draft");
    }

    #[test]
    fn empty_chapter_can_save_empty_content() {
        let (dir, project, volume, chapter) = setup_chapter();
        let data_root = dir.path();

        save_chapter_verified(&data_root.join("projects").join(&project.id), &volume.id, &chapter.id, "").unwrap();

        let content = read_chapter(&data_root.join("projects").join(&project.id), &volume.id, &chapter.id).unwrap();
        assert_eq!(content.content, "");
    }

    #[test]
    fn non_empty_chapter_blocks_empty_overwrite_and_keeps_original() {
        let (dir, project, volume, chapter) = setup_chapter();
        let data_root = dir.path();

        save_chapter_verified(&data_root.join("projects").join(&project.id),
            &volume.id,
            &chapter.id,
            "Original content",
        )
        .unwrap();

        let err = save_chapter_verified(&data_root.join("projects").join(&project.id), &volume.id, &chapter.id, "")
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

        let content = read_chapter(&data_root.join("projects").join(&project.id), &volume.id, &chapter.id).unwrap();
        assert_eq!(content.content, "Original content");

        let whitespace_err = save_chapter_verified(&data_root.join("projects").join(&project.id),
            &volume.id,
            &chapter.id,
            " \n\t",
        )
        .unwrap_err();
        assert!(matches!(
            whitespace_err,
            Error::EmptyOverwriteBlocked { .. }
        ));

        let content = read_chapter(&data_root.join("projects").join(&project.id), &volume.id, &chapter.id).unwrap();
        assert_eq!(content.content, "Original content");
    }

    #[test]
    fn explicit_clear_chapter_content_succeeds() {
        let (dir, project, volume, chapter) = setup_chapter();
        let data_root = dir.path();

        save_chapter_verified(&data_root.join("projects").join(&project.id),
            &volume.id,
            &chapter.id,
            "Text to clear",
        )
        .unwrap();
        clear_chapter_content(&data_root.join("projects").join(&project.id), &volume.id, &chapter.id).unwrap();

        let content = read_chapter(&data_root.join("projects").join(&project.id), &volume.id, &chapter.id).unwrap();
        assert_eq!(content.content, "");
    }

    #[test]
    fn user_allowed_empty_overwrite_succeeds_and_persists() {
        let (dir, project, volume, chapter) = setup_chapter();
        let data_root = dir.path();

        save_chapter_verified(&data_root.join("projects").join(&project.id),
            &volume.id,
            &chapter.id,
            "User text",
        )
        .unwrap();

        let blocked = save_chapter_verified_with_allow_empty_overwrite(&data_root.join("projects").join(&project.id),
            &volume.id,
            &chapter.id,
            "",
            false,
        )
        .unwrap_err();
        assert!(matches!(blocked, Error::EmptyOverwriteBlocked { .. }));

        save_chapter_verified_with_allow_empty_overwrite(&data_root.join("projects").join(&project.id),
            &volume.id,
            &chapter.id,
            "",
            true,
        )
        .unwrap();

        let content = read_chapter(&data_root.join("projects").join(&project.id), &volume.id, &chapter.id).unwrap();
        assert_eq!(content.content, "");
    }

    #[test]
    fn end_to_end_write_save_reopen_verify_hash_and_word_count() {
        let dir = tempdir().unwrap();
        let data_root = dir.path();
        std::fs::create_dir_all(data_root.join("projects")).unwrap();

        let project = create_project(&data_root.join("projects"), "My Novel").unwrap();
        let volume = create_volume(&data_root.join("projects").join(&project.id), "Volume 1").unwrap();
        let chapter = create_chapter(&data_root.join("projects").join(&project.id), &volume.id, "Chapter 1").unwrap();

        let content = "这是一个测试章节的内容。\n它包含多行中文文本，用于验证字数统计和哈希校验。\n第三行内容，确保换行符被正确处理。";
        let receipt = save_chapter_verified(&data_root.join("projects").join(&project.id),
            &volume.id,
            &chapter.id,
            content,
        )
        .unwrap();

        assert_eq!(receipt.content_len, content.len());
        assert!(receipt.word_count > 0);
        assert!(!receipt.content_hash.is_empty());

        let reopened = read_chapter(&data_root.join("projects").join(&project.id), &volume.id, &chapter.id).unwrap();
        assert_eq!(reopened.content, content);
        assert_eq!(reopened.meta.hash, receipt.content_hash);
        assert_eq!(reopened.meta.word_count, receipt.word_count);
    }

    #[test]
    fn end_to_end_overwrite_with_new_content_updates_hash() {
        let (dir, project, volume, chapter) = setup_chapter();
        let data_root = dir.path();

        let first_content = "First draft content";
        let receipt1 = save_chapter_verified(&data_root.join("projects").join(&project.id),
            &volume.id,
            &chapter.id,
            first_content,
        )
        .unwrap();

        let second_content = "Second draft with more words and different content";
        let receipt2 = save_chapter_verified(&data_root.join("projects").join(&project.id),
            &volume.id,
            &chapter.id,
            second_content,
        )
        .unwrap();

        assert_ne!(receipt1.content_hash, receipt2.content_hash);
        assert_ne!(receipt1.word_count, receipt2.word_count);

        let reopened = read_chapter(&data_root.join("projects").join(&project.id), &volume.id, &chapter.id).unwrap();
        assert_eq!(reopened.content, second_content);
        assert_eq!(reopened.meta.hash, receipt2.content_hash);
    }

    #[test]
    fn end_to_end_empty_overwrite_blocked_for_non_empty_chapter() {
        let (dir, project, volume, chapter) = setup_chapter();
        let data_root = dir.path();

        save_chapter_verified(&data_root.join("projects").join(&project.id),
            &volume.id,
            &chapter.id,
            "Some content",
        )
        .unwrap();

        let result =
            save_chapter_verified(&data_root.join("projects").join(&project.id), &volume.id, &chapter.id, "");
        assert!(matches!(result, Err(Error::EmptyOverwriteBlocked { .. })));

        let content = read_chapter(&data_root.join("projects").join(&project.id), &volume.id, &chapter.id).unwrap();
        assert_eq!(content.content, "Some content");
    }

    #[test]
    fn test_calculate_word_count() {
        // Empty strings
        assert_eq!(calculate_word_count(""), 0);

        // Strings with only whitespaces
        assert_eq!(calculate_word_count(" "), 0);
        assert_eq!(calculate_word_count("   \t\n  \r "), 0);

        // Standard English words
        assert_eq!(calculate_word_count("Hello"), 5);
        assert_eq!(calculate_word_count("Hello World"), 10);
        assert_eq!(calculate_word_count("A B C"), 3);

        // English words with ASCII punctuation
        assert_eq!(calculate_word_count("Hello, world!"), 12);
        assert_eq!(calculate_word_count("It's a beautiful day."), 18);

        // CJK characters with Unicode punctuation
        assert_eq!(calculate_word_count("你好，世界！"), 6);
        assert_eq!(calculate_word_count("測試—-…“”‘’"), 9);
        assert_eq!(calculate_word_count("这是一个测试章节的内容。"), 12);

        // Mixed English and CJK text
        assert_eq!(calculate_word_count("Hello 世界"), 7);
        assert_eq!(calculate_word_count("Rust语言真的很好用"), 11);

        // Emoji and special symbols
        assert_eq!(calculate_word_count("Hello 😊"), 6);
        assert_eq!(calculate_word_count("🚀✨🎉"), 3);
    }

    #[test]
    fn end_to_end_clear_chapter_content_explicit() {
        let (dir, project, volume, chapter) = setup_chapter();
        let data_root = dir.path();

        save_chapter_verified(&data_root.join("projects").join(&project.id),
            &volume.id,
            &chapter.id,
            "Content to be cleared",
        )
        .unwrap();

        clear_chapter_content(&data_root.join("projects").join(&project.id), &volume.id, &chapter.id).unwrap();

        let content = read_chapter(&data_root.join("projects").join(&project.id), &volume.id, &chapter.id).unwrap();
        assert_eq!(content.content, "");
        assert_eq!(content.meta.word_count, 0);
    }

    #[test]
    fn test_delete_chapter_moves_to_trash_and_updates_tombstone() {
        let (dir, project, volume, chapter) = setup_chapter();
        let data_root = dir.path();

        // 1. Verify chapter exists
        let chapters_before = list_chapters(&data_root.join("projects").join(&project.id), &volume.id).unwrap();
        assert_eq!(chapters_before.len(), 1);
        assert_eq!(chapters_before[0].id, chapter.id);

        let chapter_dir = data_root
            .join("projects")
            .join(&project.id)
            .join("volumes")
            .join(&volume.id)
            .join("chapters")
            .join(&chapter.id);
        assert!(chapter_dir.exists());

        // 2. Perform deletion
        delete_chapter(&data_root.join("projects").join(&project.id), &volume.id, &chapter.id, data_root).unwrap();

        // 3. Verify chapter is removed from list and filesystem
        let chapters_after = list_chapters(&data_root.join("projects").join(&project.id), &volume.id).unwrap();
        assert!(chapters_after.is_empty());
        assert!(!chapter_dir.exists());

        // 4. Verify trash directory contains the deleted chapter
        let trash_dir = data_root.join("sync/trash");
        assert!(trash_dir.exists());

        let trash_entries = std::fs::read_dir(&trash_dir).unwrap();
        let mut trash_found = false;
        let mut _trash_path = None;
        for entry in trash_entries {
            let entry = entry.unwrap();
            let file_name = entry.file_name().into_string().unwrap();
            if file_name.ends_with(&chapter.id) {
                trash_found = true;
                _trash_path = Some(entry.path());
                break;
            }
        }
        assert!(trash_found, "Deleted chapter not found in trash");

        // 5. Verify tombstone in sync state
        let state = crate::sync::SyncService::load_sync_state(data_root).unwrap();
        let rel_chapter_dir = chapter_dir
            .strip_prefix(data_root)
            .unwrap()
            .to_string_lossy()
            .replace("\\", "/");

        let mut tombstone_found = false;
        for tombstone in &state.tombstones {
            if tombstone.original_path == rel_chapter_dir
                || tombstone.original_path.contains(&chapter.id)
            {
                tombstone_found = true;
                break;
            }
        }
        assert!(
            tombstone_found,
            "Tombstone for chapter not found in sync state"
        );
    }
}
