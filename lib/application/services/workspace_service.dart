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
    await Directory(p.join(rootPath, 'projects')).create();
    await Directory(p.join(rootPath, 'backups')).create();
    await Directory(p.join(rootPath, 'trash')).create();
    await Directory(p.join(rootPath, 'app-meta')).create();

    // Create sqlite folder (gitignore this usually)
    await Directory(p.join(rootPath, 'sqlite_cache')).create();

    // Create manifest
    final manifest = WorkspaceManifest(id: workspaceId, createdAt: DateTime.now());
    await _storageService.atomicWrite(
      p.join(rootPath, 'workspace_manifest.json'),
      jsonEncode(manifest.toJson()),
    );
  }

  Future<void> createProject(String rootPath, String projectId, String title) async {
    final projectDir = Directory(p.join(rootPath, 'projects', projectId));
    await projectDir.create(recursive: true);

    await Directory(p.join(projectDir.path, 'volumes')).create();
    await Directory(p.join(projectDir.path, 'characters')).create();

    final manifest = ProjectManifest(id: projectId, title: title, createdAt: DateTime.now());
    await _storageService.atomicWrite(
      p.join(projectDir.path, 'project.json'),
      jsonEncode(manifest.toJson()),
    );
  }

  Future<void> createVolume(String rootPath, String projectId, String volumeId, String title) async {
    final volumeDir = Directory(p.join(rootPath, 'projects', projectId, 'volumes', volumeId));
    await volumeDir.create(recursive: true);
    await Directory(p.join(volumeDir.path, 'chapters')).create();

    final meta = VolumeMeta(id: volumeId, title: title, createdAt: DateTime.now());
    await _storageService.atomicWrite(
      p.join(volumeDir.path, 'volume.json'),
      jsonEncode(meta.toJson()),
    );
  }
}
