import 'dart:io';
import 'package:flutter/foundation.dart';
import 'package:path_provider/path_provider.dart';
import 'package:path/path.dart' as p;

import '../../domain/models/chapter.dart';
import '../../domain/repositories/chapter_repository.dart';
import '../../domain/services_interfaces/backup_service.dart';
import '../background_tasks/file_write_queue.dart';
import '../services/workspace_service.dart';
import '../../infrastructure/database/database_helper.dart';
import '../../infrastructure/repositories/chapter_repository_impl.dart';
import '../../infrastructure/storage/atomic_writer.dart';
import '../../infrastructure/storage/trash_service_impl.dart';
import '../../infrastructure/backup/backup_service_impl.dart';

class WorkspaceController extends ChangeNotifier {
  bool isLoading = true;
  bool isSaving = false;

  String workspacePath = '';
  final String projectId = 'default_project';
  final String volumeId = 'default_volume';

  late DatabaseHelper _dbHelper;
  late IChapterRepository _chapterRepository;
  late IBackupService _backupService;

  List<Chapter> chapters = [];
  Chapter? selectedChapter;
  String currentContent = '';

  Future<void> initWorkspace() async {
    isLoading = true;
    notifyListeners();

    final homeDir = await getApplicationDocumentsDirectory();
    workspacePath = p.join(homeDir.path, 'writer_app_workspace');

    final storageService = AtomicWriter();
    final writeQueue = FileWriteQueue(storageService);
    final workspaceService = WorkspaceService(storageService);
    _backupService = BackupServiceImpl();
    final trashService = TrashServiceImpl();

    _chapterRepository = ChapterRepositoryImpl(
      writeQueue,
      workspacePath,
      trashService,
      _backupService,
    );
    _dbHelper = DatabaseHelper(workspacePath);

    await workspaceService.createWorkspace(workspacePath, 'workspace_1');
    final projectDir = Directory(p.join(workspacePath, 'projects', projectId));
    if (!await projectDir.exists()) {
      await workspaceService.createProject(workspacePath, projectId, '默认项目');
      await workspaceService.createVolume(workspacePath, projectId, volumeId, '默认卷');
    }

    await _dbHelper.initDatabase();
    await _dbHelper.rebuildCacheFromWorkspace(projectId);
    await loadChapters();

    if (chapters.isEmpty) {
      await createNewChapter('第一章');
    }

    isLoading = false;
    if (chapters.isNotEmpty) {
      await selectChapter(chapters.first);
    } else {
      notifyListeners();
    }
  }

  Future<void> loadChapters() async {
    final rawChapters = await _dbHelper.getChapters(projectId);
    final loaded = rawChapters.map((c) => Chapter(
      id: c['id'] as String,
      volumeId: c['volume_id'] as String,
      projectId: c['project_id'] as String,
      title: c['title'] as String,
      createdAt: DateTime.now(), // Approximate MVP
      updatedAt: DateTime.parse(c['updated_at'] as String),
      contentHash: c['content_hash'] as String,
      wordCount: c['word_count'] as int,
    )).toList();

    loaded.sort((a, b) => b.updatedAt.compareTo(a.updatedAt));
    chapters = loaded;
    notifyListeners();
  }

  Future<void> createNewChapter(String title) async {
    final newChapId = 'chap_${DateTime.now().millisecondsSinceEpoch}';
    final newChapter = Chapter(
      id: newChapId,
      volumeId: volumeId,
      projectId: projectId,
      title: title.isEmpty ? '未命名章节' : title,
      createdAt: DateTime.now(),
      updatedAt: DateTime.now(),
      contentHash: '',
      wordCount: 0,
    );

    await _chapterRepository.saveChapter(newChapter, '');

    await _dbHelper.rebuildCacheFromWorkspace(projectId);
    await loadChapters();

    final newlyCreated = chapters.firstWhere((c) => c.id == newChapter.id);
    await selectChapter(newlyCreated);
  }

  Future<void> selectChapter(Chapter chapter) async {
    final content = await _chapterRepository.readChapterContent(chapter);
    selectedChapter = chapter;
    currentContent = content;
    notifyListeners();
  }

  Future<void> saveCurrentChapter(String contentToSave) async {
    if (selectedChapter == null) return;

    isSaving = true;
    notifyListeners();

    try {
      await _chapterRepository.saveChapter(selectedChapter!, contentToSave);

      await _dbHelper.rebuildCacheFromWorkspace(projectId);
      await loadChapters();

      selectedChapter = chapters.firstWhere((c) => c.id == selectedChapter!.id);
      currentContent = contentToSave;
    } finally {
      isSaving = false;
      notifyListeners();
    }
  }

  Future<void> deleteCurrentChapter() async {
    if (selectedChapter == null) return;

    await _chapterRepository.deleteChapter(selectedChapter!);
    await _dbHelper.rebuildCacheFromWorkspace(projectId);
    await loadChapters();

    if (chapters.isNotEmpty) {
      await selectChapter(chapters.first);
    } else {
      selectedChapter = null;
      currentContent = '';
      notifyListeners();
    }
  }

  Future<void> backupProject() async {
    await _backupService.backupProject(workspacePath, projectId);
  }
}
