import '../../domain/models/ai_models.dart';
import '../../domain/services_interfaces/ai_provider.dart';

class DeepSeekProviderStub implements IAIProvider {
  final String baseUrl;
  final String apiKey;
  final bool strictMode;

  DeepSeekProviderStub({
    this.baseUrl = 'https://api.deepseek.com/v1',
    required this.apiKey,
    this.strictMode = true,
  });

  @override
  Future<AIResult> executeTask(
    AITask task,
    List<AIMessage> messages,
    List<AIToolDefinition> tools,
    CancellationToken token
  ) async {
    // This is just a stub showing where the http library and JSON decoding would go.
    // In a real implementation:
    // 1. Map `AIMessage` to the OpenAI-compatible JSON structure.
    // 2. Map `AIToolDefinition` to the `tools` JSON array.
    // 3. Make POST request.
    // 4. Handle tool_calls response or final content response.
    // 5. If tool_calls exist, return an AIResult containing the tool calls parsed into `AIToolCall` objects
    //    (but wait, usually the provider itself doesn't execute the tools, the caller handles execution and re-calls).
    throw UnimplementedError("DeepSeek actual network API is postponed.");
  }
}
