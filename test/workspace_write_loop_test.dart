import 'dart:io';
import 'package:flutter_test/flutter_test.dart';
import 'package:sqflite_common_ffi/sqflite_ffi.dart';

import 'package:writer_app/core/utils/content_utils.dart';
import 'package:writer_app/application/background_tasks/file_write_queue.dart';
import 'package:writer_app/infrastructure/storage/atomic_writer.dart';
import 'package:writer_app/application/services/workspace_service.dart';
import 'package:writer_app/domain/models/chapter.dart';
import 'package:writer_app/infrastructure/repositories/chapter_repository_impl.dart';
import 'package:writer_app/infrastructure/database/database_helper.dart';

void main() {
  setUpAll(() {
    sqfliteFfiInit();
    databaseFactory = databaseFactoryFfi;
  });

  group('Workspace Write Loop Tests', () {
    late Directory tempWorkspace;
    late AtomicWriter storageService;
    late FileWriteQueue writeQueue;
    late WorkspaceService workspaceService;
    late ChapterRepositoryImpl chapterRepository;
    late DatabaseHelper dbHelper;

    setUp(() async {
      tempWorkspace = await Directory.systemTemp.createTemp('writer_app_test_');
      storageService = AtomicWriter();
      writeQueue = FileWriteQueue(storageService);
      workspaceService = WorkspaceService(storageService);
      chapterRepository = ChapterRepositoryImpl(writeQueue, tempWorkspace.path);
      dbHelper = DatabaseHelper(tempWorkspace.path);
    });

    tearDown(() async {
      if (await tempWorkspace.exists()) {
        await tempWorkspace.delete(recursive: true);
      }
    });

    test('ContentUtils computes words and hash correctly', () {
      final content = "Hello world 这是测试";
      expect(ContentUtils.calculateWordCount(content), 6); // Hello, world, 这, 是, 测, 试
      expect(ContentUtils.calculateHash(content).length, 64);
    });

    test('Complete Workspace Write Loop and Cache Rebuild', () async {
      // 1. Create Workspace Hierarchy
      final workspaceId = 'ws-1';
      final projectId = 'proj-1';
      final volumeId = 'vol-1';

      await workspaceService.createWorkspace(tempWorkspace.path, workspaceId);
      await workspaceService.createProject(tempWorkspace.path, projectId, 'My Book');
      await workspaceService.createVolume(tempWorkspace.path, projectId, volumeId, 'Volume 1');

      // 2. Save a Chapter
      final chapterContent = "This is the very first chapter.";
      final chapter = Chapter(
        id: 'chap-1',
        projectId: projectId,
        volumeId: volumeId,
        title: 'Chapter 1',
        createdAt: DateTime.now(),
        updatedAt: DateTime.now(),
        contentHash: ContentUtils.calculateHash(chapterContent),
        wordCount: ContentUtils.calculateWordCount(chapterContent),
      );

      await chapterRepository.saveChapter(chapter, chapterContent);

      // Wait for write queue to process (in a real app we'd await the Future returned by enqueueWrite)
      await Future.delayed(Duration(milliseconds: 100));

      // 3. Verify Files Written
      final mdPath = '${tempWorkspace.path}/projects/$projectId/volumes/$volumeId/chapters/chap-1.md';
      final metaPath = '${tempWorkspace.path}/projects/$projectId/volumes/$volumeId/chapters/chap-1.meta.json';

      expect(await File(mdPath).exists(), isTrue);
      expect(await File(metaPath).exists(), isTrue);
      expect(await File(mdPath).readAsString(), chapterContent);

      // 4. Rebuild SQLite Cache
      await dbHelper.initDatabase();
      await dbHelper.rebuildCacheFromWorkspace(projectId);

      // 5. Verify SQLite Cache
      final chapters = await dbHelper.getChapters(projectId);
      expect(chapters.length, 1);
      expect(chapters[0]['id'], 'chap-1');
      expect(chapters[0]['word_count'], 6);
    });
  });
}
