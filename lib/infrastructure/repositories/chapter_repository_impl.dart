import 'dart:convert';
import 'package:path/path.dart' as p;

import 'dart:io';

import '../../core/utils/content_utils.dart';
import '../../domain/models/chapter.dart';
import '../../domain/models/manifests.dart';
import '../../domain/repositories/chapter_repository.dart';
import '../../domain/services_interfaces/trash_service.dart';
import '../../domain/services_interfaces/backup_service.dart';
import '../../application/background_tasks/file_write_queue.dart';

class ChapterRepositoryImpl implements IChapterRepository {
  final FileWriteQueue _writeQueue;
  final String _workspaceRoot;
  final ITrashService _trashService;
  final IBackupService _backupService;

  ChapterRepositoryImpl(
    this._writeQueue,
    this._workspaceRoot,
    this._trashService,
    this._backupService,
  );

  @override
  Future<void> saveChapter(Chapter chapter, String content) async {
    final chaptersDir = p.join(
      _workspaceRoot,
      'projects',
      chapter.projectId,
      'volumes',
      chapter.volumeId,
      'chapters',
    );

    final mdPath = p.join(chaptersDir, '${chapter.id}.md');
    final metaPath = p.join(chaptersDir, '${chapter.id}.meta.json');

    // 1. Enqueue Markdown write
    await _writeQueue.enqueueWrite(mdPath, content);

    // 2. Recalculate hash and word count from raw content (Do not trust caller)
    final realHash = ContentUtils.calculateHash(content);
    final realWordCount = ContentUtils.calculateWordCount(content);

    // 3. Generate and Enqueue Meta write
    final meta = ChapterMeta(
      id: chapter.id,
      volumeId: chapter.volumeId,
      projectId: chapter.projectId,
      title: chapter.title,
      createdAt: chapter.createdAt,
      updatedAt: DateTime.now(),
      contentHash: realHash,
      wordCount: realWordCount,
    );

    await _writeQueue.enqueueWrite(metaPath, jsonEncode(meta.toJson()));
  }

  @override
  Future<String> readChapterContent(Chapter chapter) async {
    final chaptersDir = p.join(
      _workspaceRoot,
      'projects',
      chapter.projectId,
      'volumes',
      chapter.volumeId,
      'chapters',
    );
    final mdPath = p.join(chaptersDir, '${chapter.id}.md');
    final file = File(mdPath);
    if (await file.exists()) {
      return await file.readAsString();
    }
    return '';
  }

  @override
  Future<void> deleteChapter(Chapter chapter) async {
    final chaptersDir = p.join(
      _workspaceRoot,
      'projects',
      chapter.projectId,
      'volumes',
      chapter.volumeId,
      'chapters',
    );
    final mdPath = p.join(chaptersDir, '${chapter.id}.md');
    final metaPath = p.join(chaptersDir, '${chapter.id}.meta.json');

    // 1. Backup single files before deletion as a safeguard
    await _backupService.backupFile(_workspaceRoot, mdPath);
    await _backupService.backupFile(_workspaceRoot, metaPath);

    // 2. Move to trash instead of physical delete
    await _trashService.moveToTrash(_workspaceRoot, mdPath);
    await _trashService.moveToTrash(_workspaceRoot, metaPath);
  }

  @override
  Future<void> rebuildIndexFromWorkspace(String projectId) async {
    // To be implemented via DatabaseHelper
    throw UnimplementedError();
  }
}
