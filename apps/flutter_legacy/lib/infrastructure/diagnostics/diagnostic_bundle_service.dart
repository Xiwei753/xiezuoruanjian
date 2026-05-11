import 'dart:convert';
import 'dart:io';
import 'package:flutter/foundation.dart';
import 'package:path_provider/path_provider.dart';
import 'package:writer_app/application/controllers/settings_controller.dart';
import 'package:writer_app/infrastructure/logging/app_logger.dart';

class DiagnosticBundleService {
  static const int _maxLogLines = 500;

  /// Exports diagnostic information safely into a timestamped directory
  /// Returns the path to the diagnostic directory.
  static Future<String> exportDiagnostics(
    SettingsController settingsController,
  ) async {
    try {
      final timestamp = DateTime.now().millisecondsSinceEpoch;
      final workspacePath = settingsController.workspacePath.isNotEmpty
          ? settingsController.workspacePath
          : '${(await getApplicationDocumentsDirectory()).path}/writer_app_workspace';

      final diagDir = Directory(
        '$workspacePath/app-meta/diagnostics/diagnostic_$timestamp',
      );
      if (!await diagDir.exists()) {
        await diagDir.create(recursive: true);
      }

      await _writeAppInfo(diagDir);
      await _writeEnvironment(diagDir);
      await _writeSettingsSummary(diagDir, settingsController);
      await _writeLogsTail(diagDir, workspacePath);
      await _writeGitInfo(diagDir, workspacePath);
      await _writeReadme(diagDir);

      return diagDir.path;
    } catch (e, st) {
      AppLogger.error('Failed to export diagnostics', e, st);
      rethrow;
    }
  }

  static Future<void> _writeAppInfo(Directory diagDir) async {
    final info = {
      'os': Platform.operatingSystem,
      'osVersion': Platform.operatingSystemVersion,
      'isWeb': kIsWeb,
      'locale': Platform.localeName,
    };
    final file = File('${diagDir.path}/app_info.json');
    await file.writeAsString(jsonEncode(info));
  }

  static Future<void> _writeEnvironment(Directory diagDir) async {
    final Map<String, String> env = Platform.environment;
    final Map<String, String> allowedEnv = {};

    final keys = [
      'XDG_SESSION_TYPE',
      'WAYLAND_DISPLAY',
      'DISPLAY',
      'GDK_BACKEND',
      'GTK_IM_MODULE',
      'QT_IM_MODULE',
      'SDL_IM_MODULE',
      'XMODIFIERS',
      'XDG_CURRENT_DESKTOP',
    ];

    for (var key in keys) {
      if (env.containsKey(key)) {
        allowedEnv[key] = env[key]!;
      }
    }

    final file = File('${diagDir.path}/environment.json');
    await file.writeAsString(jsonEncode(allowedEnv));
  }

  static Future<void> _writeSettingsSummary(
    Directory diagDir,
    SettingsController controller,
  ) async {
    final local = controller.localSettings;
    final syncable = controller.syncableSettings;

    // Only allow specific keys that are helpful for debugging layout/UI/saving
    final summary = {
      'themeMode': syncable.themeMode,
      'editorFontSize': syncable.editorFontSize,
      'editorLineHeight': syncable.editorLineHeight,
      'editorContentWidth': syncable.editorContentWidth,
      'smoothScrollingEnabled': syncable.smoothScrollingEnabled,
      'inputAnimationEnabled': syncable.inputAnimationEnabled,
      'imeSafeModeEnabled': syncable.imeSafeModeEnabled,
      'startupBehavior': syncable.startupBehavior,
      'autoSaveEnabled': syncable.autoSaveEnabled,
      'autoSaveIntervalSeconds': syncable.autoSaveIntervalSeconds,
      'loggingEnabled': local.loggingEnabled,
      'performanceLoggingEnabled': local.performanceLoggingEnabled,
    };

    final file = File('${diagDir.path}/settings_summary.json');
    await file.writeAsString(jsonEncode(summary));
  }

  static Future<void> _writeLogsTail(
    Directory diagDir,
    String workspacePath,
  ) async {
    final logFile = File('$workspacePath/app-meta/logs/app.log');
    final outFile = File('${diagDir.path}/logs_tail.jsonl');

    if (!await logFile.exists()) {
      await outFile.writeAsString(
        jsonEncode({'note': 'Log file not found or empty'}),
      );
      return;
    }

    try {
      final lines = await logFile.readAsLines();
      final tail = lines.length > _maxLogLines
          ? lines.sublist(lines.length - _maxLogLines)
          : lines;

      // Sanitize lines again just in case, though AppLogger should have done it
      final sanitizedLines = tail.map((line) {
        String sanitized = line;
        // Additional check for key/token exposure just in case
        sanitized = sanitized.replaceAll(
          RegExp(
            r'"(deepSeekApiKey|githubToken|password|secret|apiKey|token)":\s*".*?"',
            caseSensitive: false,
          ),
          '"\$1": "***"',
        );
        return sanitized;
      }).toList();

      await outFile.writeAsString(sanitizedLines.join('\n'));
    } catch (e) {
      await outFile.writeAsString(
        jsonEncode({'error': 'Failed to read logs: $e'}),
      );
    }
  }

  static Future<void> _writeGitInfo(
    Directory diagDir,
    String workspacePath,
  ) async {
    final file = File('${diagDir.path}/git_info.txt');
    try {
      final result = await Process.run('git', ['status', '--short']);
      if (result.exitCode == 0) {
        await file.writeAsString('Git Status (Short):\n${result.stdout}\n');
      } else {
        await file.writeAsString('Git not available or not a repo.\n');
      }
    } catch (e) {
      await file.writeAsString('Failed to get git info: $e\n');
    }
  }

  static Future<void> _writeReadme(Directory diagDir) async {
    final file = File('${diagDir.path}/README.txt');
    const content = '''
This is a diagnostic bundle for the Writer App.
It contains strictly non-sensitive configuration, environment details, and recent logs.
It DOES NOT contain manuscript content, API keys, or raw AI reasoning.
''';
    await file.writeAsString(content);
  }
}
