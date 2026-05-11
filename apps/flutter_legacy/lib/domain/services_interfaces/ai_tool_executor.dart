import '../models/ai_models.dart';

abstract class IAIToolExecutor {
  /// Executes a single tool call from the AI and returns the result string/json.
  Future<AIToolResult> executeTool(AIToolCall call);
}

abstract class AIToolRegistry {
  List<AIToolDefinition> get availableTools;

  /// Get tools by their names
  List<AIToolDefinition> resolveTools(List<String> names);
}
