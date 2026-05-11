import 'package:flutter/foundation.dart';
import '../../domain/models/manifests.dart';
import '../services/workspace_service.dart';
import '../../infrastructure/storage/atomic_writer.dart';
import 'package:uuid/uuid.dart';

class ProjectController extends ChangeNotifier {
  final WorkspaceService _workspaceService;

  ProjectController({WorkspaceService? workspaceService})
    : _workspaceService = workspaceService ?? WorkspaceService(AtomicWriter());

  List<ProjectManifest> projects = [];
  bool isLoading = false;
  String? errorMessage;

  Future<void> loadProjects(String workspacePath) async {
    isLoading = true;
    errorMessage = null;
    notifyListeners();

    try {
      projects = await _workspaceService.listProjects(workspacePath);
    } catch (e) {
      errorMessage = 'Failed to load projects: $e';
    } finally {
      isLoading = false;
      notifyListeners();
    }
  }

  Future<ProjectManifest?> createNewProject(
    String workspacePath,
    String title,
  ) async {
    isLoading = true;
    errorMessage = null;
    notifyListeners();

    try {
      final projectId =
          'proj_${const Uuid().v4().replaceAll('-', '').substring(0, 12)}';
      await _workspaceService.createProject(workspacePath, projectId, title);

      // Default volume
      final volumeId = 'vol_default';
      await _workspaceService.createVolume(
        workspacePath,
        projectId,
        volumeId,
        '默认卷',
      );

      await loadProjects(workspacePath);
      return projects.firstWhere((p) => p.id == projectId);
    } catch (e) {
      errorMessage = 'Failed to create project: $e';
      isLoading = false;
      notifyListeners();
      return null;
    }
  }
}
