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
import 'package:writer_app/infrastructure/storage/trash_service_impl.dart';
import 'package:writer_app/infrastructure/backup/backup_service_impl.dart';

void main() {
  setUpAll(() {
    sqfliteFfiInit();
    databaseFactory = databaseFactoryFfi;
  });

  group('Life Saving Layer V2 Tests', () {
    late Directory tempWorkspace;
    late AtomicWriter storageService;
    late FileWriteQueue writeQueue;
    late WorkspaceService workspaceService;
    late ChapterRepositoryImpl chapterRepository;
    late DatabaseHelper dbHelper;
    late TrashServiceImpl trashService;
    late BackupServiceImpl backupService;

    setUp(() async {
      tempWorkspace = await Directory.systemTemp.createTemp('writer_app_test_v2_');
      storageService = AtomicWriter();
      writeQueue = FileWriteQueue(storageService);
      workspaceService = WorkspaceService(storageService);
      trashService = TrashServiceImpl();
      backupService = BackupServiceImpl();
      chapterRepository = ChapterRepositoryImpl(
        writeQueue,
        tempWorkspace.path,
        trashService,
        backupService,
      );
      dbHelper = DatabaseHelper(tempWorkspace.path);
    });

    tearDown(() async {
      if (await tempWorkspace.exists()) {
        await tempWorkspace.delete(recursive: true);
      }
    });

    test('ChapterRepository recalculates hash and word count regardless of input', () async {
      final projectId = 'proj-2';
      final volumeId = 'vol-2';
      await workspaceService.createWorkspace(tempWorkspace.path, 'ws-2');
      await workspaceService.createProject(tempWorkspace.path, projectId, 'Proj 2');
      await workspaceService.createVolume(tempWorkspace.path, projectId, volumeId, 'Vol 2');

      final fakeHash = "fakeHash";
      final fakeCount = 999;

      final chapterContent = "Real Content Here";
      final realHash = ContentUtils.calculateHash(chapterContent);
      final realCount = ContentUtils.calculateWordCount(chapterContent);

      final chapter = Chapter(
        id: 'chap-2',
        projectId: projectId,
        volumeId: volumeId,
        title: 'Chap 2',
        createdAt: DateTime.now(),
        updatedAt: DateTime.now(),
        contentHash: fakeHash, // Intentionally wrong
        wordCount: fakeCount,  // Intentionally wrong
      );

      await chapterRepository.saveChapter(chapter, chapterContent);
      await Future.delayed(Duration(milliseconds: 100)); // wait for write queue

      final metaPath = '${tempWorkspace.path}/projects/$projectId/volumes/$volumeId/chapters/chap-2.meta.json';
      final metaContent = await File(metaPath).readAsString();

      expect(metaContent.contains(fakeHash), isFalse);
      expect(metaContent.contains(realHash), isTrue);
      expect(metaContent.contains('"$fakeCount"'), isFalse);
    });

    test('TrashService moves files instead of deleting', () async {
      final filePath = '${tempWorkspace.path}/test_file.txt';
      await File(filePath).writeAsString('To be deleted');

      await trashService.moveToTrash(tempWorkspace.path, filePath);

      expect(await File(filePath).exists(), isFalse);

      final trashDir = Directory('${tempWorkspace.path}/trash');
      expect(await trashDir.exists(), isTrue);

      final trashFiles = await trashDir.list().toList();
      expect(trashFiles.length, 1);
      expect(trashFiles.first.path.endsWith('test_file.txt'), isTrue);
    });

    test('DatabaseHelper requires paired MD and JSON to cache chapter', () async {
      final projectId = 'proj-3';
      final volumeId = 'vol-3';
      await workspaceService.createWorkspace(tempWorkspace.path, 'ws-3');
      await workspaceService.createProject(tempWorkspace.path, projectId, 'Proj 3');
      await workspaceService.createVolume(tempWorkspace.path, projectId, volumeId, 'Vol 3');

      final chapDir = '${tempWorkspace.path}/projects/$projectId/volumes/$volumeId/chapters';

      // Create ONLY meta json, no MD file
      await File('$chapDir/chap-3.meta.json').writeAsString('{"id":"chap-3", "projectId":"$projectId", "volumeId":"$volumeId", "title":"Chap 3", "createdAt":"2023-01-01T00:00:00.000", "updatedAt":"2023-01-01T00:00:00.000", "contentHash":"hash", "wordCount":0}');

      await dbHelper.initDatabase();
      await dbHelper.rebuildCacheFromWorkspace(projectId);

      final chapters = await dbHelper.getChapters(projectId);
      expect(chapters.isEmpty, isTrue, reason: 'Should ignore chapter without paired MD file');
    });
  });
}
