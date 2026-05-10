import 'package:flutter/material.dart';
import '../../application/controllers/settings_controller.dart';
import '../../application/controllers/project_controller.dart';
import '../../domain/models/manifests.dart';
import 'workspace_screen.dart';
import '../dialogs/settings_dialog.dart';

class ProjectHomeScreen extends StatefulWidget {
  final SettingsController settingsController;

  const ProjectHomeScreen({super.key, required this.settingsController});

  @override
  State<ProjectHomeScreen> createState() => _ProjectHomeScreenState();
}

class _ProjectHomeScreenState extends State<ProjectHomeScreen> {
  final ProjectController _projectController = ProjectController();

  @override
  void initState() {
    super.initState();
    _loadProjects();
  }

  Future<void> _loadProjects() async {
    final workspacePath = widget.settingsController.workspacePath;
    if (workspacePath.isNotEmpty) {
      await _projectController.loadProjects(workspacePath);
    }
  }

  Future<void> _createNewProject() async {
    final titleController = TextEditingController();
    final result = await showDialog<String>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('新建作品'),
        content: TextField(
          controller: titleController,
          autofocus: true,
          decoration: const InputDecoration(
            labelText: '作品标题',
            border: OutlineInputBorder(),
          ),
        ),
        actions: [
          TextButton(
            onPressed: () => Navigator.pop(context),
            child: const Text('取消'),
          ),
          ElevatedButton(
            onPressed: () => Navigator.pop(context, titleController.text),
            child: const Text('创建'),
          ),
        ],
      ),
    );

    if (result != null && result.isNotEmpty) {
      final newProject = await _projectController.createNewProject(
        widget.settingsController.workspacePath,
        result,
      );
      if (newProject != null) {
        _openProject(newProject);
      }
    }
  }

  void _openProject(ProjectManifest project, {String? chapterId}) {
    // Determine the appropriate chapterId to use
    // If a chapterId is explicitly passed, use it.
    // If not, use the saved lastOpenedChapterId ONLY IF we are opening the lastOpenedProjectId.
    // Otherwise, we shouldn't attempt to open a chapter from a different project.
    String finalChapterId = '';

    if (chapterId != null && chapterId.isNotEmpty) {
      finalChapterId = chapterId;
    } else if (project.id ==
        widget.settingsController.localSettings.lastOpenedProjectId) {
      finalChapterId =
          widget.settingsController.localSettings.lastOpenedChapterId;
    }

    // Update LocalSettings with lastOpenedProjectId
    final newLocalSettings = widget.settingsController.localSettings.copyWith(
      lastOpenedProjectId: project.id,
      lastOpenedChapterId: finalChapterId,
    );
    widget.settingsController.updateLocalSettings(newLocalSettings);
    // Project opening usually triggers saves; we only save local state here to avoid rewriting sync settings
    widget.settingsController.saveLocal();

    Navigator.pushReplacement(
      context,
      MaterialPageRoute(
        builder: (context) => WorkspaceScreen(
          settingsController: widget.settingsController,
          projectId: project.id,
        ),
      ),
    );
  }

  Future<void> _showSettingsDialog() async {
    await showDialog(
      context: context,
      barrierDismissible: false,
      builder: (context) =>
          SettingsDialog(controller: widget.settingsController),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      appBar: AppBar(
        title: const Text('Writer App - 作品管理'),
        actions: [
          IconButton(
            icon: const Icon(Icons.settings),
            onPressed: _showSettingsDialog,
            tooltip: '设置',
          ),
        ],
      ),
      body: ListenableBuilder(
        listenable: _projectController,
        builder: (context, _) {
          if (_projectController.isLoading) {
            return const Center(child: CircularProgressIndicator());
          }

          if (_projectController.errorMessage != null) {
            return Center(
              child: Text(
                _projectController.errorMessage!,
                style: const TextStyle(color: Colors.red),
              ),
            );
          }

          if (_projectController.projects.isEmpty) {
            return const Center(child: Text('没有发现任何作品。点击右下角按钮创建一个新作品。'));
          }

          final lastOpenedId =
              widget.settingsController.localSettings.lastOpenedProjectId;
          final hasLastOpened =
              lastOpenedId.isNotEmpty &&
              _projectController.projects.any((p) => p.id == lastOpenedId);

          return Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              if (hasLastOpened)
                Padding(
                  padding: const EdgeInsets.all(16.0),
                  child: ElevatedButton.icon(
                    onPressed: () {
                      final project = _projectController.projects.firstWhere(
                        (p) => p.id == lastOpenedId,
                      );
                      _openProject(
                        project,
                        chapterId: widget
                            .settingsController
                            .localSettings
                            .lastOpenedChapterId,
                      );
                    },
                    icon: const Icon(Icons.play_arrow),
                    label: const Text('继续上次写作'),
                    style: ElevatedButton.styleFrom(
                      padding: const EdgeInsets.symmetric(vertical: 16.0),
                    ),
                  ),
                ),
              Expanded(
                child: GridView.builder(
                  padding: const EdgeInsets.all(16.0),
                  gridDelegate: const SliverGridDelegateWithFixedCrossAxisCount(
                    crossAxisCount: 3,
                    childAspectRatio: 3 / 4,
                    crossAxisSpacing: 16,
                    mainAxisSpacing: 16,
                  ),
                  itemCount: _projectController.projects.length,
                  itemBuilder: (context, index) {
                    final project = _projectController.projects[index];
                    return Card(
                      clipBehavior: Clip.antiAlias,
                      child: InkWell(
                        onTap: () => _openProject(project),
                        child: Padding(
                          padding: const EdgeInsets.all(16.0),
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              const Icon(
                                Icons.book,
                                size: 48,
                                color: Colors.blueGrey,
                              ),
                              const SizedBox(height: 16),
                              Text(
                                project.title,
                                style: Theme.of(context).textTheme.titleLarge,
                                maxLines: 2,
                                overflow: TextOverflow.ellipsis,
                              ),
                              const Spacer(),
                              Text(
                                '创建于: ${project.createdAt.toLocal().toString().split('.')[0]}',
                                style: Theme.of(context).textTheme.bodySmall,
                              ),
                            ],
                          ),
                        ),
                      ),
                    );
                  },
                ),
              ),
            ],
          );
        },
      ),
      floatingActionButton: FloatingActionButton.extended(
        onPressed: _createNewProject,
        icon: const Icon(Icons.add),
        label: const Text('新建作品'),
      ),
    );
  }
}
