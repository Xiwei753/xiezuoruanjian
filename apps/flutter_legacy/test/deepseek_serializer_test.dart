import 'package:flutter_test/flutter_test.dart';
import 'package:writer_app/domain/models/ai_models.dart';
import 'package:writer_app/infrastructure/ai/deepseek_message_serializer.dart';

void main() {
  group('DeepSeekMessageSerializer Tests', () {
    test('System and user messages are serialized correctly', () {
      final messages = [
        AIMessage(role: AIMessageRole.system, content: 'You are an AI.'),
        AIMessage(role: AIMessageRole.user, content: 'Hello'),
      ];

      final result = DeepSeekMessageSerializer.serialize(messages);

      expect(result.length, 2);
      expect(result[0]['role'], 'system');
      expect(result[0]['content'], 'You are an AI.');
      expect(result[1]['role'], 'user');
      expect(result[1]['content'], 'Hello');
    });

    test(
      'Assistant message WITH tool calls retains reasoning_content for DeepSeek provider',
      () {
        final messages = [
          AIMessage(
            role: AIMessageRole.assistant,
            content: 'I will call a tool',
            reasoningContent: 'Here is my raw reasoning...',
            toolCalls: [
              AIToolCall(
                id: 'call_1',
                name: 'read_chapter',
                arguments: {'chapterId': '123'},
              ),
            ],
          ),
        ];

        final result = DeepSeekMessageSerializer.serialize(
          messages,
          isDeepSeekProvider: true,
        );

        expect(result.length, 1);
        expect(result[0]['role'], 'assistant');
        expect(result[0]['content'], 'I will call a tool');
        expect(result[0]['reasoning_content'], 'Here is my raw reasoning...');
        expect(result[0]['tool_calls'], isNotNull);
      },
    );

    test(
      'Assistant message WITH tool calls DROPS reasoning_content for non-DeepSeek provider',
      () {
        final messages = [
          AIMessage(
            role: AIMessageRole.assistant,
            content: 'I will call a tool',
            reasoningContent: 'Here is my raw reasoning...',
            toolCalls: [
              AIToolCall(
                id: 'call_1',
                name: 'read_chapter',
                arguments: {'chapterId': '123'},
              ),
            ],
          ),
        ];

        final result = DeepSeekMessageSerializer.serialize(
          messages,
          isDeepSeekProvider: false,
        );

        expect(result.length, 1);
        expect(result[0]['role'], 'assistant');
        expect(result[0]['content'], 'I will call a tool');
        expect(result[0].containsKey('reasoning_content'), isFalse);
      },
    );

    test(
      'Assistant message WITHOUT tool calls drops reasoning_content even for DeepSeek',
      () {
        final messages = [
          AIMessage(
            role: AIMessageRole.assistant,
            content: 'Here is your answer',
            reasoningContent: 'I have finished thinking.',
            toolCalls: null,
          ),
        ];

        final result = DeepSeekMessageSerializer.serialize(
          messages,
          isDeepSeekProvider: true,
        );

        expect(result.length, 1);
        expect(result[0]['role'], 'assistant');
        expect(result[0]['content'], 'Here is your answer');
        expect(result[0].containsKey('reasoning_content'), isFalse);
      },
    );

    test('Tool messages correctly serialize with tool_call_id', () {
      final messages = [
        AIMessage(
          role: AIMessageRole.tool,
          content: '{"content":"chapter1 text"}',
          toolResult: AIToolResult(
            toolCallId: 'call_1',
            name: 'read_chapter',
            result: {},
          ),
        ),
      ];

      final result = DeepSeekMessageSerializer.serialize(messages);
      expect(result.length, 1);
      expect(result[0]['role'], 'tool');
      expect(result[0]['tool_call_id'], 'call_1');
      expect(result[0]['content'], '{"content":"chapter1 text"}');
    });

    test(
      'Reasoning content is preserved verbatim without trimming or mutating',
      () {
        final rawReasoning =
            '   \n  Some raw reasoning with \n whitespace  \t ';
        final messages = [
          AIMessage(
            role: AIMessageRole.assistant,
            content: '',
            reasoningContent: rawReasoning,
            toolCalls: [AIToolCall(id: '1', name: 'tool', arguments: {})],
          ),
        ];

        final result = DeepSeekMessageSerializer.serialize(
          messages,
          isDeepSeekProvider: true,
        );
        expect(result[0]['reasoning_content'], rawReasoning);
      },
    );
  });
}
