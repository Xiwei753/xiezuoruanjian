import 'dart:io';
import 'package:flutter_test/flutter_test.dart';

import 'package:writer_app/domain/models/ai_models.dart';
import 'package:writer_app/domain/services_interfaces/ai_provider.dart';
import 'package:writer_app/infrastructure/ai/mock_ai_provider.dart';
import 'package:writer_app/application/ai_tools/ai_tools.dart';
import 'package:writer_app/application/ai_tools/ai_tool_executor_impl.dart';
import 'package:writer_app/infrastructure/storage/atomic_writer.dart';
import 'package:writer_app/infrastructure/database/database_helper.dart';

void main() {
  group('AI Tool Calling Architecture Tests', () {
    late Directory tempWorkspace;
    late AtomicWriter storageService;
    late DatabaseHelper dbHelper;
    late AIToolExecutorImpl toolExecutor;

    setUp(() async {
      tempWorkspace = await Directory.systemTemp.createTemp('writer_app_ai_test_');
      storageService = AtomicWriter();
      dbHelper = DatabaseHelper(tempWorkspace.path);
      toolExecutor = AIToolExecutorImpl(AITools.allTools, tempWorkspace.path, dbHelper, storageService);
    });

    tearDown(() async {
      if (await tempWorkspace.exists()) {
        await tempWorkspace.delete(recursive: true);
      }
    });

    test('MockAIProvider successfully simulates returning a tool call', () async {
      final mockResponse = AIMessage(
        role: AIMessageRole.assistant,
        toolCalls: [
          AIToolCall(
            id: 'call_123',
            name: 'read_chapter',
            arguments: {'chapterId': 'chap-1'}
          )
        ]
      );

      final provider = MockAIProvider([mockResponse]);
      final dummyTask = AITask(
        id: 'task1',
        projectId: 'proj1',
        sourceChapterIds: [],
        promptTemplate: PromptTemplate(
          id: 'p1',
          name: 'p1',
          templateText: '',
          currentVersion: PromptVersion(id: 'v1', versionString: 'v1', createdAt: DateTime.now())
        ),
        inputHash: '',
        modelName: 'mock-model',
        providerName: 'mock'
      );

      final result = await provider.executeTask(dummyTask, [], AITools.allTools, CancellationToken());

      // Validate the parsing contract for tool calls
      expect(result.rawResponse['tool_calls'], isNotNull);
      expect(result.rawResponse['tool_calls'][0]['function']['name'], 'read_chapter');
    });

    test('AIToolExecutor strictly blocks dangerous tools', () async {
      final dangerousCall = AIToolCall(
        id: 'call_danger',
        name: 'rewrite_chapter_content',
        arguments: {'chapterId': 'c1', 'content': 'hacked'}
      );

      final result = await toolExecutor.executeTool(dangerousCall);
      expect(result.isError, isTrue);
      expect(result.result['error'].toString().contains('DANGEROUS'), isTrue);
    });

    test('AIToolExecutor allows safe read tools', () async {
      final safeCall = AIToolCall(
        id: 'call_safe_read',
        name: 'read_chapter',
        arguments: {'chapterId': 'c2'}
      );

      final result = await toolExecutor.executeTool(safeCall);
      expect(result.isError, isFalse);
      expect(result.result['content'].toString().contains('c2'), isTrue);
    });

    test('AIToolExecutor routes save tools to isolated directories', () async {
      final saveCall = AIToolCall(
        id: 'call_save_extract',
        name: 'save_character_extraction',
        arguments: {
          'projectId': 'p1',
          'characters': [{'name': 'Alice'}]
        }
      );

      final result = await toolExecutor.executeTool(saveCall);
      expect(result.isError, isFalse);

      final savedPath = result.result['path'] as String;
      expect(savedPath.contains('characters'), isTrue);
      expect(savedPath.contains('ai_extract'), isTrue);

      final savedContent = await File(savedPath).readAsString();
      expect(savedContent.contains('Alice'), isTrue);
    });
  });
}
