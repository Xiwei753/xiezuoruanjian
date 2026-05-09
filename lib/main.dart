import 'dart:io';
import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';
import 'package:path/path.dart' as p;
import 'package:sqflite_common_ffi/sqflite_ffi.dart';

import 'application/background_tasks/file_write_queue.dart';
import 'application/services/workspace_service.dart';
import 'domain/models/chapter.dart';
import 'infrastructure/database/database_helper.dart';
import 'infrastructure/repositories/chapter_repository_impl.dart';
import 'infrastructure/storage/atomic_writer.dart';
import 'infrastructure/storage/trash_service_impl.dart';
import 'infrastructure/backup/backup_service_impl.dart';

void main() async {
  WidgetsFlutterBinding.ensureInitialized();

  if (Platform.isWindows || Platform.isLinux) {
    sqfliteFfiInit();
    databaseFactory = databaseFactoryFfi;
  }

  runApp(const WriterApp());
}

class WriterApp extends StatelessWidget {
  const WriterApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Writer App',
      theme: ThemeData(
        colorScheme: ColorScheme.fromSeed(seedColor: Colors.deepPurple, brightness: Brightness.dark),
        useMaterial3: true,
      ),
      home: const WorkspaceScreen(),
    );
  }
}

class WorkspaceScreen extends StatefulWidget {
  const WorkspaceScreen({super.key});

  @override
  State<WorkspaceScreen> createState() => _WorkspaceScreenState();
}

class _WorkspaceScreenState extends State<WorkspaceScreen> {
  bool _isLoading = true;
  String _workspacePath = '';
  final String _projectId = 'default_project';
  final String _volumeId = 'default_volume';

  late DatabaseHelper _dbHelper;
  late ChapterRepositoryImpl _chapterRepository;
  late BackupServiceImpl _backupService;

  List<Chapter> _chapters = [];
  Chapter? _selectedChapter;
  String _currentContent = '';
  bool _isSaving = false;

  final TextEditingController _textController = TextEditingController();

  @override
  void initState() {
    super.initState();
    _initWorkspace();
  }

  Future<void> _initWorkspace() async {
    final homeDir = await getApplicationDocumentsDirectory();
    _workspacePath = p.join(homeDir.path, 'writer_app_workspace');

    final storageService = AtomicWriter();
    final writeQueue = FileWriteQueue(storageService);
    final workspaceService = WorkspaceService(storageService);
    _backupService = BackupServiceImpl();
    final trashService = TrashServiceImpl();

    _chapterRepository = ChapterRepositoryImpl(
      writeQueue,
      _workspacePath,
      trashService,
      _backupService,
    );
    _dbHelper = DatabaseHelper(_workspacePath);

    await workspaceService.createWorkspace(_workspacePath, 'workspace_1');
    final projectDir = Directory(p.join(_workspacePath, 'projects', _projectId));
    if (!await projectDir.exists()) {
      await workspaceService.createProject(_workspacePath, _projectId, '默认项目');
      await workspaceService.createVolume(_workspacePath, _projectId, _volumeId, '默认卷');
    }

    await _dbHelper.initDatabase();
    await _dbHelper.rebuildCacheFromWorkspace(_projectId);
    await _loadChapters();

    if (_chapters.isEmpty) {
      await _createNewChapter('第一章');
    }

    setState(() {
      _isLoading = false;
      if (_chapters.isNotEmpty) {
        _selectChapter(_chapters.first);
      }
    });
  }

  Future<void> _loadChapters() async {
    final rawChapters = await _dbHelper.getChapters(_projectId);
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

    // Sort by updatedAt descending
    loaded.sort((a, b) => b.updatedAt.compareTo(a.updatedAt));

    setState(() {
      _chapters = loaded;
    });
  }

  Future<void> _createNewChapter(String title) async {
    final newChapId = 'chap_${DateTime.now().millisecondsSinceEpoch}';
    final newChapter = Chapter(
      id: newChapId,
      volumeId: _volumeId,
      projectId: _projectId,
      title: title.isEmpty ? '未命名章节' : title,
      createdAt: DateTime.now(),
      updatedAt: DateTime.now(),
      contentHash: '',
      wordCount: 0,
    );

    await _chapterRepository.saveChapter(newChapter, '');

    await _dbHelper.rebuildCacheFromWorkspace(_projectId);
    await _loadChapters();

    final newlyCreated = _chapters.firstWhere((c) => c.id == newChapter.id);
    _selectChapter(newlyCreated);
  }

  Future<void> _showNewChapterDialog() async {
    String inputTitle = '';
    final result = await showDialog<String>(
      context: context,
      builder: (context) {
        return AlertDialog(
          title: const Text('新建章节'),
          content: TextField(
            autofocus: true,
            decoration: const InputDecoration(hintText: '章节标题'),
            onChanged: (val) => inputTitle = val,
          ),
          actions: [
            TextButton(onPressed: () => Navigator.pop(context), child: const Text('取消')),
            TextButton(onPressed: () => Navigator.pop(context, inputTitle), child: const Text('创建')),
          ],
        );
      }
    );

    if (result != null) {
      await _createNewChapter(result);
    }
  }

  Future<void> _showEditTitleDialog() async {
    if (_selectedChapter == null) return;
    String inputTitle = _selectedChapter!.title;
    final result = await showDialog<String>(
      context: context,
      builder: (context) {
        return AlertDialog(
          title: const Text('修改标题'),
          content: TextFormField(
            initialValue: inputTitle,
            autofocus: true,
            decoration: const InputDecoration(hintText: '新标题'),
            onChanged: (val) => inputTitle = val,
          ),
          actions: [
            TextButton(onPressed: () => Navigator.pop(context), child: const Text('取消')),
            TextButton(onPressed: () => Navigator.pop(context, inputTitle), child: const Text('保存')),
          ],
        );
      }
    );

    if (result != null && result.isNotEmpty && result != _selectedChapter!.title) {
      final updatedChapter = _selectedChapter!.copyWith(title: result);
      await _chapterRepository.saveChapter(updatedChapter, _textController.text);
      await _dbHelper.rebuildCacheFromWorkspace(_projectId);
      await _loadChapters();

      if (mounted) {
        setState(() {
          _selectedChapter = _chapters.firstWhere((c) => c.id == updatedChapter.id);
        });
      }
    }
  }

  Future<bool> _promptUnsavedChanges() async {
    if (_selectedChapter == null || _currentContent == _textController.text) {
      return true;
    }

    final result = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('未保存的更改'),
        content: const Text('当前章节有未保存的内容，是否放弃更改？'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context, false), child: const Text('取消')),
          TextButton(onPressed: () => Navigator.pop(context, true), child: const Text('放弃更改', style: TextStyle(color: Colors.red))),
          TextButton(
            onPressed: () async {
              await _saveCurrentChapter();
              if (context.mounted) {
                Navigator.pop(context, true);
              }
            },
            child: const Text('保存')
          ),
        ],
      )
    );

    return result ?? false;
  }

  Future<void> _selectChapter(Chapter chapter) async {
    if (!await _promptUnsavedChanges()) return;

    final content = await _chapterRepository.readChapterContent(chapter);
    setState(() {
      _selectedChapter = chapter;
      _currentContent = content;
      _textController.text = content;
    });
  }

  Future<void> _saveCurrentChapter() async {
    if (_selectedChapter == null) return;

    setState(() {
      _isSaving = true;
    });

    try {
      final contentToSave = _textController.text;
      await _chapterRepository.saveChapter(_selectedChapter!, contentToSave);

      await _dbHelper.rebuildCacheFromWorkspace(_projectId);
      await _loadChapters();

      setState(() {
        _selectedChapter = _chapters.firstWhere((c) => c.id == _selectedChapter!.id);
        _currentContent = contentToSave;
      });
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('保存失败: $e')));
      }
    } finally {
      if (mounted) {
        setState(() {
          _isSaving = false;
        });
      }
    }
  }

  Future<void> _deleteCurrentChapter() async {
    if (_selectedChapter == null) return;

    final confirm = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('删除章节'),
        content: Text('确定要删除《${_selectedChapter!.title}》吗？\n文件将被移动到回收站。'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context, false), child: const Text('取消')),
          TextButton(onPressed: () => Navigator.pop(context, true), child: const Text('确认删除', style: TextStyle(color: Colors.red))),
        ],
      )
    );

    if (confirm != true) return;

    try {
      await _chapterRepository.deleteChapter(_selectedChapter!);
      await _dbHelper.rebuildCacheFromWorkspace(_projectId);
      await _loadChapters();

      setState(() {
        if (_chapters.isNotEmpty) {
          _selectChapter(_chapters.first);
        } else {
          _selectedChapter = null;
          _textController.clear();
          _currentContent = '';
        }
      });
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('删除失败: $e')));
      }
    }
  }

  Future<void> _backupProject() async {
    try {
      await _backupService.backupProject(_workspacePath, _projectId);
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(const SnackBar(content: Text('项目备份成功！')));
      }
    } catch (e) {
      if (mounted) {
        ScaffoldMessenger.of(context).showSnackBar(SnackBar(content: Text('备份失败: $e')));
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_isLoading) {
      return const Scaffold(body: Center(child: CircularProgressIndicator()));
    }

    return Scaffold(
      appBar: AppBar(
        title: const Text('Writer App (Local First MVP)'),
        actions: [
          IconButton(
            icon: const Icon(Icons.backup),
            onPressed: _backupProject,
            tooltip: '备份当前项目',
          ),
          IconButton(
            icon: const Icon(Icons.save),
            onPressed: _isSaving ? null : _saveCurrentChapter,
            tooltip: '保存当前章节',
          ),
          if (_selectedChapter != null)
            IconButton(
              icon: const Icon(Icons.delete, color: Colors.redAccent),
              onPressed: _deleteCurrentChapter,
              tooltip: '删除当前章节',
            ),
        ],
      ),
      body: Row(
        children: [
          // Left Column: List
          SizedBox(
            width: 250,
            child: Column(
              children: [
                ListTile(
                  title: const Text('默认项目 / 默认卷', style: TextStyle(fontWeight: FontWeight.bold)),
                  trailing: IconButton(
                    icon: const Icon(Icons.add),
                    onPressed: _showNewChapterDialog,
                  ),
                ),
                const Divider(),
                Expanded(
                  child: ListView.builder(
                    itemCount: _chapters.length,
                    itemBuilder: (context, index) {
                      final chap = _chapters[index];
                      final isSelected = _selectedChapter?.id == chap.id;
                      return ListTile(
                        selected: isSelected,
                        title: Text(chap.title),
                        subtitle: Text('${chap.wordCount} 字'),
                        onTap: () => _selectChapter(chap),
                      );
                    },
                  ),
                )
              ],
            ),
          ),
          const VerticalDivider(width: 1),

          // Middle Column: Editor
          Expanded(
            flex: 3,
            child: Padding(
              padding: const EdgeInsets.all(16.0),
              child: _selectedChapter == null
                ? const Center(child: Text('无章节，请新建或选择章节。'))
                : TextField(
                    controller: _textController,
                    maxLines: null,
                    expands: true,
                    decoration: const InputDecoration(
                      border: InputBorder.none,
                      hintText: '开始你的创作...',
                    ),
                    style: const TextStyle(fontSize: 16, height: 1.6),
                  ),
            ),
          ),
          const VerticalDivider(width: 1),

          // Right Column: Info Panel
          SizedBox(
            width: 250,
            child: Padding(
              padding: const EdgeInsets.all(16.0),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text('工作区', style: Theme.of(context).textTheme.titleSmall),
                  Text(_workspacePath, style: const TextStyle(fontSize: 10, color: Colors.grey)),
                  const SizedBox(height: 24),
                  if (_selectedChapter != null) ...[
                    Row(
                      mainAxisAlignment: MainAxisAlignment.spaceBetween,
                      children: [
                        Text('章节信息', style: Theme.of(context).textTheme.titleLarge),
                        IconButton(
                          icon: const Icon(Icons.edit, size: 16),
                          onPressed: _showEditTitleDialog,
                        )
                      ],
                    ),
                    const SizedBox(height: 16),
                    Text('标题: ${_selectedChapter!.title}', style: const TextStyle(fontWeight: FontWeight.bold)),
                    const SizedBox(height: 8),
                    Text('字数: ${_selectedChapter!.wordCount}'),
                    const SizedBox(height: 8),
                    Text('Hash:'),
                    Text(_selectedChapter!.contentHash.isEmpty ? 'N/A' : '${_selectedChapter!.contentHash.substring(0, 16)}...', style: const TextStyle(fontSize: 12, color: Colors.grey)),
                    const SizedBox(height: 24),
                    Row(
                      children: [
                        Icon(
                          _isSaving ? Icons.sync : Icons.check_circle,
                          color: _isSaving ? Colors.orange : Colors.green,
                          size: 16
                        ),
                        const SizedBox(width: 8),
                        Text(_isSaving ? '保存中...' : '已保存'),
                      ],
                    )
                  ]
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}
