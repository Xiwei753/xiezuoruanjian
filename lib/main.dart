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
    // 1. Determine workspace path
    final homeDir = await getApplicationDocumentsDirectory();
    _workspacePath = p.join(homeDir.path, 'writer_app_workspace');

    // 2. Initialize Services
    final storageService = AtomicWriter();
    final writeQueue = FileWriteQueue(storageService);
    final workspaceService = WorkspaceService(storageService);

    _chapterRepository = ChapterRepositoryImpl(writeQueue, _workspacePath);
    _dbHelper = DatabaseHelper(_workspacePath);

    // 3. Create defaults if missing
    await workspaceService.createWorkspace(_workspacePath, 'workspace_1');
    final projectDir = Directory(p.join(_workspacePath, 'projects', _projectId));
    if (!await projectDir.exists()) {
      await workspaceService.createProject(_workspacePath, _projectId, '默认项目');
      await workspaceService.createVolume(_workspacePath, _projectId, _volumeId, '默认卷');
    }

    // 4. Rebuild Cache and Load Chapters
    await _dbHelper.initDatabase();
    await _dbHelper.rebuildCacheFromWorkspace(_projectId);
    await _loadChapters();

    // 5. If completely empty, create first chapter
    if (_chapters.isEmpty) {
      await _createNewChapter();
    }

    setState(() {
      _isLoading = false;
      _selectChapter(_chapters.first);
    });
  }

  Future<void> _loadChapters() async {
    final rawChapters = await _dbHelper.getChapters(_projectId);
    setState(() {
      _chapters = rawChapters.map((c) => Chapter(
        id: c['id'] as String,
        volumeId: c['volume_id'] as String,
        projectId: c['project_id'] as String,
        title: c['title'] as String,
        createdAt: DateTime.tryParse(c['updated_at'] as String? ?? '') ?? DateTime.now(), // approximation for MVP
        updatedAt: DateTime.parse(c['updated_at'] as String),
        contentHash: c['content_hash'] as String,
        wordCount: c['word_count'] as int,
      )).toList();
    });
  }

  Future<void> _createNewChapter() async {
    final newChapId = 'chap_${DateTime.now().millisecondsSinceEpoch}';
    final newChapter = Chapter(
      id: newChapId,
      volumeId: _volumeId,
      projectId: _projectId,
      title: '未命名章节',
      createdAt: DateTime.now(),
      updatedAt: DateTime.now(),
      contentHash: '',
      wordCount: 0,
    );

    await _chapterRepository.saveChapter(newChapter, '');

    // Rebuild cache to pick up new chapter
    await _dbHelper.rebuildCacheFromWorkspace(_projectId);
    await _loadChapters();
  }

  Future<void> _selectChapter(Chapter chapter) async {
    setState(() {
      _selectedChapter = chapter;
    });

    final content = await _chapterRepository.readChapterContent(chapter);
    _currentContent = content;
    _textController.text = content;
  }

  Future<void> _saveCurrentChapter() async {
    if (_selectedChapter == null) return;

    setState(() {
      _isSaving = true;
    });

    final contentToSave = _textController.text;
    await _chapterRepository.saveChapter(_selectedChapter!, contentToSave);

    // Rebuild and reload to get updated word count/hash from cache
    await _dbHelper.rebuildCacheFromWorkspace(_projectId);
    await _loadChapters();

    // Update selected chapter reference
    final updatedChap = _chapters.firstWhere((c) => c.id == _selectedChapter!.id);

    setState(() {
      _selectedChapter = updatedChap;
      _currentContent = contentToSave;
      _isSaving = false;
    });
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
            icon: const Icon(Icons.save),
            onPressed: _isSaving ? null : _saveCurrentChapter,
            tooltip: '保存当前章节',
          )
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
                    onPressed: _createNewChapter,
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
              child: TextField(
                controller: _textController,
                maxLines: null,
                expands: true,
                decoration: const InputDecoration(
                  border: InputBorder.none,
                  hintText: '开始你的创作...',
                ),
                style: const TextStyle(fontSize: 16, height: 1.6),
                onChanged: (val) {
                   _currentContent = val;
                },
              ),
            ),
          ),
          const VerticalDivider(width: 1),

          // Right Column: Info Panel
          SizedBox(
            width: 250,
            child: Padding(
              padding: const EdgeInsets.all(16.0),
              child: _selectedChapter == null ? const SizedBox() : Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text('章节信息', style: Theme.of(context).textTheme.titleLarge),
                  const SizedBox(height: 16),
                  Text('标题: ${_selectedChapter!.title}'),
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
                ],
              ),
            ),
          ),
        ],
      ),
    );
  }
}
