import 'dart:convert';
import 'dart:io';
import 'package:path/path.dart' as p;
import 'package:sqflite/sqflite.dart';

import '../../domain/models/manifests.dart';

class DatabaseHelper {
  final String _workspaceRoot;
  Database? _db;

  DatabaseHelper(this._workspaceRoot);

  Future<void> initDatabase() async {
    final dbPath = p.join(_workspaceRoot, 'sqlite_cache', 'index.db');

    // In tests using sqflite_common_ffi, databaseFactory is overridden.
    _db = await databaseFactory.openDatabase(
      dbPath,
      options: OpenDatabaseOptions(
        version: 1,
        onCreate: (db, version) async {
          await db.execute('''
            CREATE TABLE projects_cache (
              id TEXT PRIMARY KEY,
              title TEXT,
              created_at TEXT
            )
          ''');
          await db.execute('''
            CREATE TABLE volumes_cache (
              id TEXT PRIMARY KEY,
              project_id TEXT,
              title TEXT,
              created_at TEXT
            )
          ''');
          await db.execute('''
            CREATE TABLE chapters_cache (
              id TEXT PRIMARY KEY,
              volume_id TEXT,
              project_id TEXT,
              title TEXT,
              word_count INTEGER,
              updated_at TEXT,
              content_hash TEXT
            )
          ''');
        },
      ),
    );
  }

  Future<void> rebuildCacheFromWorkspace(String projectId) async {
    if (_db == null) throw StateError("Database not initialized");

    final batch = _db!.batch();

    // 1. Clear existing cache for this project
    batch.delete('projects_cache', where: 'id = ?', whereArgs: [projectId]);
    batch.delete(
      'volumes_cache',
      where: 'project_id = ?',
      whereArgs: [projectId],
    );
    batch.delete(
      'chapters_cache',
      where: 'project_id = ?',
      whereArgs: [projectId],
    );

    // 2. Read project.json
    final projectFile = File(
      p.join(_workspaceRoot, 'projects', projectId, 'project.json'),
    );
    if (await projectFile.exists()) {
      final pMeta = ProjectManifest.fromJson(
        jsonDecode(await projectFile.readAsString()),
      );
      batch.insert('projects_cache', {
        'id': pMeta.id,
        'title': pMeta.title,
        'created_at': pMeta.createdAt.toIso8601String(),
      });
    }

    // 3. Read file system
    final volumesDir = Directory(
      p.join(_workspaceRoot, 'projects', projectId, 'volumes'),
    );
    if (!await volumesDir.exists()) return;

    final volumeEntities = await volumesDir.list().toList();
    for (var vEntity in volumeEntities) {
      if (vEntity is Directory) {
        // Read volume.json
        final volFile = File(p.join(vEntity.path, 'volume.json'));
        if (await volFile.exists()) {
          final vMeta = VolumeMeta.fromJson(
            jsonDecode(await volFile.readAsString()),
          );
          batch.insert('volumes_cache', {
            'id': vMeta.id,
            'project_id': projectId,
            'title': vMeta.title,
            'created_at': vMeta.createdAt.toIso8601String(),
          });
        }

        final chaptersDir = Directory(p.join(vEntity.path, 'chapters'));
        if (!await chaptersDir.exists()) continue;

        final chapterEntities = await chaptersDir.list().toList();
        for (var cEntity in chapterEntities) {
          if (cEntity is File && cEntity.path.endsWith('.meta.json')) {
            final mdFile = File(cEntity.path.replaceAll('.meta.json', '.md'));

            // Critical: Ensure BOTH files exist before caching
            if (!await mdFile.exists()) continue;

            final content = await cEntity.readAsString();
            final metaJson = jsonDecode(content);
            final meta = ChapterMeta.fromJson(metaJson);

            batch.insert('chapters_cache', {
              'id': meta.id,
              'volume_id': meta.volumeId,
              'project_id': meta.projectId,
              'title': meta.title,
              'word_count': meta.wordCount,
              'updated_at': meta.updatedAt.toIso8601String(),
              'content_hash': meta.contentHash,
            });
          }
        }
      }
    }

    // 3. Execute
    await batch.commit(noResult: true);
  }

  Future<List<Map<String, dynamic>>> getChapters(String projectId) async {
    if (_db == null) throw StateError("Database not initialized");
    return await _db!.query(
      'chapters_cache',
      where: 'project_id = ?',
      whereArgs: [projectId],
    );
  }

  Future<Map<String, dynamic>?> getChapter(String chapterId) async {
    if (_db == null) throw StateError("Database not initialized");
    final results = await _db!.query(
      'chapters_cache',
      where: 'id = ?',
      whereArgs: [chapterId],
      limit: 1,
    );
    if (results.isNotEmpty) {
      return results.first;
    }
    return null;
  }
}
