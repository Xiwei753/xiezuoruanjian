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
    // Deduplication logic: If a task for this path is already in the pending queue,
    // we simply update its content and return the existing future, rather than adding a new task.
    final existingTaskIndex = _queue.indexWhere((t) => t.filePath == filePath);

    if (existingTaskIndex != -1) {
      final existingTask = _queue[existingTaskIndex];
      // Update content to the latest
      _queue[existingTaskIndex] = WriteTask(filePath, content, existingTask.completer);
      return existingTask.completer.future;
    }

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
