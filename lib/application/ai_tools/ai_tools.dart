import '../../domain/models/ai_models.dart';

class AITools {
  static const listChapters = AIToolDefinition(
    name: 'list_chapters',
    description: 'Lists all chapters in the project.',
    riskLevel: AIRiskLevel.readOnly,
    requiresUserConfirmation: false,
    parametersJsonSchema: {
      "type": "object",
      "properties": {
        "projectId": {"type": "string"},
      },
      "required": ["projectId"],
    },
  );

  static const readChapter = AIToolDefinition(
    name: 'read_chapter',
    description: 'Reads the markdown content of a specific chapter.',
    riskLevel: AIRiskLevel.readOnly,
    requiresUserConfirmation: false,
    parametersJsonSchema: {
      "type": "object",
      "properties": {
        "chapterId": {"type": "string"},
      },
      "required": ["chapterId"],
    },
  );

  static const searchText = AIToolDefinition(
    name: 'search_text',
    description: 'Searches for text across the project.',
    riskLevel: AIRiskLevel.readOnly,
    requiresUserConfirmation: false,
    parametersJsonSchema: {
      "type": "object",
      "properties": {
        "projectId": {"type": "string"},
        "query": {"type": "string"},
      },
      "required": ["projectId", "query"],
    },
  );

  static const listCharacterCards = AIToolDefinition(
    name: 'list_character_cards',
    description: 'Lists all available character cards.',
    riskLevel: AIRiskLevel.readOnly,
    requiresUserConfirmation: false,
    parametersJsonSchema: {
      "type": "object",
      "properties": {
        "projectId": {"type": "string"},
      },
      "required": ["projectId"],
    },
  );

  static const saveCharacterExtraction = AIToolDefinition(
    name: 'save_character_extraction',
    description: 'Saves extracted character data to the characters folder.',
    riskLevel: AIRiskLevel.writeDraft,
    requiresUserConfirmation: true,
    parametersJsonSchema: {
      "type": "object",
      "properties": {
        "projectId": {"type": "string"},
        "characters": {
          "type": "array",
          "items": {"type": "object"},
        },
      },
      "required": ["projectId", "characters"],
    },
  );

  static const saveChapterSummary = AIToolDefinition(
    name: 'save_chapter_summary',
    description: 'Saves a summary for a specific chapter.',
    riskLevel: AIRiskLevel.writeDraft,
    requiresUserConfirmation: true,
    parametersJsonSchema: {
      "type": "object",
      "properties": {
        "chapterId": {"type": "string"},
        "summary": {"type": "string"},
      },
      "required": ["chapterId", "summary"],
    },
  );

  static const saveConsistencyReport = AIToolDefinition(
    name: 'save_consistency_report',
    description: 'Saves a consistency analysis report.',
    riskLevel: AIRiskLevel.writeDraft,
    requiresUserConfirmation: true,
    parametersJsonSchema: {
      "type": "object",
      "properties": {
        "projectId": {"type": "string"},
        "report": {"type": "string"},
      },
      "required": ["projectId", "report"],
    },
  );

  // Example of a blocked tool
  static const rewriteChapterContent = AIToolDefinition(
    name: 'rewrite_chapter_content',
    description: 'Automatically rewrites the user chapter markdown.',
    riskLevel: AIRiskLevel.dangerous,
    requiresUserConfirmation: true,
    parametersJsonSchema: {
      "type": "object",
      "properties": {
        "chapterId": {"type": "string"},
        "content": {"type": "string"},
      },
      "required": ["chapterId", "content"],
    },
  );

  static final List<AIToolDefinition> allTools = [
    listChapters,
    readChapter,
    searchText,
    listCharacterCards,
    saveCharacterExtraction,
    saveChapterSummary,
    saveConsistencyReport,
    rewriteChapterContent,
  ];
}
