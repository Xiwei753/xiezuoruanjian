import 'dart:convert';
import 'dart:developer';
import 'dart:io';
import 'package:path/path.dart' as p;
import '../../domain/models/ai_models.dart';
import '../../domain/services_interfaces/ai_tool_executor.dart';
import '../../domain/services_interfaces/storage_service.dart';
import '../../infrastructure/database/database_helper.dart';

class AIToolExecutorImpl implements IAIToolExecutor, AIToolRegistry {
  final List<AIToolDefinition> _registeredTools;
  final String _workspaceRoot;
  final IStorageService _storageService;
  final DatabaseHelper? _dbHelper;

  AIToolExecutorImpl(
    this._registeredTools,
    this._workspaceRoot,
    this._storageService, [
    this._dbHelper,
  ]);

  @override
  List<AIToolDefinition> get availableTools => _registeredTools;

  @override
  List<AIToolDefinition> resolveTools(List<String> names) {
    return _registeredTools.where((t) => names.contains(t.name)).toList();
  }

  @override
  Future<AIToolResult> executeTool(AIToolCall call) async {
    final toolDef = _registeredTools.cast<AIToolDefinition?>().firstWhere(
      (t) => t?.name == call.name,
      orElse: () => null,
    );

    if (toolDef == null) {
      return AIToolResult(
        toolCallId: call.id,
        name: call.name,
        result: {'error': 'Tool not found'},
        isError: true,
      );
    }

    if (toolDef.riskLevel == AIRiskLevel.dangerous) {
      return AIToolResult(
        toolCallId: call.id,
        name: call.name,
        result: {'error': 'Tool execution blocked: DANGEROUS risk level'},
        isError: true,
      );
    }

    try {
      final result = await _dispatch(call);
      return AIToolResult(toolCallId: call.id, name: call.name, result: result);
    } catch (e, stack) {
      log('Error executing tool ${call.name}: $e\n$stack');
      return AIToolResult(
        toolCallId: call.id,
        name: call.name,
        result: {'error': e.toString()},
        isError: true,
      );
    }
  }

  Future<Map<String, dynamic>> _dispatch(AIToolCall call) async {
    switch (call.name) {
      case 'list_chapters':
        final projectId = call.arguments['projectId'] as String;
        if (_dbHelper != null) {
          final chapters = await _dbHelper.getChapters(projectId);
          return {'chapters': chapters};
        }
        return {'chapters': []};

      case 'read_chapter':
        final chapterId = call.arguments['chapterId'] as String;
        if (_dbHelper != null) {
          final chapter = await _dbHelper.getChapter(chapterId);
          if (chapter != null) {
            final projectId = chapter['project_id'] as String;
            final volumeId = chapter['volume_id'] as String;
            final path = p.join(
              _workspaceRoot,
              'projects',
              projectId,
              'volumes',
              volumeId,
              'chapters',
              '$chapterId.md',
            );
            final file = File(path);
            if (await file.exists()) {
              final content = await file.readAsString();
              return {'content': content};
            }
          }
        }
        return {'error': 'Chapter not found'};

      case 'search_text':
        final projectId = call.arguments['projectId'] as String;
        final query = call.arguments['query'] as String;
        final results = [];

        if (_dbHelper != null) {
          final chapters = await _dbHelper.getChapters(projectId);
          for (final chapter in chapters) {
            final chapterId = chapter['id'] as String;
            final title = chapter['title'] as String;
            final volumeId = chapter['volume_id'] as String;

            final mdPath = p.join(
              _workspaceRoot,
              'projects',
              projectId,
              'volumes',
              volumeId,
              'chapters',
              '$chapterId.md',
            );

            final file = File(mdPath);
            if (await file.exists()) {
              final content = await file.readAsString();
              final index = content.indexOf(query);
              if (index != -1) {
                // extract snippet
                final start = (index - 20).clamp(0, content.length);
                final end = (index + query.length + 20).clamp(
                  0,
                  content.length,
                );
                final snippet = content.substring(start, end);

                results.add({
                  'chapterId': chapterId,
                  'title': title,
                  'matchedSnippet': snippet,
                  'offset': index,
                });
              }
            }
          }
        }
        return {'results': results};

      case 'list_character_cards':
        final projectId = call.arguments['projectId'] as String;
        final charDir = Directory(
          p.join(_workspaceRoot, 'projects', projectId, 'characters'),
        );
        final characters = [];

        if (await charDir.exists()) {
          final entities = await charDir.list().toList();
          for (final entity in entities) {
            if (entity is File &&
                (entity.path.endsWith('.json') ||
                    entity.path.endsWith('.md'))) {
              characters.add({
                'fileName': p.basename(entity.path),
                'path': entity.path,
              });
            }
          }
        }
        return {'characters': characters};

      case 'save_chapter_summary':
        final chapterId = call.arguments['chapterId'] as String;
        final summary = call.arguments['summary'] as String;
        // Strictly prevent writing to chapters/ folder. Write to ai/summaries/
        final aiPath = p.join(
          _workspaceRoot,
          'app-meta',
          'ai',
          'summaries',
          '${chapterId}_summary.json',
        );
        await _storageService.atomicWrite(
          aiPath,
          jsonEncode({'summary': summary}),
        );
        return {'status': 'saved', 'path': aiPath};

      case 'save_character_extraction':
        final projectId = call.arguments['projectId'] as String;
        final aiPath = p.join(
          _workspaceRoot,
          'projects',
          projectId,
          'characters',
          'ai_extract_${DateTime.now().millisecondsSinceEpoch}.json',
        );
        await _storageService.atomicWrite(
          aiPath,
          jsonEncode(call.arguments['characters']),
        );
        return {'status': 'saved', 'path': aiPath};

      default:
        throw Exception("Dispatcher logic not implemented for ${call.name}");
    }
  }
}
