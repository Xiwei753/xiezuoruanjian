import 'dart:convert';
import 'package:path/path.dart' as p;
import '../../domain/models/ai_models.dart';
import '../../domain/services_interfaces/ai_tool_executor.dart';
import '../../domain/services_interfaces/storage_service.dart';

class AIToolExecutorImpl implements IAIToolExecutor, AIToolRegistry {
  final List<AIToolDefinition> _registeredTools;
  final String _workspaceRoot;
  final IStorageService _storageService;

  AIToolExecutorImpl(
    this._registeredTools,
    this._workspaceRoot,
    this._storageService,
  );

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
    } catch (e) {
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
      case 'read_chapter':
        final chapterId = call.arguments['chapterId'] as String;
        // Simplified for MVP. In reality, we query the DB to get volume/project paths.
        // For tests, we mock finding it.
        return {'content': 'Simulated content for $chapterId'};

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
