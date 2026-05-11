import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:writer_app/application/controllers/settings_controller.dart';
import 'package:writer_app/domain/models/settings.dart';
import 'package:writer_app/infrastructure/diagnostics/diagnostic_bundle_service.dart';

// A fake or mocked settings controller just for testing the service
class MockSettingsController extends SettingsController {
  @override
  String get workspacePath => Directory.systemTemp.path;

  @override
  LocalSettings get localSettings => const LocalSettings();

  @override
  SyncableSettings get syncableSettings => const SyncableSettings(
    deepSeekApiKey: 'super_secret_api_key_123',
    githubToken: 'ghp_secret_token_456',
  );
}

void main() {
  group('DiagnosticBundleService Tests', () {
    late MockSettingsController controller;
    late String tempWorkspacePath;
    late File mockLogFile;

    setUp(() async {
      controller = MockSettingsController();
      tempWorkspacePath = controller.workspacePath;

      final logDir = Directory('$tempWorkspacePath/app-meta/logs');
      if (!await logDir.exists()) {
        await logDir.create(recursive: true);
      }
      mockLogFile = File('${logDir.path}/app.log');

      // Create a mock log with sensitive data
      await mockLogFile.writeAsString('''
{"level":"info","message":"App started"}
{"level":"debug","metadata":{"deepSeekApiKey":"real_key_here","other":"value"}}
{"level":"error","message":"Failed to sync","metadata":{"githubToken":"my_token"}}
''');
    });

    tearDown(() async {
      if (await mockLogFile.exists()) {
        await mockLogFile.delete();
      }
      // Try to clean up diagnostics
      final diagDir = Directory('$tempWorkspacePath/app-meta/diagnostics');
      if (await diagDir.exists()) {
        await diagDir.delete(recursive: true);
      }
    });

    test('exports diagnostics successfully and sanitizes settings', () async {
      final path = await DiagnosticBundleService.exportDiagnostics(controller);
      final dir = Directory(path);

      expect(await dir.exists(), isTrue);

      final settingsFile = File('${dir.path}/settings_summary.json');
      expect(await settingsFile.exists(), isTrue);

      final settingsContent = await settingsFile.readAsString();
      final settingsJson = jsonDecode(settingsContent) as Map<String, dynamic>;

      // Check allowed keys are present
      expect(settingsJson.containsKey('themeMode'), isTrue);
      expect(settingsJson.containsKey('editorFontSize'), isTrue);

      // Check sensitive keys are strictly absent
      expect(settingsJson.containsKey('deepSeekApiKey'), isFalse);
      expect(settingsJson.containsKey('githubToken'), isFalse);
      expect(settingsContent.contains('super_secret_api_key_123'), isFalse);
      expect(settingsContent.contains('ghp_secret_token_456'), isFalse);
    });

    test('exports logs and sanitizes sensitive data in tail', () async {
      final path = await DiagnosticBundleService.exportDiagnostics(controller);
      final dir = Directory(path);

      final logFile = File('${dir.path}/logs_tail.jsonl');
      expect(await logFile.exists(), isTrue);

      final logContent = await logFile.readAsString();

      // Check standard content
      expect(logContent.contains('App started'), isTrue);

      // Check that the regex replacement works
      expect(logContent.contains('real_key_here'), isFalse);
      expect(logContent.contains('my_token'), isFalse);
      expect(logContent.contains('***'), isTrue);
    });

    test('handles missing log gracefully', () async {
      await mockLogFile.delete(); // No log file exists
      final path = await DiagnosticBundleService.exportDiagnostics(controller);
      final dir = Directory(path);

      final logFile = File('${dir.path}/logs_tail.jsonl');
      expect(await logFile.exists(), isTrue);

      final logContent = await logFile.readAsString();
      expect(logContent.contains('Log file not found or empty'), isTrue);
    });
  });
}
