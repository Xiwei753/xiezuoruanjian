class LocalSettings {
  final String workspacePath;
  final String lastOpenedProjectId;
  final String lastOpenedChapterId;
  final double windowWidth;
  final double windowHeight;
  final String deviceId;
  final String deviceName;
  final bool useSystemProxy;
  final String manualProxyUrl;

  const LocalSettings({
    this.workspacePath = '',
    this.lastOpenedProjectId = '',
    this.lastOpenedChapterId = '',
    this.windowWidth = 1024,
    this.windowHeight = 768,
    this.deviceId = '',
    this.deviceName = 'My Device',
    this.useSystemProxy = true,
    this.manualProxyUrl = '',
  });

  LocalSettings copyWith({
    String? workspacePath,
    String? lastOpenedProjectId,
    String? lastOpenedChapterId,
    double? windowWidth,
    double? windowHeight,
    String? deviceId,
    String? deviceName,
    bool? useSystemProxy,
    String? manualProxyUrl,
  }) {
    return LocalSettings(
      workspacePath: workspacePath ?? this.workspacePath,
      lastOpenedProjectId: lastOpenedProjectId ?? this.lastOpenedProjectId,
      lastOpenedChapterId: lastOpenedChapterId ?? this.lastOpenedChapterId,
      windowWidth: windowWidth ?? this.windowWidth,
      windowHeight: windowHeight ?? this.windowHeight,
      deviceId: deviceId ?? this.deviceId,
      deviceName: deviceName ?? this.deviceName,
      useSystemProxy: useSystemProxy ?? this.useSystemProxy,
      manualProxyUrl: manualProxyUrl ?? this.manualProxyUrl,
    );
  }

  Map<String, dynamic> toJson() {
    return {
      'workspacePath': workspacePath,
      'lastOpenedProjectId': lastOpenedProjectId,
      'lastOpenedChapterId': lastOpenedChapterId,
      'windowWidth': windowWidth,
      'windowHeight': windowHeight,
      'deviceId': deviceId,
      'deviceName': deviceName,
      'useSystemProxy': useSystemProxy,
      'manualProxyUrl': manualProxyUrl,
    };
  }

  factory LocalSettings.fromJson(Map<String, dynamic> json) {
    return LocalSettings(
      workspacePath: json['workspacePath'] as String? ?? '',
      lastOpenedProjectId: json['lastOpenedProjectId'] as String? ?? '',
      lastOpenedChapterId: json['lastOpenedChapterId'] as String? ?? '',
      windowWidth: (json['windowWidth'] as num?)?.toDouble() ?? 1024,
      windowHeight: (json['windowHeight'] as num?)?.toDouble() ?? 768,
      deviceId: json['deviceId'] as String? ?? '',
      deviceName: json['deviceName'] as String? ?? 'My Device',
      useSystemProxy: json['useSystemProxy'] as bool? ?? true,
      manualProxyUrl: json['manualProxyUrl'] as String? ?? '',
    );
  }
}

class SyncableSettings {
  final int schemaVersion;

  // 通用
  final bool autoSaveEnabled;
  final int autoSaveIntervalSeconds;
  final bool backupBeforeSync;
  final int backupRetentionCount;

  // 编辑器
  final double editorFontSize;
  final double editorLineHeight;
  final double editorParagraphSpacing;
  final double editorContentWidth;
  final String themeMode;
  final bool typewriterModeEnabled;
  final bool focusModeEnabled;
  final bool inputAnimationEnabled;
  final bool typedCharacterAnimationEnabled;
  final bool cursorAnimationEnhanced;

  // AI
  final String defaultAIProvider;
  final String defaultAIModel;
  final String deepSeekBaseUrl;
  final String deepSeekApiKey;
  final bool aiToolsEnabled;
  final bool aiThinkingModeEnabled;
  final int aiPromptTemplateVersion;
  final int aiToolDefinitionVersion;
  final int aiSerializerVersion;

  // 纠错
  final bool correctionEnabled;

  // 同步
  final String githubRepoUrl;
  final String githubBranch;
  final String githubSyncMethod;
  final String githubToken;
  final bool syncApiKeysInPlaintext;

  const SyncableSettings({
    this.schemaVersion = 1,
    this.autoSaveEnabled = true,
    this.autoSaveIntervalSeconds = 60,
    this.backupBeforeSync = true,
    this.backupRetentionCount = 5,
    this.editorFontSize = 16.0,
    this.editorLineHeight = 1.6,
    this.editorParagraphSpacing = 16.0,
    this.editorContentWidth = 800.0,
    this.themeMode = 'system',
    this.typewriterModeEnabled = false,
    this.focusModeEnabled = false,
    this.inputAnimationEnabled = false,
    this.typedCharacterAnimationEnabled = false,
    this.cursorAnimationEnhanced = false,
    this.defaultAIProvider = 'deepseek',
    this.defaultAIModel = 'deepseek-chat',
    this.deepSeekBaseUrl = 'https://api.deepseek.com',
    this.deepSeekApiKey = '',
    this.aiToolsEnabled = true,
    this.aiThinkingModeEnabled = false,
    this.aiPromptTemplateVersion = 1,
    this.aiToolDefinitionVersion = 1,
    this.aiSerializerVersion = 1,
    this.correctionEnabled = false,
    this.githubRepoUrl = '',
    this.githubBranch = 'main',
    this.githubSyncMethod = 'ssh',
    this.githubToken = '',
    this.syncApiKeysInPlaintext = true,
  });

  SyncableSettings copyWith({
    int? schemaVersion,
    bool? autoSaveEnabled,
    int? autoSaveIntervalSeconds,
    bool? backupBeforeSync,
    int? backupRetentionCount,
    double? editorFontSize,
    double? editorLineHeight,
    double? editorParagraphSpacing,
    double? editorContentWidth,
    String? themeMode,
    bool? typewriterModeEnabled,
    bool? focusModeEnabled,
    bool? inputAnimationEnabled,
    bool? typedCharacterAnimationEnabled,
    bool? cursorAnimationEnhanced,
    String? defaultAIProvider,
    String? defaultAIModel,
    String? deepSeekBaseUrl,
    String? deepSeekApiKey,
    bool? aiToolsEnabled,
    bool? aiThinkingModeEnabled,
    int? aiPromptTemplateVersion,
    int? aiToolDefinitionVersion,
    int? aiSerializerVersion,
    bool? correctionEnabled,
    String? githubRepoUrl,
    String? githubBranch,
    String? githubSyncMethod,
    String? githubToken,
    bool? syncApiKeysInPlaintext,
  }) {
    return SyncableSettings(
      schemaVersion: schemaVersion ?? this.schemaVersion,
      autoSaveEnabled: autoSaveEnabled ?? this.autoSaveEnabled,
      autoSaveIntervalSeconds:
          autoSaveIntervalSeconds ?? this.autoSaveIntervalSeconds,
      backupBeforeSync: backupBeforeSync ?? this.backupBeforeSync,
      backupRetentionCount: backupRetentionCount ?? this.backupRetentionCount,
      editorFontSize: editorFontSize ?? this.editorFontSize,
      editorLineHeight: editorLineHeight ?? this.editorLineHeight,
      editorParagraphSpacing:
          editorParagraphSpacing ?? this.editorParagraphSpacing,
      editorContentWidth: editorContentWidth ?? this.editorContentWidth,
      themeMode: themeMode ?? this.themeMode,
      typewriterModeEnabled:
          typewriterModeEnabled ?? this.typewriterModeEnabled,
      focusModeEnabled: focusModeEnabled ?? this.focusModeEnabled,
      inputAnimationEnabled:
          inputAnimationEnabled ?? this.inputAnimationEnabled,
      typedCharacterAnimationEnabled:
          typedCharacterAnimationEnabled ?? this.typedCharacterAnimationEnabled,
      cursorAnimationEnhanced:
          cursorAnimationEnhanced ?? this.cursorAnimationEnhanced,
      defaultAIProvider: defaultAIProvider ?? this.defaultAIProvider,
      defaultAIModel: defaultAIModel ?? this.defaultAIModel,
      deepSeekBaseUrl: deepSeekBaseUrl ?? this.deepSeekBaseUrl,
      deepSeekApiKey: deepSeekApiKey ?? this.deepSeekApiKey,
      aiToolsEnabled: aiToolsEnabled ?? this.aiToolsEnabled,
      aiThinkingModeEnabled:
          aiThinkingModeEnabled ?? this.aiThinkingModeEnabled,
      aiPromptTemplateVersion:
          aiPromptTemplateVersion ?? this.aiPromptTemplateVersion,
      aiToolDefinitionVersion:
          aiToolDefinitionVersion ?? this.aiToolDefinitionVersion,
      aiSerializerVersion: aiSerializerVersion ?? this.aiSerializerVersion,
      correctionEnabled: correctionEnabled ?? this.correctionEnabled,
      githubRepoUrl: githubRepoUrl ?? this.githubRepoUrl,
      githubBranch: githubBranch ?? this.githubBranch,
      githubSyncMethod: githubSyncMethod ?? this.githubSyncMethod,
      githubToken: githubToken ?? this.githubToken,
      syncApiKeysInPlaintext:
          syncApiKeysInPlaintext ?? this.syncApiKeysInPlaintext,
    );
  }

  Map<String, dynamic> toJson() {
    return {
      'schemaVersion': schemaVersion,
      'autoSaveEnabled': autoSaveEnabled,
      'autoSaveIntervalSeconds': autoSaveIntervalSeconds,
      'backupBeforeSync': backupBeforeSync,
      'backupRetentionCount': backupRetentionCount,
      'editorFontSize': editorFontSize,
      'editorLineHeight': editorLineHeight,
      'editorParagraphSpacing': editorParagraphSpacing,
      'editorContentWidth': editorContentWidth,
      'themeMode': themeMode,
      'typewriterModeEnabled': typewriterModeEnabled,
      'focusModeEnabled': focusModeEnabled,
      'inputAnimationEnabled': inputAnimationEnabled,
      'typedCharacterAnimationEnabled': typedCharacterAnimationEnabled,
      'cursorAnimationEnhanced': cursorAnimationEnhanced,
      'defaultAIProvider': defaultAIProvider,
      'defaultAIModel': defaultAIModel,
      'deepSeekBaseUrl': deepSeekBaseUrl,
      'deepSeekApiKey': deepSeekApiKey,
      'aiToolsEnabled': aiToolsEnabled,
      'aiThinkingModeEnabled': aiThinkingModeEnabled,
      'aiPromptTemplateVersion': aiPromptTemplateVersion,
      'aiToolDefinitionVersion': aiToolDefinitionVersion,
      'aiSerializerVersion': aiSerializerVersion,
      'correctionEnabled': correctionEnabled,
      'githubRepoUrl': githubRepoUrl,
      'githubBranch': githubBranch,
      'githubSyncMethod': githubSyncMethod,
      'githubToken': githubToken,
      'syncApiKeysInPlaintext': syncApiKeysInPlaintext,
    };
  }

  factory SyncableSettings.fromJson(Map<String, dynamic> json) {
    return SyncableSettings(
      schemaVersion: json['schemaVersion'] as int? ?? 1,
      autoSaveEnabled: json['autoSaveEnabled'] as bool? ?? true,
      autoSaveIntervalSeconds: json['autoSaveIntervalSeconds'] as int? ?? 60,
      backupBeforeSync: json['backupBeforeSync'] as bool? ?? true,
      backupRetentionCount: json['backupRetentionCount'] as int? ?? 5,
      editorFontSize: (json['editorFontSize'] as num?)?.toDouble() ?? 16.0,
      editorLineHeight: (json['editorLineHeight'] as num?)?.toDouble() ?? 1.6,
      editorParagraphSpacing:
          (json['editorParagraphSpacing'] as num?)?.toDouble() ?? 16.0,
      editorContentWidth:
          (json['editorContentWidth'] as num?)?.toDouble() ?? 800.0,
      themeMode: json['themeMode'] as String? ?? 'system',
      typewriterModeEnabled: json['typewriterModeEnabled'] as bool? ?? false,
      focusModeEnabled: json['focusModeEnabled'] as bool? ?? false,
      inputAnimationEnabled: json['inputAnimationEnabled'] as bool? ?? false,
      typedCharacterAnimationEnabled:
          json['typedCharacterAnimationEnabled'] as bool? ?? false,
      cursorAnimationEnhanced:
          json['cursorAnimationEnhanced'] as bool? ?? false,
      defaultAIProvider: json['defaultAIProvider'] as String? ?? 'deepseek',
      defaultAIModel: json['defaultAIModel'] as String? ?? 'deepseek-chat',
      deepSeekBaseUrl:
          json['deepSeekBaseUrl'] as String? ?? 'https://api.deepseek.com',
      deepSeekApiKey: json['deepSeekApiKey'] as String? ?? '',
      aiToolsEnabled: json['aiToolsEnabled'] as bool? ?? true,
      aiThinkingModeEnabled: json['aiThinkingModeEnabled'] as bool? ?? false,
      aiPromptTemplateVersion: json['aiPromptTemplateVersion'] as int? ?? 1,
      aiToolDefinitionVersion: json['aiToolDefinitionVersion'] as int? ?? 1,
      aiSerializerVersion: json['aiSerializerVersion'] as int? ?? 1,
      correctionEnabled: json['correctionEnabled'] as bool? ?? false,
      githubRepoUrl: json['githubRepoUrl'] as String? ?? '',
      githubBranch: json['githubBranch'] as String? ?? 'main',
      githubSyncMethod: json['githubSyncMethod'] as String? ?? 'ssh',
      githubToken: json['githubToken'] as String? ?? '',
      syncApiKeysInPlaintext: json['syncApiKeysInPlaintext'] as bool? ?? true,
    );
  }
}
