import 'package:flutter_test/flutter_test.dart';
import 'package:writer_app/domain/models/settings.dart';

void main() {
  group('SyncableSettings Models', () {
    test('default values are correct', () {
      const settings = SyncableSettings();

      expect(settings.schemaVersion, 1);
      expect(settings.autoSaveEnabled, true);
      expect(settings.autoSaveIntervalSeconds, 60);
      expect(settings.backupBeforeSync, true);
      expect(settings.backupRetentionCount, 5);
      expect(settings.editorFontSize, 16.0);
      expect(settings.themeMode, 'system');
      expect(settings.defaultAIProvider, 'deepseek');
      expect(settings.defaultAIModel, 'deepseek-chat');
      expect(settings.deepSeekBaseUrl, 'https://api.deepseek.com');
      expect(settings.deepSeekApiKey, '');
      expect(settings.githubToken, '');
      expect(settings.syncApiKeysInPlaintext, true);
      expect(settings.correctionEnabled, false);
      expect(settings.inputAnimationEnabled, false);
      expect(settings.typedCharacterAnimationEnabled, false);
      expect(settings.cursorAnimationEnhanced, false);
    });

    test('can save and read JSON', () {
      const original = SyncableSettings(
        autoSaveEnabled: false,
        autoSaveIntervalSeconds: 120,
        editorFontSize: 18.0,
        themeMode: 'dark',
        deepSeekApiKey: 'test_api_key',
        githubToken: 'test_github_token',
        inputAnimationEnabled: true,
        typedCharacterAnimationEnabled: true,
        cursorAnimationEnhanced: true,
      );

      final json = original.toJson();
      final restored = SyncableSettings.fromJson(json);

      expect(restored.autoSaveEnabled, false);
      expect(restored.autoSaveIntervalSeconds, 120);
      expect(restored.editorFontSize, 18.0);
      expect(restored.themeMode, 'dark');
      expect(restored.deepSeekApiKey, 'test_api_key');
      expect(restored.githubToken, 'test_github_token');
      expect(restored.inputAnimationEnabled, true);
      expect(restored.typedCharacterAnimationEnabled, true);
      expect(restored.cursorAnimationEnhanced, true);
    });

    test('can contain deepSeekApiKey and githubToken', () {
      const settings = SyncableSettings(
        deepSeekApiKey: 'secret_deepseek_key',
        githubToken: 'secret_github_token',
      );

      expect(settings.deepSeekApiKey, 'secret_deepseek_key');
      expect(settings.githubToken, 'secret_github_token');
    });
  });

  group('LocalSettings Models', () {
    test('default values are correct', () {
      const settings = LocalSettings();
      expect(settings.workspacePath, '');
      expect(settings.deviceName, 'My Device');
    });

    test('can save and read JSON', () {
      const original = LocalSettings(
        workspacePath: '/test/path',
        deviceName: 'Test PC',
      );

      final json = original.toJson();
      final restored = LocalSettings.fromJson(json);

      expect(restored.workspacePath, '/test/path');
      expect(restored.deviceName, 'Test PC');
    });
  });
}
