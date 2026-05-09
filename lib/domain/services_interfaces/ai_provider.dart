class AITask {
  final String id;
  final String targetId;
  AITask({required this.id, required this.targetId});
}

class AIResult {
  final String content;
  AIResult({required this.content});
}

class CancellationToken {
  bool isCancelled = false;
  void cancel() => isCancelled = true;
}

abstract class IAIProvider {
  Future<AIResult> analyze(AITask request, CancellationToken token);
}
