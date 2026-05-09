import 'dart:async';
import '../../domain/services_interfaces/storage_service.dart';

class WriteTask {
  final String filePath;
  final String content;
  final Completer<void> completer;

  WriteTask(this.filePath, this.content, this.completer);
}

class FileWriteQueue {
  final IStorageService _storageService;
  final List<WriteTask> _queue = [];
  bool _isProcessing = false;

  FileWriteQueue(this._storageService);

  Future<void> enqueueWrite(String filePath, String content) {
    // Basic debounce / deduplication logic could be added here
    // For MVP, we just queue them sequentially to prevent concurrent writes to the same file

    final completer = Completer<void>();
    _queue.add(WriteTask(filePath, content, completer));
    _processQueue();
    return completer.future;
  }

  Future<void> _processQueue() async {
    if (_isProcessing || _queue.isEmpty) return;

    _isProcessing = true;

    while (_queue.isNotEmpty) {
      final task = _queue.removeAt(0);
      try {
        await _storageService.atomicWrite(task.filePath, task.content);
        task.completer.complete();
      } catch (e) {
        task.completer.completeError(e);
      }
    }

    _isProcessing = false;
  }
}
