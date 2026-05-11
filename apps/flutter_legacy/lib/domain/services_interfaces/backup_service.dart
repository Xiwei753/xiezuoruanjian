abstract class IBackupService {
  Future<void> backupProject(String workspaceRoot, String projectId);
  Future<void> backupFile(String workspaceRoot, String filePath);
}
