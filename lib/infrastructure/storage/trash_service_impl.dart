import 'dart:io';
import 'package:path/path.dart' as p;
import '../../domain/services_interfaces/trash_service.dart';

class TrashServiceImpl implements ITrashService {
  @override
  Future<void> moveToTrash(String workspaceRoot, String filePath) async {
    final file = File(filePath);
    if (!await file.exists()) return;

    final trashDir = Directory(p.join(workspaceRoot, 'trash'));
    if (!await trashDir.exists()) {
      await trashDir.create(recursive: true);
    }

    final fileName = p.basename(filePath);
    final timestamp = DateTime.now().millisecondsSinceEpoch;
    final trashPath = p.join(trashDir.path, '${timestamp}_$fileName');

    await file.rename(trashPath);
  }
}
