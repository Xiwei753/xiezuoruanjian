abstract class ITrashService {
  Future<void> moveToTrash(String workspaceRoot, String filePath);
}
