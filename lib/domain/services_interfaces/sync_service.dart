class SyncResult {
  final bool success;
  final String? error;
  SyncResult({required this.success, this.error});
}

class ConflictFile {
  final String path;
  ConflictFile({required this.path});
}

class PullResult {
  final bool success;
  final bool hasConflicts;
  PullResult({required this.success, required this.hasConflicts});
}

abstract class ISyncService {
  Future<SyncResult> syncProject(String projectId);
  Future<List<ConflictFile>> getConflicts(String projectId);
}

abstract class IGitClient {
  Future<PullResult> pullSafe(String repoPath);
}
