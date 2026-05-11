import '../models/ai_models.dart';

class CancellationToken {
  bool isCancelled = false;
  void cancel() => isCancelled = true;
}

abstract class IAIProvider {
  /// Base API for completing multi-turn conversations and potentially invoking tools.
  Future<AIResult> executeTask(
    AITask task,
    List<AIMessage> messages,
    List<AIToolDefinition> tools,
    CancellationToken token,
  );
}
