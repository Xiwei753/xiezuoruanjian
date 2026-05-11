import 'dart:convert';
import 'dart:io';
import 'package:path/path.dart' as p;

import '../../domain/models/manifests.dart';
import '../../domain/services_interfaces/storage_service.dart';

class WorkspaceService {
  final IStorageService _storageService;

  WorkspaceService(this._storageService);

  Future<void> createWorkspace(String rootPath, String workspaceId) async {
    final workspaceDir = Directory(rootPath);
    if (!await workspaceDir.exists()) {
      await workspaceDir.create(recursive: true);
    }

    // Create standard folders
    await Directory(p.join(rootPath, 'projects')).create(recursive: true);
    await Directory(p.join(rootPath, 'backups')).create(recursive: true);
    await Directory(p.join(rootPath, 'trash')).create(recursive: true);
    await Directory(p.join(rootPath, 'app-meta')).create(recursive: true);

    // Create sqlite folder (gitignore this usually)
    await Directory(p.join(rootPath, 'sqlite_cache')).create(recursive: true);

    // Create manifest
    final manifest = WorkspaceManifest(
      id: workspaceId,
      createdAt: DateTime.now(),
    );
    await _storageService.atomicWrite(
      p.join(rootPath, 'workspace_manifest.json'),
      jsonEncode(manifest.toJson()),
    );
  }

  Future<void> createProject(
    String rootPath,
    String projectId,
    String title,
  ) async {
    final projectDir = Directory(p.join(rootPath, 'projects', projectId));
    await projectDir.create(recursive: true);

    await Directory(p.join(projectDir.path, 'volumes')).create(recursive: true);
    await Directory(
      p.join(projectDir.path, 'characters'),
    ).create(recursive: true);

    final manifest = ProjectManifest(
      id: projectId,
      title: title,
      createdAt: DateTime.now(),
    );
    await _storageService.atomicWrite(
      p.join(projectDir.path, 'project.json'),
      jsonEncode(manifest.toJson()),
    );
  }

  Future<List<ProjectManifest>> listProjects(String rootPath) async {
    final projectsDir = Directory(p.join(rootPath, 'projects'));
    if (!await projectsDir.exists()) {
      return [];
    }

    final List<ProjectManifest> projects = [];
    final entities = await projectsDir.list().toList();

    for (var entity in entities) {
      if (entity is Directory) {
        final projectFile = File(p.join(entity.path, 'project.json'));
        if (await projectFile.exists()) {
          try {
            final json = jsonDecode(await projectFile.readAsString());
            projects.add(ProjectManifest.fromJson(json));
          } catch (e) {
            // Ignore corrupted project manifest for now
          }
        }
      }
    }

    projects.sort((a, b) => b.createdAt.compareTo(a.createdAt));
    return projects;
  }

  Future<void> createVolume(
    String rootPath,
    String projectId,
    String volumeId,
    String title,
  ) async {
    final volumeDir = Directory(
      p.join(rootPath, 'projects', projectId, 'volumes', volumeId),
    );
    await volumeDir.create(recursive: true);
    await Directory(p.join(volumeDir.path, 'chapters')).create(recursive: true);

    final meta = VolumeMeta(
      id: volumeId,
      title: title,
      createdAt: DateTime.now(),
    );
    await _storageService.atomicWrite(
      p.join(volumeDir.path, 'volume.json'),
      jsonEncode(meta.toJson()),
    );
  }
}
