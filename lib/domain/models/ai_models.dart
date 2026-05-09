enum AIRiskLevel {
  readOnly,
  writeDraft,
  dangerous,
}

class AIToolDefinition {
  final String name;
  final String description;
  final Map<String, dynamic> parametersJsonSchema;
  final bool strict;
  final AIRiskLevel riskLevel;
  final bool requiresUserConfirmation;

  const AIToolDefinition({
    required this.name,
    required this.description,
    required this.parametersJsonSchema,
    this.strict = false,
    this.riskLevel = AIRiskLevel.dangerous,
    this.requiresUserConfirmation = true,
  });
}

class AIToolCall {
  final String id;
  final String name;
  final Map<String, dynamic> arguments;

  AIToolCall({required this.id, required this.name, required this.arguments});
}

class AIToolResult {
  final String toolCallId;
  final String name;
  final Map<String, dynamic> result;
  final bool isError;

  AIToolResult({
    required this.toolCallId,
    required this.name,
    required this.result,
    this.isError = false,
  });
}

enum AIMessageRole { system, user, assistant, tool }

class AIMessage {
  final AIMessageRole role;
  final String? content;
  final List<AIToolCall>? toolCalls;
  final AIToolResult? toolResult;

  AIMessage({required this.role, this.content, this.toolCalls, this.toolResult});
}

class PromptVersion {
  final String id;
  final String versionString;
  final DateTime createdAt;

  PromptVersion({required this.id, required this.versionString, required this.createdAt});
}

class PromptTemplate {
  final String id;
  final String name;
  final String templateText;
  final PromptVersion currentVersion;

  PromptTemplate({
    required this.id,
    required this.name,
    required this.templateText,
    required this.currentVersion,
  });
}

class AITask {
  final String id;
  final String projectId;
  final List<String> sourceChapterIds;
  final PromptTemplate promptTemplate;
  final String inputHash;
  final String modelName;
  final String providerName;

  AITask({
    required this.id,
    required this.projectId,
    required this.sourceChapterIds,
    required this.promptTemplate,
    required this.inputHash,
    required this.modelName,
    required this.providerName,
  });
}

class AIResult {
  final String taskId;
  final String content;
  final Map<String, dynamic>? jsonOutput;
  final Map<String, dynamic> rawRequest;
  final Map<String, dynamic> rawResponse;

  // Tracing information
  final List<String> sourceChapterIds;
  final String modelName;
  final String providerName;
  final String promptVersion;
  final String inputHash;
  final DateTime createdAt;

  AIResult({
    required this.taskId,
    required this.content,
    this.jsonOutput,
    required this.rawRequest,
    required this.rawResponse,
    required this.sourceChapterIds,
    required this.modelName,
    required this.providerName,
    required this.promptVersion,
    required this.inputHash,
    required this.createdAt,
  });
}
