import 'dart:convert';
import 'dart:io';
import 'dart:isolate';
import 'package:flutter/foundation.dart';
import 'package:path_provider/path_provider.dart';

enum LogLevel { debug, info, warning, error, performance }

class AppLogger {
  static const int _maxLogSizeBytes = 1024 * 1024; // 1MB
  static const int _maxLogFiles = 5;
  static File? _logFile;
  static bool _initialized = false;
  static bool _loggingEnabled = true;
  static bool _debugLoggingEnabled = false;
  static bool _isExportingDiagnostics = false;
  static bool _performanceLoggingEnabled = true;
  static int _slowOperationThresholdMs = 100;
  static String? _logDirPath;

  // Rate limiting map for specific messages
  static final Map<String, int> _lastLogTimestamps = {};

  static Future<void> init({
    required bool loggingEnabled,
    required bool debugLoggingEnabled,
    required bool performanceLoggingEnabled,
    required int slowOperationThresholdMs,
    String? workspacePath,
  }) async {
    _loggingEnabled = loggingEnabled;
    _debugLoggingEnabled = debugLoggingEnabled;
    _performanceLoggingEnabled = performanceLoggingEnabled;
    _slowOperationThresholdMs = slowOperationThresholdMs;

    if (_initialized) return;

    try {
      String baseDir;
      if (workspacePath != null && workspacePath.isNotEmpty) {
        baseDir = workspacePath;
      } else {
        final directory = await getApplicationDocumentsDirectory();
        baseDir = '${directory.path}/writer_app_workspace';
      }

      final logDir = Directory('$baseDir/app-meta/logs');
      if (!await logDir.exists()) {
        await logDir.create(recursive: true);
      }

      _logDirPath = logDir.path;
      _logFile = File('${logDir.path}/app.log');
      _initialized = true;
    } catch (e) {
      debugPrint('Failed to initialize AppLogger: $e');
    }
  }

  static void configure({
    required bool loggingEnabled,
    required bool debugLoggingEnabled,
    required bool performanceLoggingEnabled,
    required int slowOperationThresholdMs,
  }) {
    _loggingEnabled = loggingEnabled;
    _debugLoggingEnabled = debugLoggingEnabled;
    _performanceLoggingEnabled = performanceLoggingEnabled;
    _slowOperationThresholdMs = slowOperationThresholdMs;
  }

  static String? get logDirPath => _logDirPath;

  static void debug(String message, [Map<String, dynamic>? metadata]) {
    if (!_debugLoggingEnabled) return;
    _log(LogLevel.debug, message, metadata: metadata);
  }

  static void info(
    String message, {
    String? key,
    int limitMs = 0,
    Map<String, dynamic>? metadata,
  }) {
    if (key != null && limitMs > 0) {
      final now = DateTime.now().millisecondsSinceEpoch;
      final lastTime = _lastLogTimestamps[key] ?? 0;
      if (now - lastTime <= limitMs) {
        return;
      }
      _lastLogTimestamps[key] = now;
    }
    _log(LogLevel.info, message, metadata: metadata);
  }

  static void warning(String message, [Map<String, dynamic>? metadata]) {
    _log(LogLevel.warning, message, metadata: metadata);
  }

  static void error(
    String message,
    Object error,
    StackTrace stackTrace, [
    Map<String, dynamic>? metadata,
  ]) {
    _log(
      LogLevel.error,
      message,
      error: error,
      stackTrace: stackTrace,
      metadata: metadata,
    );
  }

  static void performance(
    String operationName,
    int elapsedMs, [
    Map<String, dynamic>? metadata,
  ]) {
    if (!_performanceLoggingEnabled) return;
    _log(
      LogLevel.performance,
      operationName,
      metadata: {...?metadata, 'elapsedMs': elapsedMs},
    );
  }

  static void rateLimitedWarning(
    String key,
    String message, {
    int limitMs = 5000,
    Map<String, dynamic>? metadata,
  }) {
    final now = DateTime.now().millisecondsSinceEpoch;
    final lastTime = _lastLogTimestamps[key] ?? 0;
    if (now - lastTime > limitMs) {
      warning(message, metadata);
      _lastLogTimestamps[key] = now;
    }
  }

  static Future<T> measure<T>(
    String operationName,
    Future<T> Function() operation, [
    Map<String, dynamic>? metadata,
  ]) async {
    final stopwatch = Stopwatch()..start();
    try {
      return await operation();
    } finally {
      stopwatch.stop();
      if (_performanceLoggingEnabled &&
          stopwatch.elapsedMilliseconds >= _slowOperationThresholdMs) {
        performance(operationName, stopwatch.elapsedMilliseconds, metadata);
      }
    }
  }

  static void pauseLogging() {
    _isExportingDiagnostics = true;
  }

  static void resumeLogging() {
    _isExportingDiagnostics = false;
  }

  static void _log(
    LogLevel level,
    String message, {
    Object? error,
    StackTrace? stackTrace,
    Map<String, dynamic>? metadata,
  }) {
    if (!_loggingEnabled || _isExportingDiagnostics) return;
    if (!_initialized || _logFile == null) return;

    final logEntry = {
      'timestamp': DateTime.now().toIso8601String(),
      'level': level.name,
      'message': message,
      'platform': Platform.operatingSystem,
      'appVersion': '', // Can be populated later
      if (metadata != null) 'metadata': sanitizeMetadata(metadata),
      if (error != null) 'errorType': error.runtimeType.toString(),
      if (error != null) 'errorMessage': error.toString(),
      if (stackTrace != null) 'stackTrace': stackTrace.toString(),
    };

    final logLine = jsonEncode(logEntry);

    // Fire and forget
    Isolate.run(() => _writeLogLine(_logFile!.path, logLine));
  }

  static Future<void> _writeLogLine(String path, String line) async {
    try {
      final file = File(path);

      // Simple log rotation
      if (await file.exists() && await file.length() >= _maxLogSizeBytes) {
        await _rotateLogs(path);
      }

      await file.writeAsString('$line\n', mode: FileMode.append, flush: true);
    } catch (e) {
      // Swallow errors silently to avoid crashing the app
      debugPrint('Failed to write log: $e');
    }
  }

  static Future<void> _rotateLogs(String basePath) async {
    try {
      for (int i = _maxLogFiles - 1; i > 0; i--) {
        final currentFile = File('$basePath${i == 1 ? '' : '.${i - 1}'}');
        final nextFile = File('$basePath.$i');

        if (await currentFile.exists()) {
          if (await nextFile.exists()) {
            await nextFile.delete();
          }
          await currentFile.rename(nextFile.path);
        }
      }

      // The original file was moved to .1, so we just create a new empty one by writing to it later.
    } catch (e) {
      debugPrint('Failed to rotate logs: $e');
    }
  }

  static Map<String, dynamic> sanitizeMetadata(Map<String, dynamic> metadata) {
    final Map<String, dynamic> sanitized = {};

    metadata.forEach((key, value) {
      final lowerKey = key.toLowerCase();

      // Filter sensitive keys
      if (lowerKey.contains('apikey') ||
          lowerKey.contains('token') ||
          lowerKey.contains('password') ||
          lowerKey.contains('secret')) {
        sanitized[key] = '***';
        return;
      }

      // Prevent logging raw content / prompt
      if (lowerKey == 'content' ||
          lowerKey == 'prompt' ||
          lowerKey == 'reasoning_content' ||
          lowerKey == 'rawrequest' ||
          lowerKey == 'rawresponse' ||
          lowerKey == 'fulltext') {
        sanitized[key] = '[REDACTED]';
        return;
      }

      // Truncate long strings
      if (value is String) {
        if (value.length > 200) {
          sanitized[key] = '${value.substring(0, 200)}...[TRUNCATED]';
        } else {
          sanitized[key] = value;
        }
      } else if (value is Map) {
        sanitized[key] = sanitizeMetadata(Map<String, dynamic>.from(value));
      } else if (value is List) {
        // Simple list truncation
        if (value.length > 20) {
          sanitized[key] = '[LIST TRUNCATED] length: ${value.length}';
        } else {
          sanitized[key] = value
              .map(
                (e) => e is String && e.length > 200
                    ? '${e.substring(0, 200)}...[TRUNCATED]'
                    : e,
              )
              .toList();
        }
      } else {
        sanitized[key] = value;
      }
    });

    return sanitized;
  }
}
