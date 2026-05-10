import 'dart:async';
import 'package:flutter/material.dart';
import '../../application/controllers/workspace_controller.dart';
import '../../application/controllers/settings_controller.dart';
import '../../domain/models/chapter.dart';
import '../dialogs/chapter_title_dialog.dart';
import '../dialogs/settings_dialog.dart';
import '../widgets/chapter_list_panel.dart';
import '../widgets/editor_panel.dart';
import '../widgets/chapter_info_panel.dart';
import 'project_home_screen.dart';

class WorkspaceScreen extends StatefulWidget {
  final SettingsController settingsController;
  final String projectId;

  const WorkspaceScreen({
    super.key,
    required this.settingsController,
    required this.projectId,
  });

  @override
  State<WorkspaceScreen> createState() => _WorkspaceScreenState();
}

class _WorkspaceScreenState extends State<WorkspaceScreen> {
  final WorkspaceController _controller = WorkspaceController();
  final TextEditingController _textController = TextEditingController();
  final ScrollController _scrollController = ScrollController();
  String? _currentChapterId;

  Timer? _stateDebounceTimer;
  bool _isRestoringState = false;

  @override
  void initState() {
    super.initState();
    _controller.addListener(_onControllerUpdate);

    _textController.addListener(_onEditorStateChanged);
    _scrollController.addListener(_onEditorStateChanged);

    final lastOpenedChapterId =
        widget.settingsController.localSettings.lastOpenedChapterId;
    _controller.initWorkspace(
      widget.projectId,
      lastOpenedChapterId: lastOpenedChapterId,
    );
  }

  @override
  void dispose() {
    _stateDebounceTimer?.cancel();
    _controller.removeListener(_onControllerUpdate);
    _controller.dispose();
    _textController.dispose();
    _scrollController.dispose();
    super.dispose();
  }

  void _debounceSaveLocalSettings() {
    final localSettings = widget.settingsController.localSettings;
    final newSettings = localSettings.copyWith(
      lastCursorOffset: _textController.selection.baseOffset,
      lastSelectionBaseOffset: _textController.selection.baseOffset,
      lastSelectionExtentOffset: _textController.selection.extentOffset,
      lastScrollOffset: _scrollController.hasClients
          ? _scrollController.offset
          : 0.0,
      lastEditorStateUpdatedAt: DateTime.now(),
    );
    widget.settingsController.updateLocalSettings(newSettings);
    // Use the explicit saveLocal method which strictly only saves local settings
    widget.settingsController.saveLocal();
  }

  void _onEditorStateChanged() {
    if (_isRestoringState || _controller.selectedChapter == null) return;

    _stateDebounceTimer?.cancel();
    _stateDebounceTimer = Timer(const Duration(milliseconds: 500), () {
      if (!mounted) return;
      _debounceSaveLocalSettings();
    });
  }

  void _restoreEditorState() {
    final localSettings = widget.settingsController.localSettings;
    if (localSettings.lastOpenedChapterId == _currentChapterId) {
      final contentLen = _controller.currentContent.length;

      // Clamp selection
      var base = localSettings.lastSelectionBaseOffset;
      var extent = localSettings.lastSelectionExtentOffset;

      if (base >= 0 && extent >= 0) {
        if (base > contentLen) base = contentLen;
        if (extent > contentLen) extent = contentLen;

        _textController.selection = TextSelection(
          baseOffset: base,
          extentOffset: extent,
        );
      } else if (localSettings.lastCursorOffset >= 0) {
        var offset = localSettings.lastCursorOffset;
        if (offset > contentLen) offset = contentLen;
        _textController.selection = TextSelection.collapsed(offset: offset);
      }

      // Restore scroll
      if (localSettings.lastScrollOffset > 0 && _scrollController.hasClients) {
        WidgetsBinding.instance.addPostFrameCallback((_) {
          if (_scrollController.hasClients) {
            final maxScroll = _scrollController.position.maxScrollExtent;
            var targetScroll = localSettings.lastScrollOffset;
            if (targetScroll > maxScroll) targetScroll = maxScroll;
            _scrollController.jumpTo(targetScroll);
          }
        });
      }
    }
  }

  void _onControllerUpdate() {
    if (mounted) {
      bool shouldRebuild = false;

      // Only set text when switching to a DIFFERENT chapter
      if (_currentChapterId != _controller.selectedChapter?.id) {
        _currentChapterId = _controller.selectedChapter?.id;
        if (!(_controller.isSaving)) {
          _isRestoringState = true;
          _textController.text = _controller.currentContent;

          // Restore state if available for this chapter
          _restoreEditorState();

          _isRestoringState = false;
        }
        shouldRebuild = true;
      }

      // Sync workspace path to settings controller when loaded
      if (!_controller.isLoading &&
          _controller.workspacePath.isNotEmpty &&
          widget.settingsController.workspacePath !=
              _controller.workspacePath) {
        widget.settingsController.initWithWorkspacePath(
          _controller.workspacePath,
        );
      }

      // Only force setState if it's not a generic text keystroke notify
      if (shouldRebuild || _controller.isSaving || _controller.isLoading) {
        setState(() {});
      } else {
        setState(() {});
      }
    }
  }

  Future<void> _showNewChapterDialog() async {
    final result = await showDialog<String>(
      context: context,
      builder: (context) => const ChapterTitleDialog(title: '新建章节'),
    );

    if (result != null) {
      try {
        await _controller.createNewChapter(result);
      } catch (e) {
        _showError('创建章节失败: $e');
      }
    }
  }

  Future<void> _showEditTitleDialog() async {
    if (_controller.selectedChapter == null) return;

    final result = await showDialog<String>(
      context: context,
      builder: (context) => ChapterTitleDialog(
        title: '修改标题',
        initialValue: _controller.selectedChapter!.title,
        confirmText: '保存',
      ),
    );

    if (result != null &&
        result.isNotEmpty &&
        result != _controller.selectedChapter!.title) {
      final updatedChapter = _controller.selectedChapter!.copyWith(
        title: result,
      );
      _controller.selectedChapter = updatedChapter;
      await _saveCurrentChapter();
    }
  }

  Future<bool> _promptUnsavedChanges() async {
    if (_controller.selectedChapter == null ||
        _controller.currentContent == _textController.text) {
      return true;
    }

    final result = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('未保存的更改'),
        content: const Text('当前章节有未保存的内容，是否放弃更改？'),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('取消'),
          ),
          TextButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('放弃更改', style: TextStyle(color: Colors.red)),
          ),
          TextButton(
            onPressed: () async {
              await _saveCurrentChapter();
              if (context.mounted) {
                Navigator.pop(context, true);
              }
            },
            child: const Text('保存'),
          ),
        ],
      ),
    );

    return result ?? false;
  }

  Future<void> _selectChapter(Chapter chapter) async {
    if (!await _promptUnsavedChanges()) return;

    // Flush remaining debounce save before switching out
    if (_stateDebounceTimer?.isActive ?? false) {
      _stateDebounceTimer?.cancel();
      _debounceSaveLocalSettings();
    }

    try {
      await _controller.selectChapter(chapter);

      // Update last opened chapter in LocalSettings
      final newLocalSettings = widget.settingsController.localSettings.copyWith(
        lastOpenedChapterId: chapter.id,
        // Reset offsets when explicitly switching chapters
        lastCursorOffset: -1,
        lastSelectionBaseOffset: -1,
        lastSelectionExtentOffset: -1,
        lastScrollOffset: 0.0,
      );
      widget.settingsController.updateLocalSettings(newLocalSettings);
      widget.settingsController.saveLocal();
    } catch (e) {
      _showError('读取章节失败: $e');
    }
  }

  void _backToHome() {
    if (_stateDebounceTimer?.isActive ?? false) {
      _stateDebounceTimer?.cancel();
      _debounceSaveLocalSettings();
    }
    Navigator.pushReplacement(
      context,
      MaterialPageRoute(
        builder: (context) =>
            ProjectHomeScreen(settingsController: widget.settingsController),
      ),
    );
  }

  Future<void> _saveCurrentChapter() async {
    try {
      await _controller.saveCurrentChapter(_textController.text);
    } catch (e) {
      _showError('保存失败: $e');
    }
  }

  Future<void> _deleteCurrentChapter() async {
    if (_controller.selectedChapter == null) return;

    final confirm = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('删除章节'),
        content: Text(
          '确定要删除《${_controller.selectedChapter!.title}》吗？\n文件将被移动到回收站。',
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context, false),
            child: const Text('取消'),
          ),
          TextButton(
            onPressed: () => Navigator.pop(context, true),
            child: const Text('确认删除', style: TextStyle(color: Colors.red)),
          ),
        ],
      ),
    );

    if (confirm != true) return;

    try {
      await _controller.deleteCurrentChapter();
    } catch (e) {
      _showError('删除失败: $e');
    }
  }

  Future<void> _backupProject() async {
    try {
      await _controller.backupProject();
      if (mounted) {
        ScaffoldMessenger.of(
          context,
        ).showSnackBar(const SnackBar(content: Text('项目备份成功！')));
      }
    } catch (e) {
      _showError('备份失败: $e');
    }
  }

  Future<void> _showSettingsDialog() async {
    await showDialog(
      context: context,
      barrierDismissible: false,
      builder: (context) =>
          SettingsDialog(controller: widget.settingsController),
    );
  }

  void _showError(String message) {
    if (mounted) {
      ScaffoldMessenger.of(
        context,
      ).showSnackBar(SnackBar(content: Text(message)));
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_controller.isLoading) {
      return const Scaffold(body: Center(child: CircularProgressIndicator()));
    }

    return Scaffold(
      appBar: AppBar(
        leading: IconButton(
          icon: const Icon(Icons.arrow_back),
          onPressed: _backToHome,
          tooltip: '返回作品列表',
        ),
        title: const Text('Writer App (Local First MVP)'),
        actions: [
          IconButton(
            icon: const Icon(Icons.settings),
            onPressed: _showSettingsDialog,
            tooltip: '设置',
          ),
          IconButton(
            icon: const Icon(Icons.backup),
            onPressed: _backupProject,
            tooltip: '备份当前项目',
          ),
          IconButton(
            icon: const Icon(Icons.save),
            onPressed: _controller.isSaving ? null : _saveCurrentChapter,
            tooltip: '保存当前章节',
          ),
          if (_controller.selectedChapter != null)
            IconButton(
              icon: const Icon(Icons.delete, color: Colors.redAccent),
              onPressed: _deleteCurrentChapter,
              tooltip: '删除当前章节',
            ),
        ],
      ),
      body: Row(
        children: [
          ChapterListPanel(
            chapters: _controller.chapters,
            selectedChapter: _controller.selectedChapter,
            onAddChapter: _showNewChapterDialog,
            onSelectChapter: _selectChapter,
          ),
          const VerticalDivider(width: 1),
          Expanded(
            flex: 3,
            child: EditorPanel(
              hasChapter: _controller.selectedChapter != null,
              textController: _textController,
              scrollController: _scrollController,
              inputAnimationEnabled: widget
                  .settingsController
                  .syncableSettings
                  .inputAnimationEnabled,
              typedCharacterAnimationEnabled: widget
                  .settingsController
                  .syncableSettings
                  .typedCharacterAnimationEnabled,
              cursorAnimationEnhanced: widget
                  .settingsController
                  .syncableSettings
                  .cursorAnimationEnhanced,
              editorFontSize:
                  widget.settingsController.syncableSettings.editorFontSize,
              editorLineHeight:
                  widget.settingsController.syncableSettings.editorLineHeight,
              editorContentWidth:
                  widget.settingsController.syncableSettings.editorContentWidth,
              activeChapterId: _controller.selectedChapter?.id,
              onChanged: (val) {
                // To keep state logic simple for prompt trigger
              },
            ),
          ),
          const VerticalDivider(width: 1),
          ChapterInfoPanel(
            workspacePath: _controller.workspacePath,
            chapter: _controller.selectedChapter,
            isSaving: _controller.isSaving,
            onEditTitle: _showEditTitleDialog,
          ),
        ],
      ),
    );
  }
}
