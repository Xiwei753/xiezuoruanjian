import 'dart:convert';
import '../../domain/models/ai_models.dart';

class DeepSeekMessageSerializer {
  /// Converts internal AIMessage list into a format acceptable by DeepSeek/OpenAI.
  static List<Map<String, dynamic>> serialize(
    List<AIMessage> messages, {
    bool isDeepSeekProvider = true,
  }) {
    final List<Map<String, dynamic>> result = [];

    // We only echo reasoning_content if it's within a tool call loop.
    // Deepseek requires `reasoning_content` to be passed back *only* if the assistant made tool calls.
    // If the assistant just answered (no tool calls), we drop the reasoning_content for normal multi-turn.
    for (int i = 0; i < messages.length; i++) {
      final msg = messages[i];
      final serialized = <String, dynamic>{};

      switch (msg.role) {
        case AIMessageRole.system:
          serialized['role'] = 'system';
          serialized['content'] = msg.content ?? '';
          break;
        case AIMessageRole.user:
          serialized['role'] = 'user';
          serialized['content'] = msg.content ?? '';
          break;
        case AIMessageRole.assistant:
          serialized['role'] = 'assistant';
          serialized['content'] = msg.content ?? '';

          if (msg.toolCalls != null && msg.toolCalls!.isNotEmpty) {
            serialized['tool_calls'] = msg.toolCalls!
                .map(
                  (tc) => {
                    'id': tc.id,
                    'type': 'function',
                    'function': {
                      'name': tc.name,
                      'arguments': jsonEncode(tc.arguments),
                    },
                  },
                )
                .toList();

            // Only include reasoning_content for tool_calls if provider is DeepSeek
            if (isDeepSeekProvider && msg.reasoningContent != null) {
              serialized['reasoning_content'] = msg.reasoningContent;
            }
          }
          break;
        case AIMessageRole.tool:
          serialized['role'] = 'tool';
          serialized['content'] = msg.content ?? '';
          if (msg.toolResult != null) {
            serialized['tool_call_id'] = msg.toolResult!.toolCallId;
          }
          break;
      }

      result.add(serialized);
    }

    return result;
  }
}
