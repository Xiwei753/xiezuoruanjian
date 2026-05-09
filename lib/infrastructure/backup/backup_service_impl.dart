import 'dart:io';
import 'package:path/path.dart' as p;
import '../../domain/services_interfaces/backup_service.dart';

class BackupServiceImpl implements IBackupService {
  @override
  Future<void> backupProject(String workspaceRoot, String projectId) async {
    final projectDir = Directory(p.join(workspaceRoot, 'projects', projectId));
    if (!await projectDir.exists()) return;

    final timestamp = DateTime.now().millisecondsSinceEpoch;
    final backupDir = Directory(p.join(workspaceRoot, 'backups', 'project_${projectId}_$timestamp'));

    await _copyDirectory(projectDir, backupDir);
  }

  @override
  Future<void> backupFile(String workspaceRoot, String filePath) async {
    final file = File(filePath);
    if (!await file.exists()) return;

    final backupsDir = Directory(p.join(workspaceRoot, 'backups'));
    if (!await backupsDir.exists()) {
      await backupsDir.create(recursive: true);
    }

    final fileName = p.basename(filePath);
    final timestamp = DateTime.now().millisecondsSinceEpoch;
    final backupPath = p.join(backupsDir.path, '${timestamp}_$fileName');

    await file.copy(backupPath);
  }

  Future<void> _copyDirectory(Directory source, Directory destination) async {
    await destination.create(recursive: true);
    await for (final entity in source.list(recursive: false)) {
      if (entity is Directory) {
        final newDirectory = Directory(p.join(destination.absolute.path, p.basename(entity.path)));
        await _copyDirectory(entity, newDirectory);
      } else if (entity is File) {
        await entity.copy(p.join(destination.path, p.basename(entity.path)));
      }
    }
  }
}
