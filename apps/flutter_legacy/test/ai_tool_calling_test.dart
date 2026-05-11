import 'dart:io';
import 'package:flutter_test/flutter_test.dart';
import 'package:sqflite_common_ffi/sqflite_ffi.dart';
import 'package:path/path.dart' as p;

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

    setUpAll(() {
      sqfliteFfiInit();
      databaseFactory = databaseFactoryFfi;
    });

    setUp(() async {
      tempWorkspace = await Directory.systemTemp.createTemp(
        'writer_app_ai_test_',
      );
      storageService = AtomicWriter();
      dbHelper = DatabaseHelper(tempWorkspace.path);
      await dbHelper.initDatabase();
      toolExecutor = AIToolExecutorImpl(
        AITools.allTools,
        tempWorkspace.path,
        storageService,
        dbHelper,
      );
    });

    tearDown(() async {
      if (await tempWorkspace.exists()) {
        await tempWorkspace.delete(recursive: true);
      }
    });

    test(
      'MockAIProvider successfully simulates returning a tool call',
      () async {
        final mockResponse = AIMessage(
          role: AIMessageRole.assistant,
          toolCalls: [
            AIToolCall(
              id: 'call_123',
              name: 'read_chapter',
              arguments: {'chapterId': 'chap-1'},
            ),
          ],
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
            currentVersion: PromptVersion(
              id: 'v1',
              versionString: 'v1',
              createdAt: DateTime.now(),
            ),
          ),
          inputHash: '',
          modelName: 'mock-model',
          providerName: 'mock',
        );

        final result = await provider.executeTask(
          dummyTask,
          [],
          AITools.allTools,
          CancellationToken(),
        );

        // Validate the parsing contract for tool calls
        expect(result.rawResponse['tool_calls'], isNotNull);
        expect(
          result.rawResponse['tool_calls'][0]['function']['name'],
          'read_chapter',
        );
      },
    );

    test('AIToolExecutor strictly blocks dangerous tools', () async {
      final dangerousCall = AIToolCall(
        id: 'call_danger',
        name: 'rewrite_chapter_content',
        arguments: {'chapterId': 'c1', 'content': 'hacked'},
      );

      final result = await toolExecutor.executeTool(dangerousCall);
      expect(result.isError, isTrue);
      expect(result.result['error'].toString().contains('DANGEROUS'), isTrue);
    });

    test('AIToolExecutor routes save tools to isolated directories', () async {
      final saveCall = AIToolCall(
        id: 'call_save_extract',
        name: 'save_character_extraction',
        arguments: {
          'projectId': 'p1',
          'characters': [
            {'name': 'Alice'},
          ],
        },
      );

      final result = await toolExecutor.executeTool(saveCall);
      expect(result.isError, isFalse);

      final savedPath = result.result['path'] as String;
      expect(savedPath.contains('characters'), isTrue);
      expect(savedPath.contains('ai_extract'), isTrue);

      final savedContent = await File(savedPath).readAsString();
      expect(savedContent.contains('Alice'), isTrue);
    });

    test(
      'save_chapter_summary only writes to ai/summaries, not chapters',
      () async {
        final call = AIToolCall(
          id: 'sum',
          name: 'save_chapter_summary',
          arguments: {'chapterId': 'c1', 'summary': 'test summary'},
        );
        final result = await toolExecutor.executeTool(call);
        expect(result.isError, isFalse);

        final path = result.result['path'] as String;
        expect(path.contains('chapters'), isFalse);
        expect(path.contains('ai/summaries'), isTrue);
      },
    );

    test('list_chapters returns real chapters', () async {
      // Create another instance of DatabaseHelper/or use dbHelper's instance
      // Using an external connection caused database_closed if the wrapper didn't share it.
      // Actually we should mock the insert via raw sqlite or cache
      // The issue was `await db.close()` closes it for the whole app
      final db = await databaseFactory.openDatabase(
        p.join(tempWorkspace.path, 'sqlite_cache', 'index.db'),
      );
      await db.insert('chapters_cache', {
        'id': 'c1',
        'volume_id': 'v1',
        'project_id': 'p1',
        'title': 'Chapter 1',
        'word_count': 100,
        'updated_at': DateTime.now().toIso8601String(),
        'content_hash': 'abc',
      });
      // Don't close db so test can use it

      final call = AIToolCall(
        id: 'list_chap',
        name: 'list_chapters',
        arguments: {'projectId': 'p1'},
      );
      final result = await toolExecutor.executeTool(call);
      expect(result.isError, isFalse);

      final chapters = result.result['chapters'] as List;
      expect(chapters.length, 1);
      expect(chapters.first['id'], 'c1');
    });

    test('read_chapter reads real md content', () async {
      final db = await databaseFactory.openDatabase(
        p.join(tempWorkspace.path, 'sqlite_cache', 'index.db'),
      );
      await db.insert('chapters_cache', {
        'id': 'c2',
        'volume_id': 'v1',
        'project_id': 'p1',
        'title': 'Chapter 2',
        'word_count': 100,
        'updated_at': DateTime.now().toIso8601String(),
        'content_hash': 'def',
      });

      final mdPath = p.join(
        tempWorkspace.path,
        'projects',
        'p1',
        'volumes',
        'v1',
        'chapters',
        'c2.md',
      );
      await File(mdPath).create(recursive: true);
      await File(mdPath).writeAsString('Real Content C2');

      final call = AIToolCall(
        id: 'read_chap',
        name: 'read_chapter',
        arguments: {'chapterId': 'c2'},
      );
      final result = await toolExecutor.executeTool(call);
      expect(result.isError, isFalse);
      expect(result.result['content'], 'Real Content C2');
    });

    test('search_text finds real snippet in md', () async {
      final db = await databaseFactory.openDatabase(
        p.join(tempWorkspace.path, 'sqlite_cache', 'index.db'),
      );
      await db.insert('chapters_cache', {
        'id': 'c3',
        'volume_id': 'v1',
        'project_id': 'p1',
        'title': 'Chapter 3',
        'word_count': 100,
        'updated_at': DateTime.now().toIso8601String(),
        'content_hash': 'xyz',
      });

      final mdPath = p.join(
        tempWorkspace.path,
        'projects',
        'p1',
        'volumes',
        'v1',
        'chapters',
        'c3.md',
      );
      await File(mdPath).create(recursive: true);
      await File(
        mdPath,
      ).writeAsString('Some text here before ALICE was walking.');

      final call = AIToolCall(
        id: 'search',
        name: 'search_text',
        arguments: {'projectId': 'p1', 'query': 'ALICE'},
      );
      final result = await toolExecutor.executeTool(call);
      expect(result.isError, isFalse);

      final results = result.result['results'] as List;
      expect(results.length, 1);
      expect(results.first['chapterId'], 'c3');
      expect(
        results.first['matchedSnippet'].toString().contains('ALICE'),
        isTrue,
      );
    });

    test('list_character_cards lists real cards', () async {
      final charDir = p.join(
        tempWorkspace.path,
        'projects',
        'p1',
        'characters',
      );
      await Directory(charDir).create(recursive: true);
      await File(p.join(charDir, 'alice.json')).writeAsString('{}');
      await File(p.join(charDir, 'bob.md')).writeAsString('{}');
      await File(p.join(charDir, 'ignore.txt')).writeAsString('{}');

      final call = AIToolCall(
        id: 'list_char',
        name: 'list_character_cards',
        arguments: {'projectId': 'p1'},
      );
      final result = await toolExecutor.executeTool(call);
      expect(result.isError, isFalse);

      final chars = result.result['characters'] as List;
      expect(chars.length, 2);
      final fileNames = chars.map((e) => e['fileName']).toList();
      expect(fileNames, contains('alice.json'));
      expect(fileNames, contains('bob.md'));
    });
  });
}
