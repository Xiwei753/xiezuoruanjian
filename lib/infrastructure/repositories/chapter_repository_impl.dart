import 'dart:convert';
import 'package:path/path.dart' as p;

import '../../domain/models/chapter.dart';
import '../../domain/models/manifests.dart';
import '../../domain/repositories/chapter_repository.dart';
import '../../application/background_tasks/file_write_queue.dart';

class ChapterRepositoryImpl implements IChapterRepository {
  final FileWriteQueue _writeQueue;
  final String _workspaceRoot;

  ChapterRepositoryImpl(this._writeQueue, this._workspaceRoot);

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

    // 2. Generate and Enqueue Meta write
    final meta = ChapterMeta(
      id: chapter.id,
      volumeId: chapter.volumeId,
      projectId: chapter.projectId,
      title: chapter.title,
      createdAt: chapter.createdAt,
      updatedAt: chapter.updatedAt,
      contentHash: chapter.contentHash,
      wordCount: chapter.wordCount,
    );

    await _writeQueue.enqueueWrite(metaPath, jsonEncode(meta.toJson()));
  }

  @override
  Future<void> rebuildIndexFromWorkspace(String projectId) async {
    // To be implemented in next step via DatabaseHelper
    throw UnimplementedError();
  }
}
