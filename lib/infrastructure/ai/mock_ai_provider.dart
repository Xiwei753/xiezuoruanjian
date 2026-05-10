import '../../domain/models/ai_models.dart';
import '../../domain/services_interfaces/ai_provider.dart';

class MockAIProvider implements IAIProvider {
  /// We can inject mock behavior to test the multi-turn loop.
  /// First call might return a tool_call, second call might return final text.
  final List<AIMessage> mockResponses;
  int _callCount = 0;

  MockAIProvider(this.mockResponses);

  @override
  Future<AIResult> executeTask(
    AITask task,
    List<AIMessage> messages,
    List<AIToolDefinition> tools,
    CancellationToken token,
  ) async {
    if (token.isCancelled) {
      throw Exception("Task cancelled");
    }

    if (_callCount >= mockResponses.length) {
      throw Exception("No more mock responses available");
    }

    final responseMsg = mockResponses[_callCount++];

    // If it's a tool call request, we return it embedded in a temporary result structure
    // (In reality, AIResult might just carry rawResponse, and the Application Layer parses it to find ToolCalls).
    // For this Mock, we'll store tool calls in `rawResponse` so the executor knows.

    return AIResult(
      taskId: task.id,
      content: responseMsg.content ?? '',
      reasoningContent: responseMsg.reasoningContent,
      toolCalls: responseMsg.toolCalls,
      requiresReasoningContentEcho:
          responseMsg.toolCalls != null && responseMsg.toolCalls!.isNotEmpty,
      rawRequest: {'mock': 'request', 'messages_count': messages.length},
      rawResponse: {
        'tool_calls': responseMsg.toolCalls
            ?.map(
              (tc) => {
                'id': tc.id,
                'function': {'name': tc.name, 'arguments': tc.arguments},
              },
            )
            .toList(),
        'reasoning_content': responseMsg.reasoningContent,
      },
      sourceChapterIds: task.sourceChapterIds,
      modelName: task.modelName,
      providerName: task.providerName,
      promptVersion: task.promptTemplate.currentVersion.versionString,
      inputHash: task.inputHash,
      createdAt: DateTime.now(),
    );
  }
}
