import 'dart:io';
import '../../domain/services_interfaces/storage_service.dart';

class AtomicWriter implements IStorageService {
  @override
  Future<void> atomicWrite(String filePath, String content) async {
    final tempFile = File('$filePath.tmp');

    try {
      // 1. Write to a temporary file
      await tempFile.writeAsString(content, flush: true);

      // 2. Perform OS-level atomic rename to replace the original file
      await tempFile.rename(filePath);
    } catch (e) {
      // Clean up the temporary file if something fails
      if (await tempFile.exists()) {
        await tempFile.delete();
      }
      rethrow;
    }
  }
}
